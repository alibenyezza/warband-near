// Optional wallet layer: raid, loot, buy, sell, DAO registration and weight sync through @hot-labs/near-connect (testnet).
// The read-only page works without it.
import { NearConnector } from "https://cdn.jsdelivr.net/npm/@hot-labs/near-connect@0.12.0/+esm";

const TOKEN = new URLSearchParams(location.search).get("token") ?? "warband-raids.testnet";
const $ = (s) => document.querySelector(s);
const status = (t) => ($("#tx-status").textContent = t);
const GAS = "100000000000000"; // 100 Tgas

const connector = new NearConnector({ network: "testnet", features: { signAndSendTransaction: true, testnet: true } });
let accountId = null;

function setAccount(id) {
  accountId = id;
  $("#connect").textContent = id ? `${id} · sign out` : "Connect wallet";
  if (id) {
    $("#actions").removeAttribute("data-off");
    $("#account").value = id;
    $("#lookup").requestSubmit();
  } else {
    $("#actions").setAttribute("data-off", "");
  }
}

connector.on("wallet:signIn", (t) => setAccount(t.accounts[0].accountId));
connector.on("wallet:signOut", () => setAccount(null));

$("#connect").addEventListener("click", async () => {
  if (accountId) return (await connector.wallet()).signOut();
  await connector.connect();
});

// yocto amounts as strings, without floating point
function toUnits(value, decimals) {
  const [w, f = ""] = String(value).trim().split(".");
  return (BigInt(w || "0") * 10n ** BigInt(decimals) + BigInt((f + "0".repeat(decimals)).slice(0, decimals) || "0")).toString();
}

async function call(methodName, args, deposit, label) {
  if (!accountId) return status("Connect a testnet wallet first.");
  try {
    status(`${label}: waiting for the wallet…`);
    const wallet = await connector.wallet();
    const out = await wallet.signAndSendTransaction({
      receiverId: TOKEN,
      actions: [{ type: "FunctionCall", params: { methodName, args, gas: GAS, deposit } }],
    });
    const hash = out?.transaction?.hash ?? out?.transaction_outcome?.id;
    status(`${label}: done${hash ? ` (tx ${hash})` : ""}.`);
    $("#lookup").requestSubmit();
  } catch (e) {
    status(`${label}: ${e?.message ?? e}`);
  }
}

$("#buy-form").addEventListener("submit", (e) => {
  e.preventDefault();
  call("buy", {}, toUnits($("#buy-near").value, 24), "Buy");
});
$("#sell-form").addEventListener("submit", (e) => {
  e.preventDefault();
  call("sell", { amount: toUnits($("#sell-band").value, 18) }, "1", "Sell");
});
$("#join").addEventListener("click", () => call("join_raid", {}, "0", "Join the raid"));
$("#claim").addEventListener("click", () => call("claim_raid", {}, "0", "Claim"));
$("#pull").addEventListener("click", () => call("burn_for_pull", {}, "1", "Extra pull"));
$("#warlord").addEventListener("click", () => call("claim_warlord", {}, "0", "Warlord"));
$("#dao-register").addEventListener("click", () => call("dao_register", {}, "160000000000000000000", "DAO registration"));
$("#sync").addEventListener("click", () => call("sync_weight", { account_id: accountId }, "0", "Weight sync"));

setAccount(null);
document.body.dataset.wallet = "ready";
connector.wallet().then((w) => w.getAccounts()).then((a) => a?.[0] && setAccount(a[0].accountId)).catch(() => {});
