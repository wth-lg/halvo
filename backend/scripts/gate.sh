#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
. "$ROOT/scripts/env.sh"
GATE_LINE_OF_THE_PLAN="10-subscription 15-lifecycle 20-card-entry 30-cap 40-prompts 50-concurrency 55-metering 60-schema 70-contract 80-storage 85-licenses 90-outbound 95-model"
CANNOT_RUN=2
BAD_USAGE=64
FIXTURES="$ROOT/gate/fixtures/driver"

usage() {
  cat >&2 <<'EOF'
Runs the 13 checks that this script lists, in the order of the list, and prints one line per
check of the list.

  sh scripts/gate.sh                 exit 0 only when every line is PASS, else 1
  sh scripts/gate.sh --except ID     leave out the check ID and print "SKIP name: left out by
                                     --except ID" in its place (the outbound check runs the
                                     others this way)
  sh scripts/gate.sh --self-test     prove the driver on fixture checks, then run the self-test
                                     of scripts/check-machine-paths.sh and of each check; exit 0
                                     only when all of them hold, else 1

A check is gate/checks/NN-name.sh. It prints exactly one result line: "PASS name: ..." with
exit 0, or "FAIL name: ..." with exit 1. Everything else it prints goes to its log. A check that
breaks this contract is reported as FAIL, and so is a check of the list whose file is absent.
With --self-test a check passes on a good fixture and fails on a bad one, and prints
"self-test name: good FIXTURE: PASS as expected" and "self-test name: bad FIXTURE: FAIL as
expected" beside the lines of those runs; a check that lacks either one, or prints these lines
and nothing else, fails the self-test, unless it is still a stub. Each run keeps its logs in a
new directory under .tmp/, named on stderr.

Before its checks the gate runs scripts/check-machine-paths.sh over the tracked files, so it
needs a git checkout. Exit 2 when the gate cannot run: that script fails, gate/checks holds a
file that the list does not name, every check was left out, or the log directory under .tmp/
cannot be made. The reason is on stderr, and no check ran. Exit 64 on bad usage, which includes
an ID that names no check and a GATE_CHECKS_DIR that the driver refuses.

GATE_CHECKS_DIR points the driver at a directory of fixture checks. Its list is its file "ids",
one ID per line, or without that file every NN-name.sh in it. GATE_NESTED marks a run that the
driver's self-test started; a self-test with it leaves out the driver's own fixtures. After it
has read its arguments, the driver names each of the two on stderr when it is set. Without
--self-test it refuses GATE_CHECKS_DIR, unless GATE_NESTED is set or scripts/outbound-hosts.sh
started the run, which the driver reads from OUTBOUND_RUN.
EOF
  exit "$BAD_USAGE"
}

refuse() {
  echo "gate: $1" >&2
  exit "$BAD_USAGE"
}

cannot_run() {
  echo "gate: cannot run: $1" >&2
  exit "$CANNOT_RUN"
}

checks_found_by_name() {
  for check in "$CHECKS"/[0-9][0-9]-*.sh; do
    [ -f "$check" ] || continue
    basename "$check" .sh
  done | tr '\n' ' '
}

if [ -z "${GATE_CHECKS_DIR:-}" ]; then
  CHECKS="$ROOT/gate/checks"
  ids=$GATE_LINE_OF_THE_PLAN
  listed=1
elif [ -f "$GATE_CHECKS_DIR/ids" ]; then
  CHECKS=$GATE_CHECKS_DIR
  ids=$(tr '\n' ' ' < "$CHECKS/ids")
  listed=1
else
  CHECKS=$GATE_CHECKS_DIR
  ids=$(checks_found_by_name)
  listed=
fi

is_one_id_of_a_list() {
  for one_id in $GATE_LINE_OF_THE_PLAN $ids; do
    [ "$one_id" != "$1" ] || return 0
  done
  return 1
}

mode=run
except=
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) mode=self-test ;;
    --except)
      [ $# -ge 2 ] || usage
      is_one_id_of_a_list "$2" || refuse "--except $2 names no check; the list: $ids"
      except="$except $2"
      shift
      ;;
    *) usage ;;
  esac
  shift
done

started_by_the_self_test_or_by_the_outbound_check() {
  [ -n "${GATE_NESTED:-}" ] || [ -n "${OUTBOUND_RUN:-}" ]
}

if [ -n "${GATE_NESTED:-}" ]; then
  echo "gate: GATE_NESTED is set: a self-test leaves out the driver's own fixtures, and a run takes GATE_CHECKS_DIR" >&2
fi
if [ -n "${GATE_CHECKS_DIR:-}" ]; then
  echo "gate: GATE_CHECKS_DIR is set: the checks are those of $GATE_CHECKS_DIR, not the 13 of the gate" >&2
  [ "$mode" = self-test ] || started_by_the_self_test_or_by_the_outbound_check ||
    refuse "a run of the gate takes GATE_CHECKS_DIR only from the driver's self-test or from the outbound check; unset it"
fi

files_that_the_list_does_not_name() {
  for entry in "$CHECKS"/*; do
    [ -e "$entry" ] || continue
    file=$(basename "$entry")
    [ "$file" != ids ] || continue
    case " $ids " in
      *" ${file%.sh} "*) [ "$file" = "${file%.sh}" ] || continue ;;
    esac
    printf ' %s' "$file"
  done
}

if [ -n "$listed" ]; then
  unlisted=$(files_that_the_list_does_not_name)
  [ -z "$unlisted" ] || cannot_run "$CHECKS holds files that the list of the checks does not name:$unlisted"
fi

if [ "$mode" = run ] && [ -z "${GATE_CHECKS_DIR:-}" ]; then
  (cd "$ROOT" && sh scripts/check-machine-paths.sh) >&2 ||
    cannot_run "scripts/check-machine-paths.sh failed, for the reason above"
fi

mkdir -p "$ROOT/.tmp"
GATE_STATE=$(mktemp -d "$ROOT/.tmp/gate.XXXXXX") || exit "$CANNOT_RUN"
export GATE_STATE
echo "gate: logs in $GATE_STATE" >&2

left_out() {
  case " $except " in *" $1 "*) return 0 ;; esac
  return 1
}

result_line() {
  id=$1
  exit_code=$2
  log=$3
  name=${id#*-}
  own_result_line="^(PASS|FAIL) $name: "
  count=$(grep -acE "$own_result_line" "$log")
  line=$(grep -aE "$own_result_line" "$log" | tail -1)
  case "$exit_code:$count:$line" in
    "0:1:PASS $name: "*)
      printf '%s\n' "$line"
      return 0
      ;;
    "1:1:FAIL $name: "*)
      printf '%s\n' "$line"
      return 1
      ;;
  esac
  printf 'FAIL %s: the check broke its contract (exit %s, %s result lines)\n' "$name" "$exit_code" "$count"
  return 1
}

run_checks() {
  failed=0
  ran=0
  for id in $ids; do
    name=${id#*-}
    if left_out "$id"; then
      echo "SKIP $name: left out by --except $id"
      continue
    fi
    ran=$((ran + 1))
    if [ ! -f "$CHECKS/$id.sh" ]; then
      echo "FAIL $name: no file $id.sh in the checks directory"
      failed=1
      continue
    fi
    sh "$CHECKS/$id.sh" > "$GATE_STATE/$id.log" 2>&1
    result_line "$id" "$?" "$GATE_STATE/$id.log" || failed=1
  done
  if [ "$ran" -eq 0 ]; then
    echo "gate: cannot run: no check to run under $CHECKS" >&2
    return "$CANNOT_RUN"
  fi
  return "$failed"
}

fixture_result() {
  kind=$1
  fixture=$2
  got=$3
  want_exit=$4
  out=$5
  want_out=$6
  err=$7
  want_err=$8
  if [ "$got" = "$want_exit" ] && cmp -s "$out" "$want_out" && grep -qE -- "$want_err" "$err"; then
    case "$kind" in
      good) echo "self-test driver: good $fixture: PASS as expected" ;;
      bad) echo "self-test driver: bad $fixture: FAIL as expected" ;;
      stopped) echo "self-test driver: stopped $fixture: exit $want_exit as expected" ;;
    esac
    return 0
  fi
  echo "self-test driver: $fixture: exit $got (expected $want_exit), or standard output differs from ${want_out#"$ROOT"/}, or standard error has no line that matches $want_err:"
  sed 's/^/  /' "$out" "$err"
  return 1
}

driver_fixture() {
  kind=$1
  fixture=$2
  starter=$3
  want_exit=$4
  want_out=$5
  want_err=$6
  dir=$7
  shift 7
  out="$GATE_STATE/driver-$fixture.out"
  err="$GATE_STATE/driver-$fixture.err"
  case "$starter" in
    self-test) mark=GATE_NESTED=1 ;;
    outbound-check) mark="OUTBOUND_RUN=$GATE_STATE" ;;
    caller) mark= ;;
  esac
  env -u GATE_NESTED -u OUTBOUND_RUN ${mark:+"$mark"} GATE_CHECKS_DIR="$dir" sh "$ROOT/scripts/gate.sh" "$@" > "$out" 2> "$err"
  fixture_result "$kind" "$fixture" "$?" "$want_exit" "$out" "$want_out" "$err" "$want_err"
}

list_fixture() {
  listed_names=$(for id in $GATE_LINE_OF_THE_PLAN; do printf '%s\n' "${id#*-}"; done)
  recorded_names=$(sed -n 's/^[A-Z]* \([^:]*\): .*/\1/p' "$ROOT/docs/baseline/r7-gate-before.txt")
  if [ "$listed_names" = "$recorded_names" ]; then
    echo "self-test driver: good list-equals-the-baseline: PASS as expected"
    return 0
  fi
  echo "self-test driver: list-equals-the-baseline: the names of the driver's list are not those of docs/baseline/r7-gate-before.txt, in its order:"
  printf '%s\n' "$listed_names" | tr '\n' ' ' | sed 's/^/  /'
  echo
  return 1
}

copy_fixture() {
  copy="$GATE_STATE/driver-copy"
  mkdir -p "$copy/scripts" "$copy/gate/checks" "$copy/taken-out"
  cp "$ROOT/scripts/gate.sh" "$ROOT/scripts/env.sh" "$ROOT/scripts/check-machine-paths.sh" "$copy/scripts/"
  # shellcheck disable=SC2016
  env -u GATE_CHECKS_DIR -u GATE_NESTED -u OUTBOUND_RUN -u GIT_DIR -u GIT_WORK_TREE -u GIT_INDEX_FILE sh -c '
    copy=$1
    gate_in_the_copy() {
      sh "$copy/scripts/gate.sh" > "$copy/$1.out" 2> "$copy/$1.err"
      echo "$?" > "$copy/$1.exit"
    }
    git -C "$copy" init -q . 2> /dev/null && git -C "$copy" add scripts || exit 1
    gate_in_the_copy clean
    echo "echo \"PASS new: a check that the list does not name\"" > "$copy/gate/checks/96-new.sh"
    gate_in_the_copy unlisted
    mv "$copy/gate/checks/96-new.sh" "$copy/taken-out/96-new.sh"
    printf "the notes are under %sUsers%sada%snotes.txt\n" / / / > "$copy/notes.txt"
    git -C "$copy" add notes.txt || exit 1
    git -C "$copy" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "A file with a home path" || exit 1
    gate_in_the_copy committed
  ' sh "$copy" > "$copy.log" 2>&1 || {
    echo "self-test driver: copy: git could not make an index and a commit under $copy; it commits as the configured git user:"
    sed 's/^/  /' "$copy.log"
    return 1
  }
  held=0
  fixture_result bad no-check-file "$(cat "$copy/clean.exit")" 1 "$copy/clean.out" "$FIXTURES/no-check-file.expected" "$copy/clean.err" '^gate: logs in ' || held=1
  fixture_result stopped unlisted-file-in-gate-checks "$(cat "$copy/unlisted.exit")" 2 "$copy/unlisted.out" /dev/null "$copy/unlisted.err" 'does not name: 96-new\.sh$' || held=1
  fixture_result stopped machine-path-in-a-commit "$(cat "$copy/committed.exit")" 2 "$copy/committed.out" /dev/null "$copy/committed.err" '^notes\.txt:1:' || held=1
  return "$held"
}

unproven_fixture() {
  out="$GATE_STATE/driver-unproven.out"
  GATE_NESTED=1 GATE_CHECKS_DIR="$FIXTURES/unproven" sh "$ROOT/scripts/gate.sh" --self-test > "$out" 2>&1
  got=$?
  passed=
  for name in bare good-only bad-only stub-and-more stub-without-call commented-stub-call stub-call-and-more other-load-line code-before-a-comment claims-only failing; do
    grep -q "^self-test $name: FAILED" "$out" || passed="$passed $name"
  done
  if [ "$got" = 1 ] && [ -z "$passed" ]; then
    echo "self-test driver: bad unproven: FAIL as expected"
    return 0
  fi
  echo "self-test driver: unproven: exit $got (expected 1), or these checks, which prove nothing, did not fail:$passed"
  sed 's/^/  /' "$out"
  return 1
}

is_stub() {
  grep -q '^stub ' "$1" || return 1
  comment='#.*'
  # shellcheck disable=SC2016
  load_of_stub='\. "\$\(dirname "\$0"\)/\.\./lib/stub\.sh"'
  stub_call='stub [a-z0-9-]+ "[^"]*" "\$\{1:-\}"'
  blank_or_one_of="^($comment|$load_of_stub|$stub_call)?\$"
  ! grep -vE "$blank_or_one_of" "$1" | grep -q .
}

proven_by_its_self_test() {
  name=$1
  status=$2
  log=$3
  stub=$4
  good=$(grep -acE "^self-test $name: good .*: PASS as expected\$" "$log")
  bad=$(grep -acE "^self-test $name: bad .*: FAIL as expected\$" "$log")
  lines_of_the_runs=$(grep -avcE "^self-test $name: (good|bad) " "$log")
  if [ "$status" = 0 ] && [ -n "$stub" ]; then
    echo "self-test $name: stub, nothing to prove yet"
  elif [ "$status" = 0 ] && [ "$good" -ge 1 ] && [ "$bad" -ge 1 ] && [ "$lines_of_the_runs" -ge 1 ]; then
    grep -aE "^self-test $name: (good|bad) " "$log"
  else
    echo "self-test $name: FAILED (exit $status, good fixtures $good, bad fixtures $bad, other lines $lines_of_the_runs); the end of its log:"
    tail -20 "$log" | sed 's/^/  /'
    return 1
  fi
}

self_test() {
  failed=0
  ran=0
  if [ -z "${GATE_NESTED:-}" ]; then
    a_run='^gate: logs in '
    driver_fixture good all-pass self-test 0 "$FIXTURES/all-pass.expected" '^gate: GATE_NESTED is set: ' "$FIXTURES/all-pass" || failed=1
    driver_fixture bad mixed self-test 1 "$FIXTURES/mixed.expected" '^gate: GATE_CHECKS_DIR is set: ' "$FIXTURES/mixed" || failed=1
    driver_fixture stopped checks-dir-of-a-caller caller 64 /dev/null '^gate: a run of the gate takes GATE_CHECKS_DIR only' "$FIXTURES/all-pass" || failed=1
    driver_fixture good checks-dir-of-the-outbound-check outbound-check 0 "$FIXTURES/all-pass.expected" '^gate: GATE_CHECKS_DIR is set: ' "$FIXTURES/all-pass" || failed=1
    driver_fixture bad fail-line self-test 1 "$FIXTURES/fail-line.expected" "$a_run" "$FIXTURES/fail-line" || failed=1
    driver_fixture bad contract-break self-test 1 "$FIXTURES/contract-break.expected" "$a_run" "$FIXTURES/contract-break" || failed=1
    driver_fixture bad stub self-test 1 "$FIXTURES/stub.expected" "$a_run" "$FIXTURES/stub" || failed=1
    mkdir -p "$GATE_STATE/empty"
    driver_fixture stopped empty self-test 2 /dev/null '^gate: cannot run: no check to run' "$GATE_STATE/empty" || failed=1
    driver_fixture stopped empty-in-a-self-test self-test 2 /dev/null '^gate: cannot run: no check to run' "$GATE_STATE/empty" --self-test || failed=1
    driver_fixture stopped unknown-argument self-test 64 /dev/null '^Runs the 13 checks' "$FIXTURES/all-pass" --bogus || failed=1
    driver_fixture stopped except-without-an-id self-test 64 /dev/null '^Runs the 13 checks' "$FIXTURES/all-pass" --except || failed=1
    driver_fixture bad absent self-test 1 "$FIXTURES/absent.expected" "$a_run" "$FIXTURES/absent" || failed=1
    driver_fixture stopped unlisted self-test 2 /dev/null 'does not name: 10-here 20_near\.sh$' "$FIXTURES/unlisted" || failed=1
    driver_fixture good except self-test 0 "$FIXTURES/except.expected" "$a_run" "$FIXTURES/except" --except 20-left-out || failed=1
    driver_fixture stopped except-of-no-check self-test 64 /dev/null '^gate: --except 20-left-uot names no check' "$FIXTURES/except" --except 20-left-uot || failed=1
    driver_fixture stopped except-of-two-ids-in-one-argument self-test 64 /dev/null '^gate: --except 10-kept 20-left-out names no check' "$FIXTURES/except" --except '10-kept 20-left-out' || failed=1
    driver_fixture good except-of-a-check-of-the-gate self-test 0 "$FIXTURES/all-pass.expected" "$a_run" "$FIXTURES/all-pass" --except 90-outbound || failed=1
    driver_fixture stopped every-check-left-out self-test 2 "$FIXTURES/every-check-left-out.expected" '^gate: cannot run: no check to run' "$FIXTURES/except" --except 10-kept --except 20-left-out || failed=1
    list_fixture || failed=1
    driver_fixture good stub-in-a-self-test self-test 0 "$FIXTURES/stub-in-a-self-test.expected" "$a_run" "$FIXTURES/stub" --self-test || failed=1
    driver_fixture good proven self-test 0 "$FIXTURES/proven.expected" "$a_run" "$FIXTURES/proven" --self-test || failed=1
    driver_fixture bad absent-in-a-self-test self-test 1 "$FIXTURES/absent-in-a-self-test.expected" "$a_run" "$FIXTURES/absent" --self-test || failed=1
    driver_fixture good except-in-a-self-test self-test 0 "$FIXTURES/except-in-a-self-test.expected" "$a_run" "$FIXTURES/except" --self-test --except 20-left-out || failed=1
    unproven_fixture || failed=1
    copy_fixture || failed=1
    log="$GATE_STATE/machine-paths.self-test.log"
    sh "$ROOT/scripts/check-machine-paths.sh" --self-test > "$log" 2>&1
    proven_by_its_self_test machine-paths "$?" "$log" "" || failed=1
  fi
  for id in $ids; do
    name=${id#*-}
    if left_out "$id"; then
      echo "SKIP $name: left out by --except $id"
      continue
    fi
    ran=$((ran + 1))
    check="$CHECKS/$id.sh"
    log="$GATE_STATE/$id.self-test.log"
    if [ ! -f "$check" ]; then
      echo "self-test $name: FAILED: no file $id.sh in the checks directory"
      failed=1
      continue
    fi
    sh "$check" --self-test > "$log" 2>&1
    status=$?
    stub=
    ! is_stub "$check" || stub=1
    proven_by_its_self_test "$name" "$status" "$log" "$stub" || failed=1
  done
  if [ "$ran" -eq 0 ]; then
    echo "gate: cannot run: no check to run under $CHECKS" >&2
    return "$CANNOT_RUN"
  fi
  return "$failed"
}

case "$mode" in
  run) run_checks ;;
  self-test) self_test ;;
esac
