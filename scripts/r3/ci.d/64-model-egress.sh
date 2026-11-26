#!/bin/sh
# jobs: macos windows
# rule: The door halvo-model, built in its stand-in form with the feature loopback and started by sh scripts/egress/check.sh with the list scripts/egress/lists/loopback.txt, sends its one granted request to the stand-in host on the list's one entry, 127.0.0.1:47201, writes the reply line, and reaches nothing else: the egress line of the check reads command=0 denied=0, as the ignored test the_built_door_under_the_egress_check_reaches_the_granted_loopback_origin_alone of crates/halvo-model/tests/egress.rs shows when this check runs it alone by its exact name; the shipped build of the door, which has no loopback port, is not run here (62-build reads it for the markers). In the macos job the check then runs the one ignored unit test of halvo-runner's library that starts that built door from the runner's own start, model::tests::real_door::a_run_asks_the_built_door_through_the_runners_own_start, by its exact name and under sh scripts/egress/check.sh in its mode sandbox with a list of 127.0.0.1 at the ports 49152 to 65535 alone, which the check writes at its run: the test passes, and the egress line of that call reads command=0 denied=0. The windows job does not run that test, whose module a unix build alone holds.
# lane: alone
set -u
name=64-model-egress
test_name=the_built_door_under_the_egress_check_reaches_the_granted_loopback_origin_alone

# The test of the second call, which the macos job alone makes: the one ignored unit test of
# halvo-runner's library that starts the built door from the runner's own start (r6 U5.4). The door
# that it starts is target/debug/halvo-model, which the first call of this check builds: cargo test
# -p halvo-model --test egress builds the package's program for that target, with the feature
# loopback, which the package's dev-dependency on itself turns on in a test build.
runner_test=model::tests::real_door::a_run_asks_the_built_door_through_the_runners_own_start

# The ports of the list of the second call: the range from which the system picks the port of a
# socket bound to port 0, which the test's stand-in host on 127.0.0.1:0 gets.
first_port=49152
last_port=65535

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# write_list FILE: writes the list of the egress check for the second call, 127.0.0.1 at each port
# of the range, sorted as the check asks, between the headings [fixed] and [agent]: the list that
# scripts/r6/ci.d/61-r6-senders-egress.sh writes.
write_list() {
  {
    printf '[fixed]\n'
    awk -v first="$first_port" -v last="$last_port" 'BEGIN { for (p = first; p <= last; p++) print "127.0.0.1:" p }' |
      LC_ALL=C sort
    printf '[agent]\n'
  } >"$1"
}

# The defect of one call of the check, from its exit status $1, its output $2, the name $3 of its
# test and the words $4 that say where that test stands; none when the one test passed and the
# output holds the egress line with command=0 denied=0.
verdict_defect() {
  egress=$(grep '^egress mode=' "$2" | sed 1q)
  if [ "$1" -ne 0 ]; then
    printf 'the call exited %s (%s): read %s/%s of the artifacts\n' "$1" "${egress:-no egress line}" "$name" "${2##*/}"
    return 1
  fi
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  if [ "$counts" != '1 0' ]; then
    printf 'the filter ran %s passed and %s failed tests, not the one ignored test %s of %s: a rename of the test changes its name in this check, and the test keeps its ignore reason, which keeps it out of the merge gate\n' "${counts% *}" "${counts#* }" "$3" "$4"
    return 1
  fi
  case $egress in
    *' command=0 denied=0 '*) return 0 ;;
  esac
  printf 'the test passed and the output holds no egress line with command=0 denied=0 (%s)\n' "${egress:-none}"
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --locked -p halvo-model --test egress -- --exact "$test_name" --ignored --nocapture >"$CI_D_ARTIFACTS/test.txt" 2>&1
  code=$?
  grep -e '^egress mode=' -e '^test result: ' "$CI_D_ARTIFACTS/test.txt"
  defect=$(verdict_defect "$code" "$CI_D_ARTIFACTS/test.txt" "$test_name" 'the target egress of halvo-model') || fail "$defect"
  if [ "${CI_D_JOB:-}" = macos ]; then
    # The second call. The library tests of halvo-runner are built first, outside the egress check,
    # so that the call under the check builds nothing and runs offline.
    list=$(mktemp "${TMPDIR:-/tmp}/$name.list.XXXXXX") || exit 2
    write_list "$list" || fail 'the list of the egress check could not be written'
    cargo test --locked -p halvo-runner --lib --no-run >"$CI_D_ARTIFACTS/runner-build.txt" 2>&1 ||
      fail "the build of the library tests of halvo-runner exited $?: read $name/runner-build.txt of the artifacts"
    sh scripts/egress/check.sh --mode sandbox "$list" -- cargo test --locked --offline -p halvo-runner --lib -- --exact --ignored "$runner_test" >"$CI_D_ARTIFACTS/runner-test.txt" 2>&1 </dev/null
    code=$?
    grep -e '^egress mode=' -e '^test result: ' "$CI_D_ARTIFACTS/runner-test.txt"
    defect=$(verdict_defect "$code" "$CI_D_ARTIFACTS/runner-test.txt" "$runner_test" 'the library of halvo-runner') || fail "$defect"
  fi
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$3" >"$work/$1.txt"
  got=PASS
  verdict_defect "$2" "$work/$1.txt" "$test_name" 'the target egress of halvo-model' >/dev/null || got=FAIL
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
  # The second call's output: the test's lines first, then the egress line of the check around it.
  case_of the-runners-test-under-the-check 0 "running 1 test
test $runner_test ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 197 filtered out; finished in 0.41s

egress mode=sandbox list=list.txt command=0 denied=0 log=target/egress/3/denied.log" PASS
  case_of the-runners-test-under-another-name 0 "running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 198 filtered out; finished in 0.00s

egress mode=sandbox list=list.txt command=0 denied=0 log=target/egress/4/denied.log" FAIL
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
