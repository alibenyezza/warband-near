// Gas-key keeper (NEP-611, nearcore 2.13) on the Raids deployment, set up and demonstrated on testnet.
//
//   npx tsx src/gas-keeper.ts <HQ keychain file> <keychain dir> <attacker account>
//
// The keeper account's only key is a function-call GAS KEY on the Warband contract, limited to the four permissionless
// upkeep methods (settle_raids, buyback, flush_to_dao, sync_weight). The key holds its own prepaid gas, which the HQ
// tops up from outside with TransferToGasKey, so the keeper never locks itself out and HQ holds no key on it. Four
// nonce lanes let the four upkeep calls go out at once. The new private key is written to the keychain file only.
// Log: near/scripts/gas-keeper.log (GAS_KEEPER_LOG to change it; the daily deployment logs to scripts/daily/gas-keeper.log)
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Account, KeyPair, KeyPairSigner, PublicKey, actions, teraToGas, type KeyPairString } from "near-api-js";
import { provider } from "./near.js";

const [hqFile, keychainDir, attacker] = process.argv.slice(2);
if (!hqFile || !keychainDir || !attacker) throw new Error("usage: gas-keeper.ts <HQ keychain file> <keychain dir> <attacker>");
const HQ = process.env.HQ ?? "warband-hq.testnet";
const KEEPER = process.env.GAS_KEEPER ?? `gk-keeper.${HQ}`;
const TOKEN = process.env.WARBAND ?? "warband-raids.testnet";
const DAO = process.env.DAO ?? "dao-raids.warband-hq.testnet";
const METHODS = ["settle_raids", "buyback", "flush_to_dao", "sync_weight"];
const NEAR = 10n ** 24n;
const near = (y: bigint | string) => (Number(BigInt(y) / 10n ** 18n) / 1e6).toString();
const LOG = process.env.GAS_KEEPER_LOG ?? new URL("../../scripts/gas-keeper.log", import.meta.url);
writeFileSync(LOG, `# gas-key keeper ${new Date().toISOString()} on ${TOKEN}, keeper ${KEEPER}, funder ${HQ}\n`);
const say = (s = "") => { console.log(s); appendFileSync(LOG, s + "\n"); };

const hq = new Account(HQ, provider, KeyPairSigner.fromSecretKey(JSON.parse(readFileSync(hqFile, "utf8")).private_key as KeyPairString));
const dir = join(keychainDir, KEEPER);

// 0. create the keeper once
const exists = await provider.viewAccount({ accountId: KEEPER }).then(() => true, () => false);
if (!exists) {
  const kp = KeyPair.fromRandom("ed25519");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `${kp.getPublicKey().toString().replace(":", "_")}.json`);
  writeFileSync(file, JSON.stringify({ account_id: KEEPER, public_key: kp.getPublicKey().toString(), private_key: kp.toString() }), { mode: 0o600 });
  const tx = await hq.signAndSendTransaction({
    receiverId: KEEPER,
    actions: [
      actions.createAccount(),
      actions.transfer(NEAR / 50n),
      actions.addFunctionCallGasKey(kp.getPublicKey(), TOKEN, METHODS, 4),
      actions.transferToGasKey(kp.getPublicKey(), NEAR / 5n),
    ],
    waitUntil: "FINAL",
  });
  say(`0. ${HQ} creates ${KEEPER} with one gas key (4 lanes, 0.2 NEAR of gas) in one transaction: tx ${tx.transaction.hash}`);
}
const { readdirSync } = await import("node:fs");
const keyFile = join(dir, readdirSync(dir).find((f) => f.endsWith(".json"))!);
const { private_key, public_key } = JSON.parse(readFileSync(keyFile, "utf8"));
const keeper = new Account(KEEPER, provider, KeyPairSigner.fromSecretKey(private_key));
const pk = PublicKey.fromString(public_key);

const gasKey = async () => {
  const k = await provider.viewAccessKey({ accountId: KEEPER, publicKey: public_key, finalityQuery: { finality: "final" } });
  return (k.permission as any).GasKeyFunctionCall;
};
const settled = async () => {
  let last = (await gasKey()).balance;
  for (let i = 0; i < 10; i++) {
    await new Promise((r) => setTimeout(r, 2000));
    const now = (await gasKey()).balance;
    if (now === last) return now;
    last = now;
  }
  return last;
};
const lanes = async () => (await provider.viewGasKeyNonces({ accountId: KEEPER, publicKey: public_key, finalityQuery: { finality: "final" } })).nonces.map(String);
const send = async (receiverId: string, action: ReturnType<typeof actions.transfer>) => {
  try {
    const r: any = await keeper.signAndSendTransaction({ receiverId, actions: [action], waitUntil: "FINAL", throwOnFailure: false });
    const f = r.status?.Failure;
    return f ? `refused by the contract: ${JSON.stringify(f).match(/panicked: ([^"\\]*)/)?.[1] ?? JSON.stringify(f).slice(0, 140)}  tx ${r.transaction.hash}` : `ok  tx ${r.transaction.hash}`;
  } catch (e: any) {
    return `refused by the protocol: ${e.type ?? e.name} ${String(e.message).split("\n")[0].slice(0, 140)}`;
  }
};

say(`1. The keeper's only key, read from the chain`);
const k = await gasKey();
const keys = await provider.viewAccessKeyList({ accountId: KEEPER });
say(`   ${keys.keys.length} key: ${public_key} GasKeyFunctionCall, receiver ${k.receiver_id}, methods ${JSON.stringify(k.method_names)}, ${k.num_nonces} lanes, gas ${near(k.balance)} NEAR`);

say(`\n2. The four upkeep calls at once, one per nonce lane`);
const before = { gas: await settled(), lanes: await lanes() };
const someone = ((await provider.callFunction({ contractId: TOKEN, method: "holders", args: { from_index: 0, limit: 100 } }).catch(() => [])) as any[]).find((h) => h.dao_registered);
const calls: [string, Record<string, unknown>][] = [["settle_raids", {}], ["buyback", {}], ["flush_to_dao", {}], ["sync_weight", { account_id: someone?.account_id ?? "warband-alice.testnet" }]];
const out = await Promise.all(calls.map(([m, a]) => send(TOKEN, actions.functionCall(m, a, teraToGas("60"), 0n))));
calls.forEach(([m], i) => say(`   ${m.padEnd(13)} ${out[i]}`));
say(`   lanes ${JSON.stringify(before.lanes)} -> ${JSON.stringify(await lanes())}`);
say(`   gas key ${near(before.gas)} -> ${near(await settled())} NEAR`);

say(`\n3. What a hijacked keeper would try`);
say(`   send 0.01 NEAR to ${attacker}: ${await send(attacker, actions.transfer(NEAR / 100n))}`);
say(`   ft_transfer BAND to ${attacker}: ${await send(TOKEN, actions.functionCall("ft_transfer", { receiver_id: attacker, amount: "1" }, teraToGas("30"), 1n))}`);
say(`   same, without the 1 yocto: ${await send(TOKEN, actions.functionCall("ft_transfer", { receiver_id: attacker, amount: "1" }, teraToGas("30"), 0n))}`);
say(`   a proposal to the DAO: ${await send(DAO, actions.functionCall("add_proposal", { proposal: { description: "pay me", kind: { Transfer: { token_id: "", receiver_id: attacker, amount: "1" } } } }, teraToGas("30"), 0n))}`);
say(`   withdraw its own gas key: ${await send(KEEPER, actions.withdrawFromGasKey(pk, NEAR / 100n))}`);

say(`\n4. ${HQ} refills the gas key from outside, holding no key on the keeper`);
const g0 = await settled();
const refill = await hq.signAndSendTransaction({ receiverId: KEEPER, actions: [actions.transferToGasKey(pk, NEAR / 20n)], waitUntil: "FINAL" });
say(`   gas key ${near(g0)} -> ${near(await settled())} NEAR  tx ${refill.transaction.hash}`);
