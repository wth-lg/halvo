#!/bin/sh
# Sub-check: a crypto-shredded segment is not decryptable with any remaining key.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=deletion

self_test() {
  if predicate_not_decryptable "segment=42 shred=ok decryptable=no" \
     && ! predicate_not_decryptable "segment=42 shred=ok decryptable=yes"; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  store=$(mktemp -d); out=$(mktemp); trap 'rm -rf "$store" "$out"' EXIT
  if HALVO_STORE_DIR="$store" run_capture_bin --verify-deletion > "$out" 2>&1; then
    if predicate_not_decryptable "$(cat "$out")"; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "a shredded segment is still decryptable"; return 1
  fi
  emit_fail "$name" "verify-deletion unavailable: $(head -1 "$out" 2>/dev/null)"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure ;;
  *) measure ;;
esac
