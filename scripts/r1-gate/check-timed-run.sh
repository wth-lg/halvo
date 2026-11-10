#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=timed-run

report_fixture() {
  printf 'modality=screen_frame count=%s\nmodality=keystroke count=%s\nmodality=pointer count=%s\nmodality=foreground_app count=%s\n' "$2" "$3" "$4" "$5" > "$1"
}

cases() {
  scratch_root
  report_fixture "$gate_scratch/good" 10 5 7 3
  t every-kind-recorded PASS predicate_modalities_present "$gate_scratch/good"
  report_fixture "$gate_scratch/screen_frame-zero" 0 5 7 3
  report_fixture "$gate_scratch/keystroke-zero" 10 0 7 3
  report_fixture "$gate_scratch/pointer-zero" 10 5 0 3
  report_fixture "$gate_scratch/foreground_app-zero" 10 5 7 0
  for kind in screen_frame keystroke pointer foreground_app; do
    t "$kind-zero" FAIL predicate_modalities_present "$gate_scratch/$kind-zero"
    grep -v "^modality=$kind " "$gate_scratch/good" > "$gate_scratch/$kind-absent"
    t "$kind-absent" FAIL predicate_modalities_present "$gate_scratch/$kind-absent"
  done
  t report-missing FAIL predicate_modalities_present "$gate_scratch/no-such-report"
}

measure() {
  capture_helper=yes
  capture_prepare || { fail_defect; return 1; }
  capture_run "$1" || { fail_defect; return 1; }
  counts=$(report_counts "$run_dir/stdout")
  line="${counts% }over ${1}s with the helper window"
  defect=$(sources_defect "$run_dir/stdout" "$run_dir/stderr") || { emit_fail "$name" "$defect; $line"; return 1; }
  if predicate_modalities_present "$run_dir/stdout"; then emit_pass "$name" "$line"; return 0; fi
  emit_fail "$name" "a required kind recorded no event: $line"; return 1
}

check_main "$@"
