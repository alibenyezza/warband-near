// The keeper agent's job: keep the war chest's rules running. Each round it
//   1. calls `buyback` when the contract says the price is under the floor,
//   2. pushes level-weighted vote weight to the DAO for holders whose weight changed (levels grow with time),
//   3. flushes the DAO's share of fees once 0.1 NEAR is pending.
// It cannot do anything else: its key only allows these three methods on the Warband contract, with no deposit.
// Usage: npm run guardian            (one round)
//        npm run guardian -- --watch (every 60 s)
import { teraToGas } from "near-api-js";
import { agentAccount, log, reason, view, WARBAND, type Holder, type Market } from "./near.js";

const agent = agentAccount();

async function call(methodName: string, args: Record<string, unknown> = {}) {
  try {
    const tx = await agent.callFunction({ contractId: WARBAND, methodName, args, gas: teraToGas("60") });
    log("ok", { method: methodName, args, result: tx ?? null });
    return true;
  } catch (e) {
    log("refused", { method: methodName, args, reason: reason(e) });
    return false;
  }
}

async function round() {
  const m = await view<Market>("market");
  log("market", { day: m.day_index, price: m.price, floor: m.floor_price, buyback_open: m.buyback_open });
  if (m.buyback_open) await call("buyback");

  for (let from = 0; ; from += 50) {
    const page = await view<Holder[]>("holders", { from_index: from, limit: 50 });
    for (const h of page) {
      if (h.dao_registered && h.weight !== h.delegated) await call("sync_weight", { account_id: h.account_id });
    }
    if (page.length < 50) break;
  }

  if (BigInt(m.dao_pending) >= 10n ** 23n) await call("flush_to_dao");
}

await round();
if (process.argv.includes("--watch")) setInterval(() => round().catch((e) => log("error", { reason: reason(e) })), 60_000);
