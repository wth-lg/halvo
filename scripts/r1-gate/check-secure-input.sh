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
  standin_build || { emit_fail "self-test:$name" "cc could not build helper/load-standin.c"; return 1; }
  t standin-held-run PASS against_the_standin typed operator
  t standin-held-run-with-no-input NO-INPUT against_the_standin ok none
  t standin-keystroke-while-held-with-no-input FAIL against_the_standin keystroke-while-held none
  t standin-binary-fails-with-no-input FAIL against_the_standin exit-1 none
  t standin-keystroke-while-held FAIL against_the_standin keystroke-while-held operator
  t standin-binary-fails FAIL against_the_standin exit-1 operator
}

against_the_standin() {
  export STANDIN_NEEDS_HOLD=1
  standin_as_the_binary "$1" "$2"
  measure 1
}

measure() {
  export HALVO_GATE_SECURE_INPUT=1
  capture_prepare || { fail_defect; return 1; }
  capture_run "$1" --hold-secure-input --switch screen_activity=on || { fail_defect; return 1; }
  held=$(sed -n 's/^secure_input_keystrokes=//p' "$run_dir/stdout")
  line="secure_input_keystrokes=${held:-absent} over ${1}s of held secure input"
  if ! predicate_zero_secure_keystrokes "$run_dir/stdout"; then
    emit_fail "$name" "a keystroke was recorded while secure input was held, or the line is absent: $line"; return 1
  fi
  verdict_under_input "so no key was typed during the hold" "$line"
}

check_main "$@"
