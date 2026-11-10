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
  timed_child "$gate_scratch/live-$1" "$standin" "$@" || return 1
  child_report_defect || return 1
  cpu=$(time_file_cpu_pct "$gate_scratch/live-$1/time") || return 1
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

child_of_script() {
  mkdir -p "$gate_scratch/script-child"
  printf '#!/bin/sh\nsleep 2\n' > "$gate_scratch/wrapper-script"
  chmod 755 "$gate_scratch/wrapper-script"
  timed_child "$gate_scratch/script-child" "$gate_scratch/wrapper-script"
}

cases() {
  scratch_root
  standin_build || { emit_fail "self-test:$name" "cc could not build helper/load-standin.c"; return 1; }
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
  live_floor=50
  t live-busy-loop-reads-above-50 PASS live_cpu_above burn 2
  t live-idle-reads-under-50 FAIL live_cpu_above sleep 1
  t german-locale-of-the-caller PASS under_a_german_locale
  t comm-is-bin-sh FAIL child_of_script
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

measure() {
  seconds=$1
  need_bound cpu_max_pct_of_one_core || return 1
  cpu_max=$bound
  need_bound disk_max_bytes_per_day || return 1
  per_day=$bound
  capture_prepare || { fail_defect; return 1; }
  before=$(dir_disk_usage_bytes "$run_store") || { emit_fail "$name" "the store directory could not be measured before the run"; return 1; }
  capture_run "$seconds" || { fail_defect; return 1; }
  after=$(dir_disk_usage_bytes "$run_store") || { emit_fail "$name" "the store directory could not be measured after the run"; return 1; }
  growth=$((after - before))
  cpu=$(time_file_cpu_pct "$run_dir/time") || { emit_fail "$name" "the time file holds no real, user and sys figures"; return 1; }
  note=$(run_machine_note)
  line="cpu=${cpu}% of one core, growth=${growth}B over ${seconds}s, $note"
  if ! predicate_cost_within_bound "$cpu" "$growth" "$seconds" "$cpu_max" "$per_day"; then
    emit_fail "$name" "$line: above a bound"; return 1
  fi
  if ! strict_run_ok; then
    emit_fail "$name" "$line: --strict needs pressure level 1 at both ends and no other halvo-capture"; return 1
  fi
  emit_pass "$name" "$line"
}

check_main "$@"
