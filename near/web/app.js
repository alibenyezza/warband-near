// Read-only view of the Warband contract and its DAO on NEAR testnet, over plain JSON-RPC (no wallet, no backend).
const RPC = "https://test.rpc.fastnear.com";
const params = new URLSearchParams(location.search);
const TOKEN = params.get("token") ?? "warband-band.testnet";
const DAO = params.get("dao") ?? "dao.warband-hq.testnet";
const RANKS = ["Recruit", "Soldier", "Corporal", "Sergeant", "Lieutenant", "Captain", "Commander", "Colonel", "General", "Marshal", "Warlord"];

const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

async function rpc(params) {
  const r = await fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "query", params: { finality: "optimistic", ...params } }),
  });
  const d = await r.json();
  if (d.error || d.result?.error) throw new Error(JSON.stringify(d.error ?? d.result.error));
  return d.result;
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
const near = (s, d = 3) => units(s, 24, d);
const band = (s) => units(s, 18, 0);
const price = (s) => (Number(big(s)) / 1e24 * 1e6).toFixed(4); // µNEAR per BAND
const ago = (ns) => {
  const s = Math.max(0, Math.round((Date.now() - Number(big(ns) / 1_000_000n)) / 1000));
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`;
};
const cell = (label, value, sub = "") => `<div class="cell"><span>${label}</span><strong>${value}</strong>${sub ? `<em>${sub}</em>` : ""}</div>`;

async function market() {
  const [m, cfg] = await Promise.all([view(TOKEN, "market"), view(TOKEN, "config")]);
  $("#facts").innerHTML = [
    `<span class="chip">1 day = <b>${cfg.day_seconds / 60} min</b> on testnet</span>`,
    `<span class="chip">fee <b>${cfg.fee_bps / 100}%</b>, ${cfg.dao_share_bps / 100}% of it to the DAO</span>`,
    `<span class="chip">day <b>${m.day_index}</b></span>`,
    `<span class="chip">holders <b>${m.holders}</b></span>`,
    `<span class="chip ${m.buyback_open ? "open" : ""}">buyback <b>${m.buyback_open ? "open" : "closed"}</b></span>`,
  ].join("");
  $("#market-grid").innerHTML = [
    cell("Price", price(m.price), "µNEAR per BAND"),
    cell("Floor", price(m.floor_price), `20% under the recent high (${price(m.reference_high)})`),
    cell("Circulating", band(m.circulating), "BAND"),
    cell("Burned", band(m.burned), "BAND, by buybacks"),
    cell("Buyback reserve", near(m.buyback_reserve), `NEAR · spent today ${near(m.buyback_spent_today)} / ${near(m.buyback_cap_today)}`),
    cell("Curve NEAR", near(m.real_near), "real NEAR backing the curve"),
  ].join("");
  return m;
}

async function account(id) {
  const a = await view(TOKEN, "account", { account_id: id }).catch(() => null);
  if (!a) {
    $("#account-view").innerHTML = `<p class="muted">${esc(id)} holds no BAND (or is not registered).</p>`;
    return;
  }
  const max = a.lots.reduce((m, l) => (big(l.amount) > m ? big(l.amount) : m), 1n);
  $("#account-view").innerHTML = `
    <div class="grid">
      ${cell("Rank", `<img class="badge" src="ranks/${String(a.top_level).padStart(2, "0")}-${RANKS[a.top_level].toLowerCase()}.svg" alt="">${esc(a.rank)}`, `top lot level ${a.top_level} of 10`)}
      ${cell("Balance", band(a.balance), "BAND")}
      ${cell("Vote weight", band(a.weight), "Σ lot × level")}
      ${cell("Delegated to the DAO", band(a.delegated), a.dao_registered ? "registered delegator" : "not registered yet")}
    </div>
    <div class="lots">${a.lots
      .map((l) => `<div class="lot"><span>L${l.level} ${esc(l.rank)}</span><div class="bar"><i style="width:${Number((big(l.amount) * 100n) / max)}%"></i></div><span>${band(l.amount)}</span></div>`)
      .join("")}</div>
    <p class="note">Lots are listed oldest first. A transfer or a sale takes from the bottom (youngest) first.</p>`;
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
    .map((h, i) => `<tr><td>${i + 1}</td><td>${esc(h.account_id)}</td><td>${esc(h.rank)}</td><td class="num">${band(h.balance)}</td><td class="num">${band(h.weight)}</td><td class="num">${band(h.delegated)}</td></tr>`)
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

async function history() {
  const h = await view(TOKEN, "history", { limit: 30 });
  $("#history tbody").innerHTML = h
    .map((e) => `<tr><td>${e.n}</td><td>${ago(e.at)}</td><td><span class="tag ${esc(e.kind)}">${esc(e.kind)}</span></td><td>${esc(e.account)}</td><td class="num">${big(e.near) ? near(e.near) : ""}</td><td class="num">${big(e.tokens) ? band(e.tokens) : ""}</td></tr>`)
    .join("");
}

async function refresh() {
  await Promise.allSettled([market(), holders(), dao(), history(), account($("#account").value.trim())]);
}

$("#token-id").textContent = TOKEN;
$("#lookup").addEventListener("submit", (e) => {
  e.preventDefault();
  account($("#account").value.trim());
});
refresh();
setInterval(refresh, 20_000);
export { RANKS };
