#!/bin/sh
# Sub-check: the capture crate installs no low-level global input hook. The
# Windows backend is a listen-only Raw Input path; SetWindowsHookEx or a
# WH_KEYBOARD_LL / WH_MOUSE_LL hook would be a lower-level global hook the crate
# forbids (comment-hunter-3 DELETE constraint).
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
name=no-ll-hook

self_test() {
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r1-gate-check-no-ll-hook.XXXXXX")
  mkdir -p "$tmp/clean" "$tmp/dirty"
  printf 'let ok = RegisterRawInputDevices(&[dev], n).is_ok();\n' > "$tmp/clean/a.rs"
  printf 'let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(cb), hmod, 0);\n' > "$tmp/dirty/b.rs"
  if predicate_no_ll_hook "$tmp/clean" && ! predicate_no_ll_hook "$tmp/dirty"; then
    rm -rf "$tmp"; emit_pass "self-test:$name"; return 0
  fi
  rm -rf "$tmp"; emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
}

measure() {
  if predicate_no_ll_hook "$GATE_ROOT/crates/halvo-capture/src"; then
    emit_pass "$name"; return 0
  fi
  emit_fail "$name" "a low-level input hook (SetWindowsHookEx/WH_KEYBOARD_LL) is present"; return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure ;;
  *) measure ;;
esac
