#!/usr/bin/env bash
# Funds the twelve camp raiders for the daily deployment: each sells half of its v2 BAND back to the v2 curve
# (warband-raids.testnet). Real sells, recorded in v2's history.
set -uo pipefail
cd "$(dirname "$0")"
source ../raids/env.sh
source ../raids/play-lib.sh
for r in $ALL; do
  b=$(q "$TOKEN" ft_balance_of "{\"account_id\":\"$r\"}" | tr -d '"')
  half=$(python3 -c "print(int('$b') // 2)")
  [ "$half" != 0 ] || continue
  echo "-- $r sells half of its v2 BAND"
  tx "$r" sell "{\"amount\":\"$half\"}" '1 yoctoNEAR'
done
