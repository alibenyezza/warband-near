# Helpers shared by the recorded runs (02-play.sh, 03-resume.sh). Source after env.sh.
# Views and transactions are retried when the public RPC rate-limits or a transaction expires (both happen before
# anything is executed, so a retry cannot run an action twice; a retry after a send timeout can, and the contract
# then refuses it: "already in today's raid").
q() { for t in 1 2 3 4 5 6; do out=$(view "$@" 2>/dev/null) && [ -n "$out" ] && [ "${out:0:1}" != "{" -o "${out:1:1}" = '"' ] && { echo "$out"; return; }; sleep 4; done; echo "$out"; }
day() { q "$TOKEN" market | python3 -c 'import sys,json;print(json.load(sys.stdin)["day_index"])' 2>/dev/null || echo -1; }
tx() { # signer method args deposit gas
  for t in 1 2 3 4 5; do
    out=$(near contract call-function as-transaction "$TOKEN" "$2" json-args "$3" prepaid-gas "${5:-100 Tgas}" \
      attached-deposit "$4" sign-as "$1" network-config "$NET" sign-with-legacy-keychain send 2>&1 | sed 's/\x1b\[[0-9;]*m//g')
    if grep -qE "rate limit|has expired|Failed to fetch|timed out|503|502" <<<"$out"; then sleep $((t * 4)); continue; fi
    grep -E "Transaction ID|succeeded|panicked|Smart contract panicked|Error" <<<"$out" | grep -v "^Error: *$" | head -3
    if [ "$t" -gt 1 ] && grep -q "already in today's raid" <<<"$out"; then
      echo "   (resent after a send timeout: the first send went through, the contract refused this copy)"; fi
    return
  done
  echo "gave up: $1 $2"
}
wait_day() { while :; do d=$(day); [ "$d" -ge "$1" ] && break; sleep 4; done; echo; echo "== day $1 ($(date +%T))"; }
join() { for r in "$@"; do echo "-- $r joins"; tx "$r" join_raid '{}' '0 NEAR'; done; }
pull() { for r in "$@"; do echo "-- $r burns for a pull"; tx "$r" burn_for_pull '{}' '1 yoctoNEAR'; done; }
holds() { [ "$(q "$TOKEN" account "{\"account_id\":\"$1\"}")" != "null" ]; }

ALL=""; for r in $RAIDERS; do ALL="$ALL $(raider $r)"; done
FIVE="$(raider archer) $(raider bard) $(raider drummer) $(raider hunter) $(raider mason)"
