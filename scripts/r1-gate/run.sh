#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"

help_text() {
  cat <<'EOF'
R1 gate driver. Runs the six sub-checks and prints one PASS/FAIL line each.

  sh scripts/r1-gate/run.sh --self-test
      Prove the harness's own logic on fixtures. Needs no crate, cargo, gh or
      network. Exits 0 iff every sub-check's predicate classifies its fixtures
      correctly.

  sh scripts/r1-gate/run.sh [--duration N]
      Measure the real pipeline: drive the halvo-capture bin, grep the store,
      sample cost, read Windows CI. Exits 0 iff every sub-check PASSes. On a
      tree with no capture crate every capture and store sub-check FAILs, which
      is the P0 baseline.

Safe to run twice: each sub-check makes and removes its own temp store.
EOF
}

mode=measure
verbose=''
duration=$GATE_DURATION
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) mode=self-test ;;
    -v) verbose=-v ;;
    --duration) shift; duration=${1:-$GATE_DURATION} ;;
    -h|--help) help_text; exit 0 ;;
    *) printf 'run.sh: unknown argument: %s\n' "$1" >&2; exit 64 ;;
  esac
  shift
done

CHECKS="timed-run store deletion secure-input cost memory windows-ci no-ll-hook"
rc=0
for c in $CHECKS; do
  if [ "$mode" = self-test ]; then
    "$DIR/check-$c.sh" --self-test ${verbose:+"$verbose"} || rc=1
  else
    "$DIR/check-$c.sh" --measure "$duration" || rc=1
  fi
done
[ "$mode" = self-test ] || printf 'OPEN second-account-read (owner step; no test bed)\n'
exit $rc
