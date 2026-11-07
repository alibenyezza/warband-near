#!/usr/bin/env bash
# Takes RemoveProposal away from every role of a war chest DAO. In Sputnik 3.0.0 a `kind:*` permission includes
# `RemoveProposal`, which deletes a proposal at once, with no vote; with `transfer:*` any holder (one token held a
# day) could delete every treasury proposal. The new policy lists the actions instead: AddProposal, VoteApprove,
# VoteReject, VoteRemove (a weighted vote to remove) and Finalize. Everything else in the live policy stays the same.
#
#   bash near/scripts/dao-policy-fix.sh <dao> <proposer> <voter>...
# v1 and v2: the council (warband-hq) proposes and passes it alone, it still holds the policy powers there.
# v3: the holders vote, two thirds of the level-weighted weight (the daily task runs it once from day 1).
set -uo pipefail
cd "$(dirname "$0")/.."
source scripts/lib.sh
export NET=testnet-fastnear
DAO=$1; PROPOSER=$2; shift 2; VOTERS="$*"

open_kinds() { view "$DAO" get_policy | python3 -c 'import sys,json;p=json.load(sys.stdin);print(sum(1 for r in p["roles"] for x in r["permissions"] if x.endswith(":*") and not x.startswith("*:")))'; }
pstatus() { view "$DAO" get_proposal "{\"id\":$1}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["status"])'; }

echo "== $(date -u '+%F %T') UTC, $DAO: $(open_kinds) role permission(s) of the form kind:* (they include RemoveProposal)"
[ "$(open_kinds)" = 0 ] && { echo "   already fixed"; exit 0; }
policy=$(view "$DAO" get_policy | python3 -c '
import sys, json
ACTIONS = ["AddProposal", "VoteApprove", "VoteReject", "VoteRemove", "Finalize"]
p = json.load(sys.stdin)
for r in p["roles"]:
    out = []
    for perm in r["permissions"]:
        kind, act = perm.split(":")
        out += [f"{kind}:{a}" for a in ACTIONS] if act == "*" and kind != "*" else [perm]
    r["permissions"] = out
print(json.dumps(p))')
call "$PROPOSER" "$DAO" add_proposal "{\"proposal\":{\"description\":\"No RemoveProposal for any role: every kind:* becomes AddProposal, VoteApprove, VoteReject, VoteRemove, Finalize. A proposal can then only be removed by a vote.\",\"kind\":{\"ChangePolicy\":{\"policy\":$policy}}}}" '0.1 NEAR' '100 Tgas'
id=$(( $(view "$DAO" get_last_proposal_id) - 1 ))
echo "   ChangePolicy is proposal $id"
for v in $VOTERS; do
  [ "$(pstatus $id)" = InProgress ] || break
  echo "   $v votes for proposal $id"
  call "$v" "$DAO" act_proposal "{\"id\":$id,\"action\":\"VoteApprove\"}" '0 NEAR' '150 Tgas'
done
echo "   proposal $id: $(pstatus $id); kind:* permissions left: $(open_kinds)"
