#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=timed-run

report_fixture() {
  printf 'modality=screen_frame count=%s\nmodality=keystroke count=%s\nmodality=pointer count=%s\nmodality=foreground_app count=%s\n' "$2" "$3" "$4" "$5" > "$1"
}

kind_zero() {
  sed "s/^modality=$2 count=.*/modality=$2 count=0/" "$GATE_FIXTURES/c5-pass.txt" > "$gate_scratch/$1"
}

cases() {
  scratch_root
  good=$GATE_FIXTURES/c5-pass.txt
  t every-kind-recorded PASS predicate_modalities_present "$good"
  for kind in screen_frame keystroke pointer foreground_app; do
    kind_zero "$kind-zero" "$kind"
    t "$kind-zero" FAIL predicate_modalities_present "$gate_scratch/$kind-zero"
    grep -v "^modality=$kind " "$good" > "$gate_scratch/$kind-absent"
    t "$kind-absent" FAIL predicate_modalities_present "$gate_scratch/$kind-absent"
  done
  kind_zero window-title-zero window_title
  t window-title-zero FAIL predicate_modalities_present "$gate_scratch/window-title-zero"
  grep -v "^modality=window_title " "$good" > "$gate_scratch/window-title-absent"
  t window-title-absent FAIL predicate_modalities_present "$gate_scratch/window-title-absent"
  gate_input=operator
  t c5-pass PASS predicate_timed_run_report "$good"
  t c5-hold-pass PASS predicate_timed_run_report "$GATE_FIXTURES/c5-hold-pass.txt"
  t c5-fail-input FAIL predicate_timed_run_report "$GATE_FIXTURES/c5-fail-input.txt"
  sed "s/^modality=foreground_app count=3\$/modality=foreground_app count=1/" "$good" > "$gate_scratch/foreground-count-1"
  t foreground-count-1 FAIL predicate_timed_run_report "$gate_scratch/foreground-count-1"
  t recorded-685d846 FAIL predicate_foreground_switches "$GATE_FIXTURES/recorded-685d846-four-switches.txt"
  t recorded-685d846-report FAIL predicate_timed_run_report "$GATE_FIXTURES/recorded-685d846-four-switches.txt"
  sed 's/^failed_sources=0$/failed_sources=1/' "$good" > "$gate_scratch/failed-count-only"
  t failed-sources-1-without-its-line FAIL predicate_timed_run_report "$gate_scratch/failed-count-only"
  printf 'modality=screen_frame count=3\nmodality=foreground_app count=2\n' > "$gate_scratch/sources-report"
  : > "$gate_scratch/sources-quiet"
  for source_name in screen2 screen-frame Screen; do
    printf 'halvo-capture: source %s did not start: os_error\n' "$source_name" > "$gate_scratch/sources-$source_name"
    t "source-$source_name-did-not-start" FAIL predicate_sources_ran "$gate_scratch/sources-report" "$gate_scratch/sources-$source_name"
  done
  t sources-ran-with-a-quiet-stderr PASS predicate_sources_ran "$gate_scratch/sources-report" "$gate_scratch/sources-quiet"
  gate_input=none
  t foreground-count-1-under-no-input PASS predicate_foreground_switches "$gate_scratch/foreground-count-1"
  t report-missing FAIL predicate_modalities_present "$gate_scratch/no-such-report"
  report_fixture "$gate_scratch/no-input" 10 0 0 3
  t no-input-under-none PASS input_under none "$gate_scratch/no-input"
  t count-line-absent-under-none FAIL input_under none "$gate_scratch/pointer-absent"
  report_fixture "$gate_scratch/leading-zeros" 10 00 00 3
  t counts-of-00-are-no-input-under-operator FAIL input_under operator "$gate_scratch/leading-zeros"
  t counts-of-00-are-no-input-under-none PASS input_under none "$gate_scratch/leading-zeros"
  standin_build || { emit_fail "self-test:$name" "cc could not build helper/load-standin.c"; return 1; }
  t standin-run-with-typed-input PASS against_the_standin typed operator
  t standin-run-with-no-input NO-INPUT against_the_standin ok none
  t standin-typed-run-fails-under-none FAIL fails_saying 'input reached a run that was started with --input none: keystroke=5 pointer=7' against_the_standin typed none
  t standin-binary-fails-with-no-input FAIL against_the_standin exit-1 none
  t standin-screen-source-failed-with-no-input FAIL against_the_standin screen-failed none
  t standin-run-with-no-event-of-two-kinds FAIL against_the_standin ok operator
  t standin-operator-gave-no-input FAIL fails_saying 'the operator gave no input: the report of this run holds keystroke=0 pointer=0' against_the_standin ok operator
  t standin-binary-fails FAIL against_the_standin exit-1 operator
  t standin-binary-prints-usage FAIL against_the_standin exit-64 operator
  t standin-report-then-exit-1 FAIL against_the_standin report-then-exit-1 operator
  t standin-run-ends-early FAIL against_the_standin ends-early operator
  t standin-helper-missing FAIL without_the_helper
  t standin-deadline-bound-unreadable FAIL with_an_unreadable_deadline
  t input-option-none-reaches-the-verdict NO-INPUT through_the_options none
  t input-option-operator-reaches-the-verdict PASS through_the_options operator
  t no-input-line-starts-with-its-word PASS first_word_is NO-INPUT ok none
  t pass-line-starts-with-its-word PASS first_word_is PASS typed operator
  t standin-screen-source-failed FAIL against_the_standin screen-failed operator
}

input_under() {
  gate_input=$1; shift
  input_defect "$@" keystroke pointer > /dev/null
}

without_the_helper() {
  standin_as_the_binary typed operator
  capture_helper_bin=$gate_scratch/no-such-helper
  measure 1
}

with_an_unreadable_deadline() {
  export GATE_OVERRIDE_child_deadline_extra_s=soon
  against_the_standin typed operator
}

through_the_options() {
  case $1 in
    none) standin_as_the_binary ok operator ;;
    *) standin_as_the_binary typed none ;;
  esac
  check_main --measure 1 --input "$1"
}

first_word_is() {
  standin_as_the_binary "$2" "$3"
  out=$(measure 1)
  [ "${out%% *}" = "$1" ]
}

against_the_standin() {
  standin_as_the_binary "$1" "$2"
  measure 1
}

measure() {
  capture_helper=yes
  capture_prepare || { fail_defect; return 1; }
  capture_run "$1" --switch screen_activity=on || { fail_defect; return 1; }
  counts=$(report_counts "$run_dir/stdout")
  line="${counts% } over ${1}s with --switch screen_activity=on and the helper window"
  defect=$(sources_defect "$run_dir/stdout" "$run_dir/stderr") || { emit_fail "$name" "$defect; $line"; return 1; }
  defect=$(input_defect "$run_dir/stdout" keystroke pointer) || { emit_fail "$name" "$defect; $line"; return 1; }
  if [ "$gate_input" = none ]; then
    verdict_under_input "and no keystroke or pointer event reached the run, so the kinds that need input are not judged" "$line"
    return
  fi
  if predicate_modalities_present "$run_dir/stdout"; then
    foreground_seen=$(modality_count "$run_dir/stdout" foreground_app)
    defect=$(foreground_switches_defect "$foreground_seen") || { emit_fail "$name" "$defect; $line"; return 1; }
    emit_pass "$name" "input=$gate_input: $line"; return 0
  fi
  emit_fail "$name" "a required kind recorded no event under input=$gate_input: $line"; return 1
}

check_main "$@"
