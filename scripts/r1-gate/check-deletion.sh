#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=deletion

good="segment=42 shred=ok file_gone=yes decryptable=no sibling_readable=yes remaining_keys=1 wal_bytes=0 delete_all_keys=2 root_destroyed=yes store_sealed=yes"

with() { printf '%s\n' "$good" | sed "$1"; }

defects_are() {
  defects=$(deletion_report_defects "$1")
  [ "$defects" = "$2" ]
}

cases() {
  scratch_root
  standin_build || { emit_fail "self-test:$name" "cc could not build helper/load-standin.c"; return 1; }
  t standin-deletion PASS against_the_standin ok none
  t standin-root-kept FAIL against_the_standin root-kept none
  t standin-binary-fails FAIL against_the_standin exit-1 none
  t standin-report-then-exit-1 FAIL against_the_standin report-then-exit-1 none
  t standin-verify-past-its-deadline FAIL fails_saying 'verify-deletion ran past its deadline of 1s and was ended' past_the_deadline
  t term-ends-the-verify-child PASS signal_leaves_no_child sh TERM
  if command -v dash > /dev/null 2>&1; then
    t term-ends-the-verify-child-under-dash PASS signal_leaves_no_child dash TERM
  fi
  for pair in decryptable=no file_gone=yes sibling_readable=yes wal_bytes=0 root_destroyed=yes; do
    line=$(with "s/ $pair//")
    t "${pair%%=*}-absent" FAIL predicate_deletion_report "$line"
  done
  t good-line PASS predicate_deletion_report "$good"
  line=$(with 's/decryptable=no/decryptable=yes/'); t decryptable-yes FAIL predicate_deletion_report "$line"
  line=$(with 's/file_gone=yes/file_gone=no/'); t file-gone-no FAIL predicate_deletion_report "$line"
  line=$(with 's/sibling_readable=yes/sibling_readable=no/'); t sibling-unreadable FAIL predicate_deletion_report "$line"
  line=$(with 's/wal_bytes=0/wal_bytes=78312/'); t wal-bytes-78312 FAIL predicate_deletion_report "$line"
  line=$(with 's/root_destroyed=yes/root_destroyed=no/'); t root-not-destroyed FAIL predicate_deletion_report "$line"
  line=$(with 's/store_sealed=yes/store_sealed=no/'); t store-not-sealed FAIL predicate_deletion_report "$line"
  t field-missing FAIL predicate_deletion_report "segment=42 shred=ok decryptable=no"
  line=$(with 's/wal_bytes=0/wal_bytes=0 expired_events_left=0/'); t line-of-store-4 PASS predicate_deletion_report "$line"
  line=$(with 's/wal_bytes=0/wal_bytes=0 expired_events_left=3/'); t expired-events-left-3 FAIL predicate_deletion_report "$line"
  line=$(with 's/ store_sealed=yes//'); t defects-name-the-key PASS defects_are "$line" "store_sealed "
  line=$(printf '%s\nexpired_events_left=3' "$good"); t expired-events-left-3-on-a-line-of-its-own FAIL predicate_deletion_report "$line"
  t expired-events-left-on-a-line-of-its-own-is-named PASS defects_are "$line" "expired_events_left "
}

past_the_deadline() {
  export GATE_OVERRIDE_child_deadline_extra_s=1
  against_the_standin hang none
}

against_the_standin() {
  standin_as_the_binary "$1" "$2"
  measure
}

measure() {
  build_for_measure || { fail_defect; return 1; }
  scratch_root
  store=$gate_scratch/${capture_label}store; out=$gate_scratch/${capture_label}stdout; err=$gate_scratch/${capture_label}stderr
  mkdir "$store"
  export HALVO_STORE_DIR="$store"
  if watched "$capture_bin" --verify-deletion --store-dir "$store" > "$out" 2> "$err"; then
    report=$(cat "$out")
    defects=$(deletion_report_defects "$report")
    if [ -z "$defects" ]; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "verify-deletion measured: ${defects% }"; return 1
  fi
  [ -z "$watched_defect" ] || { emit_fail "$name" "verify-deletion $watched_defect"; return 1; }
  first=$(head -1 "$err" 2>/dev/null)
  emit_fail "$name" "verify-deletion unavailable: $first"; return 1
}

check_main "$@"
