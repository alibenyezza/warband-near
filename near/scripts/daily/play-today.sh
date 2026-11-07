#!/usr/bin/env bash
# One day of the daily deployment, run by the scheduled task twice a day (run-daily.ps1); safe to rerun.
#   - the twelve camp raiders and alice join today's raid (skipped for whoever already joined); joining also claims
#     the previous raid's mint and loot;
#   - on days 4, 7 and 10 two raiders burn tokens for an extra pull; from day 10 every lot bought on launch day is at
#     level 10 and its holder claims the Warlord badge;
#   - from day 1 (the first day lots carry vote weight) the bounty board runs its three payout tiers once against
#     this deployment's DAO: near/scripts/bounties/run.sh, log in near/scripts/daily/board-run.log.
# The keeper's upkeep (settle, buyback, flush, weights) runs just before, from the gas-key keeper (daily-keeper.ts).
set -uo pipefail
cd "$(dirname "$0")"
source ./env.sh
source ../raids/play-lib.sh

D=$(day)
echo "== $(date -u '+%F %T') UTC, day $D of $TOKEN"
if [ "$D" -lt 1 ]; then echo "   launch day: lots season for one day, the first raid is day 1"; exit 0; fi

joined_today() { q "$TOKEN" raid_of "{\"account_id\":\"$1\"}" | python3 -c "import sys,json;j=json.load(sys.stdin);print('yes' if j and int(j['raid'])==$D else 'no')" 2>/dev/null; }
RAIDING="$ALL $ALICE"
# day 5 is a bad day on purpose: only six of the camp show up (the raid needs ten), so the record also shows a lost
# raid, its buyback and burn, and the streak starting again from zero
if [ "$D" -eq 5 ]; then RAIDING=$(for r in $RAIDERS; do echo "$(raider "$r")"; done | head -6 | tr '\n' ' '); fi
for r in $RAIDING; do
  [ "$(joined_today "$r")" = yes ] && continue
  echo "-- $r joins"; tx "$r" join_raid '{}' '0 NEAR'
done
case "$D" in
  4) pull "$(raider scout)" "$(raider smith)" ;;
  7) pull "$(raider hunter)" "$(raider monk)" ;;
  10) pull "$(raider archer)" "$ALICE" ;;
esac
if [ "$D" -ge 10 ]; then
  for r in $ALL $ALICE $BOB; do
    q "$TOKEN" account "{\"account_id\":\"$r\"}" | grep -q '"Warlord"' && continue
    echo "-- $r claims Warlord"; tx "$r" claim_warlord '{}' '0 NEAR'
  done
fi
q "$TOKEN" raid_status | python3 -c 'import sys,json;r=json.load(sys.stdin);print("   raid",r.get("raid"),"joiners",r.get("joiners"),"streak",r.get("streak"),"chest",int(r.get("chest",0))/1e24,"NEAR")' 2>/dev/null || q "$TOKEN" raid_status

# The bounty board, once: petty cash voted by the holders, then the council, auto and DAO tiers.
if ! grep -q "== 5. the board" board-run.log 2>/dev/null; then
  n=$(view "$BOARD" bounties '{"limit":100}' | python3 -c 'import sys,json;print(len(json.load(sys.stdin)))')
  if [ "$n" = 0 ]; then
    echo "-- bounty board run (log: board-run.log)"
    VOTERS="$ALICE $BOB $ALL" BOARD=$BOARD DAO=$DAO DEPLOY=0 bash ../bounties/run.sh > board-run.log 2>&1
    tail -6 board-run.log
  else
    echo "   bounty board run incomplete ($n bounties, no final step in board-run.log): resume by hand with FROM=<step> (bounties/run.sh)"
  fi
fi

# The DAO policy, once: no role may RemoveProposal (with `kind:*` any holder could delete a treasury proposal
# without a vote). The holders pass it by two thirds of their level-weighted weight, so it waits for day 1.
if view "$DAO" get_policy | grep -q '"transfer:\*"'; then
  echo "-- DAO policy fix (log: policy-fix.log)"
  bash ../dao-policy-fix.sh "$DAO" "$ALICE" $ALICE $BOB $ALL >> policy-fix.log 2>&1
  tail -2 policy-fix.log
fi
