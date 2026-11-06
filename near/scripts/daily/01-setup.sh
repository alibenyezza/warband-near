#!/usr/bin/env bash
# Creates the daily deployment (v3) on testnet: Warband contract with launch parameters (one raid per real day), its
# Sputnik DAO (release 3.0.0, unmodified) and the holders' policy. Rerun safe: each step is skipped once done.
#   bash near/scripts/daily/01-setup.sh
#
# The DAO starts with the v1 policy (the council can set the staking contract), the council points it at the Warband
# contract, then a ChangePolicy moves every governance power (policy, roles, vote token, upgrades) to the holders'
# level-weighted vote. After that the council can only edit the DAO's name, purpose and metadata.
set -euo pipefail
cd "$(dirname "$0")"
source ./env.sh
WASM=${WASM:-$HOME/build/warband/near/warband.wasm}
WASM_DAO=../../dao/res/sputnikdao2-v3.0.0.wasm
acct() {
  curl -s -X POST "$RPC" -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_account\",\"finality\":\"final\",\"account_id\":\"$1\"}}" |
    python3 -c "import sys,json;r=json.load(sys.stdin).get('result',{});print(r.get('code_hash','none'))"
}
clean() { sed 's/\x1b\[[0-9;]*m//g' | grep -E "Transaction ID|succeeded|Error|panicked|deployed|created" || true; }
last_id() { echo $(( $(view "$DAO" get_last_proposal_id) - 1 )); }

[ "$(acct "$TOKEN")" != none ] || near account create-account sponsor-by-faucet-service "$TOKEN" \
  autogenerate-new-keypair save-to-legacy-keychain network-config "$NET" create

if [ "$(acct "$DAO")" = none ]; then
  echo "== DAO $DAO (5.8 NEAR of storage, paid by $HQ)"
  [ "$(git hash-object "$WASM_DAO")" = c64845ff8c964a3fbc62650342b878f30496683d ] || { echo "dao wasm hash mismatch"; exit 1; }
  init=$(python3 -c "import json;print(json.dumps({'config':{'name':'warband-daily','purpose':'WARBAND war chest (daily raids): spent by level-weighted holder votes','metadata':''},'policy':json.load(open('../../dao/policy.json'))}))")
  near account create-account fund-myself "$DAO" '5.8 NEAR' autogenerate-new-keypair save-to-legacy-keychain \
    sign-as "$HQ" network-config "$NET" sign-with-legacy-keychain send 2>&1 | clean
  near contract deploy "$DAO" use-file "$WASM_DAO" with-init-call new json-args "$init" \
    prepaid-gas '100 Tgas' attached-deposit '0 NEAR' network-config "$NET" sign-with-legacy-keychain send 2>&1 | clean
fi

if [ "$(acct "$TOKEN")" = 11111111111111111111111111111111 ]; then
  echo "== Warband on $TOKEN, launch parameters, day_seconds $DAY_SECONDS"
  init=$(cat <<JSON
{"config": {
  "name": "Warband", "symbol": "BAND", "icon": null,
  "day_seconds": $DAY_SECONDS,
  "curve_supply": "1000000000000000000000000000",
  "virtual_near": "30000000000000000000000000",
  "fee_bps": 1000, "dao_share_bps": 5000,
  "floor_drawdown_bps": 2000, "buyback_chunk_bps": 1000, "buyback_daily_cap_bps": 2000,
  "dao_id": "$DAO",
  "raid": {
    "chest_share_bps": 3000, "min_join_balance": "1000000000000000000000000", "min_joiners": 10,
    "target_bps": 2500, "mint_bps": 10, "fail_burn_bps": 1000, "raid_pulls": 20, "ticket_cost_bps": 40
  }
}}
JSON
)
  near contract deploy "$TOKEN" use-file "$WASM" with-init-call new json-args "$init" \
    prepaid-gas '100 Tgas' attached-deposit '0 NEAR' network-config "$NET" sign-with-legacy-keychain send 2>&1 | clean
fi

if [ "$(view "$DAO" get_staking_contract)" != "\"$TOKEN\"" ]; then
  echo "== the council points the DAO at $TOKEN (SetStakingContract, can only be set once)"
  call "$HQ" "$DAO" add_proposal "{\"proposal\":{\"description\":\"Warband pushes level-weighted vote weight\",\"kind\":{\"SetStakingContract\":{\"staking_id\":\"$TOKEN\"}}}}" '0.1 NEAR' '50 Tgas'
  call "$HQ" "$DAO" act_proposal "{\"id\":$(last_id),\"action\":\"VoteApprove\"}" '0 NEAR' '100 Tgas'
fi

if view "$DAO" get_policy | grep -q '"policy:\*"'; then
  echo "== the council hands governance to the holders (ChangePolicy to dao/policy-daily.json)"
  pol=$(python3 -c "import json;print(json.dumps(json.load(open('../../dao/policy-daily.json'))))")
  call "$HQ" "$DAO" add_proposal "{\"proposal\":{\"description\":\"Policy, roles, vote token and upgrades move to the holders' level-weighted vote; the council keeps only name, purpose and metadata\",\"kind\":{\"ChangePolicy\":{\"policy\":$pol}}}}" '0.1 NEAR' '50 Tgas'
  call "$HQ" "$DAO" act_proposal "{\"id\":$(last_id),\"action\":\"VoteApprove\"}" '0 NEAR' '100 Tgas'
fi

echo "== state"
view "$DAO" get_staking_contract
view "$DAO" get_policy | python3 -c 'import sys,json;p=json.load(sys.stdin);[print("  ",r["name"],r["permissions"]) for r in p["roles"]]'
view "$TOKEN" market | python3 -c 'import sys,json;m=json.load(sys.stdin);print("   day",m["day_index"])'
