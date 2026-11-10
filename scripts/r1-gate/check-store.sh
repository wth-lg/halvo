#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=store

good="store rows=2000 secure_delete=1 temp_store=2 fts_secure_delete=1 cipher_provider=libtomcrypt segment_roundtrip=ok unwrap=ok unwrap_ms=3 prompt=none"

with() { printf '%s\n' "$good" | sed "$1"; }

cases() {
  scratch_root
  marker=HALVOMARKERselftest
  mkdir -p "$gate_scratch/clean" "$gate_scratch/leaky"
  printf 'bytes-that-are-ciphertext\n' > "$gate_scratch/clean/seg0001"
  printf 'here is the %s in plaintext\n' "$marker" > "$gate_scratch/leaky/seg0001"
  t clean-dir PASS predicate_marker_absent "$gate_scratch/clean" "$marker"
  t leaky-dir FAIL predicate_marker_absent "$gate_scratch/leaky" "$marker"
  t clean-file PASS predicate_ciphertext "$gate_scratch/clean/seg0001" "$marker"
  t leaky-file FAIL predicate_ciphertext "$gate_scratch/leaky/seg0001" "$marker"
  t good-line PASS predicate_store_report "$good"
  line=$(with 's/segment_roundtrip=ok/segment_roundtrip=fail/'); t roundtrip-fail FAIL predicate_store_report "$line"
  line=$(with 's/cipher_provider=libtomcrypt/cipher_provider=openssl/'); t provider-openssl FAIL predicate_store_report "$line"
  line=$(with 's/secure_delete=1/secure_delete=0/'); t secure-delete-0 FAIL predicate_store_report "$line"
  line=$(with 's/prompt=none/prompt=shown/'); t prompt-shown FAIL predicate_store_report "$line"
  line=$(with 's/ prompt=none//'); t field-missing FAIL predicate_store_report "$line"
  t two-fields-only FAIL predicate_store_report "store unwrap=ok prompt=none"
}

measure() {
  store=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-store.XXXXXX"); out=$(mktemp "${TMPDIR:-/tmp}/r1-gate-check-store.XXXXXX"); err=$(mktemp "${TMPDIR:-/tmp}/r1-gate-check-store.XXXXXX"); trap 'rm -rf "$store" "$out" "$err"' EXIT
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
  first=$(head -1 "$err" 2>/dev/null)
  emit_fail "$name" "verify-store unavailable: $first"; return 1
}

check_main "$@"
