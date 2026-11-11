#!/bin/sh
# jobs: macos windows
# rule: The egress check's self-test passes in each mode of the job, fails with probes=miss when its deny rule is off on a hosted runner, and the tests of the mode pass.
set -u
name=10-egress

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

modes_of() {
  case $1 in
    macos) printf '%s\n' sandbox ;;
    windows) printf '%s\n' wfp ;;
    *) printf '\n' ;;
  esac
}

checks_line() {
  printf '%s\n' "$1" | sed -n 's/^checks //p'
}

# decide KIND CODE OUTPUT: prints pass, or the cause of a failure, for one command of the file.
decide() {
  checks=$(checks_line "$3") || checks=
  cause=$(printf '%s\n' "$3" | sed -n 's/^REFUSED \([a-z-]*\):.*/\1/p') || cause=
  cause=${cause%%"
"*}
  case $1:$2 in
    self-test:0)
      if [ "$checks" = 'probes=ok quiet=ok loopback=ok default=ok' ]; then
        printf '%s\n' pass
      else
        printf '%s\n' 'self-test exited 0 without four ok checks'
      fi
      ;;
    self-test:1) printf 'self-test exited 1 (%s)\n' "${checks:-no checks line}" ;;
    self-test:2) printf 'self-test refused (%s)\n' "$cause" ;;
    control:1)
      case " $checks " in
        *" probes=miss "*) printf '%s\n' pass ;;
        *) printf 'control exited 1 without probes=miss (%s)\n' "${checks:-no checks line}" ;;
      esac
      ;;
    control:0) printf '%s\n' 'control exited 0: the self-test passes without its deny rule' ;;
    control:2) printf 'control refused (%s)\n' "$cause" ;;
    tests:0)
      case $3 in
        *"tests passed="*" failed=0") printf '%s\n' pass ;;
        *) printf '%s\n' 'tests exited 0 without failed=0' ;;
      esac
      ;;
    tests:*) printf 'tests exited %s (%s)\n' "$2" "$(printf '%s\n' "$3" | grep -c '^not ok ') not ok" ;;
    *) printf '%s exited %s\n' "$1" "$2" ;;
  esac
}

run_one() {
  kind=$1
  mode=$2
  shift 2
  out=$(sh scripts/egress/check.sh --mode "$mode" --self-test "$@" 2>"$CI_D_ARTIFACTS/$kind-$mode.err")
  code=$?
  printf '%s\n' "$out" >"$CI_D_ARTIFACTS/$kind-$mode.out"
  printf '%s\n' "$out"
  err=$(cat "$CI_D_ARTIFACTS/$kind-$mode.err") || err=
  verdict=$(decide "$kind" "$code" "$out
$err") || verdict="decide failed"
  printf '%s mode=%s exit=%s verdict=%s\n' "$kind" "$mode" "$code" "$verdict"
  [ "$verdict" = pass ] || fail "$kind --mode $mode: $verdict"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  modes=$(modes_of "${CI_D_JOB:-}") || modes=
  [ -n "$modes" ] || fail "no mode"
  if ! command -v rustc >/dev/null 2>&1; then
    if [ "${CI:-}" = true ]; then
      fail "rustc is missing"
    fi
    printf 'SKIP %s (rustc is not installed)\n' "$name"
    exit 77
  fi
  for mode in $modes; do
    if [ "${GITHUB_ACTIONS:-}" != true ] && [ "$mode" != sandbox ]; then
      continue
    fi
    run_one self-test "$mode"
  done
  if [ "${GITHUB_ACTIONS:-}" = true ]; then
    for mode in $modes; do
      run_one control "$mode" --control-no-deny
    done
  fi
  for mode in $modes; do
    if [ "${GITHUB_ACTIONS:-}" != true ] && [ "$mode" != sandbox ]; then
      continue
    fi
    out=$(sh scripts/egress/test.sh probe lists "$mode" 2>"$CI_D_ARTIFACTS/tests-$mode.err")
    code=$?
    printf '%s\n' "$out" >"$CI_D_ARTIFACTS/tests-$mode.out"
    printf '%s\n' "$out" | grep -E '^(not ok |tests passed=)'
    verdict=$(decide tests "$code" "$out") || verdict="decide failed"
    printf 'tests mode=%s exit=%s verdict=%s\n' "$mode" "$code" "$verdict"
    [ "$verdict" = pass ] || fail "test.sh $mode: $verdict"
  done
  printf 'PASS %s\n' "$name"
}

fixture() {
  got=$(decide "$2" "$3" "$4") || got="decide failed"
  if [ "$got" = pass ]; then
    got=pass
  else
    got=fail
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$5" "$got"
  if [ "$got" != "$5" ]; then
    wrong="$wrong $1"
  elif [ "$5" = fail ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  rejected=0
  wrong=
  good='egress self-test mode=wfp deny=on
probe name=ipv4 result=denied
checks probes=ok quiet=ok loopback=ok default=ok'
  fixture self-test-passes self-test 0 "$good" pass
  fixture self-test-exit-1 self-test 1 'checks probes=miss quiet=ok loopback=ok default=ok' fail
  fixture self-test-default-miss self-test 0 'checks probes=ok quiet=ok loopback=ok default=miss' fail
  fixture self-test-no-checks self-test 0 'egress self-test mode=sandbox deny=on' fail
  fixture self-test-refused self-test 2 'REFUSED mode: wfp needs an elevated shell' fail
  fixture self-test-port self-test 2 'REFUSED port: 127.0.0.1:47201' fail
  fixture control-passes control 1 'egress self-test mode=wfp deny=off
checks probes=miss quiet=ok loopback=miss default=ok' pass
  fixture control-exit-0 control 0 'checks probes=ok quiet=ok loopback=ok default=ok' fail
  fixture control-probes-ok control 1 'checks probes=ok quiet=ok loopback=miss default=ok' fail
  fixture control-refused control 2 'REFUSED usage: --control-no-deny runs on a hosted runner only' fail
  fixture control-exit-3 control 3 '' fail
  fixture tests-pass tests 0 'ok sandbox/quiet
tests passed=40 failed=0' pass
  fixture tests-fail tests 1 'not ok sandbox/quiet: want 0 0 0, got 1 0 1
tests passed=39 failed=1' fail
  fixture tests-exit-0-with-a-failure tests 0 'tests passed=39 failed=1' fail
  modes=$(modes_of macos) || modes=
  [ "$modes" = sandbox ] || wrong="$wrong modes-macos"
  modes=$(modes_of windows) || modes=
  [ "$modes" = wfp ] || wrong="$wrong modes-windows"
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
