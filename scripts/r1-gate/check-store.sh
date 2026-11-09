#!/bin/sh
# Sub-check: the store holds no plaintext marker and the owner unwraps the store
# key with no prompt.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=store

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
     && predicate_unwrap_no_prompt "store unwrap=ok prompt=none" \
     && ! predicate_unwrap_no_prompt "store unwrap=ok prompt=shown"; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  store=$(mktemp -d); out=$(mktemp); trap 'rm -rf "$store" "$out"' EXIT
  marker="HALVO_GATE_MARKER_$$"
  if HALVO_STORE_DIR="$store" HALVO_GATE_MARKER="$marker" run_capture_bin --verify-store > "$out" 2>&1; then
    if predicate_marker_absent "$store" "$marker" && predicate_unwrap_no_prompt "$(cat "$out")"; then
      emit_pass "$name"; return 0
    fi
    emit_fail "$name" "marker leaked into the store or the unwrap prompted"; return 1
  fi
  emit_fail "$name" "verify-store unavailable: $(head -1 "$out" 2>/dev/null)"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure ;;
  *) measure ;;
esac
