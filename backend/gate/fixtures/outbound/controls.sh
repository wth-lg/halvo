#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd -P)
ATTEMPT="$ROOT/gate/fixtures/outbound/attempt.sh"
HOSTS="$ROOT/scripts/outbound-hosts.sh"
KINDS="node workerd stripe-mock browser native"
KINDS_WITH_A_LOOPBACK_FORM="node workerd native browser"
rounds=${1:-20}
quiet_rounds=${2:-5}
quiet_seconds=${3:-90}

mkdir -p "$ROOT/.tmp"
LOGS=$(mktemp -d "$ROOT/.tmp/outbound-controls.XXXXXX") || exit 2
echo "controls: $rounds rounds, $quiet_seconds s of quiet before each of the first $quiet_rounds; logs in $LOGS"

exit_code_of_one_run() {
  kind=$1
  where=$2
  log=$3
  case "$kind" in
    browser) sh "$HOSTS" browser sh "$ATTEMPT" "$kind" "$where" > "$log" 2>&1 ;;
    *) sh "$HOSTS" run sh "$ATTEMPT" "$kind" "$where" > "$log" 2>&1 ;;
  esac
  echo $?
}

failed=0
for kind in $KINDS; do
  : > "$LOGS/$kind.seen"
  : > "$LOGS/$kind.logged"
done
round=1
while [ "$round" -le "$rounds" ]; do
  if [ "$round" -le "$quiet_rounds" ]; then
    sleep "$quiet_seconds"
    after=", first after ${quiet_seconds} s of quiet"
  else
    after=
  fi
  set --
  for kind in $KINDS; do set -- "$@" "$kind"; done
  rotate_by=$(((round - 1) % $#))
  while [ "$rotate_by" -gt 0 ]; do
    first=$1
    shift
    set -- "$@" "$first"
    rotate_by=$((rotate_by - 1))
  done
  for kind in "$@"; do
    status=$(exit_code_of_one_run "$kind" outside "$LOGS/$kind.$round.log")
    case "$status" in
      1)
        sources=$(sed -n -e 's/^outbound: system log: \([0-9]*\) denied$/system log \1/p' \
          -e 's/^outbound: node record: \([0-9]*\) denied$/node record \1/p' \
          -e 's/^outbound: killed with no record: /killed with no record /p' \
          -e 's/^outbound: browser: system log: \([0-9]*\) denied$/browser system log \1/p' \
          -e 's/^outbound: browser: net log: \([0-9]*\) attempts.*$/browser net log \1/p' "$LOGS/$kind.$round.log" | tr '\n' ',' | sed 's/,$//; s/,/, /g')
        echo seen >> "$LOGS/$kind.seen"
        case "$kind" in
          browser) RECORD_A_GATE_RUN_RELIES_ON='^outbound:   chrome-headless-shell([0-9]*) deny(1) network-outbound .*:80$' ;;
          *) RECORD_A_GATE_RUN_RELIES_ON='^outbound: system log: [1-9][0-9]* denied$' ;;
        esac
        if grep -q "$RECORD_A_GATE_RUN_RELIES_ON" "$LOGS/$kind.$round.log"; then
          echo logged >> "$LOGS/$kind.logged"
          echo "control $kind round $round: seen ($sources)$after"
        else
          echo "control $kind round $round: seen, NOT IN THE SYSTEM LOG ($sources)$after"
          failed=1
        fi
        ;;
      0)
        echo "control $kind round $round: MISSED$after"
        failed=1
        ;;
      *)
        echo "control $kind round $round: the check could not count (exit $status)$after"
        failed=1
        ;;
    esac
    after=
  done
  round=$((round + 1))
done

for kind in $KINDS; do
  echo "control $kind: seen $(wc -l < "$LOGS/$kind.seen" | tr -d ' ') of $rounds, in the system log $(wc -l < "$LOGS/$kind.logged" | tr -d ' ') of $rounds"
done
for kind in $KINDS_WITH_A_LOOPBACK_FORM; do
  status=$(exit_code_of_one_run "$kind" loopback "$LOGS/$kind.loopback.log")
  case "$kind" in
    node) ran='node connect:' ;;
    workerd) ran='worker fetch:' ;;
    native) ran='native connect' ;;
    browser) ran='browser page: loaded' ;;
  esac
  if [ "$status" = 0 ] && ! grep -q "^outbound: command: $ran" "$LOGS/$kind.loopback.log"; then
    echo "loopback $kind: the program did not run"
    failed=1
  elif [ "$status" = 0 ] && [ "$kind" = browser ]; then
    echo "loopback $kind: nothing counted, $(sed -n 's/^outbound: browser: net log: .*, \([0-9]*\) left out as probes of IPv6$/\1/p' "$LOGS/$kind.loopback.log") left out as probes of IPv6"
  elif [ "$status" = 0 ]; then
    echo "loopback $kind: nothing counted"
  else
    echo "loopback $kind: the check did not pass (exit $status)"
    failed=1
  fi
done
exit "$failed"
