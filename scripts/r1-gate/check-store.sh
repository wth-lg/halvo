#!/bin/sh
# Sub-check: the store holds no plaintext marker, the owner reads back the
# segment it wrote through a no-prompt unwrap, and SQLCipher runs on libtomcrypt.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=store

good="store rows=2000 secure_delete=1 temp_store=2 fts_secure_delete=1 cipher_provider=libtomcrypt segment_roundtrip=ok unwrap=ok unwrap_ms=3 prompt=none"

self_test() {
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  marker=HALVOMARKERselftest
  mkdir -p "$tmp/clean" "$tmp/leaky"
  printf 'bytes-that-are-ciphertext\n' > "$tmp/clean/seg0001"
  printf 'here is the %s in plaintext\n' "$marker" > "$tmp/leaky/seg0001"
  if predicate_marker_absent "$tmp/clean" "$marker" \
     && ! predicate_marker_absent "$tmp/leaky" "$marker" \
     && predicate_ciphertext "$tmp/clean/seg0001" "$marker" \
     && ! predicate_ciphertext "$tmp/leaky/seg0001" "$marker" \
     && predicate_store_report "$good" \
     && ! predicate_store_report "$(printf '%s' "$good" | sed 's/segment_roundtrip=ok/segment_roundtrip=fail/')" \
     && ! predicate_store_report "$(printf '%s' "$good" | sed 's/cipher_provider=libtomcrypt/cipher_provider=openssl/')" \
     && ! predicate_store_report "$(printf '%s' "$good" | sed 's/secure_delete=1/secure_delete=0/')" \
     && ! predicate_store_report "$(printf '%s' "$good" | sed 's/prompt=none/prompt=shown/')" \
     && ! predicate_store_report "$(printf '%s' "$good" | sed 's/ prompt=none//')" \
     && ! predicate_store_report "store unwrap=ok prompt=none"; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  store=$(mktemp -d); out=$(mktemp); err=$(mktemp); trap 'rm -rf "$store" "$out" "$err"' EXIT
  marker="HALVO_GATE_MARKER_$$"
  if HALVO_STORE_DIR="$store" HALVO_GATE_MARKER="$marker" run_capture_bin --verify-store > "$out" 2> "$err"; then
    report=$(cat "$out")
    if ! predicate_marker_absent "$store" "$marker"; then
      emit_fail "$name" "marker leaked into the store"; return 1
    fi
    # shellcheck disable=SC2086
    defects=$(report_defects "$report" $GATE_STORE_SPEC)
    if [ -z "$defects" ]; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "verify-store measured: $defects"; return 1
  fi
  emit_fail "$name" "verify-store unavailable: $(head -1 "$err" 2>/dev/null)"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure ;;
  *) measure ;;
esac
