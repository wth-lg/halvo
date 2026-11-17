#!/bin/sh
# jobs: macos windows
# rule: The driver trial's own crate (tools/r3-trials/drive) passes cargo test --locked with every test target's test that the fence of r3-web is on among the tests that passed, as many as the pinned count, then cargo clippy --locked --all-targets with -D warnings, then the self-tests of verdict.py and rebuild.py and verdict.py --check on the committed result file, with no tracked file that differs from HEAD before or after a command; it stands on the scratch branches of unit U0.6b alone, so main's workflow does not run the crate's tests (CP-173's named limit).
set -u
LC_ALL=C
export LC_ALL
name=45-r3-drive-trial
crate=tools/r3-trials/drive
fence_test=the_fence_is_on_in_this_test_target
fence_want=2

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

tests_ok() {
  results=$(grep -c '^test result: ok\.' "$1")
  failed=$(grep -c -e '^test result: FAILED' -e ' \.\.\. FAILED$' "$1")
  fenced=$(grep -c "^test .*$fence_test \.\.\. ok\$" "$1")
  printf '%s tests: results_ok=%s failed=%s fence_ok=%s fence_want=%s\n' "$name" "$results" "$failed" "$fenced" "$fence_want"
  [ "$results" -ge "$fence_want" ] && [ "$failed" -eq 0 ] && [ "$fenced" -eq "$fence_want" ]
}

last_line_is() {
  last=$(tail -n 1 "$1")
  case $last in
    "$2"*) return 0 ;;
    *) return 1 ;;
  esac
}

clean() {
  status=$(git status --porcelain --untracked-files=no) || fail "git status failed"
  [ -z "$status" ] || fail "a tracked file differs from HEAD $1"
}

seconds_since() {
  now=$(date +%s)
  printf '%s seconds %s %s\n' "$name" "$1" "$((now - $2))"
}

run_checks() {
  out=$1
  py=python3
  command -v python3 >/dev/null 2>&1 || py=python
  clean "before the first command"
  start=$(date +%s)
  cargo test --locked --manifest-path "$crate/Cargo.toml" --no-fail-fast >"$out/test.txt" 2>&1
  code=$?
  seconds_since test "$start"
  clean "after cargo test"
  if [ "$code" -ne 0 ] || ! tests_ok "$out/test.txt"; then
    tail -n 40 "$out/test.txt"
    fail "cargo test of the drive crate (exit $code)"
  fi
  start=$(date +%s)
  cargo clippy --locked --manifest-path "$crate/Cargo.toml" --all-targets -- -D warnings >"$out/clippy.txt" 2>&1
  code=$?
  seconds_since clippy "$start"
  clean "after cargo clippy"
  if [ "$code" -ne 0 ]; then
    tail -n 40 "$out/clippy.txt"
    fail "cargo clippy of the drive crate (exit $code)"
  fi
  start=$(date +%s)
  "$py" -I "$crate/verdict.py" --self-test >"$out/verdict-self-test.txt" 2>&1
  code=$?
  clean "after verdict.py --self-test"
  if [ "$code" -ne 0 ] || ! last_line_is "$out/verdict-self-test.txt" "PASS self-test: verdict.py"; then
    tail -n 20 "$out/verdict-self-test.txt"
    fail "verdict.py --self-test (exit $code)"
  fi
  "$py" -I "$crate/rebuild.py" --self-test >"$out/rebuild-self-test.txt" 2>&1
  code=$?
  clean "after rebuild.py --self-test"
  if [ "$code" -ne 0 ] || ! last_line_is "$out/rebuild-self-test.txt" "PASS self-test: rebuild.py"; then
    tail -n 20 "$out/rebuild-self-test.txt"
    fail "rebuild.py --self-test (exit $code)"
  fi
  "$py" -I "$crate/verdict.py" --check >"$out/verdict-check.txt" 2>&1
  code=$?
  clean "after verdict.py --check"
  if [ "$code" -ne 0 ] || ! last_line_is "$out/verdict-check.txt" "verdict check same=yes"; then
    tail -n 5 "$out/verdict-check.txt"
    fail "verdict.py --check (exit $code)"
  fi
  seconds_since python "$start"
  verdict_line=$(tail -n 1 "$out/verdict-self-test.txt") || fail "the output of verdict.py --self-test cannot be read"
  rebuild_line=$(tail -n 1 "$out/rebuild-self-test.txt") || fail "the output of rebuild.py --self-test cannot be read"
  printf '%s %s\n' "$verdict_line" "$rebuild_line"
  printf 'PASS %s\n' "$name"
}

fixture() {
  printf '%s\n' "$2" >"$work/$1.txt"
  printf '%s\n' "$work/$1.txt"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/r3-drive-trial.XXXXXX") || {
    printf 'FAIL self-test:%s (mktemp failed)\n' "$name"
    exit 1
  }
  ok_line='test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
  good="test tests::$fence_test ... ok
$ok_line
test tests::$fence_test ... ok
$ok_line"
  rejected=0
  wrong=
  file=$(fixture good "$good")
  tests_ok "$file" >/dev/null || wrong="$wrong good-test-output"
  for c in one-fence-test fence-test-ignored fence-test-failed a-failed-result a-failed-test no-result-line empty three-fence-tests; do
    case $c in
      one-fence-test) text="test tests::$fence_test ... ok
$ok_line
$ok_line" ;;
      fence-test-ignored) text="test tests::$fence_test ... ok
test tests::$fence_test ... ignored
$ok_line
$ok_line" ;;
      fence-test-failed) text="test tests::$fence_test ... ok
test tests::$fence_test ... FAILED
$ok_line
$ok_line" ;;
      a-failed-result) text="$good
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s" ;;
      a-failed-test) text="$good
test body::tests::x ... FAILED" ;;
      no-result-line) text="test tests::$fence_test ... ok
test tests::$fence_test ... ok" ;;
      empty) text= ;;
      three-fence-tests) text="$good
test x::$fence_test ... ok" ;;
      *) text= ;;
    esac
    file=$(fixture "$c" "$text")
    if tests_ok "$file" >/dev/null; then
      wrong="$wrong $c"
    else
      rejected=$((rejected + 1))
    fi
  done
  file=$(fixture verdict-pass "PASS self-test: verdict.py (27 bad blocks refused)")
  last_line_is "$file" "PASS self-test: verdict.py" || wrong="$wrong verdict-pass"
  for c in verdict-fail verdict-pass-not-last check-differs; do
    case $c in
      verdict-fail) text="FAIL self-test: verdict.py (1 checks missed)"; want="PASS self-test: verdict.py" ;;
      verdict-pass-not-last) text="PASS self-test: verdict.py (1)
self-test: x"; want="PASS self-test: verdict.py" ;;
      check-differs) text="verdict check same=no"; want="verdict check same=yes" ;;
      *) text=; want=- ;;
    esac
    file=$(fixture "$c" "$text")
    if last_line_is "$file" "$want"; then
      wrong="$wrong $c"
    else
      rejected=$((rejected + 1))
    fi
  done
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

main() {
  case ${1:-} in
    --self-test) self_test ;;
    '')
      out=${CI_D_ARTIFACTS:-}
      [ -n "$out" ] || out=$(mktemp -d "${TMPDIR:-/tmp}/r3-drive-trial.XXXXXX") || fail "mktemp failed"
      mkdir -p "$out" || fail "the artifacts directory cannot be made"
      CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target/r3-drive-trial}
      export CARGO_TARGET_DIR
      run_checks "$out"
      ;;
    *) fail "usage: sh scripts/r3/ci.d/45-r3-drive-trial.sh [--self-test]" ;;
  esac
}

main "$@"; exit $?
