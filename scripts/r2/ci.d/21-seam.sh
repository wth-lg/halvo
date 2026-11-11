#!/bin/sh
# jobs: windows
# rule: cargo test -p halvo-seam passes with the committed lockfile and shows the four conformance tests, the reduction test and the memory test as passed.
set -u
name=21-seam
tests='conformance_resume conformance_shredded conformance_filters conformance_store_changed reduction_app_title transport_memory'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

decide() {
  [ "$1" -eq 0 ] || fail "cargo test -p halvo-seam --locked exited $1"
  tr -d '\r' <"$2" >"$2.lf" || fail "the output of cargo test cannot be read"
  for test in $tests; do
    grep -x "test $test \.\.\. ok" "$2.lf" || fail "cargo test -p halvo-seam --locked does not show $test as passed"
  done
  passed=$(grep -c -x 'test .* \.\.\. ok' "$2.lf")
  printf 'halvo-seam tests passed: %s, of them read by name: 6\n' "$passed"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  if ! command -v cargo >/dev/null 2>&1; then
    [ "${CI:-}" = true ] && fail "cargo is not installed"
    printf 'SKIP %s (cargo is not installed)\n' "$name"
    exit 77
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  cargo test -p halvo-seam --locked >"$work/test.txt" 2>&1
  code=$?
  cp "$work/test.txt" "$CI_D_ARTIFACTS/cargo-test.txt" || fail "the output of cargo test cannot be saved"
  grep '^test result:' "$work/test.txt"
  decide "$code" "$work/test.txt"
  printf 'PASS %s\n' "$name"
}

passing_output() {
  cat <<'EOF'
running 18 tests
test a_kind_outside_c4_4_is_refused ... ok
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 4 tests
test conformance_filters ... ok
test conformance_resume ... ok
test conformance_shredded ... ok
test conformance_store_changed ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s

running 1 test
test transport_memory ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.80s

running 4 tests
test reduction_app_title ... ok
test set_reader_refusals_report_the_path_and_the_line ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
EOF
}

check() {
  passing_output >"$work/$1.base" || exit 2
  sed "$4" "$work/$1.base" >"$work/$1.txt" || exit 2
  out=$( (decide "$3" "$work/$1.txt") 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:halvo-seam tests passed: "*) got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  cr=$(printf '\r')
  check all-passed PASS 0 's/^//'
  check crlf-lines PASS 0 "s/\$/$cr/"
  check cargo-test-fails FAIL 101 's/^//'
  check filters-missing FAIL 0 '/conformance_filters/d'
  check reduction-failed FAIL 0 's/test reduction_app_title ... ok/test reduction_app_title ... FAILED/'
  check memory-ignored FAIL 0 's/test transport_memory ... ok/test transport_memory ... ignored/'
  check resume-renamed FAIL 0 's/conformance_resume/conformance_resume_old/'
  check store-changed-with-a-suffix FAIL 0 's/conformance_store_changed ... ok/conformance_store_changed ... ok, 61 s/'
  check no-output FAIL 0 'd'
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
