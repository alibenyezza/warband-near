#!/usr/bin/env bash
# Creates the Raids deployment on testnet: contract account, DAO, demo raiders. Keys go to ~/.near-credentials/testnet.
# The faucet gives 10 testnet NEAR per new account and is rate-limited: rerun the script if it refuses (it skips what
# already exists).
set -euo pipefail
cd "$(dirname "$0")"
source ./env.sh
state() {
  curl -s -X POST "$RPC" -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_account\",\"finality\":\"final\",\"account_id\":\"$1\"}}" |
    python3 -c "import sys,json;r=json.load(sys.stdin).get('result',{});print(r.get('code_hash','none'))"
}
exists() { [ "$(state "$1")" != none ]; }

for a in "$TOKEN" "$CAMP"; do
  exists "$a" || near account create-account sponsor-by-faucet-service "$a" autogenerate-new-keypair \
    save-to-legacy-keychain network-config "$NET" create
done

# The DAO needs 5.8 NEAR of storage, paid by the council account: the contract account lends it 5 NEAR first.
if ! exists "$DAO"; then
  near tokens "$TOKEN" send-near "$HQ" '5 NEAR' network-config "$NET" sign-with-legacy-keychain send
  bash ../02-dao.sh
fi
if [ "$(state "$TOKEN")" = 11111111111111111111111111111111 ]; then bash ../03-deploy.sh; fi

# Testnet RPCs sometimes let a transaction expire: each raider is retried up to three times.
for r in $RAIDERS; do
  for try in 1 2 3; do
    exists "$(raider "$r")" && break
    near account create-account fund-myself "$(raider "$r")" '0.6 NEAR' autogenerate-new-keypair \
      save-to-legacy-keychain sign-as "$CAMP" network-config "$NET" sign-with-legacy-keychain send || sleep 5
  done
done
