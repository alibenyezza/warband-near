#!/usr/bin/env bash
# Builds and deploys the Warband contract, initialised with its launch parameters (all final), then asks the DAO to
# take it as its staking contract (SetStakingContract, approved by the council: a configuration change).
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/env.sh
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/build/warband}
(cd contracts/warband && cargo near build non-reproducible-wasm)

DAY_SECONDS=${DAY_SECONDS:-300}   # one level every 5 minutes for the demo; 86400 for a real launch
init=$(cat <<JSON
{"config": {
  "name": "Warband", "symbol": "BAND", "icon": null,
  "day_seconds": $DAY_SECONDS,
  "curve_supply": "1000000000000000000000000000",
  "virtual_near": "30000000000000000000000000",
  "fee_bps": 1000, "dao_share_bps": 5000,
  "floor_drawdown_bps": 2000, "buyback_chunk_bps": 1000, "buyback_daily_cap_bps": 2000,
  "dao_id": "$DAO"
}}
JSON
)
near contract deploy "$TOKEN" use-file "$WASM" with-init-call new json-args "$init" \
  prepaid-gas '100 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-legacy-keychain send

# Proposal: the DAO takes Warband as its staking contract. Council approves (RoleWeight, 1 of 1).
near contract call-function as-transaction "$DAO" add_proposal \
  json-args "{\"proposal\":{\"description\":\"Warband pushes level-weighted vote weight\",\"kind\":{\"SetStakingContract\":{\"staking_id\":\"$TOKEN\"}}}}" \
  prepaid-gas '50 Tgas' attached-deposit '0.1 NEAR' sign-as "$HQ" network-config testnet sign-with-legacy-keychain send
id=$(( $(near contract call-function as-read-only "$DAO" get_last_proposal_id json-args {} network-config testnet now 2>/dev/null | tail -1) - 1 ))
near contract call-function as-transaction "$DAO" act_proposal \
  json-args "{\"id\":$id,\"action\":\"VoteApprove\"}" \
  prepaid-gas '100 Tgas' attached-deposit '0 NEAR' sign-as "$HQ" network-config testnet sign-with-legacy-keychain send
