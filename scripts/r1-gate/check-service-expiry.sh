#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=service-expiry

segment_list() {
  sed -n 's/^segment day=\([0-9-]*\) file=\([^ ]*\)$/\1 \2/p' "$1" > "$3" 2>/dev/null
  [ -s "$3" ] || return 1
  : > "$3.hashed"
  while read -r list_day list_file; do
    list_hash=$(shasum -a 256 "$2/$list_file" 2>/dev/null | cut -d ' ' -f 1)
    printf '%s %s %s\n' "$list_day" "${list_hash:-none}" "$list_file" >> "$3.hashed"
  done < "$3"
}

listing_defect() {
  while read -r _ listing_file; do
    case /$listing_file/ in
      //* | */../*) printf 'the prefill named a segment file outside the data directory: %s\n' "$listing_file"; return 1 ;;
      *) ;;
    esac
  done < "$1"
  listing_files=$(cd "$2" 2>/dev/null && find store/segments -type f 2>/dev/null)
  [ -n "$listing_files" ] || return 0
  listing_left=$(printf '%s\n' "$listing_files" | awk 'FILENAME == ARGV[1] { named[$2] = 1; next } !($0 in named) { print; exit }' "$1" -)
  [ -z "$listing_left" ] || { printf 'a file under store/segments that no segment line of the prefill names: %s\n' "$listing_left"; return 1; }
}

prefill_listed() {
  segment_list "$1" "$2" "$3" || { printf 'the prefill printed no line segment day=<day> file=<path> (P3)\n'; return 1; }
  listing_defect "$3" "$2"
}

files_with_hash() {
  find "$@" -type f -exec shasum -a 256 {} + 2>/dev/null
}

expiry_defect() {
  expiry_listed=$(inventory_days_before "$1" "$2")
  [ "$expiry_listed" -eq 0 ] || { printf 'service run measured: expired_days_listed; %s days before %s are still listed\n' "$expiry_listed" "$2"; return 1; }
  expiry_kept=$(inventory_days_from "$1" "$2")
  [ "$expiry_kept" -gt 0 ] || { printf 'no row inside the window of 14 days is left\n'; return 1; }
  [ -s "$3" ] || { printf 'the prefill printed no segment line for a day, so the files of the expired days cannot be named (P3)\n'; return 1; }
  expiry_hashes=$(files_with_hash "$4" "$5")
  expiry_gone=0; expiry_kept_files=0
  while read -r seg_day seg_hash seg_file; do
    if expr "$seg_day" \< "$2" > /dev/null; then
      [ ! -e "$4/$seg_file" ] || { printf 'the file of the expired day %s is still there: %s\n' "$seg_day" "$seg_file"; return 1; }
      seg_copy=$(printf '%s\n' "$expiry_hashes" | awk -v h="$seg_hash" '$1 == h { $1 = ""; sub(/^ /, ""); print; exit }')
      [ -z "$seg_copy" ] || { printf 'the bytes of the expired day %s are still there under another name: %s\n' "$seg_day" "$seg_copy"; return 1; }
      expiry_gone=$((expiry_gone + 1))
    else
      [ -f "$4/$seg_file" ] || { printf 'the file of the kept day %s is gone: %s\n' "$seg_day" "$seg_file"; return 1; }
      expiry_kept_files=$((expiry_kept_files + 1))
    fi
  done < "$3"
  [ "$expiry_gone" -gt 0 ] || { printf 'the prefill named no file of a day before %s, so expiry was not tried\n' "$2"; return 1; }
}

predicate_expired() {
  expiry_defect "$@" > /dev/null
}

expiry_steps() {
  build_for_measure || return 1
  service_prepare || return 1
  privacy_rows_line
  take_bound expiry_bound_s || return 1; expiry_s=$bound
  take_bound stop_deadline_s || return 1; stop_s=$bound
  prefill_run prefill 170 --span-days 16 || { gate_defect="$gate_defect; not tried: --service-run, ctl"; return 1; }
  found=$(prefill_listed "$svc_dir/prefill.stdout" "$svc_data" "$svc_dir/segments") || { gate_defect=$found; return 1; }
  state_file_write "$svc_data" false paused || { gate_defect="the state file could not be written"; return 1; }
  service_start run || { gate_defect="$gate_defect; not tried: ctl"; return 1; }
  started=$(date +%s)
  cut=$(date -u -r "$((started - 14 * 86400))" +%F)
  while :; do
    ctl_or_defect expiry inventory || return 1
    listed=$(inventory_days_before "$ctl_out" "$cut")
    [ "$listed" -ne 0 ] || break
    expiry_now=$(date +%s)
    [ "$((expiry_now - started))" -lt "$expiry_s" ] || break
    sleep "$gate_poll_s"
  done
  expiry_inventory=$ctl_out
  expiry_took=$(( $(date +%s) - started ))
  found=$(expiry_defect "$expiry_inventory" "$cut" "$svc_dir/segments.hashed" "$svc_data" "$svc_proc_tmp") || { gate_defect="$found, ${expiry_took}s after the start"; return 1; }
  expiry_defect "$expiry_inventory" "$cut" "$svc_dir/segments.hashed" "$svc_data" "$svc_proc_tmp" > /dev/null
  stop_steps || return 1
}

measure() {
  expiry_took=''
  if expiry_steps; then ok=yes; else ok=no; fi
  stop_children
  where="paused, with its default switches; binary=${capture_bin#"$GATE_ROOT"/}"
  [ "$ok" = yes ] || { emit_fail "$name" "$gate_defect; $where"; return 1; }
  kept_days=$(inventory_days_from "$expiry_inventory" "$cut")
  emit_pass "$name" "no day before $cut is listed ${expiry_took}s after the start and $kept_days days from $cut on are; the $expiry_gone files that the prefill named for the days before $cut are gone by name and their bytes are nowhere under the data directory or the run's TMPDIR, and its $expiry_kept_files files of the kept days are there; $where"
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

expiry_store() {
  expiry_dir=$gate_scratch/expiry-$1
  mkdir -p "$expiry_dir/data/store/segments/00" "$expiry_dir/tmp"
  printf 'bytes of 2026-10-27\n' > "$expiry_dir/old-27"
  printf 'bytes of 2026-10-28\n' > "$expiry_dir/old-28"
  printf 'bytes of 2026-11-11\n' > "$expiry_dir/data/store/segments/00/c.seg"
  printf 'bytes of 2026-11-12\n' > "$expiry_dir/data/store/segments/00/d.seg"
  old27=$(shasum -a 256 "$expiry_dir/old-27" | cut -d ' ' -f 1)
  old28=$(shasum -a 256 "$expiry_dir/old-28" | cut -d ' ' -f 1)
  new11=$(shasum -a 256 "$expiry_dir/data/store/segments/00/c.seg" | cut -d ' ' -f 1)
  new12=$(shasum -a 256 "$expiry_dir/data/store/segments/00/d.seg" | cut -d ' ' -f 1)
  printf '2026-10-27 %s store/segments/00/a.seg\n2026-10-28 %s store/segments/00/b.seg\n2026-11-11 %s store/segments/00/c.seg\n2026-11-12 %s store/segments/00/d.seg\n' "$old27" "$old28" "$new11" "$new12" > "$expiry_dir/segments"
  printf '%s\n' "$expiry_dir"
}

expiry_case() {
  expiry_case_dir=$1; shift
  predicate_expired "$gate_scratch/inv-window" 2026-10-29 "$expiry_case_dir/segments" "$expiry_case_dir/data" "$expiry_case_dir/tmp"
}

cases() {
  scratch_root
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-11-11 modality=keystroke events=5 bytes=1\nrow day=2026-11-12 modality=pointer events=5 bytes=1\ntotal events=10 bytes=2 disk_bytes=1\n' > "$gate_scratch/inv-window"
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-10-28 modality=keystroke events=5 bytes=1\nrow day=2026-11-12 modality=pointer events=5 bytes=1\ntotal events=10 bytes=2 disk_bytes=1\n' > "$gate_scratch/inv-15-days"
  printf 'store_id=0123456789abcdef0123456789abcdef\ntotal events=0 bytes=0 disk_bytes=1\n' > "$gate_scratch/inv-empty"
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow modality=keystroke day=2026-11-11 events=5 bytes=1\nrow modality=pointer events=5 bytes=1\ntotal events=10 bytes=2 disk_bytes=1\n' > "$gate_scratch/inv-no-day"
  good=$(expiry_store good)
  t expired-days-gone PASS expiry_case "$good"
  t row-15-days-old FAIL predicate_expired "$gate_scratch/inv-15-days" 2026-10-29 "$good/segments" "$good/data" "$good/tmp"
  t no-row-left FAIL predicate_expired "$gate_scratch/inv-empty" 2026-10-29 "$good/segments" "$good/data" "$good/tmp"
  t row-without-its-day FAIL predicate_expired "$gate_scratch/inv-no-day" 2026-10-29 "$good/segments" "$good/data" "$good/tmp"
  present=$(expiry_store present)
  cp "$present/old-27" "$present/data/store/segments/00/a.seg"
  t expired-file-present FAIL expiry_case "$present"
  renamed=$(expiry_store renamed)
  cp "$renamed/old-28" "$renamed/data/store/segments/00/b.kept"
  t expired-bytes-under-another-name FAIL expiry_case "$renamed"
  moved=$(expiry_store moved)
  mkdir -p "$moved/data/store/expired"
  cp "$moved/old-27" "$moved/data/store/expired/a.seg"
  t expired-file-moved-into-another-directory FAIL expiry_case "$moved"
  spilled=$(expiry_store spilled)
  cp "$spilled/old-28" "$spilled/tmp/spill"
  t expired-bytes-in-the-run-tmpdir FAIL expiry_case "$spilled"
  wrong=$(expiry_store wrong)
  mv "$wrong/data/store/segments/00/d.seg" "$wrong/d.away"
  t newer-file-deleted-instead FAIL expiry_case "$wrong"
  : > "$gate_scratch/no-segments"
  t prefill-named-no-segment FAIL predicate_expired "$gate_scratch/inv-window" 2026-10-29 "$gate_scratch/no-segments" "$good/data" "$good/tmp"
  listed=$(listing_store every-file)
  t prefill-listing-names-every-file PASS listing_line "$listed"
  left_out=$(listing_store left-out)
  printf 'bytes of 2026-10-27, a second file\n' > "$left_out/data/store/segments/00/seg-2026-10-27-second.seg"
  t file-of-an-expired-day-left-out-of-the-listing FAIL fails_saying 'no segment line of the prefill names: store/segments/00/seg-2026-10-27-second.seg' listing_line "$left_out"
  stepped=$(listing_store steps-out)
  printf 'bytes of 2026-10-26\n' > "$stepped/outside.seg"
  printf 'segment day=2026-10-26 file=store/segments/../../../outside.seg\n' >> "$stepped/prefill.stdout"
  t segment-line-that-steps-out-of-the-data-directory FAIL fails_saying 'outside the data directory: store/segments/../../../outside.seg' listing_line "$stepped"
  absolute=$(listing_store absolute)
  printf 'bytes of 2026-10-26\n' > "$absolute/outside.seg"
  printf 'segment day=2026-10-26 file=%s/outside.seg\n' "$absolute" >> "$absolute/prefill.stdout"
  t segment-line-with-an-absolute-path FAIL fails_saying "outside the data directory: $absolute/outside.seg" listing_line "$absolute"
  t expired-row-is-named-by-its-key FAIL fails_saying 'measured: expired_days_listed' expiry_line "$gate_scratch/inv-15-days" "$good"
  t standin-good-run PASS against_the_standin good none
  t standin-keeps-expired-files FAIL against_the_standin expiry-keeps-files none
  t standin-expires-after-the-bound FAIL against_the_standin expiry-late none
  t standin-main-names-the-mode PASS says 'FAIL service-expiry (no --prefill-store mode' main none
  t privacy-line-before-the-service PASS privacy_line_first main none
}

listing_store() {
  listing_dir=$gate_scratch/listing-$1
  mkdir -p "$listing_dir/data/store/segments/00"
  printf 'prefill: 4 days\n' > "$listing_dir/prefill.stdout"
  for listing_day in 2026-10-27 2026-10-28 2026-11-11 2026-11-12; do
    printf 'bytes of %s\n' "$listing_day" > "$listing_dir/data/store/segments/00/seg-$listing_day.seg"
    printf 'segment day=%s file=store/segments/00/seg-%s.seg\n' "$listing_day" "$listing_day" >> "$listing_dir/prefill.stdout"
  done
  printf '%s\n' "$listing_dir"
}

listing_line() {
  found=$(prefill_listed "$1/prefill.stdout" "$1/data" "$1/segments") || { emit_fail "$name" "$found"; return 1; }
  emit_pass "$name"
}

expiry_line() {
  found=$(expiry_defect "$1" 2026-10-29 "$2/segments" "$2/data" "$2/tmp") || { emit_fail "$name" "$found"; return 1; }
  emit_pass "$name"
}

check_main "$@"
