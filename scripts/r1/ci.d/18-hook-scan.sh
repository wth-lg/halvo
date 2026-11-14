#!/bin/sh
# jobs: macos
# rule: The source scan of the R1 gate finds no banned input-hook, event-tap or audio-capture call in the tracked Rust sources.
set -u
name=18-hook-scan

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

measure() {
  scan=$1/scripts/r1-gate/check-no-ll-hook.sh
  [ -f "$scan" ] || fail "scripts/r1-gate/check-no-ll-hook.sh is missing"
  out=$(sh "$scan" --measure 2>&1)
  code=$?
  printf '%s\n' "$out"
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS no-ll-hook") ;;
    *) fail "check-no-ll-hook.sh --measure exited $code and ended with: $last" ;;
  esac
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(CI_D_REPO=$work/$1 sh "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

scan() {
  mkdir -p "$work/$1/scripts/r1-gate" || exit 2
  [ $# -lt 2 ] || printf '%s\n' "$2" >"$work/$1/scripts/r1-gate/check-no-ll-hook.sh"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  scan scan-passes '[ "$*" = --measure ] || exit 3; printf "PASS no-ll-hook\n"'
  check scan-passes PASS
  scan scan-finds-a-hook 'printf "FAIL no-ll-hook (windows-hook)\n"; exit 1'
  check scan-finds-a-hook FAIL 'exited 1 and ended with: FAIL no-ll-hook (windows-hook))'
  scan scan-prints-nothing 'exit 0'
  check scan-prints-nothing FAIL 'exited 0 and ended with: )'
  scan pass-line-with-exit-1 'printf "PASS no-ll-hook\n"; exit 1'
  check pass-line-with-exit-1 FAIL 'exited 1 and ended with: PASS no-ll-hook)'
  scan pass-line-with-exit-2 'printf "PASS no-ll-hook\n"; exit 2'
  check pass-line-with-exit-2 FAIL 'exited 2 and ended with: PASS no-ll-hook)'
  scan pass-line-of-another-check 'printf "PASS store\n"'
  check pass-line-of-another-check FAIL 'exited 0 and ended with: PASS store)'
  scan pass-line-is-not-last 'printf "PASS no-ll-hook\nFAIL no-ll-hook (active-tap)\n"'
  check pass-line-is-not-last FAIL 'exited 0 and ended with: FAIL no-ll-hook (active-tap))'
  scan message-on-standard-error-after-the-pass-line 'printf "PASS no-ll-hook\n"; printf "the scan stopped early\n" >&2'
  check message-on-standard-error-after-the-pass-line FAIL 'exited 0 and ended with: the scan stopped early)'
  scan pass-line-with-more-words 'printf "PASS no-ll-hook and more\n"'
  check pass-line-with-more-words FAIL 'exited 0 and ended with: PASS no-ll-hook and more)'
  scan scan-missing
  check scan-missing FAIL 'scripts/r1-gate/check-no-ll-hook.sh is missing)'

  mkdir -p "$work/hook-line-in-the-tree/scripts" "$work/hook-line-in-the-tree/crates/halvo-capture/src/windows" "$work/hook-line-in-the-tree/crates/halvo-capture/src/macos" "$work/hook-line-in-the-tree/src-tauri/src" || exit 2
  printf 'let tap = CGEventTap::new(loc, place, CGEventTapOptions::ListenOnly, mask, cb);\n' >"$work/hook-line-in-the-tree/crates/halvo-capture/src/macos/input.rs"
  printf 'fn main() {}\n' >"$work/hook-line-in-the-tree/src-tauri/src/main.rs"
  cp -R "$CI_D_REPO/scripts/r1-gate" "$work/hook-line-in-the-tree/scripts/" || exit 2
  printf 'let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), module, 0);\n' >"$work/hook-line-in-the-tree/crates/halvo-capture/src/windows/input.rs"
  git init -q "$work/hook-line-in-the-tree" && git -C "$work/hook-line-in-the-tree" add -A || exit 2
  check hook-line-in-the-tree FAIL 'FAIL no-ll-hook (source scan measured: windows-hook at crates/halvo-capture/src/windows/input.rs:1)'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure "$CI_D_REPO" ;;
  *) exit 64 ;;
esac
