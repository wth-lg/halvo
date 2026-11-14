#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=service-pause

pause_interval() {
  printf '%s %s\n' "$(($1 + 1))" "$(($2 - 1))"
}

pause_interval_is() {
  interval_got=$(pause_interval "$2" "$3")
  [ "$interval_got" = "$1" ]
}

interval_defect() {
  interval_line=$(grep -m 1 '^floor interval ' "$1")
  case $interval_line in "floor interval from=$3 to=$4 records="*) ;; *) printf 'the floor report holds no interval line for from=%s to=%s: %s\n' "$3" "$4" "${interval_line:-none}"; return 1 ;; esac
  interval_n=$(floor_value "$1" interval records)
  case $interval_n in ''|*[!0-9]*) printf 'the floor report holds no interval line\n'; return 1 ;; esac
  [ "$interval_n" -eq 0 ] && return 0
  printf '%s records have their capture instant inside the paused interval %s\n' "$interval_n" "$2"
  return 1
}

predicate_interval_empty() {
  interval_defect "$1" "" "$2" "$3" > /dev/null
}

restart_defect() {
  restart_capture=$(status_value "$1" capture)
  restart_desired=$(status_value "$1" desired)
  [ "$restart_capture:$restart_desired" = paused:paused ] || { printf 'after a pause and a restart status says capture=%s desired=%s, not paused\n' "${restart_capture:-absent}" "${restart_desired:-absent}"; return 1; }
  restart_a=$(inventory_total "$2"); restart_b=$(inventory_total "$3")
  case $restart_a:$restart_b in *[!0-9:]*|:*|*:) printf 'an inventory of the restart holds no total line\n'; return 1 ;; esac
  [ "$restart_a" -eq "$restart_b" ] && return 0
  printf 'the total moved from %s to %s while the restarted daemon was paused\n' "$restart_a" "$restart_b"
  return 1
}

predicate_restart_paused() {
  restart_defect "$@" > /dev/null
}

pause_steps() {
  build_for_measure || return 1
  service_prepare || return 1
  privacy_rows_line
  take_bound lease_bound_s || return 1; lease_s=$bound
  take_bound stop_deadline_s || return 1; stop_s=$bound
  take_bound batch_age_s || return 1; batch_s=$bound
  pause_wait=$((2 * batch_s + gate_slack_s))
  state_file_write "$svc_data" true paused || { gate_defect="the state file could not be written"; return 1; }
  service_start run || { gate_defect="$gate_defect; not tried: ctl, --floor-report"; return 1; }
  helper_outside_start 300 || return 1
  ctl_or_defect resume resume || return 1
  window_steps before || return 1
  window_a=$svc_dir/before-a.inventory; window_b=$svc_dir/before-b.inventory
  found=$(growth_defect "$window_a" "$window_b") || { gate_defect="before the pause: $found"; return 1; }
  total_a=$(inventory_total "$window_a"); total_b=$(inventory_total "$window_b")
  before_counts="$total_a to $total_b"
  ctl_or_defect pause pause || return 1
  pause_said=$(sed -n 1p "$ctl_out")
  [ "$pause_said" = capture=paused ] || { gate_defect="ctl pause printed $pause_said"; return 1; }
  answered_at=$(date +%s)
  hold_for "$pause_wait" || return 1
  resume_at=$(date +%s)
  interval_pair=$(pause_interval "$answered_at" "$resume_at")
  t1=${interval_pair% *}; t2=${interval_pair#* }
  ctl_or_defect paused-end inventory || return 1
  paused_end=$ctl_out
  ctl_or_defect resume-again resume || return 1
  window_steps after || return 1
  window_a=$svc_dir/after-a.inventory; window_b=$svc_dir/after-b.inventory
  found=$(growth_defect "$paused_end" "$window_b") || { gate_defect="after the resume: $found"; return 1; }
  total_a=$(inventory_total "$paused_end"); total_b=$(inventory_total "$window_b")
  after_counts="$total_a to $total_b"
  if [ "$gate_input" != none ]; then
    found=$(growth_defect "$paused_end" "$window_b" keystroke pointer) || { gate_defect="under --input $gate_input, after the resume: $found"; return 1; }
  fi
  ctl_or_defect pause-again pause || return 1
  stop_steps || return 1
  service_start restart || return 1
  restart_status=$ctl_out
  ctl_or_defect restart-a inventory || return 1
  restart_a_file=$ctl_out
  hold_for "$pause_wait" || return 1
  ctl_or_defect restart-b inventory || return 1
  found=$(restart_defect "$restart_status" "$restart_a_file" "$ctl_out") || { gate_defect="restart control: $found"; return 1; }
  stop_steps || return 1
  helper_outside_stop
  from_ts=$(utc_of "$t1"); to_ts=$(utc_of "$t2")
  interval="[$from_ts, $to_ts]"
  floor_run floor --from "$from_ts" --to "$to_ts" || return 1
  found=$(interval_defect "$svc_dir/floor.out" "$interval" "$from_ts" "$to_ts") || { gate_defect=$found; return 1; }
}

measure() {
  before_counts=''; after_counts=''; interval=''
  if pause_steps; then ok=yes; else ok=no; fi
  stop_children
  where=$(service_line)
  [ "$ok" = yes ] || { emit_fail "$name" "$gate_defect; $where"; return 1; }
  result="0 records in the paused interval $interval, whose ends lie within one second inside the pause answer and the resume request, the step of --from and --to, growth $before_counts before it and $after_counts after it, still paused after a restart"
  if [ "$gate_input" = none ]; then
    emit_no_input "$name" "input=none, and the done-when wants input during the pause: $result; $where"
    return 3
  fi
  emit_pass "$name" "input=$gate_input: $result; $where"
}

against_the_standin() {
  service_standin "$1" "$2"
  measure
}

says() {
  says_text=$1; shift
  service_standin "$1" "$2"
  says_out=$(measure 2>/dev/null)
  case $says_out in *"$says_text"*) return 0 ;; esac
  return 1
}

cases() {
  scratch_root
  printf 'floor records=9 undecodable=0 forbidden_fields=0\nfloor interval from=2026-11-12T09:00:01Z to=2026-11-12T09:00:10Z records=0\n' > "$gate_scratch/floor-0"
  printf 'floor records=9 undecodable=0 forbidden_fields=0\nfloor interval from=2026-11-12T09:00:01Z to=2026-11-12T09:00:10Z records=3\n' > "$gate_scratch/floor-3"
  printf 'floor records=9 undecodable=0 forbidden_fields=0\n' > "$gate_scratch/floor-no-interval"
  t interval-empty PASS predicate_interval_empty "$gate_scratch/floor-0" 2026-11-12T09:00:01Z 2026-11-12T09:00:10Z
  t events-in-interval-3 FAIL predicate_interval_empty "$gate_scratch/floor-3" 2026-11-12T09:00:01Z 2026-11-12T09:00:10Z
  printf 'floor records=9 undecodable=0 forbidden_fields=0\nfloor interval from=2026-11-12T09:00:01Z to=2026-11-12T09:00:10Z records=1\n' > "$gate_scratch/floor-1"
  printf 'floor records=9 undecodable=0 forbidden_fields=0\nfloor interval from=2026-11-12T09:00:01Z to=2026-11-12T09:00:10Z records=2\n' > "$gate_scratch/floor-2"
  t events-in-interval-1 FAIL predicate_interval_empty "$gate_scratch/floor-1" 2026-11-12T09:00:01Z 2026-11-12T09:00:10Z
  t events-in-interval-2 FAIL predicate_interval_empty "$gate_scratch/floor-2" 2026-11-12T09:00:01Z 2026-11-12T09:00:10Z
  t interval-of-the-answer-and-the-resume PASS pause_interval_is "101 199" 100 200
  t interval-line-missing FAIL predicate_interval_empty "$gate_scratch/floor-no-interval" 2026-11-12T09:00:01Z 2026-11-12T09:00:10Z
  t interval-of-another-span FAIL predicate_interval_empty "$gate_scratch/floor-0" 2026-11-12T10:00:01Z 2026-11-12T10:00:10Z
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-11-12 modality=foreground_app events=4 bytes=1\ntotal events=4 bytes=1 disk_bytes=1\n' > "$gate_scratch/inv-4"
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-11-12 modality=foreground_app events=9 bytes=1\ntotal events=9 bytes=1 disk_bytes=1\n' > "$gate_scratch/inv-9"
  t growth-around-the-pause PASS predicate_growth "$gate_scratch/inv-4" "$gate_scratch/inv-9"
  t no-growth-after-resume FAIL predicate_growth "$gate_scratch/inv-9" "$gate_scratch/inv-9"
  t no-growth-before-pause FAIL predicate_growth "$gate_scratch/inv-4" "$gate_scratch/inv-4"
  paused=$(c2_fixture c2-status-macos.txt paused 's/^desired=running$/desired=paused/; s/^capture=running$/capture=paused/')
  t paused-after-restart PASS predicate_restart_paused "$paused" "$gate_scratch/inv-9" "$gate_scratch/inv-9"
  running=$(c2_fixture c2-status-macos.txt running-desired-paused 's/^desired=running$/desired=paused/')
  t running-after-restart FAIL predicate_restart_paused "$running" "$gate_scratch/inv-9" "$gate_scratch/inv-9"
  t total-moves-after-restart FAIL predicate_restart_paused "$paused" "$gate_scratch/inv-4" "$gate_scratch/inv-9"
  t standin-good-run-with-no-input NO-INPUT against_the_standin good none
  t standin-main-names-the-mode PASS says 'FAIL service-pause (no --service-run mode' main none
  t standin-dead-after-the-resume FAIL against_the_standin dead-after-resume none
  t privacy-line-before-the-service PASS privacy_line_first main none
}

check_main "$@"
