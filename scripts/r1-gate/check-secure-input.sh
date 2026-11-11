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
  t standin-operator-gave-no-input FAIL fails_saying 'the operator gave no input: the report of this run holds keystroke=0 pointer=0' against_the_standin ok operator
  t standin-held-run-with-no-input NO-INPUT against_the_standin ok none
  t standin-input-under-none FAIL fails_saying 'input reached a run that was started with --input none: keystroke=0 pointer=7' against_the_standin typed none
  t standin-keystroke-while-held-with-no-input FAIL against_the_standin keystroke-while-held none
  t standin-binary-fails-with-no-input FAIL against_the_standin exit-1 none
  t standin-keystroke-while-held FAIL against_the_standin keystroke-while-held operator
  t standin-suppressed-keys-are-counted FAIL fails_saying '3 key events reached the tap while secure input was held, and the product suppressed them; the same report holds keystroke=0: ' against_the_standin keystroke-while-held operator
  t standin-input-source-failed-with-no-input FAIL fails_saying 'source input did not start: event tap creation failed; ' against_the_standin input-failed none
  t standin-input-source-failed FAIL fails_saying 'source input did not start: event tap creation failed; ' against_the_standin input-failed operator
  t standin-binary-fails FAIL against_the_standin exit-1 operator
  t standin-report-then-exit-1 FAIL against_the_standin report-then-exit-1 operator
  t standin-run-ends-early FAIL against_the_standin ends-early operator
  t no-input-line-starts-with-its-word PASS first_word_is NO-INPUT ok none
}

first_word_is() {
  export STANDIN_NEEDS_HOLD=1
  standin_as_the_binary "$2" "$3"
  out=$(measure 1)
  [ "${out%% *}" = "$1" ]
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
  keys=$(modality_count "$run_dir/stdout" keystroke)
  points=$(modality_count "$run_dir/stdout" pointer)
  line="secure_input_keystrokes=${held:-absent} keystroke=${keys:-absent} pointer=${points:-absent} over ${1}s of held secure input"
  defect=$(sources_defect "$run_dir/stdout" "$run_dir/stderr") || { emit_fail "$name" "$defect; $line"; return 1; }
  if ! predicate_zero_secure_keystrokes "$run_dir/stdout"; then
    case $held in
      ''|*[!0-9]*|0*) emit_fail "$name" "the report holds no one line secure_input_keystrokes=<digits>: $line" ;;
      *) emit_fail "$name" "$held key events reached the tap while secure input was held, and the product suppressed them; the same report holds keystroke=${keys:-absent}: $line" ;;
    esac
    return 1
  fi
  defect=$(input_defect "$run_dir/stdout" pointer) || { emit_fail "$name" "$defect; $line"; return 1; }
  verdict_under_input "so no key was typed during the hold" "$line"
}

check_main "$@"
