// Live view of the Warband contract and its DAO on NEAR testnet, over plain JSON-RPC (no wallet, no backend).
const params = new URLSearchParams(location.search);
// public RPCs rate-limit: on an error or a slow answer, the next one in the list is tried
const RPCS = [...new Set([params.get("rpc") ?? "https://test.rpc.fastnear.com", "https://near-testnet.drpc.org", "https://rpc.testnet.fastnear.com"])];
// Three deployments of the same contract (README, "Deployments"): v3 is the live one, with launch parameters.
const DEPLOYMENTS = {
  "warband-daily.testnet": { dao: "dao-daily.warband-hq.testnet", board: "warband-dailyboard.testnet", name: "v3 · daily raids" },
  "warband-raids.testnet": { dao: "dao-raids.warband-hq.testnet", board: "warband-bountyboard.testnet", name: "v2 · 3-minute days" },
  "warband-band.testnet": { dao: "dao.warband-hq.testnet", board: "warband-bountyboard.testnet", name: "v1 · levels only" },
};
const TOKEN = params.get("token") ?? "warband-daily.testnet";
const DEPLOY = DEPLOYMENTS[TOKEN] ?? { dao: "dao-daily.warband-hq.testnet", board: "warband-dailyboard.testnet", name: "custom" };
const DAO = params.get("dao") ?? DEPLOY.dao;
const REFRESH_MS = Number(params.get("refresh") ?? 4000);
const RANKS = ["Recruit", "Soldier", "Corporal", "Sergeant", "Lieutenant", "Captain", "Commander", "Colonel", "General", "Marshal", "Warlord"];
const TIERS = [
  { name: "common", odds: 8500, pay: 5 },
  { name: "rare", odds: 1200, pay: 50 },
  { name: "epic", odds: 270, pay: 300 },
  { name: "legendary", odds: 30, pay: 1500 },
];

const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

async function rpc(params) {
  const body = JSON.stringify({ jsonrpc: "2.0", id: 1, method: "query", params: { finality: "optimistic", ...params } });
  let failure;
  for (const url of RPCS) {
    let d;
    try {
      const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body, signal: AbortSignal.timeout(6000) });
      if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
      d = await r.json();
    } catch (e) {
      failure = e; // rate limit, timeout or network: try the next RPC
      continue;
    }
    // a contract error (unknown account, panic in a view) is the same on every RPC
    if (d.error || d.result?.error) throw new Error(JSON.stringify(d.error ?? d.result.error));
    return d.result;
  }
  throw failure;
}

async function view(contract, method, args = {}) {
  const res = await rpc({ request_type: "call_function", account_id: contract, method_name: method, args_base64: btoa(JSON.stringify(args)) });
  return JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
}

// Units: NEAR has 24 decimals, BAND 18. Prices are yoctoNEAR per whole BAND.
const big = (s) => BigInt(s);
function units(s, decimals, digits = 2) {
  const v = big(s);
  const base = 10n ** BigInt(decimals);
  const whole = v / base;
  const frac = (v % base).toString().padStart(decimals, "0").slice(0, digits);
  return `${whole.toLocaleString("en-US")}${digits ? "." + frac : ""}`;
}
// small amounts (loot from a thin chest) get the digits they need instead of showing 0.000
const near = (s, d = 3) => {
  const v = big(s);
  while (d < 8 && v > 0n && v < 10n ** BigInt(24 - d)) d++;
  return units(s, 24, d);
};
const band = (s) => units(s, 18, 0);
const price = (s) => (Number(big(s)) / 1e24 * 1e6).toFixed(4); // µNEAR per BAND
const ago = (ns) => {
  const s = Math.max(0, Math.round((Date.now() - Number(big(ns) / 1_000_000n)) / 1000));
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`;
};
const cell = (label, value, sub = "") => `<div class="cell"><span>${label}</span><strong>${value}</strong>${sub ? `<em>${sub}</em>` : ""}</div>`;
// "archer.warband-camp.testnet" -> <b>archer</b><i>.warband-camp.testnet</i>
const who = (id) => {
  const i = id.indexOf(".");
  return `<span class="who"><b>${esc(i > 0 ? id.slice(0, i) : id)}</b>${i > 0 ? `<i>${esc(id.slice(i))}</i>` : ""}</span>`;
};

// ------------------------------------------------------------------ badges (art drawn here, metadata on chain)
const BADGE_ART = {
  "First Blood": `<path d="M32 14c6 9 11 15 11 21a11 11 0 0 1-22 0c0-6 5-12 11-21z" fill="var(--wax-hi)"/>`,
  "Seven Days": [0, 1, 2, 3, 4, 5, 6].map((i) => `<rect x="${17 + i * 4.6}" y="22" width="2.4" height="20" rx="1" fill="currentColor"/>`).join("") +
    `<path d="M15 40 L49 24" stroke="var(--wax-hi)" stroke-width="2.6" stroke-linecap="round"/>`,
  "Legendary Pull": `<path d="M32 15l4.6 10.4 11.4 1.1-8.6 7.6 2.5 11.1L32 39.4l-9.9 5.8 2.5-11.1-8.6-7.6 11.4-1.1z" fill="var(--gold)"/>`,
  Warlord: `<path d="M18 42h28l3-17-9 7-8-12-8 12-9-7z" fill="var(--gold)"/><rect x="18" y="44" width="28" height="4" rx="1.5" fill="var(--gold)"/>`,
};
const BADGE_ORDER = ["First Blood", "Seven Days", "Legendary Pull", "Warlord"];
const BADGE_HOW = {
  "First Blood": "first won raid",
  "Seven Days": "a won raid on a streak of 7+",
  "Legendary Pull": "a legendary pull",
  Warlord: "a lot held to level 10",
};
const badgeSvg = (title, size = 64) =>
  `<svg class="seal" viewBox="0 0 64 64" width="${size}" height="${size}" aria-hidden="true"><circle cx="32" cy="32" r="29" fill="var(--seal)" stroke="currentColor" stroke-opacity=".25" stroke-width="1.5"/><circle cx="32" cy="32" r="24" fill="none" stroke="currentColor" stroke-opacity=".18" stroke-dasharray="2 3"/>${BADGE_ART[title] ?? ""}</svg>`;

let badges = []; // every badge, refreshed with the page
async function loadBadges() {
  const all = [];
  for (let from = 0; ; from += 100) {
    const page = await view(TOKEN, "nft_tokens", { from_index: String(from), limit: 100 });
    all.push(...page);
    if (page.length < 100) break;
  }
  badges = all;
  $("#badge-wall").innerHTML = BADGE_ORDER.map((t) => {
    const owners = all.filter((b) => b.metadata.title === t);
    return `<div class="badge-card ${owners.length ? "" : "none"}">
      ${badgeSvg(t, 72)}
      <h3>${esc(t)}</h3><p>${esc(BADGE_HOW[t])}</p>
      <strong>${owners.length}</strong><span>minted</span>
      <div class="owners">${owners.slice(-8).map((b) => `<span>${esc(b.owner_id.split(".")[0])}</span>`).join("")}</div>
    </div>`;
  }).join("");
}

// ------------------------------------------------------------------ market and raid
let cfg = null;
let raid = null;

async function market() {
  const m = await view(TOKEN, "market");
  cfg ??= await view(TOKEN, "config");
  $("#facts").innerHTML = [
    `<span class="chip">1 day = <b>${cfg.day_seconds >= 86400 ? cfg.day_seconds / 3600 + " h" : cfg.day_seconds / 60 + " min"}</b>${cfg.day_seconds >= 86400 ? " (launch setting)" : " on testnet"}</span>`,
    `<span class="chip">fee <b>${cfg.fee_bps / 100}%</b>, ${cfg.raid ? cfg.raid.chest_share_bps / 100 + "% of it to the raid chest" : cfg.dao_share_bps / 100 + "% to the DAO"}</span>`,
    `<span class="chip">day <b>${m.day_index}</b></span>`,
    `<span class="chip">holders <b>${m.holders}</b></span>`,
    `<span class="chip ${m.buyback_open ? "open" : ""}">buyback <b>${m.buyback_open ? "open" : "closed"}</b></span>`,
  ].join("");
  $("#market-grid").innerHTML = [
    cell("Price", price(m.price), "µNEAR per BAND"),
    cell("Floor", price(m.floor_price), `20% under the recent high (${price(m.reference_high)})`),
    cell("Circulating", band(m.circulating), "BAND"),
    cell("Burned", band(m.burned), "BAND, by buybacks, lost raids and pulls"),
    cell("Buyback reserve", near(m.buyback_reserve), `NEAR · spent today ${near(m.buyback_spent_today)} / ${near(m.buyback_cap_today)}`),
    cell("Curve NEAR", near(m.real_near), "real NEAR backing the curve"),
  ].join("");
  return m;
}

async function raidStatus() {
  raid = await view(TOKEN, "raid_status");
  const r = raid;
  const target = big(r.target);
  const joined = big(r.joined);
  $("#raid-title").textContent = r.raid === "0" ? "Launch day: lots season, raid #1 opens tomorrow" : `Raid #${r.raid}`;
  // the bar spans 160% of the target so the goal mark sits in view, and overflows to full when it is passed
  const span = target > 0n ? (target * 16n) / 10n : 1n;
  const pct = target > 0n ? Math.min(100, Number((joined * 1000n) / span) / 10) : 0;
  $("#raid-fill").style.width = `${pct}%`;
  $("#raid-fill").classList.toggle("met", target > 0n && joined >= target);
  $("#raid-goal").style.left = `${100 / 1.6}%`;
  $("#raid-joined").innerHTML = `<b>${band(r.joined)}</b> BAND joined${target > 0n ? ` · ${Number((joined * 1000n) / target) / 10}% of target` : ""}`;
  $("#raid-target").textContent = target > 0n ? `target ${band(r.target)} (25% of supply)` : "target set by the first raider";
  const slots = Math.max(r.min_joiners, r.joiners);
  $("#raid-pips").innerHTML =
    Array.from({ length: slots }, (_, i) => `<i class="${i < r.joiners ? "on" : ""}"></i>`).join("") +
    `<span>${r.joiners} / ${r.min_joiners} raiders</span>`;
  const bonus = 1 + Math.min(r.streak + 1, 10) / 10;
  $("#raid-grid").innerHTML = [
    cell("Streak", `${r.streak}`, `win today: mint x${bonus.toFixed(1)} · best ${r.best_streak}`),
    cell("Record", `${r.won} – ${r.lost}`, "raids won – lost"),
    cell("Chest", near(r.chest, 4), "NEAR, 30% of every fee"),
    cell("Extra pull", band(r.ticket_tokens), "BAND burned per pull"),
    cell("Minted by raids", band(r.minted), "BAND"),
    cell("Burned by lost raids", recordBurned === null ? "–" : band(recordBurned.toString()), `BAND, last ${RECORD} raids`),
  ].join("");
  $("#scoreboard").innerHTML = [
    `<div><span>win streak</span><strong>${r.streak}</strong><em>best ${r.best_streak} · record ${r.won}–${r.lost}</em></div>`,
    `<div><span>raid chest</span><strong>${near(r.chest, 3)}</strong><em>NEAR, shared by the next won raid</em></div>`,
    `<div><span>${r.raid === "0" ? "launch day · first raid opens in" : `raid #${r.raid} ends in`}</span><strong id="score-clock">–</strong><em>${r.joiners} / ${r.min_joiners} raiders in</em></div>`,
  ].join("");
  const last = r.last;
  const banner = $("#raid-last");
  if (last && last.joiners > 0) {
    banner.hidden = false;
    banner.className = `last ${last.won === true ? "won" : last.won === false ? "lost" : "pending"}`;
    banner.innerHTML =
      last.won === true
        ? `<b>Raid #${Number(r.raid) - 1} won</b> by ${last.joiners} raiders · streak ${last.streak} · claim your loot`
        : last.won === false
          ? `<b>Raid #${Number(r.raid) - 1} lost</b> · ${last.joiners} raiders showed up · the chest bought back and burned ${band(last.burned)} BAND`
          : `<b>Raid #${Number(r.raid) - 1} is over</b> · ${last.joiners} raiders · settled by the next action`;
  } else if (Number(r.raid) > 1) {
    banner.hidden = false;
    banner.className = "last lost";
    banner.innerHTML = `<b>Raid #${Number(r.raid) - 1}</b> · nobody showed up · lost`;
  } else banner.hidden = true;
  tickClock();
}

function tickClock() {
  if (!raid) return;
  const s = Math.max(0, Math.floor((Number(big(raid.ends_at) / 1_000_000n) - Date.now()) / 1000));
  const hms = s >= 3600 ? `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}` : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  $("#raid-clock").textContent = hms;
  const sc = $("#score-clock");
  if (sc) sc.textContent = s === 0 ? "settling" : hms;
  $("#raid-clock").classList.toggle("hot", s <= 30);
  if (s === 0) $("#raid-clock").textContent = "settling";
}

// ------------------------------------------------------------------ the log, the loot feed and the toasts
let seen = null;
const feedLine = (e) => {
  const k = e.kind;
  if (k === "raid_join") return { cls: "join", text: `${who(e.account)} joined the raid with <b>${band(e.tokens)}</b> BAND` };
  if (k === "raid_won") return { cls: "won", text: `<b>Raid won.</b> The band showed up.` };
  if (k === "raid_lost") return { cls: "lost", text: e.count ? `<b>${e.count} raids lost in a row.</b> Nobody showed up; the chest bought back and burned <b>${band(e.tokens)}</b> BAND.` : `<b>Raid lost.</b> The chest bought back and burned <b>${band(e.tokens)}</b> BAND.` };
  if (k === "raid_claim") return { cls: "claim", text: `${who(e.account)} claimed <b>+${band(e.tokens)}</b> BAND and <b>${near(e.near, 4)}</b> NEAR of loot` };
  if (k.startsWith("loot_")) {
    const t = k.slice(5);
    return { cls: t, text: `${who(e.account)} pulled <b class="tier ${t}">${t.toUpperCase()}</b> · <b>${near(e.near, 4)}</b> NEAR`, toast: true };
  }
  if (k.startsWith("pull_")) {
    const t = k.slice(5);
    return { cls: t, text: `${who(e.account)} burned <b>${band(e.tokens)}</b> BAND for a pull: <b class="tier ${t}">${t}</b> · ${near(e.near, 4)} NEAR`, toast: t === "epic" || t === "legendary" };
  }
  if (k === "buy") return { cls: "buy", text: `${who(e.account)} bought <b>${band(e.tokens)}</b> BAND for ${near(e.near)} NEAR` };
  if (k === "sell") return { cls: "sell", text: `${who(e.account)} sold <b>${band(e.tokens)}</b> BAND` };
  return null;
};

function toast(html, cls) {
  const t = $("#toast");
  t.className = `toast ${cls}`;
  t.innerHTML = html;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), 6000);
}

// Runs of lost raids settled one after the other (nobody showed up for days) fold into one row.
function fold(h) {
  const out = [];
  for (const e of h) {
    const prev = out[out.length - 1];
    if (e.kind === "raid_lost" && prev?.kind === "raid_lost") {
      prev.count = (prev.count ?? 1) + 1;
      prev.tokens = (big(prev.tokens) + big(e.tokens)).toString();
      prev.first = e.n;
    } else out.push({ ...e });
  }
  return out;
}

async function history() {
  const h = await view(TOKEN, "history", { limit: 100 });
  const folded = fold(h);
  $("#history tbody").innerHTML = folded
    .slice(0, 30)
    .map((e) => `<tr><td>${e.n}</td><td>${ago(e.at)}</td><td><span class="tag ${esc(e.kind)}">${esc(e.kind)}</span>${e.count ? ` <span class="muted">x${e.count} in a row</span>` : ""}</td><td>${esc(e.account)}</td><td class="num">${big(e.near) ? near(e.near, 4) : ""}</td><td class="num">${big(e.tokens) ? band(e.tokens) : ""}</td></tr>`)
    .join("");
  const lines = folded.map((e) => ({ e, l: feedLine(e) })).filter((x) => x.l);
  const newest = h.length ? Number(h[0].n) : -1;
  $("#feed").innerHTML = lines.length
    ? lines
        .slice(0, 24)
        .map(({ e, l }) => `<li class="${l.cls} ${seen !== null && Number(e.n) > seen ? "new" : ""}"><span class="when">${ago(e.at)}</span><span>${l.text}</span></li>`)
        .join("")
    : `<li class="muted">Waiting for the first raid…</li>`;
  // who joined today's raid: the log lines since the day started
  if (raid && cfg) {
    const start = big(raid.ends_at) - BigInt(cfg.day_seconds) * 1_000_000_000n;
    const today = h.filter((e) => e.kind === "raid_join" && big(e.at) >= start);
    $("#raid-joiners").innerHTML = today.length
      ? today.map((e) => `<span class="chip">${who(e.account)} <em>${band(e.tokens)}</em></span>`).join("")
      : `<span class="muted">No raider has checked in yet today.</span>`;
  }
  if (seen !== null) {
    const fresh = h.filter((e) => Number(e.n) > seen).reverse();
    for (const e of fresh) {
      const l = feedLine(e);
      if (e.kind === "raid_won") toast(`<b>RAID WON</b><span>the band showed up</span>`, "won");
      else if (e.kind === "raid_lost") toast(`<b>RAID LOST</b><span>${band(e.tokens)} BAND burned</span>`, "lost");
      else if (l?.toast) toast(`<b>${e.kind.split("_")[1].toUpperCase()} PULL</b><span>${esc(e.account.split(".")[0])} · ${near(e.near, 4)} NEAR</span>`, e.kind.split("_")[1]);
    }
  }
  seen = newest;
}

// Every raid is kept by the contract (`raid(id)`): the record of the last 14, and what lost raids burned.
const RECORD = 14;
let recordBurned = null;
async function raidRecord() {
  if (!raid) return;
  const current = Number(raid.raid);
  const ids = [];
  for (let id = current - 1; id >= 1 && ids.length < RECORD; id--) ids.push(id);
  const rows = await Promise.all(ids.map((id) => view(TOKEN, "raid", { raid: String(id) }).then((r) => [id, r]).catch(() => [id, undefined])));
  $("#raids tbody").innerHTML = rows.length
    ? rows
        .map(([id, r]) => {
          if (r === undefined) return `<tr><td>#${id}</td><td class="muted" colspan="5">could not be read</td></tr>`;
          if (r === null) return `<tr><td>#${id}</td><td class="lost">lost</td><td class="num">0</td><td class="num"></td><td class="num">0</td><td class="num muted">nobody showed up</td></tr>`;
          const result = r.won === true ? '<span class="won">won</span>' : r.won === false ? '<span class="lost">lost</span>' : '<span class="muted">not settled yet</span>';
          return `<tr><td>#${id}</td><td>${result}</td><td class="num">${r.joiners}</td><td class="num">${band(r.joined)}</td><td class="num">${r.streak}</td><td class="num">${big(r.burned) ? band(r.burned) : ""}</td></tr>`;
        })
        .join("")
    : `<tr><td class="muted" colspan="6">The first raid opens on day 1.</td></tr>`;
  recordBurned = rows.reduce((a, [, r]) => a + (r ? big(r.burned) : 0n), 0n);
  const cells = document.querySelectorAll("#raid-grid .cell");
  if (cells[5]) cells[5].innerHTML = `<span>Burned by lost raids</span><strong>${band(recordBurned.toString())}</strong><em>BAND, last ${RECORD} raids</em>`;
}

function renderTiers(odds) {
  const chest = raid ? big(raid.chest) : 0n;
  $("#tiers").innerHTML = TIERS.map(
    (t, i) => `<div class="tier-card ${t.name}"><span>${t.name}</span><strong>${((odds?.[i] ?? t.odds) / 100).toFixed(2)}%</strong>
      <em>pays ${t.pay / 100}% of the chest${chest ? ` · ${near(((chest * BigInt(t.pay)) / 10000n).toString(), 4)} NEAR now` : ""}</em></div>`,
  ).join("");
}

// ------------------------------------------------------------------ one account
async function account(id) {
  const [a, joined, odds, mine] = await Promise.all([
    view(TOKEN, "account", { account_id: id }).catch(() => null),
    view(TOKEN, "raid_of", { account_id: id }).catch(() => null),
    view(TOKEN, "pull_odds", { account_id: id }).catch(() => null),
    view(TOKEN, "nft_tokens_for_owner", { account_id: id }).catch(() => []),
  ]);
  renderTiers(odds);
  if (!a) {
    $("#account-view").innerHTML = `<p class="muted">${esc(id)} holds no BAND (or is not registered).</p>`;
    return;
  }
  const max = a.lots.reduce((m, l) => (big(l.amount) > m ? big(l.amount) : m), 1n);
  const seasoned = a.lots.filter((l) => l.level >= 1).reduce((s, l) => s + big(l.amount), 0n);
  const raidNow = raid ? Number(raid.raid) : null;
  const status = !joined
    ? seasoned >= big(cfg?.raid?.min_join_balance ?? "0")
      ? "not in today's raid · can join"
      : "needs 1,000,000 BAND held one day"
    : Number(joined.raid) === raidNow
      ? `in raid #${joined.raid} with ${band(joined.amount)} BAND`
      : `raid #${joined.raid} is over · loot to claim`;
  $("#account-view").innerHTML = `
    <div class="grid">
      ${cell("Rank", `<img class="badge" src="ranks/${String(a.top_level).padStart(2, "0")}-${RANKS[a.top_level].toLowerCase()}.svg" alt="">${esc(a.rank)}`, `top lot level ${a.top_level} of 10`)}
      ${cell("Balance", band(a.balance), `BAND · seasoned ${band(seasoned.toString())}`)}
      ${cell("Raid", joined ? "checked in" : "–", esc(status))}
      ${cell("Vote weight", band(a.weight), "Σ lot × level")}
    </div>
    <div class="mine">${mine.length ? mine.map((b) => `<span class="mine-badge" title="${esc(b.metadata.description)}">${badgeSvg(b.metadata.title, 40)}${esc(b.metadata.title)}</span>`).join("") : `<span class="muted">No badge yet.</span>`}</div>
    <div class="lots">${a.lots
      .map((l) => `<div class="lot"><span>L${l.level} ${esc(l.rank)}</span><div class="bar"><i style="width:${Number((big(l.amount) * 100n) / max)}%"></i></div><span>${band(l.amount)}</span></div>`)
      .join("")}</div>
    <p class="note">Lots are listed oldest first. A transfer or a sale takes from the bottom (youngest) first. The loot odds
      above are this account's: its top level plus two per badge raise epic and legendary odds.</p>`;
}

async function holders() {
  const all = [];
  for (let from = 0; ; from += 100) {
    const page = await view(TOKEN, "holders", { from_index: from, limit: 100 });
    all.push(...page);
    if (page.length < 100) break;
  }
  all.sort((a, b) => (big(b.weight) > big(a.weight) ? 1 : big(b.weight) < big(a.weight) ? -1 : 0));
  $("#holders tbody").innerHTML = all
    .map((h, i) => {
      const mine = badges.filter((b) => b.owner_id === h.account_id);
      return `<tr><td>${i + 1}</td><td>${esc(h.account_id)}</td><td>${esc(h.rank)}</td><td class="mini">${mine.map((b) => badgeSvg(b.metadata.title, 20)).join("")}</td><td class="num">${band(h.balance)}</td><td class="num">${band(h.weight)}</td><td class="num">${band(h.delegated)}</td></tr>`;
    })
    .join("");
}

async function dao() {
  const [acct, total, last] = await Promise.all([
    rpc({ request_type: "view_account", account_id: DAO }),
    view(DAO, "delegation_total_supply"),
    view(DAO, "get_last_proposal_id"),
  ]);
  const props = last ? await view(DAO, "get_proposals", { from_index: Math.max(0, last - 20), limit: 20 }) : [];
  $("#dao-grid").innerHTML = [
    cell("DAO account", esc(DAO), "Sputnik DAO v2, release 3.0.0"),
    cell("Treasury", near(acct.amount), "NEAR (includes the DAO's storage)"),
    cell("Delegated weight", band(total), "level-weighted, pushed by Warband"),
    cell("Proposals", String(last), "token-weighted for transfers and bounties"),
  ].join("");
  $("#proposals tbody").innerHTML = props
    .reverse()
    .map((p) => {
      const v = p.vote_counts.holders ?? p.vote_counts.council ?? ["0", "0", "0"];
      const kind = typeof p.kind === "string" ? p.kind : Object.keys(p.kind)[0];
      return `<tr><td>${p.id}</td><td>${esc(kind)}</td><td class="desc">${esc(p.description)}</td><td><span class="tag ${esc(p.status)}">${esc(p.status)}</span></td><td class="num">${p.vote_counts.holders ? band(v[0]) : v[0]}</td><td class="num">${p.vote_counts.holders ? band(v[1]) : v[1]}</td></tr>`;
    })
    .join("");
}

// The bounty board: payouts from the war chest, tiered by each contributor's record (near/contracts/bounties).
const BOARD = params.get("board") ?? DEPLOY.board;
async function board() {
  const [cfg, list] = await Promise.all([view(BOARD, "config"), view(BOARD, "bounties", { limit: 100 })]);
  const t = cfg.tiers;
  $("#board-rules").textContent =
    `Each paid bounty adds ${near(t.trust_step, 2)} NEAR to what a contributor can be paid on the poster's word alone (up to ${near(t.auto_cap, 2)}). ` +
    `Above that and up to ${near(t.council_cap, 2)} NEAR, one member of the council of ${cfg.dao} approves. Above, the board files a normal DAO proposal.`;
  const people = [...new Set(list.map((b) => b.contributor).filter(Boolean))];
  const records = await Promise.all(people.map((a) => view(BOARD, "record_of", { account_id: a })));
  const paid = list.filter((b) => b.status === "Paid");
  $("#board-grid").innerHTML = [
    cell("Board", esc(BOARD), `treasury: ${esc(cfg.dao)}`),
    cell("Petty cash", near(cfg.petty_cash, 3), "NEAR voted by the DAO"),
    cell("Open", String(list.filter((b) => ["Open", "Claimed", "Submitted", "AwaitingCouncil", "Proposed"].includes(b.status)).length), "bounties in progress"),
    cell("Paid", near(paid.reduce((s, b) => s + big(b.amount), 0n), 3), `NEAR over ${paid.length} bounties`),
  ].join("");
  $("#board tbody").innerHTML = list
    .slice()
    .reverse()
    .map((b) => `<tr><td>${b.id}</td><td class="desc">${esc(b.title)}<br><span class="muted">by ${esc(b.poster)}${b.dao_proposal ? ` · DAO proposal ${b.dao_proposal}` : ""}</span></td><td class="num">${near(b.amount, 3)}</td><td><span class="tag ${esc(b.status)}">${esc(b.status)}</span></td><td>${esc(b.tier ?? "–")}</td><td>${esc(b.contributor ?? "–")}</td><td>${esc(b.approved_by ?? "–")}</td></tr>`)
    .join("");
  $("#records tbody").innerHTML = records
    .sort((a, b) => b.finished - a.finished)
    .map((r) => `<tr><td>${esc(r.account_id)}</td><td class="num">${r.finished}</td><td class="num">${near(r.total_paid, 3)}</td><td class="num">${near(r.trust_limit, 3)}</td><td class="desc">${r.points.map((p) => `#${p.bounty} ${esc(p.tier)} (${esc(p.approved_by.split(".")[0])})`).join(", ")}</td></tr>`)
    .join("");
}

// The raid, the market and the log refresh every tick; badges, the band, the DAO, the bounty board and the account
// every fifth one, to stay under public RPC rate limits.
let tick = 0;
async function refresh() {
  const slow = tick++ % 5 === 0;
  const status = raidStatus();
  await Promise.allSettled([market(), status.then(history), ...(slow ? [loadBadges().then(holders), dao(), board(), status.then(raidRecord)] : [])]);
  if (slow) await account($("#account").value.trim()).catch(() => {});
}

$("#token-id").textContent = TOKEN;
$("#deployments").innerHTML = Object.entries(DEPLOYMENTS)
  .map(([id, d]) => (id === TOKEN ? `<b>${esc(d.name)}</b>` : `<a href="?token=${id}">${esc(d.name)}</a>`))
  .join(" · ");
$("#lookup").addEventListener("submit", (e) => {
  e.preventDefault();
  account($("#account").value.trim());
});
if (params.get("account")) $("#account").value = params.get("account");
renderTiers(null);
refresh();
setInterval(refresh, REFRESH_MS);
setInterval(tickClock, 250);
export { RANKS, TOKEN };
