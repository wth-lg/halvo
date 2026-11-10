#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=cost

bounds_fixture() {
  printf '%s\n' "$2" > "$gate_scratch/$1"
}

bound_is() {
  value=$(gate_bound "$1" "${3:-}")
  [ "$value" = "$2" ]
}

bounds_of_the_unit() {
  for pair in rss_max_bytes=50331648 cpu_max_pct_of_one_core=20 disk_max_bytes_per_day=2000000000 ci_job_timeout_min=30 child_deadline_extra_s=25 batch_age_s=5 maintenance_interval_s=60 screen_frames_max_per_s=2; do
    bound_is "${pair%%=*}" "${pair#*=}" || return 1
  done
}

line_names_the_override() {
  out=$(GATE_OVERRIDE_cpu_max_pct_of_one_core=5 sh "$DIR/check-cost.sh" --self-test --case within-bound 2>/dev/null)
  case $out in *'[override: cpu_max_pct_of_one_core; not a done-when run]') return 0 ;; esac
  return 1
}

override_is_applied() {
  export GATE_OVERRIDE_cpu_max_pct_of_one_core="$1"
  use_bound cpu_max_pct_of_one_core || return 1
  [ "$bound" = "$1" ] || return 1
  [ "$gate_bounds_used" = "cpu_max_pct_of_one_core=$1 " ]
}

line_names_the_bounds() {
  use_bound cpu_max_pct_of_one_core || return 1
  out=$(emit_pass "$name" "cpu=1.00")
  [ "$out" = "PASS cost (cpu=1.00; bounds: cpu_max_pct_of_one_core=20)" ]
}

usage_is_a_number() {
  usage=$(dir_disk_usage_bytes "$1") || return 1
  case $usage in ''|*[!0-9]*) return 1 ;; esac
  return 0
}

time_fixture() {
  printf '        %s real         %s user         %s sys\n             1081344  maximum resident set size\n' "$2" "$3" "$4" > "$gate_scratch/$1"
}

cpu_of_time_file_is() {
  cpu=$(time_file_cpu_pct "$1") || return 1
  [ "$cpu" = "$2" ]
}

live_cpu_above() {
  mkdir -p "$gate_scratch/live-$1"
  timed_child 30 "$gate_scratch/live-$1" "$standin" "$@" || return 2
  child_report_defect || return 2
  cpu=$(time_file_cpu_pct "$gate_scratch/live-$1/time") || return 2
  predicate_cost_within_bound "$cpu" 0 600 "$live_floor" 2000000000 && return 1
  return 0
}

exit_status_gives() {
  child_status=$1
  child_dir=$gate_scratch
  child_report_defect
}

under_a_german_locale() {
  LC_ALL=de_DE.UTF-8 sh "$DIR/check-cost.sh" --self-test --case live-busy-loop-reads-above-50
}

deadline_run() {
  mkdir -p "$gate_scratch/deadline-$2-$3"
  deadline_from=$(date +%s)
  timed_child "$1" "$gate_scratch/deadline-$2-$3" "$standin" "$2" "$3"
  deadline_status=$?
  deadline_to=$(date +%s)
  deadline_took=$((deadline_to - deadline_from))
  return "$deadline_status"
}

deadline_ends_the_child() {
  deadline_run 2 "$1" "$2" && return 1
  [ "$gate_defect" = "the child ran past its deadline of 2s and was ended" ] || return 1
  [ "$deadline_took" -ge 2 ] || return 1
  [ "$deadline_took" -le "$3" ] || return 1
  left=$(pgrep -f "load-standin $1 $2")
  [ -z "$left" ]
}

signal_ends_the_child() {
  mini_shell=$1; shift
  mini=$gate_scratch/mini-check-$mini_shell-$1.sh
  cat > "$mini" <<MINI
#!/bin/sh
set -u
GATE_ROOT=$GATE_ROOT
. "$DIR/lib.sh"
name=mini
cases() { :; }
measure() {
  scratch_root
  timed_child 60 "\$gate_scratch" "$standin" sleep "$2"
}
check_main --measure
MINI
  "$mini_shell" "$mini" > "$gate_scratch/mini-$mini_shell-$1.out" 2>&1 &
  mini_pid=$!
  polls=0
  until pgrep -f "load-standin sleep $2" > /dev/null; do
    polls=$((polls + 1)); [ "$polls" -lt 50 ] || return 1
    sleep 0.1
  done
  kill "-$1" "$mini_pid"
  polls=0
  while kill -0 "$mini_pid" 2>/dev/null; do
    polls=$((polls + 1)); [ "$polls" -lt 50 ] || return 1
    sleep 0.1
  done
  left=$(pgrep -f "load-standin sleep $2")
  [ -z "$left" ]
}

watched_child_ends() {
  mini=$gate_scratch/mini-watched.sh
  cat > "$mini" <<MINI
#!/bin/sh
set -u
GATE_ROOT=$GATE_ROOT
. "$DIR/lib.sh"
name=mini
cases() { :; }
measure() { watched "$standin" sleep 36.25; }
check_main --measure
MINI
  sh "$mini" > "$gate_scratch/mini-watched.out" 2>&1 &
  mini_pid=$!
  polls=0
  until pgrep -f "load-standin sleep 36.25" > /dev/null; do
    polls=$((polls + 1)); [ "$polls" -lt 50 ] || return 1
    sleep 0.1
  done
  kill -TERM "$mini_pid"
  polls=0
  while kill -0 "$mini_pid" 2>/dev/null; do
    polls=$((polls + 1)); [ "$polls" -lt 50 ] || return 1
    sleep 0.1
  done
  left=$(pgrep -f "load-standin sleep 36.25")
  [ -z "$left" ]
}

helper_ends_with_the_check() {
  printf '#!/bin/sh\nprintf "helper visible pid=1\\n"\nexec sleep 38.25\n' > "$gate_scratch/standin-helper"
  chmod 755 "$gate_scratch/standin-helper"
  mini=$gate_scratch/mini-helper.sh
  cat > "$mini" <<MINI
#!/bin/sh
set -u
GATE_ROOT=$GATE_ROOT
. "$DIR/lib.sh"
name=mini
cases() { :; }
measure() {
  scratch_root
  helper_start 60 "$gate_scratch/standin-helper" 20 || return 1
  watched "$standin" sleep 39.25
}
check_main --measure
MINI
  sh "$mini" > "$gate_scratch/mini-helper.out" 2>&1 &
  mini_pid=$!
  polls=0
  until pgrep -f "load-standin sleep 39.25" > /dev/null; do
    polls=$((polls + 1)); [ "$polls" -lt 50 ] || return 1
    sleep 0.1
  done
  kill -TERM "$mini_pid"
  polls=0
  while kill -0 "$mini_pid" 2>/dev/null; do
    polls=$((polls + 1)); [ "$polls" -lt 50 ] || return 1
    sleep 0.1
  done
  left=$(pgrep -f "sleep 38.25")
  [ -z "$left" ]
}

live_cpu_within() {
  live_cpu_above "$@"
  [ "$?" -eq 1 ]
}

child_of_script() {
  mkdir -p "$gate_scratch/script-child"
  printf '#!/bin/sh\nsleep 2\n' > "$gate_scratch/wrapper-script"
  chmod 755 "$gate_scratch/wrapper-script"
  timed_child 30 "$gate_scratch/script-child" "$gate_scratch/wrapper-script"
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
  standin_build || { emit_fail "self-test:$name" "cc could not build helper/load-standin.c"; return 1; }
  sources_cases
  t standin-run-with-sources PASS against_the_standin ok operator
  t standin-screen-source-failed FAIL against_the_standin screen-failed operator
  t standin-binary-fails FAIL against_the_standin exit-1 operator
  t cpu-20.00-vs-20 PASS predicate_cost_within_bound 20.00 0 600 20 2000000000
  t cpu-20.09-vs-20 FAIL predicate_cost_within_bound 20.09 0 600 20 2000000000
  t cpu-with-a-comma FAIL predicate_cost_within_bound 20,09 0 600 20 2000000000
  t cpu-empty FAIL predicate_cost_within_bound "" 0 600 20 2000000000
  t bound-not-a-number FAIL predicate_cost_within_bound 10 0 600 abc 2000000000
  t disk-bound-not-a-number FAIL predicate_cost_within_bound 10 0 600 20 abc
  t duration-not-a-number FAIL predicate_cost_within_bound 10 0 abc 20 2000000000
  t duration-zero FAIL predicate_cost_within_bound 10 0 0 20 2000000000
  t growth-not-a-number FAIL predicate_cost_within_bound 10 "" 600 20 2000000000
  t growth-at-the-bound PASS predicate_cost_within_bound 10 13888888 600 20 2000000000
  t growth-above-the-bound FAIL predicate_cost_within_bound 10 13888889 600 20 2000000000
  time_fixture time-good 2.00 0.30 0.10
  time_fixture time-zero-real 0.00 0.30 0.10
  time_fixture time-comma 2,00 0,30 0,10
  printf '             1081344  maximum resident set size\n' > "$gate_scratch/time-lacks-real"
  : > "$gate_scratch/time-empty"
  t time-file-read PASS cpu_of_time_file_is "$gate_scratch/time-good" 20.00
  t zero-samples FAIL time_file_cpu_pct "$gate_scratch/time-empty"
  t time-file-missing FAIL time_file_cpu_pct "$gate_scratch/no-such-time-file"
  t time-file-lacks-real FAIL time_file_cpu_pct "$gate_scratch/time-lacks-real"
  t real-is-zero FAIL time_file_cpu_pct "$gate_scratch/time-zero-real"
  t time-file-with-commas FAIL time_file_cpu_pct "$gate_scratch/time-comma"
  live_floor=20
  t standin-quiet PASS live_cpu_within sleep 1
  t standin-short-lived-threads FAIL live_cpu_within short-lived-threads 2
  t standin-burn-last-3s FAIL live_cpu_within burn-last 1 3
  live_floor=50
  t live-short-lived-children-read-above-50 PASS live_cpu_above short-lived-children 20 0.1
  t live-busy-loop-reads-above-50 PASS live_cpu_above burn 2
  t live-idle-reads-under-50 FAIL live_cpu_above sleep 1
  t german-locale-of-the-caller PASS under_a_german_locale
  t comm-is-bin-sh FAIL child_of_script
  t child-past-deadline FAIL deadline_run 2 sleep 30.25
  t child-in-time PASS deadline_run 5 sleep 1
  t deadline-ends-the-child-inside-the-margin PASS deadline_ends_the_child sleep 31.25 5
  t kill-follows-an-ignored-term PASS deadline_ends_the_child deaf 32.25 10
  t deadline-zero FAIL deadline_run 0 sleep 1
  t term-ends-the-child PASS signal_ends_the_child sh TERM 33.25
  t hup-ends-the-child PASS signal_ends_the_child sh HUP 34.25
  if command -v dash > /dev/null 2>&1; then
    t term-ends-the-child-under-dash PASS signal_ends_the_child dash TERM 35.25
    t hup-ends-the-child-under-dash PASS signal_ends_the_child dash HUP 37.25
  fi
  t term-ends-a-watched-child PASS watched_child_ends
  t term-ends-the-helper PASS helper_ends_with_the_check
  t deadline-not-a-number FAIL deadline_run abc sleep 1
  t exit-0-report-read PASS exit_status_gives 0
  t exit-3-report-read PASS exit_status_gives 3
  t exit-1-unavailable FAIL exit_status_gives 1
  t exit-2-signal FAIL exit_status_gives 2
  t exit-64-usage FAIL exit_status_gives 64
  t within-bound PASS predicate_cost_within_bound 10 1000000 600 20 2000000000
  t cpu-50 FAIL predicate_cost_within_bound 50 1000000 600 20 2000000000
  t disk-1-gb FAIL predicate_cost_within_bound 10 1000000000 600 20 2000000000
  bounds_fixture one 'cpu_max_pct_of_one_core=20'
  two=$(printf 'cpu_max_pct_of_one_core=20\ncpu_max_pct_of_one_core=90')
  bounds_fixture two "$two"
  crlf=$(printf 'cpu_max_pct_of_one_core=20\r')
  bounds_fixture crlf "$crlf"
  bounds_fixture letters 'cpu_max_pct_of_one_core=20x'
  bounds_fixture other-key 'cpu_max_pct=20'
  bounds_fixture commented '# cpu_max_pct_of_one_core=20'
  mkdir "$gate_scratch/some-dir"
  printf 'x\n' > "$gate_scratch/some-file"
  t dir-measured PASS usage_is_a_number "$gate_scratch/some-dir"
  t dir-missing FAIL dir_disk_usage_bytes "$gate_scratch/no-such-dir"
  t dir-is-a-file FAIL dir_disk_usage_bytes "$gate_scratch/some-file"
  t bound-read PASS bound_is cpu_max_pct_of_one_core 20 "$gate_scratch/one"
  t bounds-of-the-unit PASS bounds_of_the_unit
  t bound-file-missing FAIL gate_bound cpu_max_pct_of_one_core "$gate_scratch/no-such-file"
  t bound-key-missing FAIL gate_bound cpu_max_pct_of_one_core "$gate_scratch/other-key"
  t bound-key-commented FAIL gate_bound cpu_max_pct_of_one_core "$gate_scratch/commented"
  t bound-key-duplicate FAIL gate_bound cpu_max_pct_of_one_core "$gate_scratch/two"
  t bound-crlf FAIL gate_bound cpu_max_pct_of_one_core "$gate_scratch/crlf"
  t bound-with-letters FAIL gate_bound cpu_max_pct_of_one_core "$gate_scratch/letters"
  t override-is-applied PASS override_is_applied 5
  t override-not-a-number FAIL override_is_applied 5x
  t line-names-the-override PASS line_names_the_override
  t line-names-the-bounds PASS line_names_the_bounds
}

against_the_standin() {
  standin_as_the_binary "$1" "$2"
  measure 1
}

measure() {
  seconds=$1
  need_bound cpu_max_pct_of_one_core || return 1
  cpu_max=$bound
  need_bound disk_max_bytes_per_day || return 1
  per_day=$bound
  capture_helper=yes
  capture_prepare || { fail_defect; return 1; }
  before=$(dir_disk_usage_bytes "$run_store") || { emit_fail "$name" "the store directory could not be measured before the run"; return 1; }
  capture_run "$seconds" --switch screen_activity=on || { fail_defect; return 1; }
  after=$(dir_disk_usage_bytes "$run_store") || { emit_fail "$name" "the store directory could not be measured after the run"; return 1; }
  growth=$((after - before))
  cpu=$(time_file_cpu_pct "$run_dir/time") || { emit_fail "$name" "the time file holds no real, user and sys figures"; return 1; }
  note=$(run_machine_note)
  line="cpu=${cpu}% of one core, growth=${growth}B over ${seconds}s, $note"
  if ! predicate_cost_within_bound "$cpu" "$growth" "$seconds" "$cpu_max" "$per_day"; then
    emit_fail "$name" "$line: above a bound"; return 1
  fi
  defect=$(sources_defect "$run_dir/stdout" "$run_dir/stderr") || { emit_fail "$name" "$defect; $line"; return 1; }
  if ! strict_run_ok; then
    emit_fail "$name" "$line: --strict needs pressure level 1 at both ends and no other halvo-capture"; return 1
  fi
  emit_pass "$name" "$line"
}

check_main "$@"
