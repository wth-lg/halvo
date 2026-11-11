#!/bin/sh
# Tests of the egress check that send nothing off the machine: sh scripts/egress/test.sh [probe|lists|sandbox|wfp]...
set -u
LC_ALL=C
export LC_ALL
root=$(git rev-parse --show-toplevel) || {
  printf 'FAIL test (not in a work tree)\n'
  exit 1
}
cd "$root" || exit 1
work=$(mktemp -d "${TMPDIR:-/tmp}/egress-test.XXXXXX") || exit 1
failed=0
passed=0
serves=0
bin=target/egress/bin
exe=
case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*) exe=.exe ;;
esac

result() {
  if [ "$2" = "$3" ]; then
    passed=$((passed + 1))
    printf 'ok %s\n' "$1"
  else
    failed=$((failed + 1))
    printf 'not ok %s: want %s, got %s\n' "$1" "$2" "$3"
  fi
}

expect_exit() {
  label=$1
  want=$2
  shift 2
  "$@" >"$work/out" 2>"$work/err"
  result "$label" "$want" "$?"
}

expect_refusal() {
  label=$1
  cause=$2
  shift 2
  "$@" >"$work/out" 2>"$work/err"
  code=$?
  line=$(cat "$work/err")
  case $code:$line in
    "2:REFUSED $cause: "*) got=refused-$cause ;;
    *) got="exit-$code:$line" ;;
  esac
  if [ -s "$work/out" ]; then
    got=$got-with-stdout
  fi
  result "$label" "refused-$cause" "$got"
}

serve() {
  serves=$((serves + 1))
  stop=$work/stop-$serves
  "$bin/egress-spawn$exe" serve "$1" "$stop" "$work/ready-$serves" &
  served=$!
  n=0
  while [ ! -f "$work/ready-$serves" ] && [ $n -lt 100 ]; do
    sleep 0.05
    n=$((n + 1))
  done
}

unserve() {
  : >"$stop"
  wait "$served"
}

test_probe() {
  expect_exit probe/none-exits-0 0 sh scripts/egress/probe.sh none
  for name in egress-spawn egress-probe; do
    if [ -x "$bin/$name$exe" ]; then
      result "probe/binary-$name-built" yes yes
    else
      result "probe/binary-$name-built" yes no
    fi
  done
  expect_refusal probe/unknown-name usage sh scripts/egress/probe.sh nothing
  expect_refusal probe/no-name usage sh scripts/egress/probe.sh
  expect_refusal probe/two-names usage sh scripts/egress/probe.sh none none
  expect_exit probe/loopback-without-listener-exits-1 1 sh scripts/egress/probe.sh loopback
  serve 47201
  expect_exit probe/loopback-with-listener-exits-0 0 sh scripts/egress/probe.sh loopback
  unserve
  serve 47202
  expect_exit probe/loopback2-with-listener-exits-0 0 sh scripts/egress/probe.sh loopback2
  unserve
  expect_exit probe/listen-on-a-free-port-exits-0 0 sh scripts/egress/probe.sh listen
  serve 47201
  expect_exit probe/listen-on-a-taken-port-exits-1 1 sh scripts/egress/probe.sh listen
  unserve
  expect_exit probe/serve-on-a-taken-port-exits-3 3 sh -c "\"$bin/egress-spawn$exe\" serve 47201 '$work/s1' '$work/r1' & first=\$!; sleep 0.5; \"$bin/egress-spawn$exe\" serve 47201 '$work/s2' '$work/r2'; code=\$?; : >'$work/s1'; wait \$first; exit \$code"
  printf '%s\n' 'exit 7' >"$work/seven.sh"
  "$bin/egress-spawn$exe" run "$work/status" "$work/pid" -- sh "$work/seven.sh"
  code=$?
  got=missing
  read -r got <"$work/status"
  result probe/run-records-exit-code "0:7" "$code:$got"
  "$bin/egress-spawn$exe" run "$work/status2" "$work/pid2" -- "$work/no-such-binary"
  code=$?
  got=missing
  read -r got <"$work/status2"
  result probe/run-records-not-started "0:not-started" "$code:$got"
  if [ -z "$exe" ]; then
    printf '%s\n' 'kill -9 $$' >"$work/nine.sh"
    "$bin/egress-spawn$exe" run "$work/status3" "$work/pid3" -- sh "$work/nine.sh"
    code=$?
    got=missing
    read -r got <"$work/status3"
    result probe/run-records-signal "0:signal-9" "$code:$got"
  fi
}

[ $# -gt 0 ] || set -- probe
for part in "$@"; do
  case $part in
    probe) test_probe ;;
    lists) test_lists ;;
    sandbox) test_sandbox ;;
    wfp) test_wfp ;;
    *)
      printf 'FAIL test (unknown part %s)\n' "$part"
      exit 1
      ;;
  esac
done
printf 'tests passed=%s failed=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
