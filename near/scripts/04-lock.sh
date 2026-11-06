#!/usr/bin/env bash
# Deletes every access key of the Warband contract accounts, their DAOs and their bounty boards. After this, nobody
# can sign for them: the code and parameters are final, the DAOs move funds only through their own proposals, and the
# boards pay only by their own rules.
#
#   bash near/scripts/04-lock.sh --check     list the keys of every account, change nothing
#   bash near/scripts/04-lock.sh             delete them (v1 is already locked, it is skipped)
#   bash near/scripts/04-lock.sh <account>…  only these accounts
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/env.sh

ALL=(
  warband-band.testnet dao.warband-hq.testnet                                      # v1, locked on 31 Oct 2026
  warband-raids.testnet dao-raids.warband-hq.testnet warband-bountyboard.testnet   # v2 and the board of the recorded run
  warband-daily.testnet dao-daily.warband-hq.testnet warband-dailyboard.testnet    # v3, the daily deployment
)
check=0
if [ "${1:-}" = --check ]; then check=1; shift; fi
accounts=("$@")
[ ${#accounts[@]} -eq 0 ] && accounts=("${ALL[@]}")

keys_of() {
  curl -s -X POST https://test.rpc.fastnear.com -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_access_key_list\",\"finality\":\"final\",\"account_id\":\"$1\"}}" |
    python3 -c "import sys,json;print(','.join(k['public_key'] for k in json.load(sys.stdin)['result']['keys']))"
}

if [ $check = 0 ]; then
  for acct in "${accounts[@]}"; do
    keys=$(keys_of "$acct")
    [ -z "$keys" ] && { echo "$acct: already locked"; continue; }
    near account delete-keys "$acct" public-keys "$keys" network-config testnet sign-with-legacy-keychain send
  done
fi
for acct in "${accounts[@]}"; do
  keys=$(keys_of "$acct")
  echo "$acct: ${keys:-no keys}"
done
