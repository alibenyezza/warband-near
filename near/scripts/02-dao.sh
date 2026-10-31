#!/usr/bin/env bash
# Creates the war chest DAO: the official Sputnik DAO v2 contract, release 3.0.0, unmodified.
#
# Why not the testnet factory (sputnik-v2.testnet)? It still deploys release 2.0.0, where RoleKind::Member holds a raw
# u128 inside an untagged enum: serde cannot parse it, so a token-weighted "holders" role cannot be created there
# (checked on chain: "data did not match any variant of untagged enum VersionedPolicy"). Release 3.0.0 fixed it
# (Member(U128)). We deploy the binary committed by the Sputnik team at tag 3.0.0, after checking its git blob hash.
#
# Council (warband-hq) only handles configuration; treasury transfers, bounties and calls are voted by the "holders"
# role with token weight, i.e. the level-weighted weight that the Warband contract delegates.
# The DAO account's full-access key is deleted at the end: only the DAO policy controls it.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/env.sh

WASM_DAO=dao/res/sputnikdao2-v3.0.0.wasm
BLOB=c64845ff8c964a3fbc62650342b878f30496683d   # sputnikdao2/res/sputnikdao2.wasm at tag 3.0.0
if [ ! -f "$WASM_DAO" ]; then
  mkdir -p dao/res
  curl -sSfL -o "$WASM_DAO" https://raw.githubusercontent.com/near-daos/sputnik-dao-contract/3.0.0/sputnikdao2/res/sputnikdao2.wasm
fi
[ "$(git hash-object "$WASM_DAO")" = "$BLOB" ] || { echo "wasm hash mismatch"; exit 1; }

init=$(python3 - <<'EOF'
import json
policy = json.load(open("dao/policy.json"))
cfg = {"name": "warband", "purpose": "WARBAND war chest: spent by level-weighted holder votes", "metadata": ""}
print(json.dumps({"config": cfg, "policy": policy}))
EOF
)

near account create-account fund-myself "$DAO" '5.8 NEAR' autogenerate-new-keypair save-to-legacy-keychain \
  sign-as "$HQ" network-config testnet sign-with-legacy-keychain send
near contract deploy "$DAO" use-file "$WASM_DAO" with-init-call new json-args "$init" \
  prepaid-gas '100 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-legacy-keychain send
