#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=secure-input

cases() {
  scratch_root
  printf 'secure_input_keystrokes=0\n' > "$gate_scratch/zero"
  printf 'secure_input_keystrokes=4\n' > "$gate_scratch/four"
  printf 'modality=keystroke count=0\n' > "$gate_scratch/line-absent"
  t zero-keystrokes PASS predicate_zero_secure_keystrokes "$gate_scratch/zero"
  t four-keystrokes FAIL predicate_zero_secure_keystrokes "$gate_scratch/four"
  t line-absent FAIL predicate_zero_secure_keystrokes "$gate_scratch/line-absent"
  t report-missing FAIL predicate_zero_secure_keystrokes "$gate_scratch/no-such-report"
}

measure() {
  seconds=$1
  need_build || { fail_defect; return 1; }
  export HALVO_GATE_SECURE_INPUT=1
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-secure-input.XXXXXX"); trap 'rm -rf "$tmp"' EXIT
  if run_capture_bin --timed-run "$seconds" > "$tmp/report" 2> "$tmp/err"; then
    if predicate_zero_secure_keystrokes "$tmp/report"; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "a keystroke was recorded while secure input was active"; return 1
  fi
  first=$(head -1 "$tmp/err" 2>/dev/null)
  emit_fail "$name" "timed-run unavailable: $first"; return 1
}

check_main "$@"
