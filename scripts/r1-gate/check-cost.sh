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

cases() {
  scratch_root
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

sample_mean_cpu() {
  pid=$1; sum=0; n=0
  while kill -0 "$pid" 2>/dev/null; do
    c=$(ps -o %cpu= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -n "$c" ]; then sum=$(awk -v s="$sum" -v c="$c" 'BEGIN{printf "%.2f", s+c}'); n=$((n+1)); fi
    sleep 1
  done
  awk -v s="$sum" -v n="$n" 'BEGIN{ if (n>0) printf "%.2f", s/n; else print "0" }'
}

measure() {
  seconds=$1
  store=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-cost.XXXXXX"); err=$(mktemp "${TMPDIR:-/tmp}/r1-gate-check-cost.XXXXXX"); trap 'rm -rf "$store" "$err"' EXIT
  before=$(dir_disk_usage_bytes "$store") || { emit_fail "$name" "the store directory could not be measured before the run"; return 1; }
  HALVO_STORE_DIR="$store" run_capture_bin --timed-run "$seconds" > /dev/null 2> "$err" &
  pid=$!
  cpu=$(sample_mean_cpu "$pid")
  if ! wait "$pid" 2>/dev/null; then
    first=$(head -1 "$err" 2>/dev/null)
    emit_fail "$name" "daemon did not complete a timed run: $first"; return 1
  fi
  after=$(dir_disk_usage_bytes "$store") || { emit_fail "$name" "the store directory could not be measured after the run"; return 1; }
  growth=$((after - before))
  need_bound cpu_max_pct_of_one_core || return 1
  cpu_max=$bound
  need_bound disk_max_bytes_per_day || return 1
  per_day=$bound
  if predicate_cost_within_bound "$cpu" "$growth" "$seconds" "$cpu_max" "$per_day"; then emit_pass "$name" "cpu=${cpu}% growth=${growth}B over ${seconds}s"; return 0; fi
  emit_fail "$name" "cpu=${cpu}% growth=${growth}B over ${seconds}s exceeds a bound"; return 1
}

check_main "$@"
