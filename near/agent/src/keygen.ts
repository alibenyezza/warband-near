// Creates the agent's key pair and writes it to .env (git-ignored). Prints the public key and the near-cli command
// that the human, who holds the keeper account's full-access key, runs to grant it a function-call permission.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { KeyPair } from "near-api-js";

if (existsSync(".env") && readFileSync(".env", "utf8").includes("AGENT_PRIVATE_KEY=")) {
  console.error(".env already has an agent key; delete it first to rotate");
  process.exit(1);
}
const kp = KeyPair.fromRandom("ed25519");
writeFileSync(
  ".env",
  [
    "AGENT_ACCOUNT_ID=keeper.warband-hq.testnet",
    `AGENT_PRIVATE_KEY=${kp.toString()}`,
    "WARBAND=warband-band.testnet",
    "DAO=dao.warband-hq.testnet",
    "RPC=https://test.rpc.fastnear.com",
    "",
  ].join("\n"),
  { mode: 0o600 },
);
const pk = kp.getPublicKey().toString();
console.log(`public key: ${pk}`);
console.log(
  `near account add-key keeper.warband-hq.testnet grant-function-call-access --allowance '0.25 NEAR' ` +
    `--contract-account-id warband-band.testnet --function-names 'buyback, sync_weight, flush_to_dao' ` +
    `use-manually-provided-public-key ${pk} network-config testnet sign-with-legacy-keychain send`,
);
