"""60-day simulation of the Warband mechanic, using the same rules and integer math as the contract.

It does not predict a price: the community behaviour is invented (see SCENARIOS). It shows what the rules do with
that behaviour: supply burned by buybacks, war chest paid to the DAO, how much of the vote seniority holds.

    python near/sim/simulate.py          # writes near/sim/*.png and near/sim/results.md
"""
import os
import random

NEAR = 10**24
TOKEN = 10**18
CFG = dict(curve_supply=10**9 * TOKEN, virtual_near=30 * NEAR, fee_bps=1000, dao_share_bps=5000,
           floor_drawdown_bps=2000, buyback_chunk_bps=1000, buyback_daily_cap_bps=2000)
DAYS = 60
TICKS = 24  # trades are spread over 24 slots per day


def bps(x, b):
    return x * b // 10_000


class Warband:
    """Port of near/contracts/warband (market, fees, buyback rule, LIFO lots, levels)."""

    def __init__(self, cfg):
        self.c = cfg
        self.nr, self.tr = cfg["virtual_near"], cfg["curve_supply"]
        self.circ = self.burned = self.reserve = self.dao = 0
        self.lots = {}  # account -> [[amount, day_opened]]
        self.high_today, self.high_prev, self.day = self.price(), 0, 0
        self.bb_cap = self.bb_spent = 0

    def price(self):
        return self.nr * TOKEN // self.tr

    def new_day(self, d):
        self.high_prev, self.high_today, self.day = self.high_today, self.price(), d
        self.bb_spent, self.bb_cap = 0, bps(self.reserve, self.c["buyback_daily_cap_bps"])

    def fee(self, f):
        to_dao = bps(f, self.c["dao_share_bps"])
        self.dao += to_dao
        self.reserve += f - to_dao

    def credit(self, who, amount):
        lots = self.lots.setdefault(who, [])
        if lots and lots[-1][1] == self.day:
            lots[-1][0] += amount
        else:
            lots.append([amount, self.day])

    def debit(self, who, amount):
        lots = self.lots[who]
        while amount:
            take = min(amount, lots[-1][0])
            lots[-1][0] -= take
            amount -= take
            if lots[-1][0] == 0:
                lots.pop()

    def balance(self, who):
        return sum(a for a, _ in self.lots.get(who, []))

    def weight(self, who):
        return sum(a * min(10, self.day - d) for a, d in self.lots.get(who, []))

    def buy(self, who, near_in):
        f = bps(near_in, self.c["fee_bps"])
        self.fee(f)
        net = near_in - f
        out = self.tr * net // (self.nr + net)
        self.nr += net
        self.tr -= out
        self.circ += out
        self.credit(who, out)
        self.high_today = max(self.high_today, self.price())

    def sell(self, who, amount):
        amount = min(amount, self.balance(who))
        if not amount:
            return
        self.debit(who, amount)
        gross = self.nr * amount // (self.tr + amount)
        self.nr -= gross
        self.tr += amount
        self.circ -= amount
        self.fee(bps(gross, self.c["fee_bps"]))

    def transfer(self, a, b, amount):
        self.debit(a, amount)
        self.credit(b, amount)

    def floor(self):
        return bps(max(self.high_today, self.high_prev), 10_000 - self.c["floor_drawdown_bps"])

    def buyback(self):
        if self.price() >= self.floor():
            return 0
        spend = min(bps(self.reserve, self.c["buyback_chunk_bps"]), self.bb_cap - self.bb_spent, self.reserve)
        if spend <= 0:
            return 0
        out = self.tr * spend // (self.nr + spend)
        self.reserve -= spend
        self.bb_spent += spend
        self.nr += spend
        self.tr -= out
        self.burned += out
        return out


def run(scenario, seed=7):
    rnd = random.Random(seed)
    w = Warband(CFG)
    rows = []
    for d in range(DAYS):
        if d:
            w.new_day(d)
        for t in range(TICKS):
            for kind, who, x in scenario(d, t, rnd, w):
                if kind == "buy":
                    w.buy(who, int(x * NEAR))
                elif kind == "sell":
                    w.sell(who, int(w.balance(who) * x))
                elif kind == "transfer":
                    a, b = who
                    amt = int(w.balance(a) * x)
                    if amt:
                        w.transfer(a, b, amt)
            w.buyback()  # the keeper agent calls it every tick; the rule decides
        total_w = sum(w.weight(a) for a in w.lots) or 1
        veterans = sum(w.weight(a) for a in w.lots if a.startswith("vet"))
        rows.append(dict(day=d, price=w.price() / NEAR, floor=w.floor() / NEAR, circ=w.circ / TOKEN,
                         burned=w.burned / TOKEN, reserve=w.reserve / NEAR, dao=w.dao / NEAR,
                         vet_share=veterans / total_w,
                         sybil_share=sum(w.weight(a) for a in w.lots if a.startswith("syb")) / total_w,
                         solo_share=w.weight("solo") / total_w,
                         syb_level=sum(w.weight(a) for a in w.lots if a.startswith("syb") or a == "farmer")
                         / max(1, sum(w.balance(a) for a in w.lots if a.startswith("syb") or a == "farmer")),
                         solo_level=w.weight("solo") / max(1, w.balance("solo"))))
    return rows


def steady(d, t, rnd, w):
    """A community that keeps buying a little and selling a little; 20 veterans buy on day 0 and never sell."""
    ev = []
    if d == 0 and t == 0:
        ev += [("buy", f"vet{i}", 0.5) for i in range(20)]
    if rnd.random() < 0.6:
        ev.append(("buy", f"new{d}_{t}", rnd.uniform(0.05, 0.4)))
    if rnd.random() < 0.35 and d > 1:
        ev.append(("sell", f"new{rnd.randrange(d)}_{rnd.randrange(TICKS)}", 1.0))
    return ev


def pump_and_dump(d, t, rnd, w):
    """Week one hype, then a whale dumps on day 10 and the crowd follows for a week."""
    ev = []
    if d == 0 and t == 0:
        ev += [("buy", f"vet{i}", 0.5) for i in range(20)] + [("buy", "whale", 25)]
    if d < 7 and rnd.random() < 0.8:
        ev.append(("buy", f"new{d}_{t}", rnd.uniform(0.2, 1.0)))
    if d == 10 and t == 3:
        ev.append(("sell", "whale", 1.0))
    if 10 <= d < 17 and rnd.random() < 0.5:
        ev.append(("sell", f"new{rnd.randrange(7)}_{rnd.randrange(TICKS)}", 1.0))
    if d >= 17 and rnd.random() < 0.3:
        ev.append(("buy", f"late{d}_{t}", rnd.uniform(0.05, 0.3)))
    return ev


def slow_bleed(d, t, rnd, w):
    """Interest fades: buyers dry up, holders leave a few at a time."""
    ev = []
    if d == 0 and t == 0:
        ev += [("buy", f"vet{i}", 0.5) for i in range(20)] + [("buy", f"new0_{i}", 1.0) for i in range(30)]
    if rnd.random() < max(0.05, 0.4 - d * 0.01):
        ev.append(("buy", f"new{d}_{t}", rnd.uniform(0.05, 0.2)))
    if rnd.random() < 0.25:
        ev.append(("sell", f"new0_{rnd.randrange(30)}", 0.5))
    return ev


def sybil(d, t, rnd, w):
    """Steady community, plus two farmers who buy 10 NEAR each on day 20. On day 25 one splits the bag across 50 fresh
    accounts to look like many voters, the other keeps it on one account."""
    ev = steady(d, t, rnd, w)
    if d == 20 and t == 0:
        ev += [("buy", "farmer", 10), ("buy", "solo", 10)]
    if d == 25 and t == 0:
        ev += [("transfer", ("farmer", f"syb{i}"), 1 / (50 - i)) for i in range(50)]
    return ev


SCENARIOS = {"steady": steady, "pump_and_dump": pump_and_dump, "slow_bleed": slow_bleed, "sybil_split": sybil}


def main():
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    here = os.path.dirname(os.path.abspath(__file__))
    results = {name: run(fn) for name, fn in SCENARIOS.items()}
    fig, axes = plt.subplots(2, 2, figsize=(12, 8), constrained_layout=True)
    for ax, (key, title, unit) in zip(axes.flat, [
        ("circ", "Circulating supply", "BAND"), ("burned", "Burned by buybacks", "BAND"),
        ("dao", "War chest paid to the DAO (cumulative)", "NEAR"), ("vet_share", "Vote share of day-0 veterans", "share"),
    ]):
        for name, rows in results.items():
            ax.plot([r["day"] for r in rows], [r[key] for r in rows], label=name)
        ax.set_title(title)
        ax.set_xlabel("day")
        ax.set_ylabel(unit)
        ax.grid(alpha=0.3)
    axes.flat[0].legend()
    fig.savefig(os.path.join(here, "supply.png"), dpi=110)

    fig, axes = plt.subplots(1, 2, figsize=(12, 4), constrained_layout=True)
    rows = results["pump_and_dump"]
    axes[0].plot([r["day"] for r in rows], [r["price"] for r in rows], label="price")
    axes[0].plot([r["day"] for r in rows], [r["floor"] for r in rows], label="floor (-20% of recent high)", ls="--")
    axes[0].set_title("pump_and_dump: price and buyback floor (NEAR per BAND)")
    axes[0].legend()
    rows = results["sybil_split"]
    axes[1].plot([r["day"] for r in rows], [r["sybil_share"] for r in rows], label="50 split accounts")
    axes[1].plot([r["day"] for r in rows], [r["solo_share"] for r in rows], label="same bag on 1 account", ls="--")
    axes[1].set_title("sybil_split: vote share (split on day 25)")
    axes[1].legend()
    for ax in axes:
        ax.set_xlabel("day")
        ax.grid(alpha=0.3)
    fig.savefig(os.path.join(here, "floor_and_sybil.png"), dpi=110)

    lines = ["| Scenario | Circulating day 60 | Burned | DAO paid (NEAR) | Buyback reserve left (NEAR) | Veterans' vote share |",
             "|---|---|---|---|---|---|"]
    for name, rows in results.items():
        r = rows[-1]
        lines.append(f"| {name} | {r['circ']:,.0f} | {r['burned']:,.0f} | {r['dao']:.2f} | {r['reserve']:.2f} | "
                     f"{r['vet_share']:.0%} |")
    syb = results["sybil_split"]
    lines.append("")
    by_day = {r["day"]: r for r in syb}
    for day in (24, 26, 40, 60 - 1):
        r = by_day[day]
        lines.append(f"- day {day}: average level of the split bag {r['syb_level']:.2f}, of the bag kept whole "
                     f"{r['solo_level']:.2f}")
    lines.insert(len(lines) - 4, "sybil_split (two 10 NEAR bags bought on day 20, one split across 50 accounts on day 25):")
    open(os.path.join(here, "results.md"), "w", encoding="utf-8").write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
