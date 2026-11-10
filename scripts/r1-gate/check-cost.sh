#!/bin/sh
# Sub-check: mean daemon CPU and store-dir byte growth stay within the derived
# cost bound (see lib.sh for how the bound is derived).
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=cost

self_test() {
  # 10% CPU and 1 MB over 600s is within bound; 50% CPU is not; 1 GB is not.
  if predicate_cost_within_bound 10 1000000 600 \
     && ! predicate_cost_within_bound 50 1000000 600 \
     && ! predicate_cost_within_bound 10 1000000000 600; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

# Mean %cpu of a pid over its lifetime, sampled once per second.
sample_mean_cpu() {
  pid=$1; sum=0; n=0
  while kill -0 "$pid" 2>/dev/null; do
    c=$(ps -o %cpu= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -n "$c" ]; then sum=$(awk -v s="$sum" -v c="$c" 'BEGIN{printf "%.2f", s+c}'); n=$((n+1)); fi
    sleep 1
  done
  awk -v s="$sum" -v n="$n" 'BEGIN{ if (n>0) printf "%.2f", s/n; else print "0" }'
}

measure() {
  seconds=${1:-$GATE_DURATION}
  store=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-cost.XXXXXX"); err=$(mktemp "${TMPDIR:-/tmp}/r1-gate-check-cost.XXXXXX"); trap 'rm -rf "$store" "$err"' EXIT
  before=$(dir_bytes "$store")
  HALVO_STORE_DIR="$store" run_capture_bin --timed-run "$seconds" > /dev/null 2> "$err" &
  pid=$!
  cpu=$(sample_mean_cpu "$pid")
  if ! wait "$pid" 2>/dev/null; then
    emit_fail "$name" "daemon did not complete a timed run: $(head -1 "$err" 2>/dev/null)"; return 1
  fi
  after=$(dir_bytes "$store"); growth=$((after - before))
  if predicate_cost_within_bound "$cpu" "$growth" "$seconds"; then emit_pass "$name"; return 0; fi
  emit_fail "$name" "cpu=${cpu}% growth=${growth}B over ${seconds}s exceeds bound"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure "${1:-}" ;;
  *) measure "${1:-}" ;;
esac
