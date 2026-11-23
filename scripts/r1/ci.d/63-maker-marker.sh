#!/bin/sh
# jobs: macos
# rule: A runner built in the release profile with the feature loopback holds the marker text of the browser's web request maker (crates/halvo-runner/src/web/maker.rs), and the list markers of 62-build holds that text, as the ignored test a_runner_built_with_the_feature_holds_the_maker_s_marker of halvo-runner shows when this check runs it alone by its exact name, built in the release profile in a target directory of the check's own under its TMPDIR that starts as a copy-on-write clone (cp -c) of the job's target/release, which no cargo command of this check writes: the test builds the runner into a target directory of its own under that directory's tmp, which starts as a clone of that directory's release, and reads the built file and the list, so behind 62-build in the slice sun cargo builds again only halvo-connect and halvo-runner, whose features loopback changes; the windows job never runs this check, so no run of ci.yml reads a runner built on Windows in the release profile with the feature for the marker, and the windows job builds the runner with the feature in the debug profile, for the crate's tests.
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

# Clones the directory $1 to the new path $2 with cp -c, which falls back to a plain copy across volumes, and
# prints what it did under the label $3; a target directory that is not cloned starts empty, and cargo builds
# into it from nothing.
seed() {
  if [ ! -d "$1" ]; then
    printf 'clone of %s: none, no such directory\n' "$3"
    return 0
  fi
  parent=$(dirname "$2")
  source_volume=$(stat -f %d "$1")
  target_volume=$(stat -f %d "$parent")
  same_volume=no
  [ "$source_volume" = "$target_volume" ] && same_volume=yes
  free_before=$(df -m "$parent" | awk 'NR == 2 { print $4 }')
  seed_started=$(date +%s)
  if cp -c -R -p "$1" "$2"; then
    seeded=yes
  else
    seeded=no
  fi
  seed_ended=$(date +%s)
  free_after=$(df -m "$parent" | awk 'NR == 2 { print $4 }')
  files=$(find "$2" -type f 2>/dev/null | wc -l | tr -d ' ')
  printf 'clone of %s: done=%s same_volume=%s files=%s seconds=%s free_mb_drop=%s\n' "$3" "$seeded" "$same_volume" "$files" "$((seed_ended - seed_started))" "$((free_before - free_after))"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  own=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  seed "${CARGO_TARGET_DIR:-target}/release" "$own/release" "the job's target/release"
  build_started=$(date +%s)
  CARGO_TARGET_DIR=$own cargo test --release --locked -p halvo-runner --test maker_marker --no-run >"$CI_D_ARTIFACTS/build.txt" 2>&1
  code=$?
  build_ended=$(date +%s)
  printf 'build of the test target in the release profile: seconds=%s\n' "$((build_ended - build_started))"
  [ "$code" -eq 0 ] || fail "cargo test --release --no-run exited $code: read $name/build.txt of the artifacts"
  mkdir -p "$own/tmp/maker-marker" || exit 2
  seed "$own/release" "$own/tmp/maker-marker/release" "that release for the test's own target directory"
  CARGO_TARGET_DIR=$own cargo test --release --locked -p halvo-runner --test maker_marker -- --exact "$test_name" --ignored >"$CI_D_ARTIFACTS/test.txt" 2>&1
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
  failed='test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 274.64s'
  case_of passed 0 "running 1 test
test $test_name ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 274.64s" PASS
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
