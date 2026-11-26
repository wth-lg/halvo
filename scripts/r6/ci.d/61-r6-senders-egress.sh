#!/bin/sh
# jobs: macos
# rule: The test build of each package that can start a sender, halvo-model and halvo-connect, which turns on their feature loopback, runs its lib, bin and test targets, and the test build of halvo-runner runs the one ignored unit test of its library that starts the built door halvo-model from the runner's own start, under sh scripts/egress/check.sh in its mode sandbox with a list of 127.0.0.1 at the ports 49152 to 65535 alone, which this row writes at its run: every test passes and no process opens a connection, sends a datagram or resolves a name outside that list, and the check fails where cargo test fails, where a call runs no test or where denied is above 0, with the denied lines. It does not show the windows job, the doc tests of these packages, the five tests of halvo-connect's target fixture that start the setuid program ps (skipped by their exact names), a test of a package that it does not list, any other test of halvo-runner, or what the egress check does not see (an xpc service, a launch-service open, what a process does after the command ends).
# lane: alone
# self-test: alone
set -u
set -f
name=61-r6-senders-egress

# The entries of the row. An entry is a whole package, whose lib, bin and test targets run in one
# call, or ONE target of a package as <package>:<target>, which runs as that package's --test
# <target> alone in a call of its own, or ONE ignored unit test of a package's library as
# <package>:lib:<test>, which runs as that package's --lib with --ignored and the test's exact name
# in a call of its own. The calls of the unit entries come last: the door that the runner's test
# starts is the one that the call of halvo-model built. A later unit adds its entry to this line.
entries='halvo-model halvo-connect halvo-runner:lib:model::tests::real_door::a_run_asks_the_built_door_through_the_runners_own_start'

# The tests that meet a rule of the sandbox, skipped by their exact names in every call. Each of
# these five of crates/halvo-connect/tests/fixture.rs starts ps, a setuid program, which a process
# under the sandbox cannot start.
skips='a_guard_ends_the_process_whose_pair_matches a_disarmed_guard_leaves_the_process_of_its_pair a_guard_leaves_a_process_whose_start_fact_differs a_hard_kill_of_the_test_process_ends_its_fixture_server the_wait_reads_an_ended_process_as_gone_and_a_running_one_as_running'

# The ports of the list: the range from which the system picks the port of a socket bound to port 0,
# which a stand-in on 127.0.0.1:0 gets.
first_port=49152
last_port=65535

# The control of the row, an ignored test of crates/halvo-connect/tests/egress_control.rs that
# connects to 192.0.2.1:443 and sends nothing. The self-test alone runs it, by this exact name with
# --ignored, through the call of the measure.
control=the_control_of_the_senders_row_connects_to_a_documentation_address_and_sends_nothing
control_dest=192.0.2.1:443

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# write_list FILE: writes the list of the egress check, 127.0.0.1 at each port of the range, sorted
# as the check asks, between the headings [fixed] and [agent].
write_list() {
  {
    printf '[fixed]\n'
    awk -v first="$first_port" -v last="$last_port" 'BEGIN { for (p = first; p <= last; p++) print "127.0.0.1:" p }' |
      LC_ALL=C sort
    printf '[agent]\n'
  } >"$1"
}

# calls ENTRIES: prints the arguments of cargo for each call, one line a call: one call for the whole
# packages, then one call for each target entry, then one call for each unit entry. Exits 1 on an
# entry of another form.
calls() {
  packages=
  targets=
  units=
  for entry in $1; do
    case $entry in
      *[!a-z0-9_:-]* | :* | *:) return 1 ;;
      *:lib:?*) units="$units $entry" ;;
      *:*:*) return 1 ;;
      *:*) targets="$targets $entry" ;;
      *) packages="$packages -p $entry" ;;
    esac
  done
  tail=' -- --exact'
  for skip in $skips; do
    tail="$tail --skip $skip"
  done
  [ -z "$packages" ] || printf 'test --locked --offline%s --lib --bins --tests%s\n' "$packages" "$tail"
  for entry in $targets; do
    printf 'test --locked --offline -p %s --test %s%s\n' "${entry%%:*}" "${entry#*:}" "$tail"
  done
  for entry in $units; do
    printf 'test --locked --offline -p %s --lib%s --ignored %s\n' "${entry%%:*}" "$tail" "${entry#*:lib:}"
  done
}

# tests_counts FILE: prints the sums of the test result lines of a call: passed, failed, ignored and
# the count of those lines.
tests_counts() {
  awk '/^test result: / {
      n++
      for (i = 1; i < NF; i++) {
        if ($(i + 1) == "passed;") p += $i
        if ($(i + 1) == "failed;") f += $i
        if ($(i + 1) == "ignored;") g += $i
      }
    }
    END { print p + 0, f + 0, g + 0, n + 0 }' "$1"
}

# verdict_defect STATUS FILE: the defect of one call under the egress check, from the check's exit
# status and its output; none when the egress line reads command=0 denied=0 and the call ran at least
# one test and failed none.
verdict_defect() {
  egress=$(grep '^egress mode=' "$2" | sed 1q)
  if [ -z "$egress" ]; then
    printf 'the egress check printed no egress line and exited %s\n' "$1"
    return 1
  fi
  command=$(printf '%s\n' "$egress" | sed -n 's/.* command=\([^ ]*\) .*/\1/p')
  denied=$(printf '%s\n' "$egress" | sed -n 's/.* denied=\([^ ]*\) .*/\1/p')
  case $denied in
    '' | *[!0-9]*)
      printf 'the egress line holds no count of denied attempts\n'
      return 1
      ;;
  esac
  if [ "$denied" -gt 0 ]; then
    printf 'denied=%s\n' "$denied"
    return 1
  fi
  read -r passed failed _ _ <<EOF
$(tests_counts "$2")
EOF
  if [ "$command" != 0 ]; then
    printf 'cargo test exited %s with %s failed tests\n' "${command:-without a status}" "$failed"
    return 1
  fi
  if [ "$passed" -eq 0 ]; then
    printf 'the call ran no test\n'
    return 1
  fi
  if [ "$1" -ne 0 ]; then
    printf 'the egress check exited %s\n' "$1"
    return 1
  fi
}

# denied_kinds LOG: prints the counts of the denied lines of a log: at 127.0.0.1, name lookups, and
# every other.
denied_kinds() {
  awk '/^denied / {
      n++
      if (index($0, " dest=127.0.0.1:")) l++
      else if (index($0, " proto=dns ")) d++
    }
    END { printf "loopback=%d dns=%d other=%d\n", l, d, n - l - d }' "$1"
}

# prepare: in the repo root, makes the work directory, writes the list, fetches the crates of the
# lock outside the egress check (so that the calls under it run offline) and writes the calls.
prepare() {
  cd "${CI_D_REPO:-.}" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  CI_D_ARTIFACTS=${CI_D_ARTIFACTS:-$work}
  write_list "$work/list.txt" || fail 'the list of the egress check could not be written'
  calls "$entries" >"$work/calls.txt" || fail 'an entry of the row is neither a package nor <package>:<target>'
  [ -s "$work/calls.txt" ] || fail 'the row lists no entry'
  cargo fetch --locked >"$CI_D_ARTIFACTS/fetch.txt" 2>&1 </dev/null ||
    fail "cargo fetch --locked exited $?: read $name/fetch.txt of the artifacts"
}

# run_call LINE FILE: runs cargo with the arguments of LINE under the egress check with the list,
# its output in FILE; sets code to the check's exit status.
run_call() {
  # The line holds words of a-z, 0-9, _, - and : alone, and the shell splits it on purpose.
  # shellcheck disable=SC2086
  sh scripts/egress/check.sh --mode sandbox "$work/list.txt" -- cargo $1 >"$2" 2>&1 </dev/null
  code=$?
}

measure() {
  prepare
  printf 'list entries=%s\n' "$(grep -c '^127\.0\.0\.1:' "$work/list.txt")"
  i=0
  while IFS= read -r line; do
    i=$((i + 1))
    out=$CI_D_ARTIFACTS/call-$i.txt
    run_call "$line" "$out"
    read -r passed failed ignored results <<EOF
$(tests_counts "$out")
EOF
    printf 'call %s: exit=%s passed=%s failed=%s ignored=%s results=%s\n' "$i" "$code" "$passed" "$failed" "$ignored" "$results"
    defect=$(verdict_defect "$code" "$out") && continue
    log=$(grep '^egress mode=' "$out" | sed -n '1s/.* log=\([^ ]*\).*/\1/p')
    case $defect in
      denied=*)
        kinds=-
        if [ -n "$log" ] && [ -f "$log" ]; then
          cp "$log" "$CI_D_ARTIFACTS/denied-$i.log"
          kinds=$(denied_kinds "$log")
        fi
        case $kinds in
          loopback=0* | -) fix='a test reaches beyond the loopback: give it a stand-in on 127.0.0.1:0' ;;
          *) fix="a stand-in binds 127.0.0.1 at a port outside $first_port to $last_port: bind 127.0.0.1:0" ;;
        esac
        fail "call $i: $defect ($kinds); read $name/denied-$i.log of the artifacts; $fix"
        ;;
      *) fail "call $i: $defect; read $name/call-$i.txt of the artifacts" ;;
    esac
  done <"$work/calls.txt"
  printf 'calls=%s\n' "$i"
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
  ok_line='egress mode=sandbox list=list.txt command=0 denied=0 log=target/egress/1/denied.log'
  case_of passed 0 "$ok_line
test result: ok. 60 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.06s
test result: ok. 13 passed; 0 failed; 2 ignored; 0 measured; 5 filtered out; finished in 1.96s" PASS
  case_of cargo-failed 1 "egress mode=sandbox list=list.txt command=101 denied=0 log=target/egress/2/denied.log
test result: FAILED. 12 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.21s" FAIL
  case_of denied 1 "egress mode=sandbox list=list.txt command=0 denied=1 log=target/egress/3/denied.log
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.10s" FAIL
  case_of no-egress-line 1 "test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.10s" FAIL
  case_of no-test 0 "$ok_line
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s" FAIL

  # The form of the entries: the whole packages run in one call, a target entry alone as its
  # package's --test <target>, and a unit entry alone as its package's --lib with --ignored and the
  # test's exact name, behind the others.
  want='test --locked --offline -p halvo-model -p halvo-connect --lib --bins --tests -- --exact --skip s1 --skip s2
test --locked --offline -p halvo-runner --test door_run -- --exact --skip s1 --skip s2
test --locked --offline -p halvo-runner --lib -- --exact --skip s1 --skip s2 --ignored model::tests::one'
  got=$(skips='s1 s2' && calls 'halvo-model halvo-runner:lib:model::tests::one halvo-runner:door_run halvo-connect')
  if [ "$got" = "$want" ]; then
    printf 'case %s/entries want=%s got=%s\n' "$name" ok ok
  else
    printf 'case %s/entries want=%s got=%s\n' "$name" ok other
    wrong="$wrong entries"
  fi
  if calls 'halvo-runner:' >/dev/null; then
    printf 'case %s/bad-entry want=%s got=%s\n' "$name" refused taken
    wrong="$wrong bad-entry"
  else
    printf 'case %s/bad-entry want=%s got=%s\n' "$name" refused refused
    rejected=$((rejected + 1))
  fi
  if calls 'halvo-runner:lib:' >/dev/null; then
    printf 'case %s/bad-unit-entry want=%s got=%s\n' "$name" refused taken
    wrong="$wrong bad-unit-entry"
  else
    printf 'case %s/bad-unit-entry want=%s got=%s\n' "$name" refused refused
    rejected=$((rejected + 1))
  fi

  # The list: 16,384 entries of 127.0.0.1 between the headings, in the order of LC_ALL=C sort.
  write_list "$work/list.txt" || exit 2
  shape=$(awk 'NR == 1 { a = $0 } NR == 2 { b = $0 } { c = d; d = $0 } END { print NR, a, b, c, d }' "$work/list.txt")
  sorted=yes
  sed '1d;$d' "$work/list.txt" | LC_ALL=C sort -c 2>/dev/null || sorted=no
  if [ "$shape $sorted" = '16386 [fixed] 127.0.0.1:49152 127.0.0.1:65535 [agent] yes' ]; then
    printf 'case %s/list want=%s got=%s\n' "$name" ok ok
  else
    printf 'case %s/list want=%s got=%s\n' "$name" ok "other ($shape $sorted)"
    wrong="$wrong list"
  fi

  # The kinds of the denied lines that the fix text reads.
  printf '%s\n' 'denied at=1 proto=tcp dest=127.0.0.1:47201 pid=1 image=door' \
    'denied at=2 proto=dns dest=resolver pid=1 image=door' 'denied at=3 proto=- dest=- pid=1 image=door' >"$work/denied.log"
  kinds=$(denied_kinds "$work/denied.log")
  if [ "$kinds" = 'loopback=1 dns=1 other=1' ]; then
    printf 'case %s/kinds want=%s got=%s\n' "$name" ok ok
  else
    printf 'case %s/kinds want=%s got=%s\n' "$name" ok "other ($kinds)"
    wrong="$wrong kinds"
  fi

  # The red: the control test, by its exact name with --ignored, through the first call of the
  # measure under the egress check. The verdict must read FAIL, with one denied line at the control's
  # address in the log of the check.
  st_work=$work
  prepare
  line=$(sed 1q "$work/calls.txt")
  run_call "$line --ignored $control" "$CI_D_ARTIFACTS/control.txt"
  got=PASS
  defect=$(verdict_defect "$code" "$CI_D_ARTIFACTS/control.txt") || got=FAIL
  log=$(grep '^egress mode=' "$CI_D_ARTIFACTS/control.txt" | sed -n '1s/.* log=\([^ ]*\).*/\1/p')
  at=0
  if [ -n "$log" ] && [ -f "$log" ]; then
    cp "$log" "$CI_D_ARTIFACTS/control-denied.log"
    # The report of a denial can lack the address and keep the port (dest=-:443), as the egress
    # check's own self-test reads it too.
    at=$(awk -v dest="$control_dest" '
      / proto=tcp / && / image=egress_control-/ {
        if (index($0, " dest=" dest " ")) n++
        else if (index($0, " dest=-:" substr(dest, index(dest, ":") + 1) " ")) n++
      }
      END { print n + 0 }' "$log")
  fi
  read -r passed _ ignored _ <<EOF
$(tests_counts "$CI_D_ARTIFACTS/control.txt")
EOF
  printf 'case %s/control want=FAIL got=%s (%s; denied lines at the control address=%s; passed=%s ignored=%s)\n' \
    "$name" "$got" "${defect:-no defect}" "$at" "$passed" "$ignored"
  if [ "$got" = FAIL ] && [ "$at" -ge 1 ] && [ "$passed" -eq 1 ]; then
    rejected=$((rejected + 1))
  else
    wrong="$wrong control"
  fi
  work=$st_work

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
