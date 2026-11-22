#!/bin/sh
# jobs: macos windows
# rule: The door halvo-model, built in its stand-in form with the feature loopback and started by sh scripts/egress/check.sh with the list scripts/egress/lists/loopback.txt, sends its one granted request to the stand-in host on the list's one entry, 127.0.0.1:47201, writes the reply line, and reaches nothing else: the egress line of the check reads command=0 denied=0, as the ignored test the_built_door_under_the_egress_check_reaches_the_granted_loopback_origin_alone of crates/halvo-model/tests/egress.rs shows when this check runs it alone by its exact name; the shipped build of the door, which has no loopback port, is not run here (62-build reads it for the markers).
# lane: alone
set -u
name=64-model-egress
test_name=the_built_door_under_the_egress_check_reaches_the_granted_loopback_origin_alone

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# The defect of one run of the test, from its exit status $1 and its output $2; none when the one test
# passed and printed the egress line with command=0 denied=0.
verdict_defect() {
  egress=$(grep '^egress mode=' "$2" | sed 1q)
  if [ "$1" -ne 0 ]; then
    printf 'cargo test exited %s (%s): read %s/test.txt of the artifacts\n' "$1" "${egress:-no egress line}" "$name"
    return 1
  fi
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  if [ "$counts" != '1 0' ]; then
    printf 'the filter ran %s passed and %s failed tests, not the one ignored test %s of the target egress: a rename of the test changes test_name of this check, and the test keeps its ignore reason, which keeps it out of the merge gate\n' "${counts% *}" "${counts#* }" "$test_name"
    return 1
  fi
  case $egress in
    *' command=0 denied=0 '*) return 0 ;;
  esac
  printf 'the test passed and printed no egress line with command=0 denied=0 (%s)\n' "${egress:-none}"
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --locked -p halvo-model --test egress -- --exact "$test_name" --ignored --nocapture >"$CI_D_ARTIFACTS/test.txt" 2>&1
  code=$?
  grep -e '^egress mode=' -e '^test result: ' "$CI_D_ARTIFACTS/test.txt"
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
  case_of passed 0 "running 1 test
the egress check's run of the built door took 6129 ms
egress mode=sandbox list=scripts/egress/lists/loopback.txt command=0 denied=0 log=target/egress/1/denied.log
test $test_name ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.13s" PASS
  case_of a-connection-outside-the-list 101 "running 1 test
egress mode=sandbox list=scripts/egress/lists/loopback.txt command=0 denied=1 log=target/egress/2/denied.log
test $test_name ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s" FAIL
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
