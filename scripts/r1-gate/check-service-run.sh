#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=service-run

tmp_scan_defect() {
  [ -d "$2" ] || { printf 'not scanned: the TMPDIR of the processes under test is missing\n'; return 1; }
  tree_scan_defect "the TMPDIR of the processes under test" "$1" "$2"
}

scan_into() {
  svc_dir=$1
  mkdir -p "$svc_dir" || return 2
  tmp_scan_defect "$2" "$3"
}

predicate_run_tmp_clean() {
  svc_dir=$3
  mkdir -p "$svc_dir" || return 2
  tmp_scan_defect "$1" "$2" > /dev/null
}

late_helper_is_gone() {
  service_standin good none
  late_helper=$gate_scratch/late-helper
  printf '#!/bin/sh\nsleep 3\nprintf "helper visible pid=%%s\\n" "%s"\nsleep 30\n' "\$\$" > "$late_helper"
  chmod 755 "$late_helper"
  capture_helper_bin=$late_helper
  svc_dir=$(mktemp -d "$gate_scratch/late-run.XXXXXX") || return 2
  gate_helper_polls=5
  helper_outside_start 60 && { printf 'the late helper was taken as started\n'; return 1; }
  sleep 0.5
  late_left=$(pgrep -f "$late_helper")
  [ -z "$late_left" ] && return 0
  printf 'the late helper is left after the check gave up: pid %s\n' "$late_left"
  pkill -f "$late_helper"
  return 1
}

churn_outer() {
  churn_end=$(( $(date +%s) + $1 ))
  while churn_now=$(date +%s) && [ "$churn_now" -lt "$churn_end" ]; do
    printf 'appears and goes\n' > "$2/r1-gate-churn.$$"
    rm -f "$2/r1-gate-churn.$$"
  done
}

outer_churn_changes_nothing() {
  outer=${TMPDIR:-/tmp}
  churn_proc=$(mktemp -d "$gate_scratch/churn-proc.XXXXXX") || return 2
  printf 'nothing of the marker\n' > "$churn_proc/run-file"
  churn_locked=$(mktemp "${outer%/}/r1-gate-locked.XXXXXX") || return 2
  chmod 000 "$churn_locked"
  churn_outer 3 "${outer%/}" &
  churn_pid=$!
  sleep 1
  predicate_run_tmp_clean r1gateMARKER-7 "$churn_proc" "$gate_scratch/churn-run"
  churn_verdict=$?
  wait "$churn_pid"
  chmod 600 "$churn_locked"
  rm -f "$churn_locked"
  return "$churn_verdict"
}

unreadable_run_file() {
  locked_proc=$(mktemp -d "$gate_scratch/locked-proc.XXXXXX") || return 2
  printf 'a file of the process under test\n' > "$locked_proc/locked"
  chmod 000 "$locked_proc/locked"
  locked_said=$(scan_into "$gate_scratch/locked-run" r1gateMARKER-7 "$locked_proc")
  locked_status=$?
  chmod 600 "$locked_proc/locked"
  printf '%s\n' "$locked_said"
  return "$locked_status"
}

unreadable_line_is() {
  unreadable_out=$(unreadable_run_file)
  [ "$unreadable_out" = 'not scanned: locked under the TMPDIR of the processes under test cannot be read' ]
}

log_scan_note() {
  if [ "$capture_builds" = no ]; then printf 'the unified log was not read (stand-in run)'; return 0; fi
  log_probe=$(gate_marker)probe
  logger -t r1-gate "$log_probe" 2>/dev/null
  limited 60 log show --last 2m --style compact --predicate "eventMessage CONTAINS \"$log_probe\"" > "$svc_dir/log-probe" 2>/dev/null
  log_status=$?
  [ "$log_status" -eq 0 ] || { printf 'the unified log was not read: log show of the probe exited %s' "$log_status"; return 0; }
  log_found=$(grep -v '^Filtering' "$svc_dir/log-probe" | grep -c -F -e "$log_probe")
  if [ "${log_found:-0}" -eq 0 ]; then printf 'the unified log was not read: a probe that logs a marker was not found by log show'; return 0; fi
  limited 120 log show --last 10m --style compact --predicate "eventMessage CONTAINS \"$1\"" > "$svc_dir/log-marker" 2>/dev/null
  log_status=$?
  [ "$log_status" -eq 0 ] || { printf 'the unified log was not read: log show of the marker exited %s' "$log_status"; return 0; }
  log_hits=$(grep -v '^Filtering' "$svc_dir/log-marker" | grep -c -F -e "$1")
  if [ "${log_hits:-0}" -ne 0 ]; then printf 'leak: the marker is in the unified log'; return 1; fi
  printf 'the unified log was read and holds no marker'
}

log_note_with_stub() {
  stub_dir=$gate_scratch/log-stub-$1
  mkdir -p "$stub_dir/bin" || return 1
  printf '%s\n' "$1" > "$stub_dir/mode"
  cat > "$stub_dir/bin/log" <<'EOF'
#!/bin/sh
stub=${0%/bin/log}
printf '%s\n' "$*" >> "$stub/log-calls"
read -r mode < "$stub/mode"
case " $* " in
  *" --last 2m "*) [ "$mode" = probe ] && exit 3; printf 'stub answer: %s\n' "$*"; exit 0 ;;
  *" --last 10m "*) exit 5 ;;
esac
exit 64
EOF
  cat > "$stub_dir/bin/logger" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "${0%/bin/logger}/logger-calls"
EOF
  chmod 755 "$stub_dir/bin/log" "$stub_dir/bin/logger" || return 1
  PATH=$stub_dir/bin:$PATH
  capture_builds=yes
  svc_dir=$stub_dir
  stub_said=$(log_scan_note r1gateMARKER-9)
  printf '%s\n' "$stub_said"
  [ -s "$stub_dir/logger-calls" ] || { printf 'the stub of logger was not called\n'; return 1; }
  [ "$stub_said" = "the unified log was not read: log show of the $1 exited $2" ]
}

scan_now() {
  scan_when=$1
  set -- "$svc_dir/run.stdout" "$svc_dir/run.stderr"
  for capture_file in "$svc_dir"/ctl-*; do set -- "$@" "$capture_file"; done
  scan_found=$(data_dir_scan_defect "$svc_data" "$scan_marker" "$@") || { scan_defect="$scan_when: $scan_found"; return 1; }
  scan_found=$(tmp_scan_defect "$scan_marker" "$svc_proc_tmp") || { scan_defect="$scan_when: $scan_found"; return 1; }
  scans_done="$scans_done $scan_when;"
}

control_run() {
  svc_data=$(mktemp -d "$gate_scratch/${capture_label}control.XXXXXX") || { gate_defect="mktemp could not make the control directory"; return 1; }
  state_file_write "$svc_data" true paused || { gate_defect="the state file of the control run could not be written"; return 1; }
  [ "$capture_builds" = no ] || helper_build || return 1
  helper_start 120 "$capture_helper_bin" || return 1
  service_start control || return 1
  ctl_or_defect control-resume resume || return 1
  window_steps control || return 1
  window_a=$svc_dir/control-a.inventory; window_b=$svc_dir/control-b.inventory; window_status=$svc_dir/control.status
  found=$(own_child_defect "$window_a" "$window_b") || { gate_defect="control run, helper window as the check's own child: $found"; return 1; }
  stop_steps || return 1
  helper_stop
  control_done=yes
}

run_steps() {
  build_for_measure || return 1
  service_prepare || return 1
  privacy_rows_line
  scan_marker=$(gate_marker)
  take_bound lease_bound_s || return 1; lease_s=$bound
  take_bound stop_deadline_s || return 1; stop_s=$bound
  take_bound batch_age_s || return 1; batch_s=$bound
  state_file_write "$svc_data" true paused || { gate_defect="the state file could not be written"; return 1; }
  prefill_run prefill 100 --marker "$scan_marker" --span-days 1 || { gate_defect="$gate_defect; not tried: --service-run, ctl"; return 1; }
  service_start run || { gate_defect="$gate_defect; not tried: ctl"; return 1; }
  found=$(switch_defect "$svc_first" screen_activity) || { gate_defect=$found; return 1; }
  xattr -p com.apple.metadata:com_apple_backup_excludeItem "$svc_data" > "$svc_dir/mark.out" 2> "$svc_dir/mark.err"
  found=$(backup_mark_defect "$?" "$svc_dir/mark.out") || { gate_defect=$found; return 1; }
  if [ "$capture_builds" = yes ]; then
    limited 10 tmutil isexcluded "$svc_data" > "$svc_dir/tmutil.out" 2>&1
    tm_note="tmutil isexcluded exit $?: $(sed -n '1s/^\[\([A-Za-z]*\)\].*/\1/p' "$svc_dir/tmutil.out")"
  else
    tm_note="tmutil not read (stand-in run)"
  fi
  helper_outside_start 300 || return 1
  ctl_or_defect resume resume || return 1
  window_steps run || return 1
  window_a=$svc_dir/run-a.inventory; window_b=$svc_dir/run-b.inventory; window_status=$svc_dir/run.status
  found=$(sources_running_defect "$window_status" foreground screen input) || { gate_defect=$found; return 1; }
  found=$(growth_defect "$window_a" "$window_b" screen_frame) || { gate_defect=$found; return 1; }
  found=$(off_kinds_defect "$window_b") || { gate_defect=$found; return 1; }
  total_a=$(inventory_total "$window_a"); total_b=$(inventory_total "$window_b")
  screen_a=$(inventory_events "$window_a" screen_frame); screen_b=$(inventory_events "$window_b" screen_frame)
  keys_b=$(inventory_events "$window_b" keystroke); points_b=$(inventory_events "$window_b" pointer)
  fore_a=$(inventory_events "$window_a" foreground_app); fore_b=$(inventory_events "$window_b" foreground_app)
  run_counts="total $total_a to $total_b, screen_frame $screen_a to $screen_b, keystroke=$keys_b pointer=$points_b"
  if [ "$gate_input" != none ]; then
    found=$(growth_defect "$window_a" "$window_b" keystroke pointer) || { gate_defect="under --input $gate_input: $found"; return 1; }
    found=$(foreground_switches_defect "$((fore_b - fore_a))") || { gate_defect=$found; return 1; }
  fi
  scan_now "while events were live" || scan_failed=yes
  lease_control || return 1
  ctl_or_defect before-delete status || return 1
  delete_id=$(status_value "$ctl_out" store_id)
  ctl_or_defect delete-all delete-all "$delete_id" || return 1
  case $(head -1 "$ctl_out") in deleted\ *) ;; *) gate_defect="ctl delete-all printed no deleted line: $(head -1 "$ctl_out" | cut -d ' ' -f 1)"; return 1 ;; esac
  if [ "$scan_failed" = no ]; then
    found=$(registered_names_defect "$svc_data") || { scan_defect="after the delete-all: $found"; scan_failed=yes; }
    names_checked=yes
  fi
  [ "$scan_failed" = yes ] || scan_now "after the delete-all" || scan_failed=yes
  log_note=$(log_scan_note "$scan_marker") || { scan_defect=$log_note; scan_failed=yes; }
  stop_steps || return 1
  [ "$scan_failed" = yes ] || scan_now "after the stop" || scan_failed=yes
  helper_outside_stop
  control_run || return 1
}

measure() {
  scan_failed=no; scan_defect=''; tm_note=''; run_counts=''; log_note=''; scans_done=''; names_checked=no; control_done=no
  if run_steps; then run_ok=yes; else run_ok=no; fi
  if [ "$run_ok" = yes ] && [ "$control_done" != yes ]; then run_ok=no; gate_defect="the own-child control run did not run"; fi
  if [ "$run_ok" = yes ] && [ "$scan_failed" = no ]; then
    if [ "$scans_done" != " while events were live; after the delete-all; after the stop;" ] || [ "$names_checked" != yes ]; then
      scan_failed=yes; scan_defect="the scans ran at:${scans_done:- no time}, and the registered names were checked: $names_checked; the line needs the three times of P1 and the names after the delete-all"
    fi
  fi
  stop_children
  where=$(service_line)
  scan_ran=yes
  [ -n "$scan_defect" ] || [ "$run_ok" = yes ] || scan_ran=no
  rc=0
  if [ "$run_ok" = no ]; then
    emit_fail "$name" "$gate_defect; $where"; rc=1
  elif [ "$gate_input" = none ]; then
    emit_no_input "$name" "input=none, so no keystroke or pointer row is judged: $run_counts; lease lapsed into no_indicator; $tm_note; own-child control clean; $where"
    rc=3
  else
    emit_pass "$name" "input=$gate_input: $run_counts; lease lapsed into no_indicator; $tm_note; own-child control clean; $where"
  fi
  if [ "$scan_ran" = no ]; then
    emit_line FAIL data-dir-scan "not scanned: $gate_defect"; rc=1
  elif [ "$scan_failed" = yes ]; then
    emit_line FAIL data-dir-scan "$scan_defect"; rc=1
  else
    emit_line PASS data-dir-scan "the data directory, the TMPDIR that the check gave the processes under test, the daemon's output and every ctl output hold no marker and no record field name, in content and in names, while events were live, after the delete-all and after the stop, and only registered names are left after the delete-all; not read: a file that a process under test writes outside the TMPDIR it was given and outside the data directory; $log_note"
  fi
  return "$rc"
}

status_with() {
  c2_fixture c2-status-macos.txt "$1" "$2"
}

inventory_with() {
  printf 'store_id=0123456789abcdef0123456789abcdef\n%s\ntotal events=%s bytes=1 disk_bytes=1\n' "$2" "$3" > "$gate_scratch/$1"
}

rows() {
  printf 'row day=2026-11-12 modality=screen_frame events=%s bytes=1\nrow day=2026-11-12 modality=foreground_app events=%s bytes=1\n%s' "$1" "$2" "${3:-}"
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
  running=$(status_with running 's/^switch name=screen_activity value=off/switch name=screen_activity value=on/; s/^source name=screen state=off/source name=screen state=running/')
  t status-good PASS predicate_sources_running "$running" foreground screen input
  input_failed=$(status_with input-failed 's/^switch name=screen_activity value=off/switch name=screen_activity value=on/; s/^source name=screen state=off/source name=screen state=running/; s/^source name=input state=running/source name=input state=failed reason=permission/')
  t input-failed FAIL predicate_sources_running "$input_failed" foreground screen input
  partial=$(status_with partial 's/^switch name=screen_activity value=off/switch name=screen_activity value=on/; s/^source name=screen state=off/source name=screen state=running/; s/^capture=running$/capture=partial/')
  t capture-partial-with-every-source-running FAIL predicate_sources_running "$partial" foreground screen input
  t screen-source-off FAIL predicate_sources_running "$GATE_FIXTURES/c2-status-macos.txt" foreground screen input
  t store-open PASS predicate_store_open "$GATE_FIXTURES/c2-status-macos.txt"
  sealed=$(status_with sealed 's/^store=open$/store=sealed/')
  t store-sealed FAIL predicate_store_open "$sealed"
  t store-of-an-older-version FAIL predicate_store_open "$GATE_FIXTURES/c2-status-unsupported-version.txt"
  t switch-screen-activity-on PASS predicate_switch_on "$running" screen_activity
  t switch-screen-activity-off FAIL predicate_switch_on "$GATE_FIXTURES/c2-status-macos.txt" screen_activity
  rows_a=$(rows 4 4); inventory_with inv-a "$rows_a" 8
  rows_b=$(rows 9 9); inventory_with inv-b "$rows_b" 18
  rows_c=$(rows 4 14); inventory_with inv-b-no-screen "$rows_c" 18
  rows_d=$(rows 9 9 'row day=2026-11-12 modality=microphone events=1 bytes=1
'); inventory_with inv-b-microphone "$rows_d" 19
  t growth-with-screen-rows PASS predicate_growth "$gate_scratch/inv-a" "$gate_scratch/inv-b" screen_frame
  t no-screen-activity FAIL predicate_growth "$gate_scratch/inv-a" "$gate_scratch/inv-b-no-screen" screen_frame
  t no-growth FAIL predicate_growth "$gate_scratch/inv-a" "$gate_scratch/inv-a" screen_frame
  t inventory-fixture-has-no-off-kind PASS predicate_no_off_kind_rows "$GATE_FIXTURES/c2-inventory.txt"
  t microphone-row FAIL predicate_no_off_kind_rows "$gate_scratch/inv-b-microphone"
  rows_e=$(rows 9 9 'row day=2026-11-12 modality=camera events=1 bytes=1
'); inventory_with inv-b-camera "$rows_e" 19
  t camera-row FAIL predicate_no_off_kind_rows "$gate_scratch/inv-b-camera"
  t own-child-excluded PASS predicate_own_child_excluded "$gate_scratch/inv-a" "$gate_scratch/inv-b-no-screen"
  t screen-activity-from-own-child FAIL predicate_own_child_excluded "$gate_scratch/inv-a" "$gate_scratch/inv-b"
  t own-child-control-recorded-nothing FAIL predicate_own_child_excluded "$gate_scratch/inv-a" "$gate_scratch/inv-a"
  t lease-lapsed PASS predicate_lease_lapsed "$GATE_FIXTURES/c2-status-no-indicator.txt" "$gate_scratch/inv-b" "$gate_scratch/inv-b"
  t grows-after-lease-lapse FAIL predicate_lease_lapsed "$GATE_FIXTURES/c2-status-no-indicator.txt" "$gate_scratch/inv-a" "$gate_scratch/inv-b"
  t capture-running-after-lease-lapse FAIL predicate_lease_lapsed "$GATE_FIXTURES/c2-status-macos.txt" "$gate_scratch/inv-b" "$gate_scratch/inv-b"
  t stop-flushed PASS predicate_stopped 0 "$GATE_FIXTURES/c2-stopped.txt"
  printf 'stopped flushed=no events_lost=3\n' > "$gate_scratch/stopped-unflushed"
  t stop-exit-1 FAIL predicate_stopped 1 "$GATE_FIXTURES/c2-stopped.txt"
  t stop-unflushed FAIL predicate_stopped 0 "$gate_scratch/stopped-unflushed"
  printf 'stopped flushed=yes events_lost=7\n' > "$gate_scratch/stopped-with-a-loss"
  t stop-with-events-lost FAIL predicate_stopped 0 "$gate_scratch/stopped-with-a-loss"
  t ctl-answered PASS predicate_ctl_answered 0 "$GATE_FIXTURES/c2-stopped.txt" status
  t unreachable-69 FAIL predicate_ctl_answered 69 "$GATE_FIXTURES/c2-unreachable.txt" status
  printf 'com.apple.backupd\n' > "$gate_scratch/mark-value"
  printf 'xattr: /d: No such xattr: com.apple.metadata:com_apple_backup_excludeItem\n' > "$gate_scratch/mark-error"
  : > "$gate_scratch/mark-nothing"
  t mark-present PASS predicate_backup_mark 0 "$gate_scratch/mark-value"
  t mark-absent FAIL predicate_backup_mark 1 "$gate_scratch/mark-nothing"
  t mark-empty FAIL predicate_backup_mark 0 "$gate_scratch/mark-nothing"
  gate_input=operator
  printf 'modality=foreground_app count=2\n' > "$gate_scratch/fg-2"
  printf 'modality=foreground_app count=1\n' > "$gate_scratch/fg-1"
  t foreground-switched PASS predicate_foreground_switches "$gate_scratch/fg-2"
  t foreground-count-1 FAIL predicate_foreground_switches "$gate_scratch/fg-1"
  gate_input=none
  mkdir -p "$gate_scratch/scan-data/store" "$gate_scratch/scan-captures"
  printf 'ok\n' > "$gate_scratch/scan-captures/stdout"
  printf 'r1gateMARKER-7 in the daemon stderr\n' > "$gate_scratch/scan-captures/stderr"
  t marker-scan-clean PASS predicate_data_dir_clean "$gate_scratch/scan-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  t stderr-holds-marker FAIL predicate_data_dir_clean "$gate_scratch/scan-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout" "$gate_scratch/scan-captures/stderr"
  t marker-scan-misses-a-capture-file FAIL predicate_data_dir_clean "$gate_scratch/scan-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout" "$gate_scratch/scan-captures/no-such-file"
  t standin-good-run-with-no-input NO-INPUT against_the_standin good none
  t standin-typed-run-under-operator PASS against_the_standin typed operator
  t standin-lease-ignored FAIL against_the_standin lease-ignored none
  t standin-main-names-the-mode PASS says 'FAIL service-run (no --prefill-store mode' main none
  t privacy-line-before-the-service PASS privacy_line_first main none
  t standin-main-fails FAIL against_the_standin main none
  t late-helper-is-ended-by-the-check PASS late_helper_is_gone
  t standin-leak-while-live FAIL against_the_standin live-leak none
  t standin-leak-at-the-stop FAIL against_the_standin stop-leak none
  t standin-name-left-after-delete FAIL against_the_standin delete-leaves-name none
  t standin-exit-1-after-the-stop FAIL against_the_standin exit-1-at-stop none
  t standin-camera-rows FAIL against_the_standin camera-row none
  t standin-no-backup-mark FAIL against_the_standin no-mark none
  t standin-no-exclusion-of-the-own-child FAIL against_the_standin no-exclusion none
  name=data-dir-scan
  mkdir -p "$gate_scratch/tmp-clean" "$gate_scratch/tmp-marker" "$gate_scratch/tmp-record" "$gate_scratch/tmp-name" "$gate_scratch/tmp-link"
  printf 'nothing\n' > "$gate_scratch/tmp-clean/run-file"
  printf 'spill r1gateMARKER-7\n' > "$gate_scratch/tmp-marker/halvo-spill.txt"
  printf '{"kind":"keystroke","seq":7,"app":"com.example.mail"}\n' > "$gate_scratch/tmp-record/batch.json"
  : > "$gate_scratch/tmp-name/r1gateMARKER-7.note"
  ln -s "$gate_scratch/scan-captures/stdout" "$gate_scratch/tmp-link/link"
  t run-tmpdir-clean PASS predicate_run_tmp_clean r1gateMARKER-7 "$gate_scratch/tmp-clean" "$gate_scratch/tmp-run"
  t marker-in-the-run-tmpdir FAIL predicate_run_tmp_clean r1gateMARKER-7 "$gate_scratch/tmp-marker" "$gate_scratch/tmp-run"
  t record-in-the-run-tmpdir FAIL predicate_run_tmp_clean r1gateMARKER-7 "$gate_scratch/tmp-record" "$gate_scratch/tmp-run"
  t marker-in-a-name-under-the-run-tmpdir FAIL predicate_run_tmp_clean r1gateMARKER-7 "$gate_scratch/tmp-name" "$gate_scratch/tmp-run"
  t symbolic-link-in-the-run-tmpdir FAIL predicate_run_tmp_clean r1gateMARKER-7 "$gate_scratch/tmp-link" "$gate_scratch/tmp-run"
  printf '{"kind":"keystroke","seq":7,"app":"com.example.mail"}\n' > "$gate_scratch/scan-captures/stderr-record"
  t record-in-the-daemon-stderr FAIL predicate_data_dir_clean "$gate_scratch/scan-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout" "$gate_scratch/scan-captures/stderr-record"
  for field in kind seq app; do
    mkdir -p "$gate_scratch/field-$field/store"
    case $field in kind) printf '{"kind":"pointer"}\n' ;; seq) printf '{"seq":3}\n' ;; *) printf '{"app":"com.example.mail"}\n' ;; esac > "$gate_scratch/field-$field/store/record.json"
    t "field-name-$field-in-the-data-directory" FAIL predicate_data_dir_clean "$gate_scratch/field-$field" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  done
  mkdir -p "$gate_scratch/name-data/store" "$gate_scratch/dirname-data/store/r1gateMARKER-7"
  : > "$gate_scratch/name-data/store/r1gateMARKER-7.txt"
  t marker-in-a-file-name FAIL predicate_data_dir_clean "$gate_scratch/name-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  t marker-in-a-directory-name FAIL predicate_data_dir_clean "$gate_scratch/dirname-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  mkdir -p "$gate_scratch/utf16-data/store"
  perl -e 'print join("", map { $_ . "\0" } split //, "r1gateMARKER-7")' > "$gate_scratch/utf16-data/store/note"
  t marker-in-utf-16 FAIL predicate_data_dir_clean "$gate_scratch/utf16-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  mkdir -p "$gate_scratch/socket-data/run"
  (cd "$gate_scratch/socket-data/run" && python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' control.sock)
  t socket-in-the-data-directory PASS predicate_data_dir_clean "$gate_scratch/socket-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  mkdir -p "$gate_scratch/batch-data/store"
  printf 'r1gateMARKER-7 plaintext batch\n' > "$gate_scratch/batch-data/store/batch.txt"
  t plaintext-batch-file FAIL predicate_data_dir_clean "$gate_scratch/batch-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  t capture-file-missing FAIL predicate_data_dir_clean "$gate_scratch/scan-data" r1gateMARKER-7 "$gate_scratch/scan-captures/no-such-file"
  mkdir -p "$gate_scratch/field-data/store"
  printf '{"v":1,"seq":4,"kind":"keystroke"}\n' > "$gate_scratch/field-data/store/record.json"
  t record-field-name-in-a-file FAIL predicate_data_dir_clean "$gate_scratch/field-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  mkdir -p "$gate_scratch/link-data/store"
  ln -s "$gate_scratch/scan-captures/stdout" "$gate_scratch/link-data/store/link"
  t outer-temporary-file-that-appears-and-goes PASS outer_churn_changes_nothing
  t unreadable-file-in-the-run-tmpdir FAIL unreadable_run_file
  t unreadable-file-line-is-named PASS unreadable_line_is
  t unified-log-not-read-when-log-show-of-the-probe-fails PASS log_note_with_stub probe 3
  t unified-log-not-read-when-log-show-of-the-marker-fails PASS log_note_with_stub marker 5
  t symbolic-link-in-the-data-directory FAIL predicate_data_dir_clean "$gate_scratch/link-data" r1gateMARKER-7 "$gate_scratch/scan-captures/stdout"
  registered=$gate_scratch/registered
  mkdir -p "$registered/store/segments" "$registered/roots" "$registered/run" "$registered/logs"
  : > "$registered/daemon-state.json"; : > "$registered/store/store.key"; : > "$registered/store/store.db"; : > "$registered/roots/root-1"; : > "$registered/logs/daemon.err"
  t registered-names-only PASS predicate_registered_names "$registered"
  mkdir -p "$gate_scratch/unregistered"
  cp -R "$registered/." "$gate_scratch/unregistered/"
  : > "$gate_scratch/unregistered/store/store.db-shm"
  t unregistered-name-after-delete FAIL predicate_registered_names "$gate_scratch/unregistered"
  mkdir -p "$gate_scratch/derived"
  cp -R "$registered/." "$gate_scratch/derived/"
  mkdir -p "$gate_scratch/derived/derived"
  t reserved-name-after-delete FAIL predicate_registered_names "$gate_scratch/derived"
  mkdir -p "$gate_scratch/spaced"
  cp -R "$registered/." "$gate_scratch/spaced/"
  : > "$gate_scratch/spaced/store/store.db logs"
  t name-made-of-two-registered-names FAIL predicate_registered_names "$gate_scratch/spaced"
  mkdir -p "$gate_scratch/errlog"
  cp -R "$registered/." "$gate_scratch/errlog/"
  printf 'an error kind\n' > "$gate_scratch/errlog/logs/daemon.err"
  t daemon-err-not-empty-after-delete FAIL predicate_registered_names "$gate_scratch/errlog"
  name=service-run
}

check_main "$@"
