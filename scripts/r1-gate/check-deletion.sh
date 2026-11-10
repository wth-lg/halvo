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
  if watched run_capture_bin --verify-deletion --store-dir "$store" > "$out" 2> "$err"; then
    report=$(cat "$out")
    defects=$(deletion_report_defects "$report")
    if [ -z "$defects" ]; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "verify-deletion measured: ${defects% }"; return 1
  fi
  first=$(head -1 "$err" 2>/dev/null)
  emit_fail "$name" "verify-deletion unavailable: $first"; return 1
}

check_main "$@"
