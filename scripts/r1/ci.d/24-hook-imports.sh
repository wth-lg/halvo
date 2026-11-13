#!/bin/sh
# jobs: windows
# rule: The debug builds of halvo-capture.exe and halvo.exe that this script makes import neither SetWindowsHookExA nor SetWindowsHookExW; the reader must show the KERNEL32.dll imports of halvo-capture.exe, and once that binary imports USER32.dll it must import RegisterRawInputDevices.
set -u
name=24-hook-imports

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

imports_defect() {
  [ -s "$2" ] || { printf '%s: the import listing is empty\n' "$1"; return 1; }
  hook=$(grep -m 1 -E 'Symbol: SetWindowsHookEx[AW]( |$)' "$2")
  hook_name=$(printf '%s\n' "$hook" | sed 's/.*Symbol: \([A-Za-z]*\).*/\1/')
  [ -z "$hook" ] || { printf '%s imports %s\n' "$1" "$hook_name"; return 1; }
  [ "$3" = control ] || return 0
  kernel=$(awk '{ line = tolower($0) } line ~ /^ *name: kernel32\.dll$/ { inside = 1; next } line ~ /^ *name: / { inside = 0 } inside && line ~ /^ *symbol: / { n++ } END { print n + 0 }' "$2")
  [ "$kernel" -gt 0 ] || { printf '%s: the reader shows no symbol imported from KERNEL32.dll, so it does not read the import table\n' "$1"; return 1; }
  grep -q -i -E '^ *Name: user32\.dll$' "$2" || return 0
  grep -q -E 'Symbol: RegisterRawInputDevices( |$)' "$2" && return 0
  printf '%s imports USER32.dll and not RegisterRawInputDevices, so the pointer source registers no raw input\n' "$1"
  return 1
}

readobj() {
  sysroot=$(rustc --print sysroot) || return 1
  for found in "$sysroot"/lib/rustlib/*/bin/llvm-readobj.exe "$sysroot"/lib/rustlib/*/bin/llvm-readobj; do
    [ -f "$found" ] && { printf '%s\n' "$found"; return 0; }
  done
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  if ! reader=$(readobj); then
    if [ "${CI:-}" = true ]; then rustup component add llvm-tools > "$CI_D_ARTIFACTS/rustup.txt" 2>&1 || fail "rustup component add llvm-tools failed"; fi
    reader=$(readobj) || { printf 'SKIP %s (llvm-readobj of rustup llvm-tools is not installed)\n' "$name"; exit 77; }
  fi
  "$reader" --version > "$CI_D_ARTIFACTS/reader-version.txt" 2>&1 || fail "llvm-readobj --version failed"
  reader_version=$(sed -n 's/.*LLVM version \([0-9.]*\).*/\1/p' "$CI_D_ARTIFACTS/reader-version.txt" | head -1)
  printf 'llvm-readobj version: %s\n' "${reader_version:-unread}"
  if ! cargo build --locked -p halvo-capture --bin halvo-capture -p halvo --bin halvo > "$CI_D_ARTIFACTS/build.txt" 2>&1; then
    build_error=$(grep -m 1 '^error' "$CI_D_ARTIFACTS/build.txt")
    fail "cargo build of halvo-capture.exe and halvo.exe failed: $build_error"
  fi
  target=${CARGO_TARGET_DIR:-target}
  for binary in halvo-capture halvo; do
    [ -f "$target/debug/$binary.exe" ] || fail "$target/debug/$binary.exe was not built"
    "$reader" --coff-imports "$target/debug/$binary.exe" > "$CI_D_ARTIFACTS/$binary.imports" 2>&1 || fail "llvm-readobj could not read $binary.exe"
    symbols=$(grep -c 'Symbol: ' "$CI_D_ARTIFACTS/$binary.imports")
    printf '%s.exe: %s imported symbols\n' "$binary" "$symbols"
  done
  found=$(imports_defect halvo-capture.exe "$CI_D_ARTIFACTS/halvo-capture.imports" control) || fail "$found"
  found=$(imports_defect halvo.exe "$CI_D_ARTIFACTS/halvo.imports" none) || fail "$found"
  printf 'PASS %s\n' "$name"
}

listing() {
  printf 'Import {\n  Name: KERNEL32.dll\n  Symbol: GetLastError (1)\n}\n'
  [ $# -gt 0 ] || return 0
  printf 'Import {\n  Name: USER32.dll\n'
  for symbol in "$@"; do printf '  Symbol: %s (1)\n' "$symbol"; done
  printf '}\n'
}

case_of() {
  got=PASS
  imports_defect "$2" "$3" "$4" > /dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$5" "$got"
  if [ "$got" != "$5" ]; then wrong="$wrong $1"; elif [ "$5" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  listing GetForegroundWindow RegisterRawInputDevices GetRawInputData > "$work/clean"
  listing GetForegroundWindow RegisterRawInputDevices SetWindowsHookExW > "$work/hook-w"
  listing RegisterRawInputDevices SetWindowsHookExA > "$work/hook-a"
  listing GetForegroundWindow GetRawInputData > "$work/no-raw-input"
  listing SetWindowsHookExWrapper RegisterRawInputDevices > "$work/longer-name"
  : > "$work/empty"
  case_of capture-clean halvo-capture.exe "$work/clean" control PASS
  case_of capture-imports-the-wide-hook halvo-capture.exe "$work/hook-w" control FAIL
  case_of capture-imports-the-ansi-hook halvo-capture.exe "$work/hook-a" control FAIL
  case_of capture-lacks-raw-input halvo-capture.exe "$work/no-raw-input" control FAIL
  case_of app-without-raw-input-needs-no-control halvo.exe "$work/no-raw-input" none PASS
  case_of app-imports-the-hook halvo.exe "$work/hook-w" none FAIL
  case_of a-longer-symbol-name-is-no-hook halvo-capture.exe "$work/longer-name" control PASS
  case_of empty-listing halvo-capture.exe "$work/empty" control FAIL
  listing > "$work/kernel-only"
  case_of capture-without-user32-needs-no-raw-input halvo-capture.exe "$work/kernel-only" control PASS
  printf 'Import {\n  Name: USER32.dll\n  Symbol: RegisterRawInputDevices (1)\n}\n' > "$work/no-kernel32"
  case_of reader-sees-no-kernel32 halvo-capture.exe "$work/no-kernel32" control FAIL
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
