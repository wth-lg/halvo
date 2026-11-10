#!/bin/sh
# Sub-check: typing into a password field records zero keystroke events.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=secure-input

self_test() {
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-secure-input.XXXXXX"); trap 'rm -rf "$tmp"' EXIT
  printf 'secure_input_keystrokes=0\n' > "$tmp/good"
  printf 'secure_input_keystrokes=4\n' > "$tmp/bad"
  if predicate_zero_secure_keystrokes "$tmp/good" && ! predicate_zero_secure_keystrokes "$tmp/bad"; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  seconds=${1:-$GATE_DURATION}
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-secure-input.XXXXXX"); trap 'rm -rf "$tmp"' EXIT
  if HALVO_GATE_SECURE_INPUT=1 run_capture_bin --timed-run "$seconds" > "$tmp/report" 2> "$tmp/err"; then
    if predicate_zero_secure_keystrokes "$tmp/report"; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "a keystroke was recorded while secure input was active"; return 1
  fi
  emit_fail "$name" "timed-run unavailable: $(head -1 "$tmp/err" 2>/dev/null)"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure "${1:-}" ;;
  *) measure "${1:-}" ;;
esac
