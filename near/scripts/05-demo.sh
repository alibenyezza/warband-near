#!/usr/bin/env bash
# The testnet walkthrough recorded in the README, step by step (run it with your own accounts in env.sh to replay
# it on a fresh deployment). Steps that need time to pass say so.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib.sh
who() { view $TOKEN account "{\"account_id\":\"$1\"}" | python3 -c 'import sys,json;a=json.load(sys.stdin);print(a["account_id"],"balance",round(int(a["balance"])/1e18),"weight",round(int(a["weight"])/1e18),"lots",[(round(int(l["amount"])/1e18),l["level"]) for l in a["lots"]])'; }
last() { echo $(( $(view $DAO get_last_proposal_id) - 1 )); }
status() { view $DAO get_proposal "{\"id\":$1}" | python3 -c 'import sys,json;p=json.load(sys.stdin);print("proposal",p["id"],list(p["kind"])[0],p["status"],{k:[round(int(x)/1e18) for x in v] for k,v in p["vote_counts"].items()})'; }

step() { echo; echo "== $*"; }
step "1. alice buys with 3 NEAR, bob with 1 NEAR (10% fee: half to the DAO, half to buybacks)"
call $ALICE $TOKEN buy '{}' '3 NEAR'
call $BOB $TOKEN buy '{}' '1 NEAR'
step "2. both register as DAO delegators (16 bytes of DAO storage)"
call $ALICE $TOKEN dao_register '{}' '0.00016 NEAR'
call $BOB $TOKEN dao_register '{}' '0.00016 NEAR'
step "3. wait one Warband day (day_seconds), then alice sends 20M BAND to bob: bob's copy starts at level 0"
sleep "${WAIT:-310}"
call $ALICE $TOKEN ft_transfer "{\"receiver_id\":\"$BOB\",\"amount\":\"20000000000000000000000000\"}" '1 yoctoNEAR'
who $ALICE; who $BOB
step "4. the keeper agent pushes the new weights to the DAO and flushes the war chest share"
(cd agent && npm run guardian --silent)
step "5. bob buys 6 NEAR more, becomes the biggest holder, and proposes to pay himself 1 NEAR"
call $BOB $TOKEN buy '{}' '6 NEAR'
call $BOB $DAO add_proposal "{\"proposal\":{\"description\":\"pay me, I am the biggest holder\",\"kind\":{\"Transfer\":{\"token_id\":\"\",\"receiver_id\":\"$BOB\",\"amount\":\"1000000000000000000000000\"}}}}" '0.1 NEAR'
id=$(last); call $BOB $DAO act_proposal "{\"id\":$id,\"action\":\"VoteApprove\"}" '0 NEAR'; status $id
step "6. alice, smaller but older, rejects: her weight wins"
call $ALICE $DAO act_proposal "{\"id\":$id,\"action\":\"VoteReject\"}" '0 NEAR'; status $id
step "7. native Sputnik bounty paid by the war chest: proposal, claim, done, holder vote"
call $ALICE $DAO add_proposal '{"proposal":{"description":"Bounty: draw the 11 Warband rank badges (SVG)","kind":{"AddBounty":{"bounty":{"description":"Draw the 11 Warband rank badges (SVG)","token":"","amount":"200000000000000000000000","times":1,"max_deadline":"86400000000000"}}}}}' '0.1 NEAR'
id=$(last); call $ALICE $DAO act_proposal "{\"id\":$id,\"action\":\"VoteApprove\"}" '0 NEAR'; status $id
bid=$(( $(view $DAO get_last_bounty_id) - 1 ))
call $BOB $DAO bounty_claim "{\"id\":$bid,\"deadline\":\"86400000000000\"}" '0.1 NEAR'
call $BOB $DAO bounty_done "{\"id\":$bid,\"description\":\"badges delivered\"}" '0.1 NEAR'
id=$(last); call $ALICE $DAO act_proposal "{\"id\":$id,\"action\":\"VoteApprove\"}" '0 NEAR' '150 Tgas'; status $id
step "8. bob dumps 150M BAND: LIFO sells his young lot, his DAO weight is pulled back, the price falls under the floor"
call $BOB $TOKEN sell '{"amount":"150000000000000000000000000"}' '1 yoctoNEAR'
who $BOB
step "9. the keeper agent sees the open buyback and runs it: the reserve buys BAND on the curve and burns it"
(cd agent && npm run guardian --silent)
view $TOKEN market
