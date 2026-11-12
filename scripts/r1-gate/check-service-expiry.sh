#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=service-expiry

expiry_defect() {
  expiry_listed=$(inventory_days_before "$1" "$2")
  [ "$expiry_listed" -eq 0 ] || { printf 'service run measured: expired_days_listed; %s days before %s are still listed\n' "$expiry_listed" "$2"; return 1; }
  expiry_kept=$(inventory_days_from "$1" "$2")
  [ "$expiry_kept" -gt 0 ] || { printf 'no row inside the window of 14 days is left\n'; return 1; }
  case $3:$4:$5 in *[!0-9:]*|:*|*:) printf 'a segment count is not a number\n'; return 1 ;; esac
  [ "$(($3 - $4))" -ge "$5" ] && return 0
  printf 'expired segment files are still present: %s of %s segment files were removed, and the prefill wrote %s expired days\n' "$(($3 - $4))" "$3" "$5"
  return 1
}

predicate_expired() {
  expiry_defect "$@" > /dev/null
}

segment_files() {
  find "$1/store/segments" -type f -name '*.seg' 2>/dev/null | wc -l | tr -d ' '
}

expired_days_of() {
  expired_now=$1; expired_cut=$2; expired_n=0; expired_back=$3
  while [ "$expired_back" -ge 0 ]; do
    expired_day=$(date -u -r "$((expired_now - expired_back * 86400))" +%F)
    if expr "$expired_day" \< "$expired_cut" > /dev/null; then expired_n=$((expired_n + 1)); fi
    expired_back=$((expired_back - 1))
  done
  printf '%s\n' "$expired_n"
}

expiry_steps() {
  build_for_measure || return 1
  service_prepare || return 1
  take_bound expiry_bound_s || return 1; expiry_s=$bound
  take_bound stop_deadline_s || return 1; stop_s=$bound
  prefill_run prefill 170 --span-days 16 || { gate_defect="$gate_defect; not tried: --service-run, ctl"; return 1; }
  prefill_at=$(date +%s)
  segments_before=$(segment_files "$svc_data")
  state_file_write "$svc_data" false paused || { gate_defect="the state file could not be written"; return 1; }
  service_start run || { gate_defect="$gate_defect; not tried: ctl"; return 1; }
  started=$(date +%s)
  cut=$(date -u -r "$((started - 14 * 86400))" +%F)
  expired_days=$(expired_days_of "$prefill_at" "$cut" 16)
  while :; do
    ctl_or_defect expiry inventory || return 1
    listed=$(inventory_days_before "$ctl_out" "$cut")
    [ "$listed" -ne 0 ] || break
    expiry_now=$(date +%s)
    [ "$((expiry_now - started))" -lt "$expiry_s" ] || break
    sleep "$gate_poll_s"
  done
  expiry_took=$(( $(date +%s) - started ))
  segments_after=$(segment_files "$svc_data")
  found=$(expiry_defect "$ctl_out" "$cut" "$segments_before" "$segments_after" "$expired_days") || { gate_defect="$found, ${expiry_took}s after the start"; return 1; }
  stop_steps || return 1
}

measure() {
  expiry_took=''
  if expiry_steps; then ok=yes; else ok=no; fi
  stop_children
  where="paused, with its default switches; binary=${capture_bin#"$GATE_ROOT"/}"
  [ "$ok" = yes ] || { emit_fail "$name" "$gate_defect; $where"; return 1; }
  kept_days=$(inventory_days_from "$ctl_out" "$cut")
  emit_pass "$name" "no day before $cut is listed ${expiry_took}s after the start, $kept_days days inside the window are left, $((segments_before - segments_after)) of $segments_before segment files were removed for $expired_days expired days; $where"
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
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-10-30 modality=keystroke events=5 bytes=1\nrow day=2026-11-12 modality=pointer events=5 bytes=1\ntotal events=10 bytes=2 disk_bytes=1\n' > "$gate_scratch/inv-window"
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-10-28 modality=keystroke events=5 bytes=1\nrow day=2026-11-12 modality=pointer events=5 bytes=1\ntotal events=10 bytes=2 disk_bytes=1\n' > "$gate_scratch/inv-15-days"
  printf 'store_id=0123456789abcdef0123456789abcdef\ntotal events=0 bytes=0 disk_bytes=1\n' > "$gate_scratch/inv-empty"
  t expired-days-gone PASS predicate_expired "$gate_scratch/inv-window" 2026-10-29 17 15 2
  t row-15-days-old FAIL predicate_expired "$gate_scratch/inv-15-days" 2026-10-29 17 15 2
  t no-row-left FAIL predicate_expired "$gate_scratch/inv-empty" 2026-10-29 17 0 2
  t expired-file-present FAIL predicate_expired "$gate_scratch/inv-window" 2026-10-29 17 17 2
  t expired-row-is-named-by-its-key FAIL fails_saying 'measured: expired_days_listed' expiry_line "$gate_scratch/inv-15-days"
  t standin-good-run PASS against_the_standin good none
  t standin-keeps-expired-files FAIL against_the_standin expiry-keeps-files none
  t standin-main-names-the-mode PASS says 'FAIL service-expiry (no --prefill-store mode' main none
}

expiry_line() {
  found=$(expiry_defect "$1" 2026-10-29 17 15 2) || { emit_fail "$name" "$found"; return 1; }
  emit_pass "$name"
}

check_main "$@"
