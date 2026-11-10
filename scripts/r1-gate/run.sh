#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=run

CHECKS="a:timed-run b:store b:deletion c:secure-input d:cost e:memory f:service-run g:service-memory h:service-expiry i:service-pause j:service-delete k:content-floor l:login-start m:app-memory n:ci-results -:windows-ci -:no-ll-hook"

checks_help=''
for entry in $CHECKS; do
  checks_help="$checks_help
  $entry"
done

usage() {
  cat <<EOF
R1 gate driver. One line for each sub-check of the done-when.

  sh scripts/r1-gate/run.sh --self-test [-v]
      Prove the gate's own logic on fixtures and on stand-in programs. Needs no
      capture crate, cargo, gh or network. -v prints one line for each case:
      case <check>/<name> want=<PASS|FAIL> got=<PASS|FAIL>.

  sh scripts/r1-gate/run.sh [--duration N] [--strict] [--remove-scratch]
      Measure the built binary. N is the length of each timed run in seconds,
      a positive integer (default $GATE_DURATION). --strict fails a cost or memory
      line on a machine under memory pressure or beside another halvo-capture.
      Scratch directories stay in place and their paths go to stderr, unless
      --remove-scratch is given.

Sub-checks, as letter:name (a letter of the done-when, or - for a line outside
the letters). A name with no check script prints FAIL <name> (check script missing).$checks_help
EOF
}

run_check() {
  if [ ! -f "$DIR/check-$1.sh" ]; then
    emit_fail "$1" "check script missing"
    return 1
  fi
  check_name=$1; shift
  sh "$DIR/check-$check_name.sh" "$@"
}

missing_script_line_is() {
  out=$(run_check no-such-check --self-test)
  [ "$out" = "$1" ]
}

duration_accepted() {
  sh "$DIR/run.sh" --duration "$1" --help
}

cases() {
  t check-script-missing FAIL run_check no-such-check --self-test
  t check-script-missing-is-named PASS missing_script_line_is "FAIL no-such-check (check script missing)"
  t check-script-present PASS run_check windows-ci --self-test
  t duration-600 PASS duration_accepted 600
  t duration-0 FAIL duration_accepted 0
  t duration-letters FAIL duration_accepted abc
  t duration-empty FAIL duration_accepted ""
  t duration-negative FAIL duration_accepted -5
}


main() {
  mode=measure; verbose=''; strict=''; remove=''; duration=$GATE_DURATION
  while [ $# -gt 0 ]; do
    case "$1" in
      --self-test) mode=self-test ;;
      -v) verbose=-v ;;
      --strict) strict=--strict ;;
      --remove-scratch) remove=--remove-scratch ;;
      --duration)
        shift
        case ${1:-} in
          ''|*[!0-9]*|0) usage >&2; return 64 ;;
        esac
        duration=$1 ;;
      -h|--help) usage; return 0 ;;
      *) printf 'run.sh: unknown argument: %s\n' "$1" >&2; usage >&2; return 64 ;;
    esac
    shift
  done
  rc=0
  if [ "$mode" = self-test ]; then
    for entry in $CHECKS; do
      check=${entry#*:}
      [ -f "$DIR/check-$check.sh" ] || continue
      run_check "$check" --self-test ${verbose:+"$verbose"} ${remove:+"$remove"} || rc=1
    done
    gate_verbose=${verbose:+yes}
    name=run
    selftest_run || rc=1
    return "$rc"
  fi
  for entry in $CHECKS; do
    run_check "${entry#*:}" --measure "$duration" ${strict:+"$strict"} ${remove:+"$remove"} || rc=1
  done
  printf 'OPEN second-account-read (owner step; no test bed)\n'
  return "$rc"
}

main "$@"
