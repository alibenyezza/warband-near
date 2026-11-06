#!/usr/bin/env bash
# Bounty board with earned trust, on testnet: deploy, the DAO votes petty cash, then the three payout tiers.
#   bash near/scripts/bounties/run.sh            (DEPLOY=0 to skip the deploy and init, FROM=<step> to resume)
# Output is the record in near/scripts/bounties/run.log.
set -euo pipefail
cd "$(dirname "$0")/../.."
source scripts/lib.sh
export NET=testnet-fastnear
BOARD=${BOARD:-warband-bountyboard.testnet}
DAO=dao.warband-hq.testnet           # the v1 war chest: Sputnik v2, council warband-hq, holders vote by level weight
HQ=warband-hq.testnet                # the council
CAROL=warband-carol.testnet DAVE=warband-dave.testnet
WASM=${BOUNTIES_WASM:-$HOME/build/bounties/near/warband_bounties.wasm}
N=000000000000000000000000           # 24 zeros
step() { echo; echo "== $*"; }
last() { echo $(( $(view $DAO get_last_proposal_id) - 1 )); }
status() { view $DAO get_proposal "{\"id\":$1}" | python3 -c 'import sys,json;p=json.load(sys.stdin);print("   DAO proposal",p["id"],list(p["kind"])[0],p["status"])'; }
bounty() { view $BOARD bounty "{\"id\":\"$1\"}" | python3 -c 'import sys,json;b=json.load(sys.stdin);print("   bounty",b["id"],repr(b["title"]),int(b["amount"])/1e24,"NEAR",b["status"],"tier",b["tier"],"approved by",b["approved_by"],"proposal",b["dao_proposal"])'; }
record() { view $BOARD record_of "{\"account_id\":\"$1\"}" | python3 -c 'import sys,json;r=json.load(sys.stdin);print("   record",r["account_id"],"finished",r["finished"],"paid",int(r["total_paid"])/1e24,"trust limit",int(r["trust_limit"])/1e24,[(p["bounty"],p["tier"],p["approved_by"]) for p in r["points"]])'; }
next_id() { view $BOARD bounties '{"limit":100}' | python3 -c 'import sys,json;print(len(json.load(sys.stdin)))'; }
refused() { call "$@" || true; }   # a refusal we expect: the script goes on

if [ "${DEPLOY:-1}" = 1 ]; then
  step "0. deploy the board on $BOARD, tiers: +0.05 NEAR of trust per paid bounty, auto up to 0.25, council up to 0.5, above: DAO vote"
  near contract deploy $BOARD use-file "$WASM" with-init-call new \
    json-args "{\"dao\":\"$DAO\",\"tiers\":{\"trust_step\":\"5${N:2}\",\"auto_cap\":\"25${N:2}\",\"council_cap\":\"5${N:1}\"}}" \
    prepaid-gas '30 Tgas' attached-deposit '0 NEAR' network-config $NET sign-with-legacy-keychain send 2>&1 |
    grep -E "Transaction ID|succeeded|Error|panicked" | sed 's/\x1b\[[0-9;]*m//g'
  # the account came from the testnet faucet: everything above storage goes to HQ, so the only petty cash is what
  # the DAO votes
  extra=$(view $BOARD config | python3 -c 'import sys,json;print(f"{int(json.load(sys.stdin)["petty_cash"])/1e24:.6f}")')
  echo "   faucet surplus sent to $HQ: $extra NEAR"
  near tokens $BOARD send-near $HQ "$extra NEAR" network-config $NET sign-with-legacy-keychain send 2>&1 |
    grep -E "Transaction ID|transferred|Error" | sed 's/\x1b\[[0-9;]*m//g'
  view $BOARD config
fi

if [ "${FROM:-0}" -le 1 ]; then
free() { curl -s -X POST "$RPC" -H 'content-type: application/json' -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_account\",\"finality\":\"final\",\"account_id\":\"$DAO\"}}" | python3 -c 'import sys,json;r=json.load(sys.stdin)["result"];print(f"{(int(r["amount"])-r["storage_usage"]*10**19)/1e24:.3f}")'; }
step "0b. the war chest's free balance (its balance minus what its own storage locks): $(free) NEAR"
if python3 -c "import sys;sys.exit(0 if float('$(free)') < 2.2 else 1)"; then
  echo "   below the 2.2 NEAR this run pays out: $HQ sends 2 NEAR to the war chest"
  near tokens $HQ send-near $DAO '2 NEAR' network-config $NET sign-with-legacy-keychain send 2>&1 | grep -E "Transaction ID|transferred|Error" | sed 's/\x1b\[[0-9;]*m//g'
fi

step "1. petty cash comes from the treasury: alice proposes a 1 NEAR Transfer from the DAO to the board, holders vote"
call $ALICE $DAO add_proposal "{\"proposal\":{\"description\":\"Petty cash for the bounty board $BOARD\",\"kind\":{\"Transfer\":{\"token_id\":\"\",\"receiver_id\":\"$BOARD\",\"amount\":\"1$N\"}}}}" '0.1 NEAR'
id=$(last); call $ALICE $DAO act_proposal "{\"id\":$id,\"action\":\"VoteApprove\"}" '0 NEAR' '150 Tgas'; status $id
view $BOARD config
fi

if [ "${FROM:-0}" -le 2 ]; then
step "2. dave has no record: alice's 0.05 NEAR bounty needs one council member"
b=$(next_id)
call $ALICE $BOARD post '{"title":"Draw the rank badges of the band page","amount":"5'${N:2}'"}' '0.01 NEAR'
call $DAVE $BOARD claim "{\"id\":\"$b\"}" '0 NEAR'
call $DAVE $BOARD submit "{\"id\":\"$b\",\"proof\":\"https://github.com/alibenyezza/warband-near/blob/main/near/web/app.js\"}" '0 NEAR'
call $ALICE $BOARD accept "{\"id\":\"$b\"}" '0 NEAR'; bounty $b
echo "   carol is not on the council and tries to approve:"
refused $CAROL $BOARD approve "{\"id\":\"$b\"}" '0 NEAR'
echo "   dave tries to approve his own payout:"
refused $DAVE $BOARD approve "{\"id\":\"$b\"}" '0 NEAR'
echo "   warband-hq (council, read from the DAO policy) approves:"
call $HQ $BOARD approve "{\"id\":\"$b\"}" '0 NEAR'; bounty $b; record $DAVE
fi

if [ "${FROM:-0}" -le 3 ]; then
step "3. with one paid bounty dave's trust limit is 0.05 NEAR: bob's 0.04 NEAR bounty pays out when bob accepts"
b=$(next_id)
call $BOB $BOARD post '{"title":"Translate the raid rules to French","amount":"4'${N:2}'"}' '0.01 NEAR'
call $DAVE $BOARD claim "{\"id\":\"$b\"}" '0 NEAR'
call $DAVE $BOARD submit "{\"id\":\"$b\",\"proof\":\"https://github.com/alibenyezza/warband-near/blob/main/near/RAIDS.md\"}" '0 NEAR'
call $BOB $BOARD accept "{\"id\":\"$b\"}" '0 NEAR'; bounty $b; record $DAVE
fi

step "4. a 1 NEAR bounty is above the council cap: accepting it files a normal DAO proposal (alice pays the bond)"
b=$(next_id)
call $ALICE $BOARD post '{"title":"Review the raid contract before mainnet","amount":"1'$N'"}' '0.01 NEAR'
call $CAROL $BOARD claim "{\"id\":\"$b\"}" '0 NEAR'
call $CAROL $BOARD submit "{\"id\":\"$b\",\"proof\":\"https://github.com/alibenyezza/warband-near/blob/main/near/RAIDS.md#threat-model\"}" '0 NEAR'
call $ALICE $BOARD accept "{\"id\":\"$b\"}" '0.1 NEAR' '150 Tgas'; bounty $b
pid=$(view $BOARD bounty "{\"id\":\"$b\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["dao_proposal"])')
echo "   settling before the vote is refused:"
refused $CAROL $BOARD settle_dao "{\"id\":\"$b\"}" '0 NEAR'
call $ALICE $DAO act_proposal "{\"id\":$pid,\"action\":\"VoteApprove\"}" '0 NEAR' '150 Tgas'; status $pid
call $CAROL $BOARD settle_dao "{\"id\":\"$b\"}" '0 NEAR'; bounty $b; record $CAROL

step "5. the board"
view $BOARD bounties '{"limit":100}' | python3 -c 'import sys,json
for b in json.load(sys.stdin): print("  ",b["id"],b["status"].ljust(16),str(b["tier"]).ljust(8),(str(int(b["amount"])/1e24)+" NEAR").ljust(10),b["contributor"],"<-",b["poster"],"|",b["title"])'
view $BOARD config
