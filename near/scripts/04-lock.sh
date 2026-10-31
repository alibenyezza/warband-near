#!/usr/bin/env bash
# Deletes every access key of the Warband contract account and of the DAO account. After this, nobody can sign for
# them: the token code and parameters are final, and the DAO only moves funds through its own proposals.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/env.sh
for acct in "$TOKEN" "$DAO"; do
  keys=$(curl -s -X POST https://test.rpc.fastnear.com -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_access_key_list\",\"finality\":\"final\",\"account_id\":\"$acct\"}}" |
    python3 -c "import sys,json;print(','.join(k['public_key'] for k in json.load(sys.stdin)['result']['keys']))")
  [ -z "$keys" ] && { echo "$acct: already locked"; continue; }
  near account delete-keys "$acct" public-keys "$keys" network-config testnet sign-with-legacy-keychain send
done
for acct in "$TOKEN" "$DAO"; do
  echo "$acct keys: $(curl -s -X POST https://test.rpc.fastnear.com -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_access_key_list\",\"finality\":\"final\",\"account_id\":\"$acct\"}}" |
    python3 -c "import sys,json;print(json.load(sys.stdin)['result']['keys'])")"
done
