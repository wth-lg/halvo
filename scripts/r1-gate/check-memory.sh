#!/bin/sh
# Sub-check (e): the capture daemon's peak and mean resident set stay within the
# P0 memory bound recorded in docs/baseline/r1-gate-before.txt (set from a short
# pre-measurement). Samples the capture process's own pid, not the wrapping shell.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=memory

self_test() {
  max=50331648
  # Within bound passes; a peak or a mean over the bound fails; an unset bound fails.
  if predicate_rss_within_bound 25000000 24000000 "$max" \
     && ! predicate_rss_within_bound 60000000 24000000 "$max" \
     && ! predicate_rss_within_bound 25000000 60000000 "$max" \
     && ! predicate_rss_within_bound 25000000 24000000 "" \
     && ! predicate_rss_within_bound 25000000 24000000 0; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

# Peak and mean RSS (bytes) of a pid over its lifetime, sampled once per second
# with ps -o rss= (which reports kibibytes on macOS). Echoes "peak mean".
sample_peak_mean_rss() {
  pid=$1; peak=0; sum=0; n=0
  while kill -0 "$pid" 2>/dev/null; do
    rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -n "$rss" ]; then
      [ "$rss" -gt "$peak" ] && peak=$rss
      sum=$((sum + rss)); n=$((n + 1))
    fi
    sleep 1
  done
  mean=$(awk -v s="$sum" -v n="$n" 'BEGIN{ if (n>0) printf "%d", s/n; else print 0 }')
  printf '%d %d\n' $((peak * 1024)) $((mean * 1024))
}

measure() {
  seconds=${1:-$GATE_DURATION}
  max=$(gate_rss_max_bytes)
  store=$(mktemp -d); err=$(mktemp); pidfile=$(mktemp)
  trap 'rm -rf "$store" "$err" "$pidfile"' EXIT
  if [ -z "$max" ]; then
    emit_fail "$name" "no rss_max_bytes in $GATE_RSS_BASELINE_FILE"; return 1
  fi
  HALVO_STORE_DIR="$store" HALVO_PID_FILE="$pidfile" \
    run_capture_bin --timed-run "$seconds" > /dev/null 2> "$err" &
  wrapper=$!
  daemon_pid=$(wait_for_pid_file "$pidfile" 120)
  if [ -z "$daemon_pid" ]; then
    wait "$wrapper" 2>/dev/null
    emit_fail "$name" "the daemon did not report its pid: $(head -1 "$err" 2>/dev/null)"; return 1
  fi
  set -- $(sample_peak_mean_rss "$daemon_pid")
  peak=$1; mean=$2
  if ! wait "$wrapper" 2>/dev/null; then
    emit_fail "$name" "daemon did not complete a timed run: $(head -1 "$err" 2>/dev/null)"; return 1
  fi
  if predicate_rss_within_bound "$peak" "$mean" "$max"; then
    emit_pass "$name (peak=${peak}B mean=${mean}B bound=${max}B pid=${daemon_pid})"; return 0
  fi
  emit_fail "$name" "peak=${peak}B mean=${mean}B exceeds bound ${max}B"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure "${1:-}" ;;
  *) measure "${1:-}" ;;
esac
