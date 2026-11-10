#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=no-ll-hook

cases() {
  scratch_root
  mkdir -p "$gate_scratch/clean" "$gate_scratch/dirty"
  printf 'let ok = RegisterRawInputDevices(&[dev], n).is_ok();\n' > "$gate_scratch/clean/a.rs"
  printf 'let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(cb), hmod, 0);\n' > "$gate_scratch/dirty/b.rs"
  t raw-input-only PASS predicate_no_ll_hook "$gate_scratch/clean"
  t windows-hook FAIL predicate_no_ll_hook "$gate_scratch/dirty"
}

measure() {
  if predicate_no_ll_hook "$GATE_ROOT/crates/halvo-capture/src"; then
    emit_pass "$name"; return 0
  fi
  emit_fail "$name" "a low-level input hook (SetWindowsHookEx/WH_KEYBOARD_LL) is present"; return 1
}

check_main "$@"
