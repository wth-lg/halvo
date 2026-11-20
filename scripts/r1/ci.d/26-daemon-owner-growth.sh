#!/bin/sh
# jobs: macos
# rule: The ignored test two_million_floor_records_grow_the_owner_by_at_most_the_bound runs in a release build of the lib's tests, passes as exactly one test and prints its growth line over 2,000,000 records with the child's elapsed_ms; its bytes in use over all malloc zones after Settle less at ready (all_zones_growth on its heap line) is at or under 27,872, the largest of daemon-1d's hosted readings plus their range, and a fall reads as no growth (DM-11 (4)); and in the same build the test a_release_build_refuses_the_three_gate_modes_as_usage, which exists without debug assertions alone, passes as exactly one test with its seconds printed.
# lane: alone
set -u
name=26-daemon-owner-growth
test_name=daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound
release_test=daemon::tests::a_release_build_refuses_the_three_gate_modes_as_usage
growth_pattern='owner_growth_bytes=[0-9][0-9]* records=2000000 elapsed_ms=[0-9][0-9]*'
heap_growth_max=27872

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

count_defect() {
  [ "$1" -eq 0 ] || { printf 'cargo test exited %s\n' "$1"; return 1; }
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  [ "$counts" = '1 0' ] && return 0
  printf 'the filter ran %s passed and %s failed tests, and exactly 1 must pass\n' "${counts% *}" "${counts#* }"
  return 1
}

heap_growth_of() {
  awk '/owner_heap_bytes / { for (i = 1; i <= NF; i++) if ($i ~ /^all_zones_growth=-?[0-9]+$/) { v = substr($i, 18); if (!seen || v + 0 > top + 0) top = v; seen = 1 } } END { if (seen) print top }' "$1"
}

heap_defect() {
  heap_growth=$(heap_growth_of "$1")
  if [ -z "$heap_growth" ]; then
    printf 'the test printed no bytes-in-use growth over all malloc zones\n'
    return 1
  fi
  awk -v growth="$heap_growth" -v held="$heap_growth_max" 'BEGIN { exit !(growth + 0 <= held + 0) }' && return 0
  printf 'the bytes in use over all malloc zones grew by %s bytes, above %s\n' "$heap_growth" "$heap_growth_max"
  return 1
}

outcome_defect() {
  count_defect "$1" "$2" || return 1
  grep -q -o "$growth_pattern" "$2" || {
    printf 'the test printed no growth line over 2,000,000 records\n'
    return 1
  }
  heap_defect "$2"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --release --locked -p halvo-capture --lib -- --ignored --exact "$test_name" --nocapture > "$CI_D_ARTIFACTS/test.txt" 2>&1
  code=$?
  heap=$(grep -m 1 -o 'owner_heap_bytes .*' "$CI_D_ARTIFACTS/test.txt")
  [ -z "$heap" ] || printf '%s\n' "$heap"
  growth=$(grep -m 1 -o "$growth_pattern" "$CI_D_ARTIFACTS/test.txt")
  [ -z "$growth" ] || printf '%s\n' "$growth"
  heap_growth=$(heap_growth_of "$CI_D_ARTIFACTS/test.txt")
  printf 'bytes in use over all malloc zones: growth %s, held at or under %s\n' "${heap_growth:-none}" "$heap_growth_max"
  found=$(outcome_defect "$code" "$CI_D_ARTIFACTS/test.txt") || fail "$found"
  cargo test --release --locked -p halvo-capture --lib -- --exact "$release_test" > "$CI_D_ARTIFACTS/release-modes.txt" 2>&1
  code=$?
  result=$(grep -m 1 '^test result: ' "$CI_D_ARTIFACTS/release-modes.txt")
  printf 'release refusal of the gate modes: %s\n' "${result:-no result line}"
  found=$(count_defect "$code" "$CI_D_ARTIFACTS/release-modes.txt") || fail "the release refusal of the gate modes: $found"
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$3" > "$work/$1"
  got=PASS
  outcome_defect "$2" "$work/$1" > /dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$4" "$got"
  if [ "$got" != "$4" ]; then wrong="$wrong $1"; elif [ "$4" = FAIL ]; then rejected=$((rejected + 1)); fi
}

case_release() {
  printf '%s\n' "$3" > "$work/$1"
  got=PASS
  count_defect "$2" "$work/$1" > /dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$4" "$got"
  if [ "$got" != "$4" ]; then wrong="$wrong $1"; elif [ "$4" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  growth='owner_growth_bytes=6930432 records=2000000 elapsed_ms=31322'
  result='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 555 filtered out; finished in 31.67s'
  passed_with() {
    heap_line=
    [ -z "$1" ] || heap_line="owner_heap_bytes printed_not_held all_zones_ready=1331072 all_zones_settled=1331136 $1 default_zone_ready=69664 default_zone_settled=69728 default_zone_growth=64 footprint_ready=3802048 footprint_settled=10339264 footprint_growth=6537216"
    printf 'running 1 test\ntest daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound ... %s\n%s\nok\n\n%s' "$heap_line" "$growth" "$result"
  }
  passed=$(passed_with all_zones_growth=64)
  case_of one-test-passed 0 "$passed" PASS
  fixture=$(passed_with all_zones_growth=-3520)
  case_of heap-fall-reads-as-no-growth 0 "$fixture" PASS
  fixture=$(passed_with "all_zones_growth=$heap_growth_max")
  case_of heap-at-the-held-value 0 "$fixture" PASS
  fixture=$(passed_with "all_zones_growth=$((heap_growth_max + 1))")
  case_of heap-one-byte-above-the-held-value 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_growth=211168)
  case_of heap-of-the-undrained-report-copy 0 "$fixture" FAIL
  fixture=$(passed_with '')
  case_of no-heap-line 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_settled_growth=64)
  case_of heap-line-without-its-growth 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_growth=6-4)
  case_of heap-growth-not-a-number 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_growth=-)
  case_of heap-growth-with-no-digits 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_growth=27872)
  case_of heap-at-the-ruled-27872 0 "$fixture" PASS
  fixture=$(passed_with all_zones_growth=27873)
  case_of heap-at-the-ruled-27872-plus-one 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_growth=+64)
  case_of heap-growth-with-a-plus-sign 0 "$fixture" FAIL
  fixture=$(passed_with prior_all_zones_growth=64)
  case_of heap-growth-field-with-a-prefix 0 "$fixture" FAIL
  fixture=$(passed_with all_zones_growth=-9000)
  case_of heap-fall-larger-than-the-held-value 0 "$fixture" PASS
  case_of heap-line-on-its-own-line 0 "running 1 test
owner_heap_bytes printed_not_held all_zones_ready=1331072 all_zones_settled=1331136 all_zones_growth=64 default_zone_ready=69664
$growth
test daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound ... ok

$result" PASS
  fixture=$(passed_with all_zones_growth=-99999999999999999999)
  case_of heap-fall-beyond-64-bits 0 "$fixture" PASS
  fixture=$(passed_with all_zones_growth=99999999999999999999)
  case_of heap-growth-beyond-64-bits 0 "$fixture" FAIL
  quoted='2417 |         writeln!(out, "owner_heap_bytes printed_not_held all_zones_ready={} all_zones_settled={} all_zones_growth={} default_zone_ready={}");'
  fixture=$(passed_with all_zones_growth=64)
  case_of heap-after-a-compiler-line-that-quotes-it 0 "$quoted
$fixture" PASS
  case_of a-second-heap-line-above-the-held-value 0 "$fixture
owner_heap_bytes printed_not_held all_zones_ready=1331072 all_zones_settled=1542240 all_zones_growth=211168 default_zone_ready=69664" FAIL
  case_of a-first-heap-line-above-the-held-value 0 "owner_heap_bytes printed_not_held all_zones_ready=1331072 all_zones_settled=1542240 all_zones_growth=211168 default_zone_ready=69664
$fixture" FAIL
  fixture=$(passed_with '')
  case_of the-growth-field-outside-a-heap-line 0 "owner_rss_bytes all_zones_growth=64
$fixture" FAIL
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
  case_release release-test-passed 0 "running 1 test
test daemon::tests::a_release_build_refuses_the_three_gate_modes_as_usage ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 557 filtered out; finished in 0.00s" PASS
  case_release release-test-absent-as-in-a-debug-build 0 'running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 558 filtered out; finished in 0.00s' FAIL
  case_release release-test-failed 101 "running 1 test
test daemon::tests::a_release_build_refuses_the_three_gate_modes_as_usage ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 557 filtered out; finished in 0.01s" FAIL
  case_release release-test-no-result-line 0 'error: could not compile' FAIL
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
