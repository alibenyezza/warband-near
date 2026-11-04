#!/usr/bin/env bash
# Second part of the recorded run, after a two-hour stop (each empty 3-minute day was a lost raid that burned 10%
# of the chest). A newcomer buys big: fresh tokens weigh nothing and cannot join, but the trading fee refills the
# chest. Then the band raids seven days in a row (Seven Days on the streak-7 raid), with extra pulls on the way,
# and everyone claims, Warlord for the lots held ten days. Every step is a real transaction.
set -uo pipefail
cd "$(dirname "$0")"
source ./env.sh
source ./play-lib.sh
CAROL=${NEWCOMER:-warband-carol.testnet}   # NEWCOMER=… for a new big buyer
LOSE=${LOSE:-0}                             # LOSE=1: only five raiders show up on the first day

D=$(day); echo "== resume, day $D ($(date +%T))"
holds "$CAROL" || {
  view_account() { curl -s -X POST "$RPC" -H 'content-type: application/json' -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"view_account\",\"finality\":\"final\",\"account_id\":\"$1\"}}"; }
  view_account "$CAROL" | grep -q '"amount"' || near account create-account sponsor-by-faucet-service "$CAROL" \
    autogenerate-new-keypair save-to-legacy-keychain network-config "$NET" create
  sleep 3
  echo "-- carol buys 9.5 NEAR"; tx "$CAROL" buy '{}' '9.515 NEAR'
  echo "-- carol tries to join with tokens bought minutes ago"; tx "$CAROL" join_raid '{}' '0 NEAR'
}
# a long stop leaves many raids to settle, 20 per call: move them on before the band shows up
for i in 1 2 3 4 5; do tx "$ALICE" settle_raids '{}' '0 NEAR' '300 Tgas' | grep -q succeeded && break; done

# start on a fresh day so the thirteen joins land in the same raid
left=$(q "$TOKEN" raid_status | python3 -c 'import sys,json,time;print(int(json.load(sys.stdin)["ends_at"])//10**9-int(time.time()))')
if [ "$left" -gt 150 ]; then R=$D; else R=$((D + 1)); wait_day $R; fi
if [ "$LOSE" = 1 ]; then join $FIVE; R=$((R + 1)); wait_day $R; fi
for k in 0 1 2 3 4 5 6; do
  [ $k -gt 0 ] && wait_day $((R + k))
  join $ALL $ALICE
  [ $k = 2 ] && pull "$(raider scout)" "$(raider smith)"
  [ $k = 4 ] && pull "$(raider hunter)" "$(raider monk)" "$ALICE"
done
wait_day $((R + 7))
for r in $ALL $ALICE; do
  echo "-- $r claims"; tx "$r" claim_raid '{}' '0 NEAR'
  tx "$r" claim_warlord '{}' '0 NEAR'
done
echo "== done ($(date +%T))"
q "$TOKEN" raid_status
