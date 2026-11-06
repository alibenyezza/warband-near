// Daily upkeep of the daily deployment, signed by the gas-key keeper (its only key: a function-call gas key on the
// Warband contract, limited to settle_raids, buyback, flush_to_dao and sync_weight).
//
//   npx tsx src/daily-keeper.ts <keychain dir>
//
// One call at a time, so the gas key's reservation (all attached gas at the max gas price) never exceeds its balance:
// settle the raids that ended, buy back if the price is under the floor, send the war chest share to the DAO once it
// is worth it, and push every registered holder's level-weighted weight to the DAO (levels rise with time alone).
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { Account, KeyPairSigner, actions, teraToGas, type KeyPairString } from "near-api-js";
import { provider } from "./near.js";

const [keychainDir] = process.argv.slice(2);
if (!keychainDir) throw new Error("usage: daily-keeper.ts <keychain dir>");
const KEEPER = process.env.GAS_KEEPER ?? "gk-daily.warband-hq.testnet";
const TOKEN = process.env.WARBAND ?? "warband-daily.testnet";
const dir = join(keychainDir, KEEPER);
const { private_key } = JSON.parse(readFileSync(join(dir, readdirSync(dir).find((f) => f.endsWith(".json"))!), "utf8"));
const keeper = new Account(KEEPER, provider, KeyPairSigner.fromSecretKey(private_key as KeyPairString));
const view = <T>(method: string, args: Record<string, unknown> = {}) => provider.callFunction({ contractId: TOKEN, method, args }) as Promise<T>;

async function call(method: string, args: Record<string, unknown> = {}) {
  try {
    const r: any = await keeper.signAndSendTransaction({
      receiverId: TOKEN, actions: [actions.functionCall(method, args, teraToGas("60"), 0n)], waitUntil: "FINAL", throwOnFailure: false,
    });
    const f = r.status?.Failure;
    const why = f ? JSON.stringify(f).match(/panicked: ([^"\\]*)/)?.[1] ?? JSON.stringify(f).slice(0, 120) : "";
    console.log(`   keeper ${method}${args.account_id ? ` ${args.account_id}` : ""}: ${f ? `refused (${why})` : "ok"}  tx ${r.transaction.hash}`);
  } catch (e: any) {
    console.log(`   keeper ${method}: not sent (${e.type ?? e.name}: ${String(e.message).split("\n")[0].slice(0, 120)})`);
  }
}

const market: any = await view("market");
console.log(`   day ${market.day_index}, keeper ${KEEPER} on ${TOKEN}`);
await call("settle_raids");
if (market.buyback_open) await call("buyback");
if (BigInt(market.dao_pending) >= 10n ** 23n) await call("flush_to_dao");
const holders: any[] = await view("holders", { from_index: 0, limit: 100 });
for (const h of holders.filter((h) => h.dao_registered)) await call("sync_weight", { account_id: h.account_id });
