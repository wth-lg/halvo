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

measure() {
  store=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-deletion.XXXXXX"); out=$(mktemp "${TMPDIR:-/tmp}/r1-gate-check-deletion.XXXXXX"); err=$(mktemp "${TMPDIR:-/tmp}/r1-gate-check-deletion.XXXXXX"); trap 'rm -rf "$store" "$out" "$err"' EXIT
  need_build || return 1
  export HALVO_STORE_DIR="$store"
  if run_capture_bin --verify-deletion > "$out" 2> "$err"; then
    report=$(cat "$out")
    defects=$(deletion_report_defects "$report")
    if [ -z "$defects" ]; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "verify-deletion measured: ${defects% }"; return 1
  fi
  first=$(head -1 "$err" 2>/dev/null)
  emit_fail "$name" "verify-deletion unavailable: $first"; return 1
}

check_main "$@"
