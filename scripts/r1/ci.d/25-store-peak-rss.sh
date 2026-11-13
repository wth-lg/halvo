#!/bin/sh
# jobs: macos
# rule: The ignored store test peak_rss_stays_bounded_for_a_512_mib_segment runs in a release build and passes as exactly one test.
set -u
name=25-store-peak-rss
test_name=store::hardening_tests::peak_rss_stays_bounded_for_a_512_mib_segment

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

outcome_defect() {
  [ "$1" -eq 0 ] || { printf 'cargo test exited %s\n' "$1"; return 1; }
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  [ "$counts" = '1 0' ] && return 0
  printf 'the filter ran %s passed and %s failed tests, and exactly 1 must pass\n' "${counts% *}" "${counts#* }"
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --release --locked -p halvo-capture --lib -- --ignored --exact "$test_name" --nocapture > "$CI_D_ARTIFACTS/test.txt" 2>&1
  code=$?
  grep -m 1 '^RSS ' "$CI_D_ARTIFACTS/test.txt"
  found=$(outcome_defect "$code" "$CI_D_ARTIFACTS/test.txt") || fail "$found"
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$3" > "$work/$1"
  got=PASS
  outcome_defect "$2" "$work/$1" > /dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$4" "$got"
  if [ "$got" != "$4" ]; then wrong="$wrong $1"; elif [ "$4" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  passed='running 1 test
test store::hardening_tests::peak_rss_stays_bounded_for_a_512_mib_segment ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 180 filtered out; finished in 4.10s'
  case_of one-test-passed 0 "$passed" PASS
  case_of running-0-tests 0 'running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 181 filtered out; finished in 0.00s' FAIL
  case_of the-test-failed 101 'running 1 test
test store::hardening_tests::peak_rss_stays_bounded_for_a_512_mib_segment ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 180 filtered out; finished in 3.44s' FAIL
  case_of two-tests-passed 0 'running 2 tests

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 179 filtered out; finished in 4.00s' FAIL
  case_of exit-101-after-a-pass-line 101 "$passed" FAIL
  case_of no-result-line 0 'error: could not compile' FAIL
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
