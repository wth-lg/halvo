#!/bin/sh
# jobs: macos
# rule: A runner built in the release profile with the feature loopback holds the marker text of the browser's web request maker (crates/halvo-runner/src/web/maker.rs), and the list markers of 62-build holds that text, as the ignored test a_runner_built_with_the_feature_holds_the_maker_s_marker of halvo-runner shows when this check runs it alone by its exact name: it builds the runner into a target directory of its own under target/tmp and reads the built file and the list; the windows job never runs this check, so no run of ci.yml reads a runner built on Windows in the release profile with the feature for the marker, and the windows job builds the runner with the feature in the debug profile, for the crate's tests.
set -u
name=63-maker-marker
test_name=a_runner_built_with_the_feature_holds_the_maker_s_marker
marker='halvo-runner:web-maker:this-build-has-it!'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# The defect of one run of the test, from its exit status $1 and its output $2; none when the one test passed.
verdict_defect() {
  if grep -q 'holds no marker' "$2"; then
    printf 'the runner built with the feature loopback holds no marker of the web request maker: keep std::hint::black_box(MAKER_MARKER.as_ptr()) as the first line of Runner::effect_step in crates/halvo-runner/src/web/maker.rs\n'
    return 1
  fi
  if grep -q 'holds no line' "$2"; then
    printf 'the list markers of scripts/r1/ci.d/62-build.sh holds no line %s: put that line back at the end of the list\n' "$marker"
    return 1
  fi
  if grep -q 'cargo did not build the runner' "$2"; then
    printf 'cargo did not build the runner with the feature loopback: read %s/test.txt of the artifacts and mend the build\n' "$name"
    return 1
  fi
  if [ "$1" -ne 0 ]; then
    printf 'cargo test exited %s: read %s/test.txt of the artifacts\n' "$1" "$name"
    return 1
  fi
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  if [ "$counts" != '1 0' ]; then
    printf 'the filter ran %s passed and %s failed tests, not the one ignored test %s of the target maker_marker: a rename of the test changes test_name of this check, and the test keeps its ignore reason, which keeps it out of the merge gate and the slice tue\n' "${counts% *}" "${counts#* }" "$test_name"
    return 1
  fi
  return 0
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --locked -p halvo-runner --test maker_marker -- --exact "$test_name" --ignored >"$CI_D_ARTIFACTS/test.txt" 2>&1
  code=$?
  grep '^test result: ' "$CI_D_ARTIFACTS/test.txt"
  defect=$(verdict_defect "$code" "$CI_D_ARTIFACTS/test.txt") || fail "$defect"
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$3" >"$work/$1.txt"
  got=PASS
  verdict_defect "$2" "$work/$1.txt" >/dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$4" "$got"
  if [ "$got" != "$4" ]; then wrong="$wrong $1"; elif [ "$4" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  failed='test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 185.08s'
  case_of passed 0 "running 1 test
test $test_name ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 185.08s" PASS
  case_of no-marker 101 "the runner built with the feature loopback at target/tmp/maker-marker/release/halvo-runner holds no marker $marker of the browser's request maker
$failed" FAIL
  case_of no-line-in-the-list 101 "the list markers of check 62-build holds no line $marker: halvo-standin-module-present
$failed" FAIL
  case_of nested-build-failed 101 "cargo did not build the runner with the feature loopback: error[E0425]: cannot find value
$failed" FAIL
  case_of test-target-not-built 101 'error: could not compile halvo-runner (test "maker_marker") due to 1 previous error' FAIL
  case_of test-no-longer-ignored 0 'running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s' FAIL
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
