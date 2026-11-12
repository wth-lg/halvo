#!/bin/sh
# jobs: windows
# rule: every Windows functional test runs in its own process, and a listed defect must fail with its marker
set -u
name=50-windows-functional
test_limit_s=360

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

judge() {
  out=$(tr -d '\r' <"$3")
  ran_one_and_passed=no
  case $out in
    *"
test result: ok. 1 passed; 0 failed;"*) ran_one_and_passed=yes ;;
  esac
  id=
  if [ "$2" -eq 0 ] && [ "$ran_one_and_passed" = yes ]; then
    verdict=pass
    cause=
  elif [ "$2" -eq 0 ]; then
    verdict=FAIL
    cause='exit 0, and not one test ran and passed'
  else
    verdict=FAIL
    cause="exit $2"
  fi
}

run_test() {
  if command -v timeout >/dev/null 2>&1; then
    timeout "$test_limit_s" "$exe" --ignored --exact --nocapture "$1" </dev/null >"$2" 2>&1
  else
    "$exe" --ignored --exact --nocapture "$1" </dev/null >"$2" 2>&1
  fi
}

measure() {
  cd "$CI_D_REPO" || exit 2
  artifacts=${CI_D_ARTIFACTS:-$CI_D_REPO/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"

  cargo test --locked -p halvo-capture --test windows_functional --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  exe=$(sed -n 's/.*"name":"windows_functional".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target windows_functional"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target windows_functional" ;;
  esac
  exe=$(printf '%s\n' "$exe" | sed 's/\\\\/\//g')
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  tr -d '\r' <"$work/list.txt" | sed -n 's/: test$//p' >"$work/names" || fail "the test list cannot be read"
  total=$(grep -c '' "$work/names")
  [ "$total" -gt 0 ] || fail "the test list is empty"
  if grep -v -q '^[A-Za-z0-9_:]*$' "$work/names"; then
    fail "the test list holds a name with a character outside A-Z, a-z, 0-9, _ and :"
  fi

  : >"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  passed=0
  known=0
  fixed=0
  failed=0
  index=0
  while IFS= read -r test; do
    index=$((index + 1))
    log=$work/test-$index.log
    run_test "$test" "$log"
    code=$?
    tr -d '\r' <"$log"
    judge "$test" "$code" "$log"
    case $verdict in
      pass) passed=$((passed + 1)) ;;
      known-defect) known=$((known + 1)) ;;
      FIXED-BUT-LISTED) fixed=$((fixed + 1)) ;;
      *) failed=$((failed + 1)) ;;
    esac
    [ -z "$cause" ] || printf '%s: %s: %s\n' "$name" "$test" "$cause"
    line="WF $test result=$verdict${id:+ id=$id}"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  done <"$work/names"
  [ "$index" -eq "$total" ] || fail "ran $index of the $total listed tests"

  line="WF total=$total pass=$passed known_defect=$known fail=$failed"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  [ "$fixed" -eq 0 ] || fail "$fixed listed defects passed (FIXED-BUT-LISTED): move each test out of mod known_defect"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  printf 'PASS %s\n' "$name"
}

check() {
  case_name=$1
  want=$3
  printf '%s\n' "$2" >"$work/spec"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts-$case_name
    STUB_SPEC=$work/spec
    STUB_TF=$work/bin/tf-stub
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_TF
    sh "$0" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  shift 3
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) got=$got-without-the-text ;;
    esac
  done
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
[ "$*" = 'test --locked -p halvo-capture --test windows_functional --no-run --message-format json' ] || exit 97
path=$(printf '%s\n' "$STUB_TF" | sed 's/\//\\\\/g')
printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"halvo_capture"},"executable":null,"fresh":true}\n'
printf '{"reason":"compiler-artifact","target":{"kind":["test"],"crate_types":["bin"],"name":"windows_functional","test":true},"executable":"%s","fresh":true}\n' "$path"
printf '{"reason":"build-finished","success":true}\n'
EOF
  cat >"$work/bin/tf-stub" <<'EOF'
#!/bin/sh
if [ "$*" = --list ]; then
  count=0
  while IFS='|' read -r test kind; do
    [ -n "$test" ] || continue
    printf '%s: test\n' "$test"
    count=$((count + 1))
  done <"$STUB_SPEC"
  printf '\n%s tests, 0 benchmarks\n' "$count"
  exit 0
fi
[ "$#" -eq 4 ] && [ "$1 $2 $3" = '--ignored --exact --nocapture' ] || exit 97
kind=$(sed -n "s/^$4|//p" "$STUB_SPEC")
printf '\nrunning 1 test\n'
case $kind in
  pass)
    printf 'TF %s source_built_by_test=keyboard\ntest %s ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s\n\n' "$4" "$4"
    exit 0
    ;;
  defect)
    printf 'TF %s keystroke_events_while_typing_into_password_field=5\n' "$4"
    printf "\nthread 'tf-%s' panicked at crates\\\\halvo-capture\\\\tests\\\\windows_functional.rs:1:1:\nKNOWN-DEFECT RR-windows-01\r\n" "$4" >&2
    printf 'test %s ... FAILED\n\nfailures:\n    %s\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s\n\n' "$4" "$4"
    exit 101
    ;;
  inconclusive)
    printf "\nthread 'tf-%s' panicked at crates\\\\halvo-capture\\\\tests\\\\windows_functional.rs:1:1:\nINCONCLUSIVE: SendInput inserted no events on this machine\n" "$4" >&2
    printf 'test %s ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s\n\n' "$4"
    exit 101
    ;;
  zero)
    printf '\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s\n\n'
    exit 0
    ;;
  *) exit 98 ;;
esac
EOF
  chmod +x "$work/bin/cargo" "$work/bin/tf-stub" || exit 2
  rejected=0
  wrong=
  nl='
'

  check every-test-passes "c1_raw_input|pass${nl}helper_child_window|pass" PASS \
    "${nl}WF c1_raw_input result=pass${nl}" "${nl}WF helper_child_window result=pass${nl}" \
    "${nl}WF total=2 pass=2 known_defect=0 fail=0${nl}PASS $name"
  check zero-tests-ran "c1_raw_input|zero${nl}helper_child_window|pass" FAIL \
    "${nl}$name: c1_raw_input: exit 0, and not one test ran and passed${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}WF total=2 pass=1 known_defect=0 fail=1${nl}FAIL $name (1 tests failed)"
  check empty-list '' FAIL "FAIL $name (the test list is empty)"
  check control-failed "c1_raw_input|inconclusive${nl}helper_child_window|pass" FAIL \
    "${nl}$name: c1_raw_input: exit 101${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}WF total=2 pass=1 known_defect=0 fail=1${nl}FAIL $name (1 tests failed)"

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
