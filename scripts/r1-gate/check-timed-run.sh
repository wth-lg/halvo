#!/bin/sh
# Sub-check: a timed capture run records events for every required modality.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=timed-run

self_test() {
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  printf 'modality=screen_frame count=10\nmodality=keystroke count=5\nmodality=pointer count=7\nmodality=foreground_app count=3\n' > "$tmp/good"
  printf 'modality=screen_frame count=10\nmodality=keystroke count=0\nmodality=pointer count=7\nmodality=foreground_app count=3\n' > "$tmp/bad"
  if predicate_modalities_present "$tmp/good" && ! predicate_modalities_present "$tmp/bad"; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  seconds=${1:-$GATE_DURATION}
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  if run_capture_bin --timed-run "$seconds" > "$tmp/report" 2> "$tmp/err"; then
    if predicate_modalities_present "$tmp/report"; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "a required modality recorded no events"; return 1
  fi
  emit_fail "$name" "timed-run unavailable: $(head -1 "$tmp/err" 2>/dev/null)"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure "${1:-}" ;;
  *) measure "${1:-}" ;;
esac
