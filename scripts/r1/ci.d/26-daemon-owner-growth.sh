#!/bin/sh
# jobs: macos
# rule: The ignored test two_million_floor_records_grow_the_owner_by_at_most_the_bound runs in a release build of the lib's tests, passes as exactly one test and prints its growth line over 2,000,000 records with the child's elapsed_ms; its heap bytes-in-use line is printed and not held (ruling DM-10).
set -u
name=26-daemon-owner-growth
test_name=daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound
growth_pattern='owner_growth_bytes=[0-9][0-9]* records=2000000 elapsed_ms=[0-9][0-9]*'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

outcome_defect() {
  [ "$1" -eq 0 ] || { printf 'cargo test exited %s\n' "$1"; return 1; }
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  if [ "$counts" != '1 0' ]; then
    printf 'the filter ran %s passed and %s failed tests, and exactly 1 must pass\n' "${counts% *}" "${counts#* }"
    return 1
  fi
  grep -q -o "$growth_pattern" "$2" && return 0
  printf 'the test printed no growth line over 2,000,000 records\n'
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --release --locked -p halvo-capture --lib -- --ignored --exact "$test_name" --nocapture > "$CI_D_ARTIFACTS/test.txt" 2>&1
  code=$?
  heap=$(grep -m 1 -o 'owner_heap_bytes .*' "$CI_D_ARTIFACTS/test.txt")
  [ -z "$heap" ] || printf '%s\n' "$heap"
  growth=$(grep -m 1 -o "$growth_pattern" "$CI_D_ARTIFACTS/test.txt")
  [ -z "$growth" ] || printf '%s\n' "$growth"
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
  growth='owner_growth_bytes=6930432 records=2000000 elapsed_ms=31322'
  result='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 555 filtered out; finished in 31.67s'
  passed="running 1 test
test daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound ... $growth
ok

$result"
  case_of one-test-passed 0 "$passed" PASS
  case_of no-growth-line 0 "running 1 test
test daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound ... ok

$result" FAIL
  case_of a-shorter-count 0 "running 1 test
owner_growth_bytes=6930432 records=80000 elapsed_ms=1246
test daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound ... ok

$result" FAIL
  case_of running-0-tests 0 "$growth
running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 556 filtered out; finished in 0.00s" FAIL
  case_of the-test-failed 101 "running 1 test
$growth
test daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 555 filtered out; finished in 31.40s" FAIL
  case_of two-tests-passed 0 "running 2 tests
$growth

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 554 filtered out; finished in 32.00s" FAIL
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
