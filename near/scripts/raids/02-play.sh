#!/usr/bin/env bash
# The Raids walkthrough recorded for the demo video: twelve raiders and alice play about thirteen raids (3 minutes
# each on testnet), bob only holds. Every step is a real transaction on warband-raids.testnet.
#   D     everyone buys (skipped for accounts that already hold BAND)
#   D+1   13 raiders join: won (streak 1, First Blood for all)
#   D+2   13 join: won (streak 2), the last raid's loot is claimed on the way
#   D+3   only 5 join: lost (under 10 raiders), part of the chest is bought back and burned, streak back to 0
#   D+4…  13 join every raid, extra pulls on D+6 and D+9, Seven Days on the streak-7 raid
#   D+12  everyone claims, Warlord for the lots held ten days
set -uo pipefail
cd "$(dirname "$0")"
source ./env.sh

source ./play-lib.sh

D=$(day); echo "== start, day $D ($(date +%T))"
amounts=(0.45 0.30 0.35 0.40 0.30 0.35 0.30 0.45 0.40 0.35 0.30 0.40)
i=0
for r in $ALL; do
  holds "$r" || { echo "-- $r buys ${amounts[$i]} NEAR"; tx "$r" buy '{}' "$(python3 -c "print(${amounts[$i]} + 0.015)") NEAR"; }
  i=$((i + 1))
done
holds "$ALICE" || { echo "-- alice buys 5 NEAR"; tx "$ALICE" buy '{}' '5.015 NEAR'; }
holds "$BOB" || { echo "-- bob buys 6 NEAR"; tx "$BOB" buy '{}' '6.015 NEAR'; }

wait_day $((D + 1)); sleep 80; join $ALL $ALICE   # lots bought on day D need 3 minutes to season
wait_day $((D + 2)); join $ALL $ALICE
wait_day $((D + 3)); join $FIVE
for k in 4 5 6 7 8 9 10 11; do
  wait_day $((D + k)); join $ALL $ALICE
  [ $k = 6 ] && pull "$(raider scout)" "$(raider smith)"
  [ $k = 9 ] && pull "$(raider hunter)" "$(raider monk)" "$ALICE"
done
wait_day $((D + 12))
for r in $ALL $ALICE; do
  echo "-- $r claims"; tx "$r" claim_raid '{}' '0 NEAR'
  tx "$r" claim_warlord '{}' '0 NEAR'
done
echo "== done ($(date +%T))"
q "$TOKEN" raid_status
