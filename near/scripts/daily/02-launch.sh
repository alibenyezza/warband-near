#!/usr/bin/env bash
# Launch day of the daily deployment: the twelve camp raiders, alice and bob buy on the curve, then register as DAO
# voters so the war chest counts their level-weighted weight from day 1 (a lot weighs nothing at level 0).
# Alice first sells a quarter of her v2 BAND (warband-raids.testnet) to fund her launch-day buy.
set -uo pipefail
cd "$(dirname "$0")"
source ./env.sh
source ../raids/play-lib.sh
amounts=(0.45 0.30 0.35 0.40 0.30 0.35 0.30 0.45 0.40 0.35 0.30 0.40)
i=0
for r in $ALL; do
  holds "$r" || { echo "-- $r buys ${amounts[$i]} NEAR"; tx "$r" buy '{}' "$(python3 -c "print(${amounts[$i]} + 0.015)") NEAR"; }
  i=$((i + 1))
done
if ! holds "$ALICE"; then
  b=$(TOKEN=warband-raids.testnet q warband-raids.testnet ft_balance_of "{\"account_id\":\"$ALICE\"}" | tr -d '"')
  echo "-- alice sells a quarter of her v2 BAND"
  TOKEN=warband-raids.testnet tx "$ALICE" sell "{\"amount\":\"$(python3 -c "print(int('$b') // 4)")\"}" '1 yoctoNEAR'
  echo "-- alice buys 2 NEAR"; tx "$ALICE" buy '{}' '2.015 NEAR'
fi
holds "$BOB" || { echo "-- bob buys 1 NEAR"; tx "$BOB" buy '{}' '1.015 NEAR'; }
for r in $ALL $ALICE $BOB; do
  [ "$(q "$TOKEN" account "{\"account_id\":\"$r\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["dao_registered"])')" = True ] && continue
  echo "-- $r registers as a DAO voter"; tx "$r" dao_register '{}' '0.0002 NEAR'
done
q "$TOKEN" market
