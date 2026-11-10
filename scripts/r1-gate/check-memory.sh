#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=memory

run_fixture() {
  mkdir -p "$gate_scratch/$1"
  printf '        2.00 real         0.30 user         0.10 sys\n            %s  maximum resident set size\n            %s  peak memory footprint\n' "$2" "$3" > "$gate_scratch/$1/time"
  [ -z "$4" ] || printf '%s\n' "$4" > "$gate_scratch/$1/samples"
}

verdict_of() {
  figures=$(memory_figures "$gate_scratch/$1") || return 1
  read -r peak mean_kib samples rest <<FIGURES
$figures
FIGURES
  predicate_memory_within_bound "$peak" "$mean_kib" "$samples" 50331648
}

peak_is() {
  figures=$(memory_figures "$gate_scratch/$1") || return 1
  [ "${figures%% *}" = "$2" ]
}

strict_verdict() {
  gate_strict=$1; run_before="load1=1.00 $2"; run_after="load1=1.00 $3"
  run_dir=$gate_scratch/strict-$4
  mkdir -p "$run_dir"
  printf '%s\n' "$4" > "$run_dir/others"
  strict_run_ok
}

bounds_fixture() {
  printf '%s\n' "$2" > "$gate_scratch/$1"
}

report_fixture() {
  printf 'modality=screen_frame count=%s\nmodality=keystroke count=0\nmodality=pointer count=0\nmodality=foreground_app count=%s\nmodality=window_title count=1\nsecure_input_keystrokes=0\n' "$2" "$3" > "$gate_scratch/$1"
}

sources_cases() {
  report_fixture report-ran 12 2
  report_fixture report-no-frames 0 2
  report_fixture report-no-foreground 12 0
  cp "$gate_scratch/report-ran" "$gate_scratch/report-failed-source"
  printf 'failed_sources=1\nfailed_source=screen: the capture permission for ScreenFrame is not granted\n' >> "$gate_scratch/report-failed-source"
  grep -v '^modality=screen_frame' "$gate_scratch/report-ran" > "$gate_scratch/report-no-frame-line"
  : > "$gate_scratch/stderr-clean"
  printf 'halvo-capture: source screen did not start: the capture permission for ScreenFrame is not granted\n' > "$gate_scratch/stderr-screen"
  printf 'halvo-capture: source input did not start: event tap creation failed\n' > "$gate_scratch/stderr-input"
  t sources-ran PASS predicate_sources_ran "$gate_scratch/report-ran" "$gate_scratch/stderr-clean"
  t screen-source-failed FAIL predicate_sources_ran "$gate_scratch/report-no-frames" "$gate_scratch/stderr-screen"
  t screen-source-failed-with-frames FAIL predicate_sources_ran "$gate_scratch/report-ran" "$gate_scratch/stderr-screen"
  t input-source-failed FAIL predicate_sources_ran "$gate_scratch/report-ran" "$gate_scratch/stderr-input"
  t failed-source-line-in-report FAIL predicate_sources_ran "$gate_scratch/report-failed-source" "$gate_scratch/stderr-clean"
  t screen-frames-zero FAIL predicate_sources_ran "$gate_scratch/report-no-frames" "$gate_scratch/stderr-clean"
  t screen-frame-line-absent FAIL predicate_sources_ran "$gate_scratch/report-no-frame-line" "$gate_scratch/stderr-clean"
  t foreground-zero FAIL predicate_sources_ran "$gate_scratch/report-no-foreground" "$gate_scratch/stderr-clean"
  t stderr-of-the-run-missing FAIL predicate_sources_ran "$gate_scratch/report-ran" "$gate_scratch/no-such-stderr"
  t report-of-the-run-missing FAIL predicate_sources_ran "$gate_scratch/no-such-report" "$gate_scratch/stderr-clean"
  t missing-grant-is-named PASS sources_defect_is "source screen did not start: the capture permission for ScreenFrame is not granted (needs a human grant)" "$gate_scratch/report-ran" "$gate_scratch/stderr-screen"
}

sources_defect_is() {
  defect=$(sources_defect "$2" "$3")
  [ "$defect" = "$1" ]
}

cases() {
  scratch_root
  sources_cases
  run_fixture within 25231360 8113000 '24640 24528 25'
  t within-bound PASS verdict_of within
  t peak-is-the-largest-term PASS peak_is within 25231360
  run_fixture maxrss-above 60000000 8113000 '20000 19000 5'
  t maxrss-above-bound FAIL verdict_of maxrss-above
  run_fixture footprint-above 20000000 60000000 '20000 19000 5'
  t footprint-above-bound FAIL verdict_of footprint-above
  t footprint-is-the-peak PASS peak_is footprint-above 60000000
  run_fixture ps-above 20000000 8113000 '60000 19000 5'
  t ps-max-above-bound FAIL verdict_of ps-above
  t ps-max-is-kib PASS peak_is ps-above 61440000
  run_fixture mean-above 20000000 8113000 '20000 60000 5'
  t mean-above-bound FAIL verdict_of mean-above
  run_fixture at-bound 50331648 50331648 '49152 49152 5'
  t at-the-bound PASS verdict_of at-bound
  run_fixture one-byte-above 50331649 8113000 '20000 19000 5'
  t one-byte-above-bound FAIL verdict_of one-byte-above
  run_fixture no-samples 25231360 8113000 '0 0 0'
  t n-0 FAIL verdict_of no-samples
  run_fixture no-samples-file 25231360 8113000 ''
  t samples-file-missing FAIL verdict_of no-samples-file
  run_fixture samples-with-letters 25231360 8113000 '24640 x 25'
  t samples-not-numbers FAIL verdict_of samples-with-letters
  mkdir -p "$gate_scratch/no-footprint"
  printf '        2.00 real         0.30 user         0.10 sys\n            25231360  maximum resident set size\n' > "$gate_scratch/no-footprint/time"
  printf '24640 24528 25\n' > "$gate_scratch/no-footprint/samples"
  t time-file-lacks-footprint FAIL verdict_of no-footprint
  mkdir -p "$gate_scratch/no-maxrss"
  printf '        2.00 real         0.30 user         0.10 sys\n            8113000  peak memory footprint\n' > "$gate_scratch/no-maxrss/time"
  printf '24640 24528 25\n' > "$gate_scratch/no-maxrss/samples"
  t time-file-lacks-maxrss FAIL verdict_of no-maxrss
  t bound-not-a-number FAIL predicate_memory_within_bound 25231360 24528 25 abc
  bounds_fixture one 'rss_max_bytes=50331648'
  two=$(printf 'rss_max_bytes=50331648\nrss_max_bytes=90331648')
  bounds_fixture two "$two"
  crlf=$(printf 'rss_max_bytes=50331648\r')
  bounds_fixture crlf "$crlf"
  bounds_fixture other-key 'rss_max=50331648'
  t bound-read PASS gate_bound rss_max_bytes "$gate_scratch/one"
  t bound-file-missing FAIL gate_bound rss_max_bytes "$gate_scratch/no-such-file"
  t bound-key-missing FAIL gate_bound rss_max_bytes "$gate_scratch/other-key"
  t bound-key-duplicate FAIL gate_bound rss_max_bytes "$gate_scratch/two"
  t bound-crlf FAIL gate_bound rss_max_bytes "$gate_scratch/crlf"
  t strict-quiet PASS strict_verdict yes pressure=1 pressure=1 no
  t strict-pressure-2 FAIL strict_verdict yes pressure=2 pressure=1 no
  t strict-pressure-2-at-the-end FAIL strict_verdict yes pressure=1 pressure=2 no
  t strict-pressure-unread FAIL strict_verdict yes pressure=unread pressure=1 no
  t strict-other-capture FAIL strict_verdict yes pressure=1 pressure=1 yes
  t pressure-2-without-strict PASS strict_verdict no pressure=2 pressure=2 yes
}

measure() {
  need_bound rss_max_bytes || return 1
  rss_max=$bound
  capture_prepare || { fail_defect; return 1; }
  capture_run "$1" || { fail_defect; return 1; }
  figures=$(memory_figures "$run_dir") || { emit_fail "$name" "$figures"; return 1; }
  read -r peak mean_kib samples maxrss ps_kib footprint <<FIGURES
$figures
FIGURES
  note=$(run_machine_note)
  line="peak=${peak}B (ru_maxrss=${maxrss}B ps_max=${ps_kib}KiB footprint=${footprint}B) mean=${mean_kib}KiB over ${samples} samples in ${1}s, $note"
  if ! predicate_memory_within_bound "$peak" "$mean_kib" "$samples" "$rss_max"; then
    emit_fail "$name" "$line: above the bound, or no sample"; return 1
  fi
  defect=$(sources_defect "$run_dir/stdout" "$run_dir/stderr") || { emit_fail "$name" "$defect; $line"; return 1; }
  if ! strict_run_ok; then
    emit_fail "$name" "$line: --strict needs pressure level 1 at both ends and no other halvo-capture"; return 1
  fi
  emit_pass "$name" "$line"
}

check_main "$@"
