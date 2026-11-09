#!/bin/sh
# Sub-check: a crypto-shredded segment is not decryptable with any remaining key,
# the shred left nothing in the WAL, and delete-all destroyed the store key.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=deletion

good="segment=42 shred=ok file_gone=yes decryptable=no sibling_readable=yes remaining_keys=1 wal_bytes=0 delete_all_keys=2 root_destroyed=yes store_sealed=yes"

self_test() {
  if predicate_deletion_report "$good" \
     && ! predicate_deletion_report "$(printf '%s' "$good" | sed 's/decryptable=no/decryptable=yes/')" \
     && ! predicate_deletion_report "$(printf '%s' "$good" | sed 's/file_gone=yes/file_gone=no/')" \
     && ! predicate_deletion_report "$(printf '%s' "$good" | sed 's/sibling_readable=yes/sibling_readable=no/')" \
     && ! predicate_deletion_report "$(printf '%s' "$good" | sed 's/wal_bytes=0/wal_bytes=78312/')" \
     && ! predicate_deletion_report "$(printf '%s' "$good" | sed 's/root_destroyed=yes/root_destroyed=no/')" \
     && ! predicate_deletion_report "$(printf '%s' "$good" | sed 's/store_sealed=yes/store_sealed=no/')" \
     && ! predicate_deletion_report "segment=42 shred=ok decryptable=no"; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  store=$(mktemp -d); out=$(mktemp); err=$(mktemp); trap 'rm -rf "$store" "$out" "$err"' EXIT
  if HALVO_STORE_DIR="$store" run_capture_bin --verify-deletion > "$out" 2> "$err"; then
    # shellcheck disable=SC2086
    defects=$(report_defects "$(cat "$out")" $GATE_DELETION_SPEC)
    if [ -z "$defects" ]; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "verify-deletion measured: $defects"; return 1
  fi
  emit_fail "$name" "verify-deletion unavailable: $(head -1 "$err" 2>/dev/null)"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure ;;
  *) measure ;;
esac
