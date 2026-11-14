#!/bin/sh
# Verifier 133, point 2 of S-86 E8: a database library of the pinned source without the statements on the link path.
# Run at the root of the checkout. V133_DECOY_TARGET names a build directory of its own for the decoy; without
# it the decoy is built in the checkout's own build directory and copied out.
set +e -u
py=python3
lib=libsqlcipher.a
if [ "${RUNNER_OS:-}" = Windows ]; then py=python; lib=sqlcipher.lib; fi
tmp=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
if [ "${RUNNER_OS:-}" = Windows ]; then tmp=$(cygpath -u "$tmp"); fi
decoy="$tmp/v133-decoy-$$"
mkdir -p "$decoy"
out="$tmp/v133-decoy.out"
status=0
native() {
  # native <path>: the path as a program that is not of the shell reads it
  if [ "${RUNNER_OS:-}" = Windows ]; then cygpath -m "$1"; else printf '%s\n' "$1"; fi
}
decoy_native=$(native "$decoy")
if [ -n "${RUNNER_TEMP:-}" ]; then V133_DECOY_TARGET="$tmp/v133-target-decoy"; fi

count() {
  # count <text> <file>: the number of times the text stands in the file
  LC_ALL=C grep -a -o -e "$1" "$2" | wc -l | tr -d ' '
}

echo "V133 decoy step 1: build the decoy (the pinned source without the statements, the stops and the mark; default cache size -1234)"
$py -I scratch-v133/edit.py decoy || exit 1
touch "$decoy/before-the-build"
if [ -n "${V133_DECOY_TARGET:-}" ]; then
  CARGO_TARGET_DIR="$(native "$V133_DECOY_TARGET")" cargo build --locked -p halvo-sqlcipher >"$out" 2>&1
  code=$?
  root="$V133_DECOY_TARGET"
else
  cargo build --locked -p halvo-sqlcipher >"$out" 2>&1
  code=$?
  root=target
fi
git checkout -- crates/halvo-sqlcipher/build.rs
built=$(find "$root/debug/build" -name "$lib" -newer "$decoy/before-the-build" | grep 'halvo-sqlcipher-' | head -n 1)
printf 'V133 decoy build: cargo build exit %s, library %s\n' "$code" "${built:-not found}"
if [ "$code" -ne 0 ] || [ -z "$built" ]; then
  tr -d '\r' <"$out" | tail -n 20
  exit 1
fi
cp "$built" "$decoy/$lib"
copy=$(dirname "$built")/sqlcipher.c
printf 'V133 decoy library: %s bytes; the mark name stands %s times in it; its source copy holds the statements %s times and the stops %s times\n' \
  "$(wc -c <"$decoy/$lib" | tr -d ' ')" "$(count halvo_sqlcipher_page_gap_mend "$decoy/$lib")" \
  "$(count 'size_t)(pData - p' "$copy")" "$(count 'halvo-sqlcipher: the build stops' "$copy")"

$py -I scratch-v133/edit.py probe || exit 1

variant() {
  # variant <name> [NAME=value ...]: build the crate's test binary with that environment, show the link inputs, run the probe and three tests
  name=$1
  shift
  echo "V133 decoy variant $name: environment $*"
  env "$@" cargo test --locked -p halvo-sqlcipher --lib --no-run -v >"$out" 2>&1
  code=$?
  printf 'V133 decoy variant %s: build of the test binary exit %s\n' "$name" "$code"
  for crate in libsqlite3_sys halvo_sqlcipher; do
    tr -d '\r' <"$out" | grep 'Running `' | grep -e "--crate-name $crate " | while IFS= read -r line; do
      kind=lib
      case $line in *' --test '*) kind=test ;; esac
      case $line in *'build_script_build'*|*'--crate-name build_script'*) continue ;; esac
      printf 'V133 decoy variant %s: rustc of %s (%s), its -L and -l words in order:' "$name" "$crate" "$kind"
      printf '%s\n' "$line" | tr ' ' '\n' | grep -E '^(native|dependency|static|dylib|framework)=|^-l|sqlcipher' | grep -v -e '--crate-name' -e 'feature=' -e 'bundled' -e '\.rs$' -e '^"sqlcipher",$' | tr '\n' ' '
      printf '\n'
    done
  done
  tr -d '\r' <"$out" | grep -E 'rustc-link-(lib|search)' | head -n 12
  if [ "$code" -ne 0 ]; then
    printf 'V133 decoy variant %s: the link did not end; lines of the error:\n' "$name"
    tr -d '\r' <"$out" | grep -E 'LNK[0-9]+|duplicate symbol|undefined|unresolved|error' | cut -c1-400 | head -n 12
    return 0
  fi
  exe=$(tr -d '\r' <"$out" | grep 'Executable' | grep -o '[^ (`]*halvo_sqlcipher-[0-9a-f]*\(\.exe\)\{0,1\}' | tail -n 1 | tr '\\' '/')
  if [ -n "$exe" ] && [ -f "$exe" ]; then
    printf 'V133 decoy variant %s: test binary %s bytes; DEFAULT_CACHE_SIZE=-1234 stands %s times in it, DEFAULT_CACHE_SIZE=-2000 %s times, the mark name %s times\n' \
      "$name" "$(wc -c <"$exe" | tr -d ' ')" "$(count 'DEFAULT_CACHE_SIZE=-1234' "$exe")" "$(count 'DEFAULT_CACHE_SIZE=-2000' "$exe")" "$(count halvo_sqlcipher_page_gap_mend "$exe")"
  else
    printf 'V133 decoy variant %s: no test binary found in the output\n' "$name"
  fi
  env "$@" V133_WORK="$decoy_native" cargo test --locked -p halvo-sqlcipher --lib -- --nocapture --test-threads=1 \
    tests::the_linked_library_is_the_pinned_sqlcipher_with_the_page_gap_mend page_gap::v133::v133_which_library \
    page_gap::second_place::small_rows_deleted_one_at_a_time_leave_no_text_in_a_page_edited_in_place \
    page_gap::pins >"$out" 2>&1
  code=$?
  printf 'V133 decoy variant %s: cargo test exit %s\n' "$name" "$code"
  tr -d '\r' <"$out" | grep -E 'V133 which|^test result|\.\.\. (ok|FAILED)$|panicked' | cut -c1-600
  tr -d '\r' <"$out" | grep -E '^test .* \.\.\. ' | cut -c1-200
  if [ "$name" = control ] && [ "$code" -ne 0 ]; then status=1; fi
}

variant control V133_NO_CHANGE=1
variant lib-dir "SQLCIPHER_LIB_DIR=$decoy_native"
variant lib-dir-static "SQLCIPHER_LIB_DIR=$decoy_native" SQLCIPHER_STATIC=1
variant control-again V133_NO_CHANGE=2

git checkout -- crates/halvo-sqlcipher/tests/page_gap/mod.rs
mv crates/halvo-sqlcipher/tests/page_gap/v133.rs "$tmp/v133.rs.taken-out-decoy.$$"
printf 'V133 decoy probe: status lines after the restore %s\n' "$(git status --porcelain | wc -l | tr -d ' ')"
exit "$status"
