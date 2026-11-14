#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=service-memory
has_children=no

prefill_defect() {
  prefill_n=$(report_token "$1" events)
  [ "$prefill_n" = "$3" ] || { printf 'the prefill printed events=%s, and the bound prefill_events asks %s\n' "${prefill_n:-absent}" "$3"; return 1; }
  case $2 in ''|*[!0-9]*) printf 'the time file of the prefill holds no maximum resident set size\n'; return 1 ;; esac
  [ "$2" -le "$4" ] && return 0
  printf 'the prefill helper peaked at ru_maxrss=%sB above rss_max_bytes=%s\n' "$2" "$4"
  return 1
}

predicate_prefill() {
  prefill_defect "$@" > /dev/null
}

pid_defect() {
  case $1 in ''|*[!0-9]*) printf 'status holds no pid\n'; return 1 ;; esac
  [ "$1" = "$2" ] && return 0
  printf 'status says pid=%s, and the child of the time wrapper is %s\n' "$1" "${2:-none}"
  return 1
}

predicate_pid_is_the_child() {
  pid_defect "$@" > /dev/null
}

peak_defect() {
  case $1 in ''|*[!0-9]*) printf 'the daemon reported no peak of its own (rss_peak_bytes=%s)\n' "${1:-absent}"; return 1 ;; esac
  case $2 in ''|*[!0-9]*) printf 'ps took no sample of the daemon\n'; return 1 ;; esac
  [ "$1" -ge "$2" ] || { printf 'the daemon reported rss_peak_bytes=%s, below the ps maximum %sB of its pid\n' "$1" "$2"; return 1; }
  peak_verdict=$1
  shift 2
  for peak_other in "$@"; do
    case $peak_other in ''|*[!0-9]*) printf 'a figure of the time wrapper is not a number\n'; return 1 ;; esac
    [ "$peak_other" -le "$peak_verdict" ] || peak_verdict=$peak_other
  done
}

peak_bound_defect() {
  peak_defect "$1" "$2" ${4:+$4} ${5:+$5} || return 1
  [ "$peak_verdict" -le "$3" ] && return 0
  printf 'the verdict peak %sB is above rss_max_bytes=%s\n' "$peak_verdict" "$3"
  return 1
}

predicate_peak() {
  peak_bound_defect "$@" > /dev/null
}

delete_defect() {
  deleted_file=$1; inventory_file=$2; data=$3; old_id=$4; new_id=$5; elapsed_ms=$6; bound_s=$7; segment_hashes=$8; run_tmp=$9
  case $(head -1 "$deleted_file" 2>/dev/null) in deleted\ *) ;; *) printf 'ctl delete-all printed no deleted line\n'; return 1 ;; esac
  deleted_line=$(sed -n 1p "$deleted_file")
  wrong=$(report_defects "$deleted_line" root_destroyed=yes files_left=0 database_left=no)
  [ -z "$wrong" ] || { printf 'the deleted line measured: %s\n' "${wrong% }"; return 1; }
  total_line=$(sed -n 's/^\(total events=[0-9]* bytes=[0-9]*\) .*/\1/p' "$inventory_file")
  [ "$total_line" = 'total events=0 bytes=0' ] ||
    { printf 'the inventory after the delete-all is not total events=0 bytes=0\n'; return 1; }
  ! grep -q '^row ' "$inventory_file" || { printf 'the inventory after the delete-all still lists rows beside its total of zero\n'; return 1; }
  copies=$(find "$data" "$run_tmp" -type f -exec shasum -a 256 {} + 2>/dev/null | awk 'NR == FNR { old[$1] = 1; next } ($1 in old) { $1 = ""; sub(/^ /, ""); print; exit }' "$segment_hashes" -)
  [ -z "$copies" ] || { printf 'a copy of a segment file is left after the delete-all: %s\n' "$copies"; return 1; }
  left=$(find "$data/store/segments" -type f 2>/dev/null | head -1)
  [ -z "$left" ] || { printf 'a file is left under store/segments after the delete-all\n'; return 1; }
  case $new_id in ''|-) printf 'no fresh store id after the delete-all\n'; return 1 ;; esac
  [ "$new_id" != "$old_id" ] || { printf 'the store id after the delete-all is the old one, %s\n' "$old_id"; return 1; }
  old_root=$(find "$data" -name "*$old_id*" 2>/dev/null | head -1)
  [ -z "$old_root" ] || { printf 'a file named for the old store id is left: %s\n' "${old_root#"$data"/}"; return 1; }
  names=$(registered_names_defect "$data") || { printf '%s\n' "$names"; return 1; }
  case $elapsed_ms:$bound_s in *[!0-9:]*|:*|*:) printf 'the time or the bound delete_all_max_s is not a number: %s ms, %s s\n' "$elapsed_ms" "$bound_s"; return 1 ;; esac
  [ "$elapsed_ms" -le "$((bound_s * 1000))" ] && return 0
  printf 'ctl delete-all took %s ms, above delete_all_max_s=%s\n' "$elapsed_ms" "$bound_s"
  return 1
}

predicate_delete_all() {
  delete_defect "$@" > /dev/null
}

now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'
}

keychain_defect() {
  security find-generic-password -a "$1" > /dev/null 2>&1
  keychain=$?
  [ "$keychain" -eq 44 ] && return 0
  printf 'security find-generic-password for the old store id exited %s, not 44: a keychain item came back\n' "$keychain"
  return 1
}

second_delete_defect() {
  case $(sed -n 1p "$1" 2>/dev/null) in already_deleted\ *) return 0 ;; esac
  printf 'the second delete-all with the old id printed no already_deleted line\n'
  return 1
}

second_delete_ok() {
  second_delete_defect "$1" > /dev/null
}

keychain_with() {
  mkdir -p "$gate_scratch/keychain-$1"
  printf '#!/bin/sh\nexit %s\n' "$1" > "$gate_scratch/keychain-$1/security"
  chmod 755 "$gate_scratch/keychain-$1/security"
  PATH="$gate_scratch/keychain-$1:$PATH" keychain_defect 0123456789abcdef0123456789abcdef > /dev/null
}

delete_steps() {
  ctl_or_defect before-delete status || return 1
  old_id=$(status_value "$ctl_out" store_id)
  find "$svc_data/store/segments" -type f -exec shasum -a 256 {} + 2>/dev/null > "$svc_dir/segment-hashes"
  delete_start=$(now_ms)
  ctl_or_defect delete-all delete-all "$old_id" || return 1
  delete_ms=$(( $(now_ms) - delete_start ))
  deleted_out=$ctl_out
  ctl_or_defect after-delete inventory || return 1
  after_inventory=$ctl_out
  ctl_or_defect after-delete-status status || return 1
  new_id=$(status_value "$ctl_out" store_id)
  delete_found=$(keychain_defect "$old_id") || return 1
  if use_bound delete_all_max_s; then delete_bound=$bound; else delete_bound='unset'; fi
  delete_found=$(delete_defect "$deleted_out" "$after_inventory" "$svc_data" "$old_id" "$new_id" "$delete_ms" "$delete_bound" "$svc_dir/segment-hashes" "$svc_proc_tmp") || return 1
  ctl_or_defect delete-again delete-all "$old_id" || { delete_found="the second delete-all with the old id: $gate_defect"; return 1; }
  delete_found=$(second_delete_defect "$ctl_out") || return 1
}

sample_until() {
  sample_end=$(( $(date +%s) + $1 ))
  next_inventory=$(( $(date +%s) + 15 ))
  ps_max_kib=0
  ps_samples=0
  while sample_now=$(date +%s) && [ "$sample_now" -lt "$sample_end" ]; do
    sample_rss=$(ps -o rss= -p "$daemon_pid" 2>/dev/null | tr -d ' ')
    case $sample_rss in ''|*[!0-9]*) ;; *) ps_samples=$((ps_samples + 1)); [ "$sample_rss" -le "$ps_max_kib" ] || ps_max_kib=$sample_rss ;; esac
    if [ "$((sample_now - svc_claimed))" -ge "$gate_claim_every_s" ]; then
      claim || { gate_defect=$(ctl_defect "$ctl_status" "$ctl_out.err" status); return 1; }
      last_status=$ctl_out
    fi
    if [ "$sample_now" -ge "$next_inventory" ]; then
      ctl_or_defect poll inventory || return 1
      next_inventory=$(( $(date +%s) + 15 ))
    fi
    sleep "$gate_poll_s"
  done
}

memory_steps() {
  build_for_measure || return 1
  service_prepare || return 1
  privacy_rows_line
  take_bound prefill_events || return 1; prefill_n=$bound
  take_bound rss_max_bytes || return 1; rss_max=$bound
  take_bound lease_bound_s || return 1; lease_s=$bound
  take_bound stop_deadline_s || return 1; stop_s=$bound
  take_bound batch_age_s || return 1; batch_s=$bound
  prefill_run prefill "$prefill_n" || { gate_defect="$gate_defect; not tried: --service-run, ctl"; return 1; }
  prefill_rss=$(time_file_number "$svc_dir/prefill.time" "maximum resident set size")
  found=$(prefill_defect "$svc_dir/prefill.stdout" "$prefill_rss" "$prefill_n" "$rss_max") || { gate_defect=$found; return 1; }
  state_file_write "$svc_data" true paused || { gate_defect="the state file could not be written"; return 1; }
  helper_outside_start "$(($1 + 300))" || return 1
  service_start run time || { gate_defect="$gate_defect; not tried: ctl"; return 1; }
  daemon_pid=$(daemon_of_wrapper)
  status_pid=$(status_value "$ctl_out" pid)
  found=$(pid_defect "$status_pid" "$daemon_pid") || { gate_defect=$found; return 1; }
  ctl_or_defect resume resume || return 1
  hold_for "$((batch_s + gate_slack_s))" || return 1
  claim || { gate_defect=$(ctl_defect "$ctl_status" "$ctl_out.err" status); return 1; }
  found=$(sources_running_defect "$ctl_out" foreground screen input) || { gate_defect="at the first poll: $found"; return 1; }
  ctl_or_defect first inventory || return 1
  first_inventory=$ctl_out
  last_status=''
  sample_until "$1" || return 1
  [ "$ps_samples" -gt 0 ] || { gate_defect="ps took no sample of the daemon, so its peak was not measured"; return 1; }
  claim || { gate_defect=$(ctl_defect "$ctl_status" "$ctl_out.err" status); return 1; }
  last_status=$ctl_out
  found=$(sources_running_defect "$last_status" foreground screen input) || { gate_defect="at the last poll: $found"; return 1; }
  ctl_or_defect last inventory || return 1
  found=$(growth_defect "$first_inventory" "$ctl_out") || { gate_defect=$found; return 1; }
  total_a=$(inventory_total "$first_inventory"); total_b=$(inventory_total "$ctl_out")
  memory_counts="events $total_a to $total_b over ${1}s"
  self_peak=$(status_value "$last_status" rss_peak_bytes)
  lease_control || return 1
  if delete_steps; then delete_ok=yes; else delete_ok=no; fi
  stop_steps || return 1
  helper_outside_stop
  wrapper_rss=$(time_file_number "$svc_dir/run.time" "maximum resident set size")
  wrapper_footprint=$(time_file_number "$svc_dir/run.time" "peak memory footprint")
  if [ "$has_children" = yes ]; then
    found=$(peak_bound_defect "$self_peak" "$((ps_max_kib * 1024))" "$rss_max") || { gate_defect=$found; return 1; }
  else
    found=$(peak_bound_defect "$self_peak" "$((ps_max_kib * 1024))" "$rss_max" "${wrapper_rss:-x}" "${wrapper_footprint:-x}") || { gate_defect=$found; return 1; }
  fi
  if [ "$has_children" = yes ]; then peak_defect "$self_peak" "$((ps_max_kib * 1024))" > /dev/null; else peak_defect "$self_peak" "$((ps_max_kib * 1024))" "$wrapper_rss" "$wrapper_footprint" > /dev/null; fi
  memory_figures="peak=${peak_verdict}B (rss_peak_bytes=$self_peak ps_max=${ps_max_kib}KiB wrapper ru_maxrss=${wrapper_rss:-unread}B footprint=${wrapper_footprint:-unread}B$( [ "$has_children" = no ] || printf ', wrapper figures printed only: --daemon-has-children'))"
}

measure() {
  memory_counts=''; memory_figures=''; delete_ok=''; delete_found=''; delete_ms=''
  if memory_steps "$1"; then ok=yes; else ok=no; fi
  stop_children
  where=$(service_line)
  rc=0
  if [ "$ok" = yes ]; then
    emit_pass "$name" "$memory_figures; $memory_counts; prefill_events=$prefill_n ru_maxrss=${prefill_rss}B; lease lapsed into no_indicator; $where"
  else
    emit_fail "$name" "$gate_defect; $where"; rc=1
  fi
  case $delete_ok in
    yes) emit_line PASS service-delete "ctl delete-all took ${delete_ms} ms on the store of prefill_events; root destroyed, no file left, a new store id, the old root gone, only registered names, already_deleted on the second call; $where" ;;
    no) emit_line FAIL service-delete "${delete_found:-$gate_defect}${delete_ms:+; ctl delete-all took ${delete_ms} ms}; $where"; rc=1 ;;
    *) emit_line FAIL service-delete "not run: $gate_defect; $where"; rc=1 ;;
  esac
  return "$rc"
}

against_the_standin() {
  service_standin "$1" "$2"
  GATE_OVERRIDE_delete_all_max_s=5 measure 3
}

says() {
  says_text=$1; shift
  service_standin "$1" "$2"
  says_out=$(GATE_OVERRIDE_delete_all_max_s=5 measure 3 2>/dev/null)
  case $says_out in *"$says_text"*) return 0 ;; esac
  return 1
}

delete_case() {
  delete_dir=$gate_scratch/delete-$1
  mkdir -p "$delete_dir/store/segments" "$delete_dir/roots" "$delete_dir/run" "$delete_dir/logs"
  : > "$delete_dir/daemon-state.json"; : > "$delete_dir/store/store.key"; : > "$delete_dir/store/store.db"; : > "$delete_dir/roots/root-$2"; : > "$delete_dir/logs/daemon.err"
  printf '%s\n' "$delete_dir"
}

cases() {
  scratch_root
  printf 'prefilled events=60 segments=2 store_id=0123456789abcdef0123456789abcdef elapsed_ms=10\n' > "$gate_scratch/prefilled-60"
  printf 'prefilled events=0 segments=0 store_id=0123456789abcdef0123456789abcdef elapsed_ms=1\n' > "$gate_scratch/prefilled-0"
  t prefill-within-bound PASS predicate_prefill "$gate_scratch/prefilled-60" 30000000 60 50331648
  t prefill-maxrss-above-bound FAIL predicate_prefill "$gate_scratch/prefilled-60" 224000000 60 50331648
  t n-0 FAIL predicate_prefill "$gate_scratch/prefilled-0" 3000000 60 50331648
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-11-12 modality=foreground_app events=4 bytes=1\ntotal events=4 bytes=1 disk_bytes=1\n' > "$gate_scratch/inv-4"
  printf 'store_id=0123456789abcdef0123456789abcdef\nrow day=2026-11-12 modality=foreground_app events=9 bytes=1\ntotal events=9 bytes=1 disk_bytes=1\n' > "$gate_scratch/inv-9"
  t growth PASS predicate_growth "$gate_scratch/inv-4" "$gate_scratch/inv-9"
  t no-growth FAIL predicate_growth "$gate_scratch/inv-9" "$gate_scratch/inv-9"
  running=$(c2_fixture c2-status-macos.txt running 's/^switch name=screen_activity value=off/switch name=screen_activity value=on/; s/^source name=screen state=off/source name=screen state=running/')
  failed=$(c2_fixture c2-status-macos.txt failed-at-end 's/^switch name=screen_activity value=off/switch name=screen_activity value=on/; s/^source name=screen state=off/source name=screen state=failed reason=stream_suspended/; s/^capture=running/capture=partial/')
  t sources-running-at-end PASS predicate_sources_running "$running" foreground screen input
  t source-failed-at-end FAIL predicate_sources_running "$failed" foreground screen input
  t pid-is-the-child PASS predicate_pid_is_the_child 4242 4242
  t pid-mismatch FAIL predicate_pid_is_the_child 4242 4243
  t peak-within-bound PASS predicate_peak 30000000 29000000 50331648 20000000 9000000
  t no-self-reported-peak FAIL predicate_peak "" 29000000 50331648 20000000 9000000
  t self-peak-below-ps FAIL predicate_peak 20000000 29000000 50331648 20000000 9000000
  t wrapper-figure-above-bound FAIL predicate_peak 30000000 29000000 50331648 60000000 9000000
  t constant-peak-above-bound FAIL predicate_peak 60000000 29000000 50331648 20000000 9000000
  t lease-lapsed PASS predicate_lease_lapsed "$GATE_FIXTURES/c2-status-no-indicator.txt" "$gate_scratch/inv-9" "$gate_scratch/inv-9"
  t grows-after-lease-lapse FAIL predicate_lease_lapsed "$GATE_FIXTURES/c2-status-no-indicator.txt" "$gate_scratch/inv-4" "$gate_scratch/inv-9"
  t standin-good-run PASS against_the_standin good none
  t standin-main-names-the-mode PASS says 'FAIL service-memory (no --prefill-store mode' main none
  t standin-main-fails FAIL against_the_standin main none
  t standin-status-pid-is-not-the-child FAIL against_the_standin wrong-pid none
  t privacy-line-before-the-service PASS privacy_line_first main none
  name=service-delete
  old=0123456789abcdef0123456789abcdef
  new=fedcba9876543210fedcba9876543210
  printf 'store_id=%s\ntotal events=0 bytes=0 disk_bytes=4096\n' "$new" > "$gate_scratch/inv-empty"
  mkdir -p "$gate_scratch/delete-tmp"
  printf 'segment bytes of the old store\n' > "$gate_scratch/old-segment"
  shasum -a 256 "$gate_scratch/old-segment" > "$gate_scratch/segment-hashes"
  good=$(delete_case good "$new")
  t delete-all-good PASS predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$good" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  t time-above-bound FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$good" "$old" "$new" 5001 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  t bound-unset FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$good" "$old" "$new" 900 unset "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  t same-store-id FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$good" "$old" "$old" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  left=$(delete_case segment-left "$new")
  mkdir -p "$left/store/segments/00" && : > "$left/store/segments/00/0123.seg"
  t segment-left FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$left" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  root=$(delete_case root-still-there "$old")
  t root-still-there FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$root" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  outside=$(delete_case outside "$new")
  : > "$outside/notes.txt"
  t file-outside-registered-names FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$outside" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  sed 's/root_destroyed=yes/root_destroyed=no/' "$GATE_FIXTURES/c2-deleted.txt" > "$gate_scratch/root-survived"
  t root-not-destroyed FAIL predicate_delete_all "$gate_scratch/root-survived" "$gate_scratch/inv-empty" "$good" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  printf 'store_id=%s\nrow day=2026-11-12 modality=keystroke events=3 bytes=96\ntotal events=0 bytes=0 disk_bytes=4096\n' "$new" > "$gate_scratch/inv-rows-beside-zero"
  t rows-beside-a-total-of-zero FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-rows-beside-zero" "$good" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  two=$(delete_case two-roots "$new")
  : > "$two/roots/root-another"
  t two-root-files FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$two" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  mkdir -p "$gate_scratch/tmp-with-a-copy"
  cp "$gate_scratch/old-segment" "$gate_scratch/tmp-with-a-copy/cache.bin"
  t copy-of-a-segment-in-the-run-tmpdir FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$good" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/tmp-with-a-copy"
  kept=$(delete_case copy-in-data "$new")
  cp "$gate_scratch/old-segment" "$kept/store/store.db-wal"
  t copy-of-a-segment-in-the-data-directory FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$kept" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  spaced=$(delete_case spaced "$new")
  : > "$spaced/store/store.db logs"
  t name-made-of-two-registered-names FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-empty" "$spaced" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  t keychain-has-no-item-of-the-old-store PASS keychain_with 44
  t keychain-item-came-back FAIL keychain_with 0
  printf 'already_deleted store_id=%s\n' "$new" > "$gate_scratch/second-already"
  printf 'deleted segments_shredded=0 keys_destroyed=1 root_destroyed=yes root_id=- files_left=0 database_left=no entries_removed=4 entries_left=0 unknown_entries=0 intent=written store_id=%s\n' "$new" > "$gate_scratch/second-deleted"
  t second-delete-answers-already-deleted PASS second_delete_ok "$gate_scratch/second-already"
  t second-delete-deletes-again FAIL second_delete_ok "$gate_scratch/second-deleted"
  t events-left FAIL predicate_delete_all "$GATE_FIXTURES/c2-deleted.txt" "$gate_scratch/inv-9" "$good" "$old" "$new" 900 5 "$gate_scratch/segment-hashes" "$gate_scratch/delete-tmp"
  name=service-memory
}

for argument do
  shift
  if [ "$argument" = --daemon-has-children ]; then has_children=yes; continue; fi
  set -- "$@" "$argument"
done
check_main "$@"
