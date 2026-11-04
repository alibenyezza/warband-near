"""60-day simulation of Warband Raids, on top of the Warband market model in simulate.py (same rules, integer math).

The behaviour of the community is invented (see SCENARIOS). The simulation shows what the raid rules do with it:
supply minted to raiders against supply burned, the chest, the streak, and what the attacks cost.

Rules simulated (see near/RAIDS.md):
- one raid per day. A holder joins with its seasoned balance (lots held at least one day), at least min_join_balance.
- the raid succeeds when the joiners' seasoned balance reaches target_bps of the circulating supply AND at least
  min_joiners accounts joined. Success: each joiner is minted mint_bps of its seasoned balance (times a streak bonus,
  up to x2 at streak 10) as a new level-0 lot, and the raid hands out raid_pulls loot pulls, each to a joiner drawn
  at random weighted by seasoned balance (so splitting a bag across accounts wins nothing). Failure: fail_burn_bps of the chest buys
  tokens on the curve and burns them, and the streak resets.
- a share of every trading fee feeds the chest (NEAR). A ticket is one pull: it pays a tier share of the chest.
  Higher level = better epic/legendary odds. Extra tickets are bought by burning tokens worth ticket_cost_bps of the
  chest, priced at min(spot, yesterday's high) so a same-day pump does not make tickets cheap.

    python near/sim/raids.py        # writes near/sim/raids*.png and near/sim/raids_results.md
"""
import os
import random

from simulate import NEAR, TOKEN, TICKS, Warband, bps

DAYS = 60
RAID = dict(chest_share_bps=3000, min_join_balance=1_000_000 * TOKEN, min_joiners=10, target_bps=2500,
            mint_bps=10, fail_burn_bps=1000, ticket_cost_bps=40, raid_pulls=20)
# tier: (base odds per 10_000, share of the chest paid in bps)
TIERS = [("common", 8500, 5), ("rare", 1200, 50), ("epic", 270, 300), ("legendary", 30, 1500)]
BASE = dict(curve_supply=10**9 * TOKEN, virtual_near=30 * NEAR, fee_bps=1000, dao_share_bps=5000,
            floor_drawdown_bps=2000, buyback_chunk_bps=1000, buyback_daily_cap_bps=2000)


class Raids(Warband):
    def __init__(self, cfg, raid):
        super().__init__(cfg)
        self.r = raid
        self.chest = 0
        self.joined = {}  # account -> seasoned balance at join, for today's raid
        self.streak = self.best = self.wins = self.losses = 0
        self.minted = self.burn_fail = self.burn_tickets = self.paid_out = 0
        self.tickets = {}
        self.pulls = {t[0]: 0 for t in TIERS}
        self.won = {}  # account -> NEAR won

    def fee(self, f):
        to_chest = bps(f, self.r["chest_share_bps"])
        self.chest += to_chest
        super().fee(f - to_chest)

    def seasoned(self, who):
        return sum(a for a, d in self.lots.get(who, []) if self.day - d >= 1)

    def top_level(self, who):
        return max((min(10, self.day - d) for a, d in self.lots.get(who, []) if a), default=0)

    def join(self, who):
        s = self.seasoned(who)
        if s >= self.r["min_join_balance"] and who not in self.joined:
            self.joined[who] = s

    def burn_curve(self, near_amount):
        """Spend NEAR from the chest on the curve and burn the tokens it buys."""
        if near_amount <= 0:
            return 0
        out = self.tr * near_amount // (self.nr + near_amount)
        self.nr += near_amount
        self.tr -= out
        self.burned += out
        return out

    def settle(self, rnd):
        total = sum(self.joined.values())
        ok = total >= bps(self.circ, self.r["target_bps"]) and len(self.joined) >= self.r["min_joiners"]
        if ok:
            self.streak += 1
            self.best = max(self.best, self.streak)
            self.wins += 1
            mult = 10_000 + 1_000 * min(self.streak, 10)  # x1.1 at streak 1 ... x2 at streak 10
            for who, s in self.joined.items():
                m = bps(bps(s, self.r["mint_bps"]), mult)
                self.circ += m
                self.minted += m
                self.lots[who].append([m, self.day])  # fresh lot, level 0: minting gives no seniority
            people, weights = list(self.joined), list(self.joined.values())
            for who in rnd.choices(people, weights=weights, k=self.r["raid_pulls"]):
                self.tickets[who] = self.tickets.get(who, 0) + 1
        else:
            self.streak = 0
            self.losses += 1
            spend = bps(self.chest, self.r["fail_burn_bps"])
            self.chest -= spend
            self.burn_fail += self.burn_curve(spend)
        self.joined = {}
        return ok

    def pull(self, who, rnd):
        lvl = self.top_level(who)
        odds = [o if name in ("common", "rare") else o * (10 + lvl) // 10 for name, o, _ in TIERS]
        odds[0] = 10_000 - sum(odds[1:])
        x = rnd.randrange(10_000)
        for (name, _, share), o in zip(TIERS, odds):
            if x < o:
                win = bps(self.chest, share)
                self.chest -= win
                self.paid_out += win
                self.pulls[name] += 1
                self.won[who] = self.won.get(who, 0) + win
                return name, win
            x -= o
        raise AssertionError

    def ref_price(self):
        p = self.price()
        return min(p, self.high_prev) if self.high_prev else p

    def ticket_tokens(self):
        return bps(self.chest, self.r["ticket_cost_bps"]) * TOKEN // max(1, self.ref_price())

    def buy_ticket(self, who):
        cost = self.ticket_tokens()
        if cost == 0 or self.balance(who) < cost:
            return False
        self.debit(who, cost)
        self.circ -= cost
        self.burned += cost
        self.burn_tickets += cost
        self.tickets[who] = self.tickets.get(who, 0) + 1
        return True


def community(n_members, join_p, n_traders_p=0.6, exit_p=0.01):
    """n_members buy a bag in the first week, then try to join each day with probability join_p(day).
    Traders buy and sell around them like the steady scenario of simulate.py."""

    def day(d, w, rnd):
        ev = []
        if d < 7:
            ev += [("buy", f"m{i}", rnd.uniform(0.3, 1.5)) for i in range(n_members) if i % 7 == d]
        for t in range(TICKS):
            if rnd.random() < n_traders_p:
                ev.append(("buy", f"t{d}_{t}", rnd.uniform(0.05, 0.4)))
            if rnd.random() < 0.35 and d > 1:
                ev.append(("sell", f"t{rnd.randrange(d)}_{rnd.randrange(TICKS)}", 1.0))
        for i in range(n_members):
            if d > 7 and rnd.random() < exit_p:
                ev.append(("sell", f"m{i}", 1.0))
        joins = [f"m{i}" for i in range(n_members) if rnd.random() < join_p(d)]
        return ev, joins, []

    return day


def engaged(d, w, rnd):
    return community(60, lambda d: 0.7)(d, w, rnd)


def fading(d, w, rnd):
    return community(60, lambda d: max(0.1, 0.85 - d * 0.015), exit_p=0.02)(d, w, rnd)


def gamblers(d, w, rnd):
    """Engaged community where 10 members also burn tokens for extra tickets every day (2 each)."""
    ev, joins, _ = community(60, lambda d: 0.7)(d, w, rnd)
    buys = [f"m{i}" for i in range(10) for _ in range(2)] if d > 7 else []
    return ev, joins, buys


def whale_carry(d, w, rnd):
    """Only 6 members show up, one of them a whale with 40% of the bags: the balance target is met, the head count
    is not."""
    ev, joins, _ = community(40, lambda d: 0.15)(d, w, rnd)
    if d == 0:
        ev.append(("buy", "whale", 25))
    if d > 0:
        joins.append("whale")
    return ev, joins, []


def sybil_farm(d, w, rnd):
    """A small community (8 regulars) that cannot reach min_joiners alone. On day 10 a farmer splits a seasoned 10 NEAR
    bag across 12 fresh accounts to fill the head count and farm mints. Compared with a twin who keeps the same bag on
    one account and joins as one raider."""
    ev, joins, _ = community(8, lambda d: 0.9)(d, w, rnd)
    if d == 0:
        ev += [("buy", "farmer", 10), ("buy", "twin", 10)]
    if d == 10:
        ev += [("transfer", ("farmer", f"syb{i}"), 1 / (12 - i)) for i in range(12)]
    joins += ["twin"] + ([f"syb{i}" for i in range(12)] if d > 10 else ["farmer"])
    return ev, joins, []


SCENARIOS = {"engaged": engaged, "fading": fading, "gamblers": gamblers, "whale_carry": whale_carry,
             "sybil_farm": sybil_farm}


def run(scenario, raid=RAID, seed=11):
    rnd = random.Random(seed)
    w = Raids(BASE, raid)
    rows = []
    for d in range(DAYS):
        if d:
            w.new_day(d)
        ev, joins, ticket_buys = scenario(d, w, rnd)
        if d == 1:
            w.bag0 = {a: w.balance(a) for a in ("farmer", "twin")}
        for kind, who, x in ev:
            if kind == "buy":
                w.buy(who, int(x * NEAR))
            elif kind == "sell":
                w.sell(who, min(w.balance(who), int(w.balance(who) * x)))
            elif kind == "transfer":
                a, b = who
                amt = min(w.balance(a), int(w.balance(a) * x))
                if amt:
                    w.transfer(a, b, amt)
            w.buyback()
        for who in joins:
            w.join(who)
        n_joined = len(w.joined)
        # day 0 is launch day: nobody holds seasoned tokens yet, the first raid is day 1 (as in the contract)
        ok = w.settle(rnd) if d else False
        for who in ticket_buys:
            w.buy_ticket(who)
        for who in list(w.tickets):
            while w.tickets[who]:
                w.tickets[who] -= 1
                w.pull(who, rnd)
        rows.append(dict(day=d, circ=w.circ / TOKEN, minted=w.minted / TOKEN, burned=w.burned / TOKEN,
                         burn_fail=w.burn_fail / TOKEN, burn_tickets=w.burn_tickets / TOKEN,
                         chest=w.chest / NEAR, paid=w.paid_out / NEAR, streak=w.streak, joined=n_joined, ok=ok,
                         price=w.price() / NEAR * 1e6))
    return w, rows


def attack_costs(w_final_rows):
    """Back-of-the-envelope costs of the two attacks, with the launch parameters."""
    lines = []
    storage = 0.015
    lines.append(f"- Sybil head count: each fake raider costs a {storage} NEAR storage deposit, must hold "
                 f"{RAID['min_join_balance'] // TOKEN:,} seasoned tokens and waits one day after every transfer "
                 f"(a transfer opens a level-0 lot). Mints are proportional to balance, so splitting a bag earns "
                 f"exactly what the whole bag earns: the only gain is reaching min_joiners.")
    lines.append("- Cheap tickets by pumping: tickets are priced at min(spot, yesterday's high). Pumping today raises "
                 "spot but not yesterday's high, so the ticket costs the same number of tokens; the pump itself pays "
                 f"the {BASE['fee_bps'] / 100:.0f}% fee in and out.")
    return lines


def main():
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    here = os.path.dirname(os.path.abspath(__file__))
    results = {name: run(fn) for name, fn in SCENARIOS.items()}

    fig, axes = plt.subplots(2, 2, figsize=(12, 8), constrained_layout=True)
    for ax, (key, title, unit) in zip(axes.flat, [
        ("circ", "Circulating supply", "BAND"), ("minted", "Minted to raiders (cumulative)", "BAND"),
        ("chest", "Raid chest", "NEAR"), ("streak", "Raid streak", "days"),
    ]):
        for name, (_, rows) in results.items():
            ax.plot([r["day"] for r in rows], [r[key] for r in rows], label=name)
        ax.set_title(title)
        ax.set_xlabel("day")
        ax.set_ylabel(unit)
        ax.grid(alpha=0.3)
    axes.flat[0].legend()
    fig.savefig(os.path.join(here, "raids.png"), dpi=110)

    lines = ["| Scenario | Raids won / lost | Best streak | Minted | Burned (all) | of which fail burns | "
             "of which tickets | Net supply change | Chest day 60 (NEAR) | Paid by pulls (NEAR) | Legendary pulls |",
             "|---|---|---|---|---|---|---|---|---|---|---|"]
    for name, (w, rows) in results.items():
        r = rows[-1]
        net = r["minted"] - r["burned"]
        lines.append(f"| {name} | {w.wins} / {w.losses} | {w.best} | {r['minted']:,.0f} | {r['burned']:,.0f} | "
                     f"{r['burn_fail']:,.0f} | {r['burn_tickets']:,.0f} | {net:+,.0f} | {r['chest']:.2f} | "
                     f"{r['paid']:.2f} | {w.pulls['legendary']} |")

    w, rows = results["sybil_farm"]
    farm = sum(w.won.get(f"syb{i}", 0) for i in range(12)) + w.won.get("farmer", 0)
    farm_bal = sum(w.balance(f"syb{i}") for i in range(12)) + w.balance("farmer")
    # the two bags differ in size (the farmer bought first, lower on the curve): compare per token held at day 0
    f0, t0 = w.bag0["farmer"], w.bag0["twin"]
    lines += ["", "sybil_farm, day 60:",
              f"- farmer (bag split across 12 accounts on day 10): bag x{farm_bal / f0:.3f}, "
              f"{farm / NEAR / (f0 / TOKEN / 1e8):.3f} NEAR won per 100M tokens bought, 0.18 NEAR of storage paid",
              f"- twin (same size of bag, one account): bag x{w.balance('twin') / t0:.3f}, "
              f"{w.won.get('twin', 0) / NEAR / (t0 / TOKEN / 1e8):.3f} NEAR won per 100M tokens bought",
              f"- raids won before the split (days 1-10): {sum(r['ok'] for r in rows[1:11])}, "
              f"after: {sum(r['ok'] for r in rows[11:])}"]
    farm_avg, twin_avg = [], []
    for seed in range(40):
        wx, _ = run(sybil_farm, seed=seed)
        fw = sum(wx.won.get(f"syb{i}", 0) for i in range(12)) + wx.won.get("farmer", 0)
        farm_avg.append(fw / NEAR / (wx.bag0["farmer"] / TOKEN / 1e8))
        twin_avg.append(wx.won.get("twin", 0) / NEAR / (wx.bag0["twin"] / TOKEN / 1e8))
    lines.append(f"- over 40 seeds, NEAR won per 100M tokens bought: farmer {sum(farm_avg) / 40:.3f}, "
                 f"twin {sum(twin_avg) / 40:.3f} (pulls are drawn by balance, so splitting wins nothing)")

    lines += ["", "Parameter sweep (net supply change over 60 days, minted minus burned, in BAND):", "",
              "| mint_bps | target_bps | " + " | ".join(SCENARIOS) + " |",
              "|---|---|" + "---|" * len(SCENARIOS)]
    for mint in (5, 10, 20):
        for target in (1500, 2500, 4000):
            raid = dict(RAID, mint_bps=mint, target_bps=target)
            cells = []
            for fn in SCENARIOS.values():
                wx, rx = run(fn, raid)
                cells.append(f"{rx[-1]['minted'] - rx[-1]['burned']:+,.0f} ({wx.wins}W)")
            lines.append(f"| {mint} | {target} | " + " | ".join(cells) + " |")

    lines += ["", "Attacks:"] + attack_costs(rows)
    open(os.path.join(here, "raids_results.md"), "w", encoding="utf-8").write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
