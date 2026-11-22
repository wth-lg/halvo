#!/bin/sh
# jobs: windows
# rule: the value Halvo under the Run key starts the built app under a standard account: the test target run_value_logon of the app, as the account halvo-standard of check 50's form and never as the runner's own, writes the value for the built halvo.exe with halvo-control's writer, reads it back, starts it as one command line and reads a hidden app with one halvo-capture.exe as its child and token_elevated=false, then ends the app, sees its child end and takes the value away
# lane: alone
set -u
name=36-app-windows-logon
target=run_value_logon
test_name=the_run_value_starts_a_hidden_app_with_one_child_under_a_standard_account
helper=helper_run_value

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

seconds_since() {
  now=$(date +%s) || return 1
  printf '%s\n' "$((now - $1))"
}

# Reads the output of the outer test, the file $1 with LF line ends, and fails unless the test
# passed and the helper printed each of its lines. The last result line is the outer run's own,
# which libtest prints after the helper's output that the outer test passes on.
judge() {
  last=$(grep '^test result: ' "$1" | tail -n 1)
  case $last in
    'test result: ok. 1 passed; 0 failed;'*) ;;
    *) fail "the run of $test_name reads no passed test: ${last:-no result line}" ;;
  esac
  for line in provisioned=yes read_back= children=1 visible_windows=0 token_elevated=false child_ended_after_ms= removed=yes PASS; do
    grep -q "^$helper: $line" "$1" || fail "the helper as the standard account printed no line $line"
  done
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  built=$(date +%s) || fail "the clock cannot be read"
  cargo build --locked -p halvo -p halvo-capture >"$artifacts/build.txt" 2>&1 || fail "cargo build --locked -p halvo -p halvo-capture failed; see build.txt"
  cargo test --locked -p halvo --test "$target" --no-run >>"$artifacts/build.txt" 2>&1 || fail "the test target $target of halvo did not build; see build.txt"
  took=$(seconds_since "$built") || fail "the clock cannot be read"
  printf 'build seconds: %s\n' "$took"
  ran=$(date +%s) || fail "the clock cannot be read"
  cargo test --locked -p halvo --test "$target" -- --ignored --exact --nocapture "$test_name" >"$artifacts/case.txt" 2>&1
  code=$?
  took=$(seconds_since "$ran") || fail "the clock cannot be read"
  printf 'case seconds: %s\n' "$took"
  tr -d '\r' <"$artifacts/case.txt" >"$artifacts/case-lf.txt" || fail "the output of the case cannot be read"
  grep "^$helper: " "$artifacts/case-lf.txt"
  [ "$code" -eq 0 ] || fail "cargo test of $test_name exited $code; see case.txt"
  judge "$artifacts/case-lf.txt"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the build and the case"
  printf 'PASS %s\n' "$name"
}

self_test() {
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  passed="test $test_name ... ok"
  result='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 9.12s'
  lines="$helper: provisioned=yes
$helper: written=Written
$helper: read_back=\"C:\\a\\halvo.exe\" --background
$helper: started pid=4242
$helper: child_after_ms=2100
$helper: children=1
$helper: visible_windows=0
$helper: token_elevated=false
$helper: child_ended_after_ms=900
$helper: removed=yes
$helper: PASS"
  printf '\nrunning 1 test\n%s\n%s\n\n%s\n' "$lines" "$passed" "$result" >"$work/good"
  printf '\nrunning 1 test\n%s\ntest %s ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed\n' "$lines" "$test_name" >"$work/failed"
  printf '\nrunning 1 test\n%s\n%s\n' "$lines" "$passed" >"$work/no-result"
  printf '\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 2 ignored\n' >"$work/none-ran"
  printf '%s\n' "$lines" | grep -v 'token_elevated=false' | {
    cat
    printf '%s\n\n%s\n' "$passed" "$result"
  } >"$work/no-elevation-line"
  printf '%s\n' "$lines" | sed 's/children=1/children=2/' | {
    cat
    printf '%s\n\n%s\n' "$passed" "$result"
  } >"$work/two-children"
  wrong=
  rejected=0
  for fixture in good failed no-result none-ran no-elevation-line two-children; do
    out=$(sh "$self" --judge "$work/$fixture" 2>&1)
    code=$?
    case $code:$out in
      "0:PASS $name") got=PASS ;;
      "1:FAIL $name ("*")") got=FAIL ;;
      *) got=exit-$code ;;
    esac
    case $fixture in
      good) want=PASS ;;
      *) want=FAIL ;;
    esac
    printf 'case %s/%s want=%s got=%s\n' "$name" "$fixture" "$want" "$got"
    if [ "$got" != "$want" ]; then
      printf '%s\n' "$out" | sed "s/^/  $fixture: /"
      wrong="$wrong $fixture"
    elif [ "$want" = FAIL ]; then
      rejected=$((rejected + 1))
    fi
  done
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --judge)
    judge "$2"
    printf 'PASS %s\n' "$name"
    ;;
  '') measure ;;
  *) exit 64 ;;
esac
