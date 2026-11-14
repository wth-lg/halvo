#!/bin/sh
stop_signal=''
stop_status=''
stop_row=''
stop_note=''
stop_printed=no

stop_on() {
  [ -z "$stop_signal" ] || return 0
  stop_signal=$1; stop_status=$2
  trap '' TERM INT HUP
}

stop_traps() {
  trap 'stop_on TERM 143' TERM
  trap 'stop_on INT 130' INT
  trap 'stop_on HUP 129' HUP
}

stop_traps
set -u
name=mutation
TAB='	'
stop_wait_s=60
case $0 in */*) DIR=${0%/*} ;; *) DIR=. ;; esac
DIR=$(cd "$DIR" && pwd)
[ -n "$stop_signal" ] || . "$DIR/lib.sh"

usage() {
  cat <<'EOF'
Mutant runner of the R1 gate. It reads every tracked file named mutants.rows of
HEAD and runs its rows in a throwaway copy of HEAD with its own target
directory. The row grammar is in scripts/r1-gate/README.md. It is run on macOS:
on the measuring Mac and in the macos job; its stop is not shown under Git Bash.

  sh scripts/r1-gate/mutation-test.sh [--strict] [--budget SECONDS] [--only NAME]...
      Apply each row, build the mutant and judge it by the row's one verdict
      source. Once the first row on a file that cargo builds is reached, the
      runner builds the unmutated copy and, except under --apply-only,
      requires PASS from the store, deletion and no-ll-hook checks; a run of
      rows on other files requires no PASS from them. Before a test row it
      requires one passing run of the unmutated test. PASS mutation:<name>
      names the verdict source.
      A mutant that does not build is FAIL mutation:<name> (the mutant does not
      build), never a caught mutant.
      --strict also fails on a pending or transitional row, on a row for the
      measuring Mac that did not run, on a row that the budget left unrun, on a
      name of mutants.expected that no row file holds or retires, and on a
      required row, of any place, whose name mutants.expected lacks. With
      --only, --strict reads no state of a row that --only leaves out; the
      name rules still read every row.
      --only runs the named rows in the order given, each name once.
      --budget starts no row once SECONDS have passed since the first row
      started, lets a running row end, prints NOT-RUN mutation:<name> (the
      budget of SECONDS s is used) for each row that it did not start, and
      prints seconds mutation:<name> <n> after each row that it started.
  sh scripts/r1-gate/mutation-test.sh --apply-only
      Apply the substitution of each row that a run would apply here and build
      its mutant. A pending, a retired and a not-here row is listed and not
      applied; the gate checks and the tests do not run.
  sh scripts/r1-gate/mutation-test.sh --list
      Print the rows: name, state, verdict, row file.
  Every mode but --self-test first refuses a tracked path named mutants.rows
  that is a symbolic link, that the copy lacks or whose path holds a tab, a
  newline, a backslash or a quote, and a row name that stands in more than one
  row, unless one of the two is a transitional row of
  scripts/r1-gate/mutants.rows and the other the row that takes it over in the
  rows file beside its file; it names each place.
  --keep-scratch keeps the runner's scratch directory and prints
  scratch mutation: <path>; without it the runner removes its scratch when it
  ends, on every path but a KILL of the runner itself, after which the
  throwaway copy and its build directory stay. --remove-scratch is accepted and
  is the default.
  sh scripts/r1-gate/mutation-test.sh --self-test [-v]
      Prove the classifier, the row rules and the stop on fixtures.

TERM, INT or HUP stops the runner in every mode but --self-test, which keeps
the traps of lib.sh and exits with the signal's status and no STOPPED line.
From the first signal on the runner starts no new step of the run (it still
waits for the running test or shielded step as below, and it removes its
scratch), prints
exactly one STOPPED line, prints no FAIL, PASS, OPEN, NOT-RUN or count line for
a step that the signal ended, and exits with 143, 130 or 129 for that first
signal. From the first signal on the runner ignores TERM, INT and HUP, and so
does every process that it starts after it (its own reads of the running step,
never a new step), so a later signal changes nothing; a step that runs from
before the signal has them ignored as below, but for a program that sets its
own handler, as rustc does for INT.
The line by phase:

  phase                                         STOPPED line
  before the flags are read                     STOPPED mutation (signal S; no row had started)
  the copy of HEAD                              STOPPED mutation (signal S; no row had started)
  the read of the rows files, the name check    STOPPED mutation (signal S; no row had started)
  the print of --list                           STOPPED mutation (signal S; no row had started)
  the --only selection                          STOPPED mutation (signal S; no row had started)
  a cargo build of the copy, ended in the bound STOPPED mutation:<row> (signal S; no test was running)
  a cargo build of the copy, past the bound     STOPPED mutation:<row> (signal S; the running build of the copy did not end within 60 s and was killed)
  a gate check, ended in the bound              STOPPED mutation:<row> (signal S; no test was running)
  a gate check, past the bound                  STOPPED mutation:<row> (signal S; the running check-NAME did not end within 60 s and was killed)
  a short step of a row                         STOPPED mutation:<row> (signal S; no test was running)
  the test                                      STOPPED mutation:<row> (signal S; the running case was let end)
                                                or (signal S; the running case did not end within 60 s and was killed)
  the tail: the expected list, the count line   STOPPED mutation:<last row> (signal S; no test was running)

A cargo build of the copy is the unmutated build or the build of a mutant on a
file that cargo builds; either one gives either form.

A cargo test runs with TERM, INT and HUP ignored, so that its case runs to its
end and its own guard removes what it made; a gate self-test case keeps them at
their defaults and ends its case by its own trap. On the signal the runner
sends the test's process group TERM and waits up to 60 s for the test to end;
then it kills the group, and what the case made may be left; so it is under a
caller that sends KILL sooner. A test runs with its standard input from
/dev/null and QUIT ignored. A gate check and the cargo build of the copy
(unmutated or of a mutant) run with TERM, INT and HUP ignored too, and with
their standard input from /dev/null. TERM and HUP stay ignored for the whole
step, but a compiler may set its own handler for INT: rustc does, so an INT
ends a running rustc and leaves nothing but the failed build in the throwaway
copy. When the runner got the same INT, as it does from its group, it prints
its STOPPED line (signal INT) and exits 130; when it got none, the build reads
as failed (FAIL unmutated:build, or the mutant does not build, never a caught
row). On the signal the runner
starts no next step and waits up to 60 s for that step to end, then stops it
and every process below it by KILL, and the STOPPED line names it; what the
step made may be left then, as under a caller that sends KILL sooner. The
build of a mutant on any other file (sh -n, cc, perl -c, swiftc) is not
shielded. A step that ends a child of its own by TERM, INT or HUP gets no such
end under the shield: a verify child past its deadline ends by the check's
KILL 5 s later.
Limits: a signal that comes after a test or a shielded step has started and
before the runner waits for it is acted on when the step ends by itself, with
its own limit (900 s for a cargo test, 300 s for a gate case, the check's own
deadlines for a gate check, none for a build) as its bound; a signal that
comes after the runner has started the process of a shielded step lets the
step start, because that process reads only the signals recorded before it
started, and the runner waits for it as for a running step. The helper for
timed runs sends KILL 5 s after its TERM, and at once when the runner has
ended, so under it the shield lets a step end only when the step ends within
5 s; a longer one is killed and what it made may be left. A
step whose own exit status says that a signal ended it (129, 130, 137 or 143)
is never read as an answer: with a signal of the runner's own within 2 s the
run ends as a stop, and without one it ends with exit 2 and the line
  FAIL step:<step> (ended by signal S; the runner got no signal)
A runner that was started with a signal ignored, as a shell does for INT in a
background job, cannot trap that signal. After the first signal a signal that
the runner does not catch still ends it before its wait and the removal of its
scratch are done. KILL ends it as below. QUIT ends it at once with 131, no
STOPPED line and its scratch left, when the runner was started with QUIT at
its default; a runner started as a background job of a shell without job
control has QUIT ignored, and QUIT then does not end it. Limit: USR1, USR2,
ALRM, PIPE and every other signal that the runner does not catch end it too,
with 128 plus the signal's number; what its EXIT trap then prints is not
held by a case. A shielded step or test that the signal did not reach runs on.
A KILL of the runner alone leaves its scratch too; a shielded step or test
that was running goes on with TERM, INT and HUP ignored;
a step that the kill at the bound had already stopped stays stopped while its
process group has another member and goes on when the group is orphaned; KILL
to each of its processes ends them.

Exit status: 0 when no line is FAIL (and, with --strict, nothing is open),
1 otherwise, 2 when a step that the runner needs failed (git, the copy of
HEAD, a read of its own files) or ended by a signal that the runner did not
get, 64 on bad usage, and 143, 130 or 129 after a stop by TERM, INT or HUP;
2 never follows a stop.
EOF
}

say() { printf '%s mutation:%s (%s)\n' "$1" "$2" "$3"; }

stop_now() {
  stop_printed=yes
  if [ -n "$1" ]; then say STOPPED "$1" "signal $stop_signal; $2"
  else printf 'STOPPED mutation (signal %s; no row had started)\n' "$stop_signal"; fi
  exit "$stop_status"
}

stop_check() {
  [ -z "$stop_signal" ] || stop_now "$stop_row" "${stop_note:-no test was running}"
}

step_check() {
  step_check_status=$1; step_check_name=$2
  stop_check
  case $step_check_status in 129|130|137|143) ;; *) return "$step_check_status" ;; esac
  step_check_waits=0
  while [ "$step_check_waits" -lt 20 ]; do
    sleep 0.1
    stop_check
    step_check_waits=$((step_check_waits + 1))
  done
  case $step_check_status in 129) step_check_sig=HUP ;; 130) step_check_sig=INT ;; 137) step_check_sig=KILL ;; *) step_check_sig=TERM ;; esac
  printf 'FAIL step:%s (ended by signal %s; the runner got no signal)\n' "$step_check_name" "$step_check_sig"
  exit 2
}

runner_exit() {
  runner_exit_status=$?
  gate_exit
  if [ -n "$stop_signal" ] && [ "$stop_printed" = no ]; then
    stop_printed=yes
    if [ -n "$stop_row" ]; then say STOPPED "$stop_row" "signal $stop_signal; no test was running"
    else printf 'STOPPED mutation (signal %s; no row had started)\n' "$stop_signal"; fi
    exit "$stop_status"
  fi
  exit "$runner_exit_status"
}

rows_table() {
  rows_repo=$1; rows_commit=$2; rows_tree=$3; rows_out=$4
  stop_check
  git -C "$rows_repo" -c core.quotePath=false ls-tree -r --full-tree "$rows_commit" > "$rows_out.tracked" 2> "$rows_out.err"
  step_check $? "git ls-tree" || return 2
  : > "$rows_out"
  rows_refused=0
  while IFS= read -r rows_entry; do
    rows_path=${rows_entry#*"$TAB"}
    case $rows_path in
      mutants.rows|*/mutants.rows) ;;
      \"mutants.rows\"|\"*/mutants.rows\")
        say FAIL rows "the tracked path $rows_path holds a tab, a newline, a backslash or a quote"; rows_refused=$((rows_refused + 1)); continue ;;
      *) continue ;;
    esac
    case $rows_entry in
      120000\ *) say FAIL rows "the tracked path $rows_path is a symbolic link"; rows_refused=$((rows_refused + 1)); continue ;;
    esac
    if [ ! -f "$rows_tree/$rows_path" ] || [ -h "$rows_tree/$rows_path" ]; then
      say FAIL rows "the tracked path $rows_path is not a file of the copy of HEAD"; rows_refused=$((rows_refused + 1)); continue
    fi
    stop_check
    awk -F '\t' -v file="$rows_path" 'BEGIN { OFS = "\t" }
      /^[[:space:]]*$/ { next }
      { empty = 0; for (i = 1; i <= NF; i++) if ($i == "") empty = 1
        if (NF != 6 || empty) print file, NR, "malformed", "-", "-", "-", "-", "-"
        else print file, NR, $1, $2, $3, $4, $5, $6 }' "$rows_tree/$rows_path" >> "$rows_out" 2>> "$rows_out.err"
    step_check $? "awk of $rows_path" || return 2
  done < "$rows_out.tracked"
  [ "$rows_refused" -eq 0 ] || return 3
}

row_place() {
  case $row_state in
    transitional|control) mutated=$row_file ;;
    *) row_place_dir=${rows_file%/*}
       [ "$row_place_dir" != "$rows_file" ] || row_place_dir=.
       mutated=$row_place_dir/$row_file ;;
  esac
}

row_defect() {
  defect=''
  if [ "$row_name" = malformed ] && [ "$row_state" = - ]; then
    defect="line $row_line of $rows_file has no six fields separated by tabs: name, file, substitution, verdict, state, item"; return 1
  fi
  case $row_file in /*|../*|*/../*|*/..)
    defect="the file $row_file is not under the directory of $rows_file"; return 1 ;;
  esac
  case $row_verdict in
    'measured '?*|'refused '?*|leak|'test '?*) ;;
    *) defect="the verdict $row_verdict is none of: measured KEY..., refused TEXT, leak, test NAME"; return 1 ;;
  esac
  if [ "$row_state" = transitional ] && [ "$rows_file" != scripts/r1-gate/mutants.rows ]; then
    defect="a transitional row stands in scripts/r1-gate/mutants.rows, not in $rows_file"; return 1
  fi
  case $row_state in
    required|'required on measuring-mac'|'required on hosted-runner'|'required on test-bed'|transitional|control|'pending '?*|'retired '?*) ;;
    *) defect="the state $row_state is none of: required, required on PLACE, transitional, control, pending ITEM, retired REASON"; return 1 ;;
  esac
  return 0
}

names_twice() {
  stop_check
  awk -F '\t' -v gate=scripts/r1-gate/mutants.rows '
    $3 == "malformed" && $7 == "-" { next }
    { n[$3]++; files[$3] = files[$3] (files[$3] == "" ? "" : " and in ") $1 " line " $2
      if ($7 == "transitional" && $1 == gate) { beside = $4; sub(/\/[^\/]*$/, "", beside); takeover[$3] = beside "/mutants.rows" } else holder[$3] = $1 }
    END { for (k in n) if (n[k] > 1 && !(n[k] == 2 && (k in takeover) && holder[k] == takeover[k] && takeover[k] != gate)) print k "\t" files[k] }' "$table" > "$work/twice.unsorted"
  step_check $? "awk of the name check" || return 2
  stop_check
  LC_ALL=C sort "$work/twice.unsorted" > "$work/twice.tsv"
  step_check $? "sort of the name check" || return 2
  [ -s "$work/twice.tsv" ] || return 0
  while IFS="$TAB" read -r twice_name twice_files; do
    say FAIL "$twice_name" "the name stands in $twice_files"
  done < "$work/twice.tsv"
  return 3
}

needs_cargo() {
  case $1 in crates/*|src-tauri/*|Cargo.toml|Cargo.lock) return 0 ;; esac
  return 1
}

tree_build() {
  stop_check
  # shellcheck disable=SC2016
  shield_run "build of the copy" env GATE_ROOT="$tree" sh -c '. "$1/scripts/r1-gate/lib.sh"; gate_build || { printf "%s\n" "$gate_build_error"; exit 1; }' sh "$tree" > "$1" 2>&1
  step_check $? "build of the copy"
}

mutant_builds() {
  if needs_cargo "$1"; then tree_build "$work/build.out"; return; fi
  stop_check
  case $1 in
    *.sh) sh -n "$tree/$1" > "$work/build.out" 2>&1 ;;
    *.c) cc -fsyntax-only "$tree/$1" > "$work/build.out" 2>&1 ;;
    *.pl) perl -c "$tree/$1" > "$work/build.out" 2>&1 ;;
    *.swift) swiftc -parse "$tree/$1" > "$work/build.out" 2>&1 ;;
    *) return 1 ;;
  esac
  step_check $? "build of the mutant"
}

buildable() {
  needs_cargo "$1" && return 0
  case $1 in *.sh|*.c|*.pl|*.swift) return 0 ;; esac
  return 1
}

gate_lines() {
  : > "$work/gate.out"
  stop_check
  { git init -q "$tree" && git -C "$tree" add -A; } > "$work/gate.err" 2>&1
  step_check $? "git add of the copy" || { printf 'FAIL copy (git could not stage the copy)\n' > "$work/gate.out"; return 1; }
  for gate_check in store deletion no-ll-hook; do
    stop_check
    shield_run "check-$gate_check" env GATE_ROOT="$tree" sh "$tree/scripts/r1-gate/check-$gate_check.sh" --measure >> "$work/gate.out" 2> "$work/gate.err"
    step_check $? "check-$gate_check"
  done
  return 0
}

stop_past_bound() {
  [ "$1" -ge "$2" ]
}

test_ended() {
  case $(ps -o stat= -p "$1" 2> /dev/null) in ''|Z*) return 0 ;; esac
  return 1
}

shield_run() {
  shield_name=$1; shift
  ( trap '' TERM INT HUP; [ -z "$stop_signal" ] || exit 0; exec "$@" ) &
  shield_pid=$!
  wait "$shield_pid"
  shield_status=$?
  [ -n "$stop_signal" ] || return "$shield_status"
  shield_deadline=$(date +%s)
  shield_deadline=$((shield_deadline + stop_wait_s))
  until test_ended "$shield_pid"; do
    shield_clock=$(date +%s)
    if stop_past_bound "$shield_clock" "$shield_deadline"; then
      shield_kill "$shield_pid"
      stop_note="the running $shield_name did not end within $stop_wait_s s and was killed"
      break
    fi
    sleep 0.1
  done
  wait "$shield_pid"
}

shield_kill() {
  kill -s STOP "$1" 2> /dev/null
  shield_held=" $1 "
  shield_more=yes
  while [ "$shield_more" = yes ]; do
    shield_more=no
    for shield_child in $(descendants_of "$1"); do
      case $shield_held in *" $shield_child "*) continue ;; esac
      kill -s STOP "$shield_child" 2> /dev/null
      shield_held="$shield_held$shield_child "
      shield_more=yes
    done
  done
  for shield_child in $shield_held; do kill -s KILL "$shield_child" 2> /dev/null; done
}

test_wait() {
  test_kind=$1; test_limit=$2; shift 2
  stop_check
  case $test_kind in
    cargo) ( cd "$tree" && trap '' TERM INT HUP && exec perl "$GATE_LIMIT" "$test_limit" "$@" ) > "$work/test.out" 2>&1 & ;;
    *) ( trap '' TERM INT HUP && exec perl "$GATE_LIMIT" "$test_limit" perl -e '$SIG{$_} = "DEFAULT" for qw(TERM INT HUP); exec { $ARGV[0] } @ARGV or exit 127' "$@" ) > "$work/test.out" 2>&1 & ;;
  esac
  test_pid=$!
  wait "$test_pid"
  test_status=$?
  if [ -z "$stop_signal" ]; then
    step_check "$test_status" test
    return 0
  fi
  test_group=''
  stop_deadline=$(date +%s)
  stop_deadline=$((stop_deadline + stop_wait_s))
  until test_ended "$test_pid"; do
    if [ -z "$test_group" ]; then
      test_group=$(pgrep -P "$test_pid")
      [ -z "$test_group" ] || kill -s TERM -- "-$test_group" 2> /dev/null
    fi
    stop_clock=$(date +%s)
    if stop_past_bound "$stop_clock" "$stop_deadline"; then
      [ -z "$test_group" ] || kill -s KILL -- "-$test_group" 2> /dev/null
      kill -s KILL "$test_pid" 2> /dev/null
      wait "$test_pid"
      stop_now "$row_name" "the running case did not end within $stop_wait_s s and was killed"
    fi
    sleep 0.1
  done
  wait "$test_pid"
  stop_now "$row_name" "the running case was let end"
}

test_run() {
  test_name=$1
  case $test_name in
    selftest:*/*)
      test_check=${test_name#selftest:}; test_case=${test_check#*/}; test_check=${test_check%%/*}
      test_wait case 300 env GATE_ROOT="$tree" sh "$tree/scripts/r1-gate/check-$test_check.sh" --self-test -v --remove-scratch --case "$test_case"
      [ "$test_status" -ne 124 ] || { outcome=hang; return 0; }
      stop_check
      outcome=$(awk -v want="case $test_check/$test_case" '
        index($0, want " want=") == 1 { n++; split($0, part, " "); verdict = (part[3] == "want=" substr(part[4], 5)) ? "passed" : "failed" }
        END { if (n == 1) print verdict; else print "ran " n + 0 " cases" }' "$work/test.out")
      step_check $? "read of the test's output" ;;
    *)
      test_package=${rows_file#crates/}; test_package=${test_package%%/*}
      case $rows_file in crates/*) ;; *) test_package=halvo ;; esac
      test_wait cargo 900 env CARGO_TARGET_DIR="$tree/target/r1-gate" cargo test -q --locked -p "$test_package" --lib -- --exact "$test_name"
      [ "$test_status" -ne 124 ] || { outcome=hang; return 0; }
      stop_check
      outcome=$(cargo_test_outcome "$work/test.out")
      step_check $? "read of the test's output" ;;
  esac
}

row_seconds() {
  [ -n "$timed_name" ] || return 0
  printf 'seconds mutation:%s %s\n' "$timed_name" "$((row_now - timed_start))"
  timed_name=''
}

row_clock() {
  stop_check
  row_now=$(date +%s)
  step_check $? date
}

run_rows() {
  tree=$1; run_repo=$2; run_commit=$3
  failures=0; opens=0; transitionals=0; unrun=0; budget_unrun=0; caught=0
  table=$work/rows.tsv
  rows_table "$run_repo" "$run_commit" "$tree" "$table"
  case $? in 0) ;; 3) return 1 ;; *) say FAIL rows "the row files of the copy could not be read"; return 2 ;; esac
  if [ ! -s "$table" ]; then say FAIL rows "no mutants.rows file holds a row"; return 1; fi
  names_twice
  case $? in 0) ;; 3) return 1 ;; *) return 2 ;; esac
  if [ "$list" = yes ]; then
    stop_check
    awk -F '\t' '{ printf "mutation:%s\t%s\t%s\t%s\n", $3, $7, $6, $1 }' "$table" > "$work/list.out"
    step_check $? "awk of the list" || return 2
    while IFS= read -r list_line; do
      stop_check
      printf '%s\n' "$list_line"
    done < "$work/list.out"
    stop_check
    return 0
  fi
  for only_name in $only; do
    stop_check
    only_hit=$(awk -F '\t' -v n="$only_name" '$3 == n ""' "$table")
    step_check $? "awk of --only" || return 2
    [ -n "$only_hit" ] || { say FAIL "$only_name" "--only names no row"; failures=$((failures + 1)); }
  done
  built_unmutated=no
  if [ -n "$only" ]; then
    : > "$work/rows.loop"
    only_seen=' '
    for only_name in $only; do
      case $only_seen in *" $only_name "*) continue ;; esac
      only_seen="$only_seen$only_name "
      stop_check
      awk -F '\t' -v n="$only_name" '$3 == n ""' "$table" >> "$work/rows.loop"
      step_check $? "awk of --only" || return 2
    done
    stop_check
    awk -F '\t' -v only=" $only " 'index(only, " " $3 " ") == 0' "$table" >> "$work/rows.loop"
    step_check $? "awk of --only" || return 2
  else
    stop_check
    cp "$table" "$work/rows.loop"
    step_check $? "copy of the rows table" || return 2
  fi
  budget_start=''; timed_name=''
  while IFS="$TAB" read -r rows_file row_line row_name row_file row_subst row_verdict row_state _ <&3; do
    stop_check
    stop_row=$row_name
    if ! row_defect; then
      say FAIL "$row_name" "$defect"; failures=$((failures + 1)); continue
    fi
    case " $only " in "  "|*" $row_name "*) ;; *) continue ;; esac
    if [ -n "$budget" ]; then
      row_clock
      row_seconds
      if [ -z "$budget_start" ]; then
        budget_start=$row_now
      elif [ "$((row_now - budget_start))" -ge "$budget" ]; then
        say NOT-RUN "$row_name" "the budget of $budget s is used"; budget_unrun=$((budget_unrun + 1))
        [ "$strict" = no ] || say FAIL "$row_name" "strict: the budget left the row unrun"
        continue
      fi
      timed_name=$row_name; timed_start=$row_now
    fi
    case $row_state in
      'retired '*) say RETIRED "$row_name" "${row_state#retired }"; continue ;;
      'pending '*)
        say OPEN "$row_name" "${row_state#pending }"; opens=$((opens + 1))
        [ "$strict" = no ] || say FAIL "$row_name" "strict: the row is pending"
        continue ;;
      'required on measuring-mac')
        if [ "${CI:-}" = true ]; then
          say NOT-RUN "$row_name" "runs on the measuring Mac"; unrun=$((unrun + 1))
          [ "$strict" = no ] || say FAIL "$row_name" "strict: a row for the measuring Mac did not run"
          continue
        fi ;;
      'required on hosted-runner')
        if [ "${CI:-}" != true ]; then say NOT-RUN "$row_name" "needs input; runs on the hosted runner"; continue; fi ;;
      'required on test-bed')
        if [ "${GATE_PLACE:-}" != test-bed ]; then say NOT-RUN "$row_name" "needs input; runs on the test bed"; continue; fi ;;
      transitional)
        takeover=${row_file%/*}/mutants.rows
        if [ -f "$tree/$takeover" ]; then
          stop_check
          taken=$(awk -F '\t' -v n="$row_name" -v f="$takeover" '$1 == f "" && $3 == n ""' "$table")
          step_check $? "awk of the takeover" || return 2
          if [ -n "$taken" ]; then say MOVED "$row_name" "$takeover"; continue; fi
          say FAIL "$row_name" "transitional row not taken over by $takeover"; failures=$((failures + 1)); continue
        fi
        transitionals=$((transitionals + 1))
        [ "$strict" = no ] || say FAIL "$row_name" "strict: a transitional row, and no $takeover holds it yet" ;;
    esac
    row_place
    if [ ! -f "$tree/$mutated" ]; then say FAIL "$row_name" "the file $mutated is not in the copy"; failures=$((failures + 1)); continue; fi
    if ! buildable "$mutated"; then say FAIL "$row_name" "the runner has no build for $mutated: a row mutates a file that cargo builds, or a .sh, .c, .pl or .swift file"; failures=$((failures + 1)); continue; fi
    if needs_cargo "$mutated" && [ "$built_unmutated" = no ]; then
      if ! tree_build "$work/unmutated.out"; then
        unmutated_first=''
        IFS= read -r unmutated_first < "$work/unmutated.out"
        printf 'FAIL unmutated:build (%s)\n' "$unmutated_first"; return 1
      fi
      built_unmutated=yes
      if [ "$apply_only" = no ]; then
        gate_lines
        for gate_check in store deletion no-ll-hook; do
          stop_check
          gate_line=$(grep -E "^[A-Z-]+ $gate_check( |\$)" "$work/gate.out")
          step_check $? "read of the gate lines"
          case $gate_line in
            "PASS $gate_check"*) printf 'PASS unmutated:%s\n' "$gate_check" ;;
            *) printf 'FAIL unmutated:%s (gate said: %s)\n' "$gate_check" "${gate_line:-nothing}"; failures=$((failures + 1)) ;;
          esac
        done
      fi
    fi
    if [ "$apply_only" = no ] && [ "$row_state" != control ]; then
      case $row_verdict in 'test '*)
        test_run "${row_verdict#test }"
        if [ "$outcome" != passed ]; then
          say FAIL "$row_name" "unmutated, the test ${row_verdict#test } must run as one passing test: $outcome"; failures=$((failures + 1)); continue
        fi ;;
      esac
    fi
    stop_check
    cp "$tree/$mutated" "$work/pristine"
    step_check $? "copy of the file to mutate" || return 2
    stop_check
    applied=$(ROW_SUBST=$row_subst perl "$DIR/helper/apply-row.pl" "$tree/$mutated" 2> "$work/perl.err")
    if ! step_check $? apply-row.pl; then
      stop_check
      cp "$work/pristine" "$tree/$mutated"
      step_check $? "restore of the mutated file" || return 2
      say FAIL "$row_name" "${applied:-the substitution could not be applied}"; failures=$((failures + 1)); continue
    fi
    stop_check
    cmp -s "$work/pristine" "$tree/$mutated"
    step_check $? cmp
    row_cmp=$?
    if [ "$row_cmp" -eq 0 ]; then
      say FAIL "$row_name" "the substitution did not apply"; failures=$((failures + 1)); continue
    fi
    if mutant_builds "$mutated"; then builds=yes; else builds=no; fi
    stop_check
    if [ "$row_state" = control ]; then
      if [ "$builds" = no ]; then say PASS "$row_name" "control: the mutant does not build, and the runner did not count it as caught"
      else say FAIL "$row_name" "control: the mutant built, so the build step proves nothing"; failures=$((failures + 1)); fi
    elif [ "$builds" = no ]; then
      say FAIL "$row_name" "the mutant does not build"; failures=$((failures + 1))
    elif [ "$apply_only" = yes ]; then
      say PASS "$row_name" "applies and builds"
    else
      case $row_verdict in
        'test '*)
          test_run "${row_verdict#test }"
          if [ "$outcome" = failed ]; then say PASS "$row_name" "$row_verdict: the one test failed"; caught=$((caught + 1))
          else say FAIL "$row_name" "$row_verdict: $outcome"; failures=$((failures + 1)); fi ;;
        *)
          gate_lines
          stop_check
          gate_line=$(mutant_verdict "$row_verdict" "$work/gate.out")
          if step_check $? "read of the gate lines"; then say PASS "$row_name" "$row_verdict: gate said: $gate_line"; caught=$((caught + 1))
          else
            stop_check
            gate_line=$(tr '\n' ' ' < "$work/gate.out")
            step_check $? "read of the gate lines"
            say FAIL "$row_name" "$row_verdict not shown; gate said: $gate_line"; failures=$((failures + 1))
          fi ;;
      esac
    fi
    stop_check
    cp "$work/pristine" "$tree/$mutated"
    step_check $? "restore of the mutated file" || return 2
  done 3< "$work/rows.loop"
  stop_check
  if [ -n "$budget" ]; then
    row_clock
    row_seconds
  fi
  missing=0; unnamed=0; expected_list=$tree/scripts/r1-gate/mutants.expected
  if [ ! -f "$expected_list" ]; then
    expected_list=/dev/null
    [ "$strict" = no ] || { say FAIL expected "scripts/r1-gate/mutants.expected is missing, and --strict needs it"; failures=$((failures + 1)); }
  fi
  stop_check
  awk -F '\t' 'FILENAME == ARGV[1] { if ($0 != "") listed[++names] = $0; next }
    $3 == "malformed" && $7 == "-" { next }
    { have[$3] = 1; if ($7 ~ /^required/) required[++rows] = $3 "\t" $1 }
    END {
      for (i = 1; i <= names; i++) { named[listed[i]] = 1; if (!(listed[i] in have)) print "missing\t" listed[i] }
      for (j = 1; j <= rows; j++) { split(required[j], part, "\t"); if (!(part[1] in named)) print "unnamed\t" required[j] }
    }' "$expected_list" "$table" > "$work/tail.tsv"
  step_check $? "read of mutants.expected" || return 2
  while IFS="$TAB" read -r tail_kind tail_name tail_file; do
    stop_check
    case $tail_kind in
      missing)
        missing=$((missing + 1))
        [ "$strict" = no ] || say FAIL "$tail_name" "named in mutants.expected, and no row file holds or retires it" ;;
      *)
        unnamed=$((unnamed + 1))
        [ "$strict" = no ] || [ "$expected_list" = /dev/null ] || say FAIL "$tail_name" "strict: a required row of $tail_file, and mutants.expected does not name it" ;;
    esac
  done < "$work/tail.tsv"
  stop_check
  printf 'mutation rows: %s caught, %s failed, %s open, %s transitional, %s for the measuring Mac not run, %s expected names with no row, %s required rows with no expected name\n' "$caught" "$failures" "$opens" "$transitionals" "$unrun" "$missing" "$unnamed"
  [ "$failures" -eq 0 ] || return 1
  [ "$strict" = no ] && return 0
  [ "$((opens + transitionals + unrun + budget_unrun + missing + unnamed))" -eq 0 ]
}

fixture_tree() {
  made=$gate_scratch/$1
  mkdir -p "$made/scripts/r1-gate" "$made/code"
  printf 'echo one\n' > "$made/code/a.sh"
  printf 'echo two\n' > "$made/scripts/r1-gate/b.sh"
  : > "$made/scripts/r1-gate/mutants.rows"
  : > "$made/scripts/r1-gate/mutants.expected"
}

row() {
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$2" "$3" "$4" "$5" "$6" "$7" >> "$gate_scratch/$1"
}

fixture_commit() {
  ( cd "$1" && git init -q . && git add -A -- . ':(exclude)target' && fixture_commit_tree=$(git write-tree) &&
    fixture_commit_id=$(printf 'tree %s\nauthor fixture <fixture@example.invalid> 1 +0000\ncommitter fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$fixture_commit_tree" | git hash-object -t commit -w --stdin) &&
    git update-ref HEAD "$fixture_commit_id" ) > /dev/null 2>&1
}

fixture_run() {
  list=${fixture_list:-no}; apply_only=${2:-no}; strict=${3:-no}; only=${fixture_only:-}; budget=${fixture_budget:-}
  work=$gate_scratch/work-$1
  mkdir -p "$work"
  fixture_commit "$gate_scratch/$1"
  run_rows "$gate_scratch/$1" "$gate_scratch/$1" HEAD
}

fixture_says() {
  out=$(fixture_run "$1" "${3:-no}" "${4:-no}")
  case $out in *"$2"*) return 0 ;; esac
  return 1
}

outcome_is() {
  outcome=$(cargo_test_outcome "$gate_scratch/$2")
  [ "$outcome" = "$1" ]
}

stub_gate() {
  for stub in deletion no-ll-hook; do
    printf '#!/bin/sh\nprintf "PASS %s\\n"\n' "$stub" > "$gate_scratch/$1/scripts/r1-gate/check-$stub.sh"
  done
  printf '%s\n' "$2" > "$gate_scratch/$1/scripts/r1-gate/store.mutated"
  cat > "$gate_scratch/$1/scripts/r1-gate/check-store.sh" <<'STUB'
#!/bin/sh
here=${0%/*}
if grep -q three "$here/b.sh"; then cat "$here/store.mutated"; exit 1; fi
printf 'PASS store\n'
STUB
}

stub_test() {
  printf '%s\n' "$2" > "$gate_scratch/$1/scripts/r1-gate/demo.mode"
  cat > "$gate_scratch/$1/scripts/r1-gate/check-demo.sh" <<'STUB'
#!/bin/sh
here=${0%/*}
read -r mode < "$here/demo.mode"
printf '%s\n' "$*" > "$here/demo.args"
got=FAIL
if grep -q three "$here/b.sh"; then got=PASS; fi
case $mode in
  silent) exit 0 ;;
  silent-when-mutated) [ "$got" = FAIL ] || exit 0 ;;
  wrong-unmutated) got=PASS ;;
  blind) got=FAIL ;;
  slow) sleep 1 ;;
esac
printf 'case demo/c1 want=FAIL got=%s\n' "$got"
STUB
}

with_only() {
  fixture_only=" $1"; shift
  "$@"
}

stub_cargo() {
  mkdir -p "$gate_scratch/$1/bin"
  printf '%s\n' "$2" > "$gate_scratch/$1/bin/hold"
  cat > "$gate_scratch/$1/bin/cargo" <<'STUB'
#!/bin/sh
here=${0%/*}
read -r hold < "$here/hold"
printf 'cargo\n' >> "$here/starts"
case $hold in
  0) ;;
  go)
    : > "$here/marker"
    polls=0
    until [ -e "$here/go" ]; do
      polls=$((polls + 1))
      [ "$polls" -lt 300 ] || { printf 'timeout\n' > "$here/ended"; exit 1; }
      sleep 0.1
    done
    mv "$here/marker" "$here/marker.released" ;;
  *)
    : > "$here/marker"
    sleep "$hold"
    mv "$here/marker" "$here/marker.released" ;;
esac
printf 'itself\n' > "$here/ended"
printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
STUB
  chmod +x "$gate_scratch/$1/bin/cargo"
}

stub_cc() {
  mkdir -p "$gate_scratch/$1/bin"
  printf 'int x = 1;\n' > "$gate_scratch/$1/scripts/r1-gate/x.c"
  cat > "$gate_scratch/$1/bin/cc" <<'STUB'
#!/bin/sh
here=${0%/*}
printf 'cc\n' >> "$here/starts"
: > "$here/marker"
sleep 2
mv "$here/marker" "$here/marker.released"
printf 'itself\n' > "$here/ended"
STUB
  chmod +x "$gate_scratch/$1/bin/cc"
}

stub_held_case() {
  mkdir -p "$gate_scratch/$1/bin"
  cat > "$gate_scratch/$1/scripts/r1-gate/check-hold.sh" <<'STUB'
#!/bin/sh
here=${0%/*}/../../bin
trap 'kill "$sleeper"; mv "$here/marker" "$here/marker.released"; printf "TERM\n" > "$here/ended"; exit 143' TERM
printf 'case\n' >> "$here/starts"
sleep 30 &
sleeper=$!
: > "$here/marker"
wait
mv "$here/marker" "$here/marker.released"
printf 'itself\n' > "$here/ended"
printf 'case hold/c1 want=FAIL got=FAIL\n'
STUB
}

descendants_of() {
  ps -axo pid=,ppid= | awk -v root="$1" '
    { parent[$1] = $2 }
    END { mark[root] = 1; grew = 1
      while (grew) { grew = 0; for (p in parent) if (!(p in mark) && (parent[p] in mark)) { mark[p] = 1; grew = 1 } }
      for (p in mark) if (p != root) print p }'
}

witness_start() {
  perl -e 'setpgrp(0, 0); $SIG{$_} = "DEFAULT" for qw(INT TERM HUP QUIT); sleep 300' &
  witness_pid=$!
}

witness_end() {
  witness_alive=no
  if kill -0 "$witness_pid" 2> /dev/null; then witness_alive=yes; fi
  kill -s KILL "$witness_pid" 2> /dev/null
  wait "$witness_pid" 2> /dev/null
}

stopped_by() {
  stop_fixture=$gate_scratch/$1; stop_signal_sent=$2; stop_whom=$3; stop_bound=$4; stop_mode=$5
  stop_status_wanted=$6; stop_marker_wanted=$7; stop_ended_wanted=$8; stop_starts_wanted=$9; shift 9; stop_line_wanted=$1
  fixture_commit "$stop_fixture"
  perl -e 'setpgrp(0, 0); $SIG{$_} = "DEFAULT" for qw(INT TERM HUP QUIT); exec { $ARGV[0] } @ARGV or exit 127' sh -c 'stop_arg_fixture=$1; stop_arg_bound=$2; stop_arg_mode=$3; set -- --help; . "$0" > /dev/null 2>&1
    PATH=$stop_arg_fixture/bin:$PATH; stop_wait_s=$stop_arg_bound; list=no; apply_only=no; strict=no; only=; budget=
    [ "$stop_arg_mode" = run ] || apply_only=yes
    work=$stop_arg_fixture.work; mkdir -p "$work"; stop_traps; run_rows "$stop_arg_fixture" "$stop_arg_fixture" HEAD' "$DIR/mutation-test.sh" "$stop_fixture" "$stop_bound" "$stop_mode" > "$stop_fixture.out" 2>&1 &
  stop_pid=$!
  stop_polls=0
  until [ -f "$stop_fixture/bin/marker" ]; do
    stop_polls=$((stop_polls + 1))
    [ "$stop_polls" -lt 300 ] || { kill -s KILL -- "-$stop_pid"; printf 'the stand-in did not start within %s polls of 0.1 s\n' "$stop_polls"; return 1; }
    sleep 0.1
  done
  witness_start
  stop_held_at_end=no
  case $stop_whom in
    tree) for stop_child in $(descendants_of "$stop_pid"); do kill -s "$stop_signal_sent" "$stop_child" 2> /dev/null; done
      kill -s "$stop_signal_sent" "$stop_pid" ;;
    helper|helper-again) kill -s "$stop_signal_sent" -- "-$stop_pid"
      stop_polls=0
      while kill -0 "$stop_pid" 2> /dev/null && [ "$stop_polls" -lt 300 ]; do
        polls_of_the_step "$stop_fixture"
        [ "$step_polls" -lt 3 ] || break
        stop_polls=$((stop_polls + 1))
        sleep 0.1
      done
      if [ "$stop_whom" = helper-again ]; then
        stop_again=0
        while kill -0 "$stop_pid" 2> /dev/null && [ "$stop_again" -lt 10 ]; do
          kill -s "$stop_signal_sent" -- "-$stop_pid"
          stop_again=$((stop_again + 1))
          sleep 0.1
        done
      fi
      if ! kill -0 "$stop_pid" 2> /dev/null && [ -e "$stop_fixture/bin/marker" ]; then stop_held_at_end=yes; fi
      : > "$stop_fixture/bin/go"
      stop_polls=0
      while kill -0 "$stop_pid" 2> /dev/null && [ "$stop_polls" -lt "$((helper_kill_after_s * 10))" ]; do
        stop_polls=$((stop_polls + 1))
        sleep 0.1
      done
      [ ! -e "$stop_fixture/bin/marker" ] || stop_held_at_end=yes
      stop_rest=$(pgrep -g "$stop_pid")
      [ -z "$stop_rest" ] || kill -s KILL -- "-$stop_pid" ;;
    helper-then-int|helper-then-hup)
      case $stop_pid in ''|*[!0-9]*|0|1) witness_end; printf 'the case holds no id of its runner\n'; return 1 ;; esac
      stop_then=INT
      [ "$stop_whom" = helper-then-int ] || stop_then=HUP
      kill -s "$stop_signal_sent" -- "-$stop_pid"
      stop_polls=0
      while kill -0 "$stop_pid" 2> /dev/null && [ "$stop_polls" -lt 300 ]; do
        polls_of_the_step "$stop_fixture"
        [ "$step_polls" -lt 3 ] || break
        stop_polls=$((stop_polls + 1))
        sleep 0.1
      done
      stop_again=0
      while kill -0 "$stop_pid" 2> /dev/null && [ "$stop_again" -lt 10 ]; do
        kill -s "$stop_then" -- "-$stop_pid"
        stop_again=$((stop_again + 1))
        sleep 0.1
      done
      if ! kill -0 "$stop_pid" 2> /dev/null && [ -e "$stop_fixture/bin/marker" ]; then stop_held_at_end=yes; fi
      : > "$stop_fixture/bin/go"
      stop_polls=0
      while kill -0 "$stop_pid" 2> /dev/null && [ "$stop_polls" -lt "$((helper_kill_after_s * 10))" ]; do
        stop_polls=$((stop_polls + 1))
        sleep 0.1
      done
      [ ! -e "$stop_fixture/bin/marker" ] || stop_held_at_end=yes
      stop_rest=$(pgrep -g "$stop_pid")
      [ -z "$stop_rest" ] || kill -s KILL -- "-$stop_pid" ;;
    group) kill -s "$stop_signal_sent" -- "-$stop_pid" ;;
    twice) kill -s "$stop_signal_sent" "$stop_pid"; sleep 0.5; kill -s TERM "$stop_pid" ;;
    *) kill -s "$stop_signal_sent" "$stop_pid" ;;
  esac
  stop_polls=0
  while kill -0 "$stop_pid" 2> /dev/null; do
    stop_polls=$((stop_polls + 1))
    [ "$stop_polls" -lt 400 ] || { kill -s KILL -- "-$stop_pid"; witness_end; printf 'the runner did not end within %s polls of 0.1 s after the signal\n' "$stop_polls"; return 1; }
    sleep 0.1
  done
  wait "$stop_pid"
  stopped_status=$?
  sleep 0.2
  stop_starts=0
  [ ! -f "$stop_fixture/bin/starts" ] || stop_starts=$(wc -l < "$stop_fixture/bin/starts" | tr -d ' ')
  stop_left=$(pgrep -f "$stop_fixture/" | wc -l | tr -d ' ')
  witness_end
  stop_marker=gone
  [ ! -e "$stop_fixture/bin/marker" ] || stop_marker=left
  stop_ended=none
  [ ! -f "$stop_fixture/bin/ended" ] || read -r stop_ended < "$stop_fixture/bin/ended"
  stop_defects=''
  grep -F -x -q -e "$stop_line_wanted" "$stop_fixture.out" || stop_defects="$stop_defects; the runner printed no line $stop_line_wanted"
  [ "$stopped_status" -eq "$stop_status_wanted" ] || stop_defects="$stop_defects; the runner exited $stopped_status, not $stop_status_wanted"
  ! grep -q '^mutation rows: ' "$stop_fixture.out" || stop_defects="$stop_defects; the runner printed its count line"
  stopped_lines=$(grep -c '^STOPPED ' "$stop_fixture.out")
  [ "$stopped_lines" -le 1 ] || stop_defects="$stop_defects; the runner printed $stopped_lines STOPPED lines"
  [ "$stop_marker" = "$stop_marker_wanted" ] || stop_defects="$stop_defects; the marker of the stand-in is $stop_marker, not $stop_marker_wanted"
  [ "$stop_ended" = "$stop_ended_wanted" ] || stop_defects="$stop_defects; the stand-in ended by $stop_ended, not by $stop_ended_wanted"
  [ "$stop_starts" -eq "$stop_starts_wanted" ] || stop_defects="$stop_defects; the stand-ins started $stop_starts times, not $stop_starts_wanted"
  [ "$stop_left" -eq 0 ] || stop_defects="$stop_defects; $stop_left processes of the stand-ins are left"
  stop_fail=$(grep -m 1 '^FAIL ' "$stop_fixture.out")
  [ -z "$stop_fail" ] || stop_defects="$stop_defects; the runner printed $stop_fail"
  [ "$stop_held_at_end" = no ] || stop_defects="$stop_defects; the runner ended while its stand-in held the marker"
  [ "$witness_alive" = yes ] || stop_defects="$stop_defects; the witness process of the case did not outlive the run"
  [ -n "$stop_defects" ] || return 0
  printf '%s\n' "${stop_defects#; }"
  return 1
}

with_list() {
  fixture_list=yes
  "$@"
}

with_budget() {
  fixture_budget=$1; shift
  "$@"
}

fixture_in_order() {
  out=$(fixture_run "$1" yes)
  case $out in *"$2"*"$3"*) return 0 ;; esac
  return 1
}

seconds_lines() {
  out=$(fixture_run "$1" yes)
  printf '%s\n' "$out" | awk -v want="$2" '/^seconds mutation:/ { names = names " " substr($2, 10) } END { exit names != want }'
}

seconds_own() {
  own_start=$(date +%s)
  out=$(fixture_run "$1" no)
  own_end=$(date +%s)
  printf '%s\n' "$out" | awk -v wall="$((own_end - own_start))" '
    /^seconds mutation:/ { k++; if ($3 < 1) low = 1; if ($3 > max) max = $3 }
    END { exit !(k == 3 && !low && max <= wall - 3) }'
}

fixture_once() {
  out=$(fixture_run "$1" yes)
  printf '%s\n' "$out" | awk -v line="$2" 'index($0, line) == 1 { n++ } END { exit n != 1 }'
}

limit_status_is() {
  limit_want=$1; shift
  limited "$@"
  [ "$?" -eq "$limit_want" ]
}

verdict_on() {
  printf '%s\n' "$2" > "$gate_scratch/gate-line"
  mutant_verdict "$1" "$gate_scratch/gate-line"
}

cases() {
  scratch_root
  unavailable='FAIL store (verify-store unavailable: error[E0425]: cannot find function io_error in this scope)'
  measured='FAIL deletion (verify-deletion measured: root_destroyed store_sealed)'
  t measured-with-its-keys PASS verdict_on 'measured root_destroyed store_sealed' "$measured"
  t measured-with-one-of-two-keys-named PASS verdict_on 'measured store_sealed' "$measured"
  t measured-no-key FAIL verdict_on 'measured wal_bytes' "$measured"
  t measured-with-no-key-named FAIL verdict_on measured "$measured"
  t measured-one-key-of-two-missing FAIL verdict_on 'measured root_destroyed wal_bytes' "$measured"
  t measured-key-is-a-part-of-a-word FAIL verdict_on 'measured root' "$measured"
  t measured-on-a-pass-line FAIL verdict_on 'measured rows' 'PASS store (verify-store measured: rows)'
  t measured-keys-end-at-the-bounds PASS verdict_on 'measured cpu' 'FAIL cost (run measured: cpu; bounds: cpu_max_pct_of_one_core=20)'
  t measured-key-among-the-bounds-only FAIL verdict_on 'measured bounds:' 'FAIL cost (run measured: cpu; bounds: cpu_max_pct_of_one_core=20)'
  t unavailable-no-text FAIL verdict_on 'measured segment_roundtrip' "$unavailable"
  t unavailable-is-no-leak FAIL verdict_on leak "$unavailable"
  t refused-with-its-text PASS verdict_on 'refused journal mode is not wal' 'FAIL deletion (verify-deletion unavailable: halvo-capture: journal mode is not wal)'
  t refused-with-another-text FAIL verdict_on 'refused journal mode is not wal' "$unavailable"
  t refused-on-a-measured-line FAIL verdict_on 'refused root_destroyed' "$measured"
  t leak-line PASS verdict_on leak 'FAIL store (leak: the marker is in what the binary printed)'
  t leak-on-a-measured-line FAIL verdict_on leak "$measured"
  t unknown-verdict FAIL verdict_on 'caught anyhow' "$measured"

  fixture_tree builds
  row builds/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  t row-applies-and-builds PASS fixture_run builds yes
  fixture_tree judged
  stub_gate judged 'FAIL store (verify-store measured: rows)'
  row judged/scripts/r1-gate/mutants.rows keyed b.sh 's/two/three/' 'measured rows' required X-1
  t row-judged-by-its-key PASS fixture_says judged 'PASS mutation:keyed (measured rows: gate said: FAIL store (verify-store measured: rows))'
  fixture_tree scenario-a
  stub_gate scenario-a "$unavailable"
  row scenario-a/scripts/r1-gate/mutants.rows uncompiled b.sh 's/two/three/' 'measured segment_roundtrip' required X-1
  t row-on-an-unavailable-line FAIL fixture_run scenario-a
  t row-on-an-unavailable-line-is-named PASS fixture_says scenario-a 'FAIL mutation:uncompiled (measured segment_roundtrip not shown; gate said: FAIL store (verify-store unavailable'
  fixture_tree tested
  stub_test tested sees-the-mutant
  row tested/scripts/r1-gate/mutants.rows by-test b.sh 's/two/three/' 'test selftest:demo/c1' required X-1
  t test-row-caught PASS fixture_says tested 'PASS mutation:by-test (test selftest:demo/c1: the one test failed)'
  fixture_tree blind
  stub_test blind blind
  row blind/scripts/r1-gate/mutants.rows by-test b.sh 's/two/three/' 'test selftest:demo/c1' required X-1
  t test-row-whose-test-still-passes FAIL fixture_run blind
  fixture_tree wrong
  stub_test wrong wrong-unmutated
  row wrong/scripts/r1-gate/mutants.rows by-test b.sh 's/two/three/' 'test selftest:demo/c1' required X-1
  t test-row-failing-unmutated FAIL fixture_run wrong
  t test-row-failing-unmutated-is-named PASS fixture_says wrong 'FAIL mutation:by-test (unmutated, the test selftest:demo/c1 must run as one passing test: failed)'
  fixture_tree silent
  stub_test silent silent
  row silent/scripts/r1-gate/mutants.rows by-test b.sh 's/two/three/' 'test selftest:demo/c1' required X-1
  t test-row-that-runs-no-case FAIL fixture_run silent
  fixture_tree vanishing
  stub_test vanishing silent-when-mutated
  row vanishing/scripts/r1-gate/mutants.rows by-test b.sh 's/two/three/' 'test selftest:demo/c1' required X-1
  t test-row-whose-test-vanishes-under-the-mutant FAIL fixture_run vanishing
  t only-names-no-row FAIL with_only no-such-row fixture_run builds yes
  t only-runs-the-named-row PASS with_only applies fixture_says builds 'PASS mutation:applies (applies and builds)' yes
  fixture_tree ordered
  row ordered/scripts/r1-gate/mutants.rows first b.sh 's/two/three/' leak required X-1
  row ordered/scripts/r1-gate/mutants.rows second b.sh 's/echo two/echo 2/' leak required X-1
  t only-keeps-the-order-given PASS with_only 'second first' fixture_in_order ordered 'PASS mutation:second' 'PASS mutation:first'
  t only-in-the-other-order FAIL with_only 'first second' fixture_in_order ordered 'PASS mutation:second' 'PASS mutation:first'
  t budget-used-starts-no-row PASS with_budget 0 with_only 'second first' fixture_says ordered 'NOT-RUN mutation:first (the budget of 0 s is used)' yes
  t budget-times-the-row-it-ran PASS with_budget 0 with_only 'second first' fixture_in_order ordered 'PASS mutation:second' 'seconds mutation:second '
  t budget-not-used-leaves-no-row-out FAIL with_budget 600 fixture_says ordered 'NOT-RUN mutation:' yes
  t only-runs-a-name-given-twice-once PASS with_only 'second second first' fixture_once ordered 'PASS mutation:second'
  t only-runs-a-name-given-once-once PASS with_only 'first' fixture_once ordered 'PASS mutation:first'
  t only-skips-a-name-not-given FAIL with_only 'first' fixture_once ordered 'PASS mutation:second'
  fixture_tree slow
  stub_test slow slow
  row slow/scripts/r1-gate/mutants.rows first b.sh 's/two/three/' 'test selftest:demo/c1' required X-1
  row slow/scripts/r1-gate/mutants.rows second b.sh 's/echo two/echo three/' 'test selftest:demo/c1' required X-1
  row slow/scripts/r1-gate/mutants.rows third b.sh 's/o two/o three/' 'test selftest:demo/c1' required X-1
  t budget-times-every-started-row-once PASS with_budget 600 with_only 'first second third' seconds_lines slow ' first second third'
  t budget-times-a-row-that-was-not-started FAIL with_budget 600 with_only 'first second' seconds_lines slow ' first second third'
  t budget-seconds-are-each-row-s-own PASS with_budget 600 with_only 'first second third' seconds_own slow
  t budget-counts-from-the-first-row PASS with_budget 4 with_only 'first second third' fixture_says slow 'NOT-RUN mutation:third (the budget of 4 s is used)'
  fixture_tree defective
  row defective/scripts/r1-gate/mutants.rows good b.sh 's/two/three/' leak required X-1
  printf 'broken\tline\n' >> "$gate_scratch/defective/scripts/r1-gate/mutants.rows"
  t only-still-checks-the-rows-it-does-not-name FAIL with_only good fixture_run defective yes
  t budget-not-a-number-exits-64 PASS limit_status_is 64 30 sh "$DIR/mutation-test.sh" --budget 4x
  t unknown-argument-exits-64 PASS limit_status_is 64 30 sh "$DIR/mutation-test.sh" --no-such-flag
  t limit-status-of-a-hang-is-124 PASS limit_status_is 124 1 sleep 5
  fixture_tree broken
  row broken/scripts/r1-gate/mutants.rows broken b.sh 's/echo two/if then/' leak required X-1
  t does-not-build FAIL fixture_run broken yes
  t does-not-build-is-named PASS fixture_says broken 'FAIL mutation:broken (the mutant does not build)' yes
  fixture_tree control
  row control/scripts/r1-gate/mutants.rows control-row scripts/r1-gate/b.sh 's/echo two/if then/' leak control X-1
  t control-does-not-build PASS fixture_says control 'PASS mutation:control-row (control: the mutant does not build' yes
  fixture_tree control-builds
  row control-builds/scripts/r1-gate/mutants.rows control-row scripts/r1-gate/b.sh 's/two/three/' leak control X-1
  t control-that-builds FAIL fixture_run control-builds yes
  fixture_tree unapplied
  row unapplied/scripts/r1-gate/mutants.rows stale b.sh 's/no such text/x/' leak required X-1
  t substitution-did-not-apply FAIL fixture_run unapplied yes
  t substitution-did-not-apply-is-named PASS fixture_says unapplied 'FAIL mutation:stale (the substitution did not apply)' yes
  fixture_tree half-stale
  row half-stale/scripts/r1-gate/mutants.rows half b.sh 's/two/three/; s/no such text/x/' leak required X-1
  t second-statement-did-not-apply FAIL fixture_run half-stale yes
  t second-statement-did-not-apply-is-named PASS fixture_says half-stale 'FAIL mutation:half (statement 2 of 2 of the substitution did not apply)' yes
  fixture_tree first-stale
  row first-stale/scripts/r1-gate/mutants.rows first b.sh 's/no such text/x/; s/two/three/' leak required X-1
  t first-statement-did-not-apply FAIL fixture_run first-stale yes
  fixture_tree two-statements
  row two-statements/scripts/r1-gate/mutants.rows both b.sh 's{echo}{printf} ; s/two/three/g' leak required X-1
  t two-statements-that-apply PASS fixture_says two-statements 'PASS mutation:both (applies and builds)' yes
  fixture_tree r-modifier
  row r-modifier/scripts/r1-gate/mutants.rows returns b.sh 's/two/three/; s/no such text/x/r' leak required X-1
  t statement-with-the-r-modifier FAIL fixture_run r-modifier yes
  t statement-with-the-r-modifier-is-named PASS fixture_says r-modifier 'FAIL mutation:returns (statement 2 of the substitution has the r modifier, which changes no file)' yes
  fixture_tree not-a-substitution
  row not-a-substitution/scripts/r1-gate/mutants.rows other b.sh 'tr/a-z/A-Z/' leak required X-1
  t program-that-is-no-substitution FAIL fixture_run not-a-substitution yes
  fixture_tree perl
  printf 'use strict;\nprint 1;\n' > "$gate_scratch/perl/scripts/r1-gate/x.pl"
  row perl/scripts/r1-gate/mutants.rows perl-broken x.pl 's/print 1;/print (;/' leak required X-1
  t perl-mutant-with-a-syntax-error FAIL fixture_run perl yes
  t perl-mutant-with-a-syntax-error-is-named PASS fixture_says perl 'FAIL mutation:perl-broken (the mutant does not build)' yes
  fixture_tree perl-good
  printf 'use strict;\nprint 1;\n' > "$gate_scratch/perl-good/scripts/r1-gate/x.pl"
  row perl-good/scripts/r1-gate/mutants.rows perl-two x.pl 's/print 1;/print 2;/' leak required X-1
  t perl-mutant-that-builds PASS fixture_says perl-good 'PASS mutation:perl-two (applies and builds)' yes
  fixture_tree swift
  printf 'let x = 1\n' > "$gate_scratch/swift/scripts/r1-gate/w.swift"
  row swift/scripts/r1-gate/mutants.rows swift-broken w.swift 's/let x = 1/let x = (/' leak required X-1
  t swift-mutant-with-a-syntax-error FAIL fixture_run swift yes
  t swift-mutant-with-a-syntax-error-is-named PASS fixture_says swift 'FAIL mutation:swift-broken (the mutant does not build)' yes
  fixture_tree swift-good
  printf 'let x = 1\n' > "$gate_scratch/swift-good/scripts/r1-gate/w.swift"
  row swift-good/scripts/r1-gate/mutants.rows swift-two w.swift 's/let x = 1/let x = 2/' leak required X-1
  t swift-mutant-that-builds PASS fixture_says swift-good 'PASS mutation:swift-two (applies and builds)' yes
  fixture_tree data
  printf 'limit=1\n' > "$gate_scratch/data/scripts/r1-gate/data.txt"
  row data/scripts/r1-gate/mutants.rows data-row data.txt 's/limit=1/limit=2/' leak required X-1
  t row-on-a-file-of-another-kind FAIL fixture_run data yes
  t row-on-a-file-of-another-kind-is-named PASS fixture_says data 'FAIL mutation:data-row (the runner has no build for scripts/r1-gate/data.txt: a row mutates a file that cargo builds, or a .sh, .c, .pl or .swift file)' yes
  fixture_tree outside
  row outside/scripts/r1-gate/mutants.rows outside ../../code/a.sh 's/one/1/' leak required X-1
  t file-outside-row-directory FAIL fixture_run outside yes
  fixture_tree absolute
  row absolute/scripts/r1-gate/mutants.rows absolute /etc/hosts 's/a/b/' leak required X-1
  t file-with-an-absolute-path FAIL fixture_run absolute yes
  fixture_tree moved
  row moved/scripts/r1-gate/mutants.rows shared code/a.sh 's/one/1/' leak transitional X-1
  row moved/code/mutants.rows shared a.sh 's/one/1/' leak required X-2
  t transitional-taken-over PASS fixture_says moved 'MOVED mutation:shared (code/mutants.rows)' yes
  t transitional-taken-over-exits-0 PASS fixture_run moved yes
  fixture_tree orphan
  row orphan/scripts/r1-gate/mutants.rows shared code/a.sh 's/one/1/' leak transitional X-1
  row orphan/code/mutants.rows another a.sh 's/one/1/' leak required X-2
  t transitional-not-taken-over FAIL fixture_run orphan yes
  t transitional-not-taken-over-is-named PASS fixture_says orphan 'FAIL mutation:shared (transitional row not taken over by code/mutants.rows)' yes
  fixture_tree waiting
  row waiting/scripts/r1-gate/mutants.rows shared code/a.sh 's/one/1/' leak transitional X-1
  t transitional-runs-while-no-file-is-beside-the-code PASS fixture_says waiting 'PASS mutation:shared (applies and builds)' yes
  t transitional-fails-strict FAIL fixture_run waiting yes yes
  t strict-names-the-transitional-row PASS fixture_says waiting 'FAIL mutation:shared (strict: a transitional row, and no code/mutants.rows holds it yet)' yes yes
  fixture_tree twice
  row twice/scripts/r1-gate/mutants.rows same b.sh 's/two/2/' leak required X-1
  row twice/code/mutants.rows same a.sh 's/one/1/' leak required X-2
  t name-twice FAIL fixture_run twice yes
  t name-in-two-files-fails-the-list FAIL with_list fixture_run twice
  t name-in-two-files-names-both PASS with_list fixture_says twice 'FAIL mutation:same (the name stands in code/mutants.rows line 1 and in scripts/r1-gate/mutants.rows line 1)'
  fixture_tree twice-late
  row twice-late/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  row twice-late/scripts/r1-gate/mutants.rows same b.sh 's/two/2/' leak required X-1
  row twice-late/code/mutants.rows same a.sh 's/one/1/' leak required X-2
  t name-in-two-files-applies-no-row FAIL fixture_says twice-late 'PASS mutation:applies' yes
  fixture_tree thrice
  mkdir -p "$gate_scratch/thrice/code/inner"
  printf 'echo three\n' > "$gate_scratch/thrice/code/inner/c.sh"
  row thrice/scripts/r1-gate/mutants.rows same b.sh 's/two/2/' leak required X-1
  row thrice/code/mutants.rows same a.sh 's/one/1/' leak required X-2
  row thrice/code/inner/mutants.rows same c.sh 's/three/3/' leak required X-3
  t name-in-three-files-names-all-three PASS with_list fixture_says thrice 'FAIL mutation:same (the name stands in code/inner/mutants.rows line 1 and in code/mutants.rows line 1 and in scripts/r1-gate/mutants.rows line 1)'
  t transitional-and-its-takeover-pass-the-list PASS with_list fixture_run moved
  fixture_tree same-file
  row same-file/code/mutants.rows shared code/a.sh 's/one/1/' leak transitional X-1
  row same-file/code/mutants.rows shared a.sh 's/one/uno/' leak required X-2
  t name-twice-in-one-file-with-a-transitional-row-is-refused FAIL with_list fixture_run same-file
  fixture_tree lone
  row lone/code/mutants.rows solo code/a.sh 's/one/1/' leak transitional X-1
  t transitional-row-outside-the-gate-file-is-refused FAIL fixture_run lone yes
  t transitional-row-outside-the-gate-file-is-named PASS fixture_says lone 'FAIL mutation:solo (a transitional row stands in scripts/r1-gate/mutants.rows, not in code/mutants.rows)' yes
  fixture_tree malformed-name
  row malformed-name/scripts/r1-gate/mutants.rows malformed b.sh 's/two/2/' leak required X-1
  row malformed-name/code/mutants.rows malformed a.sh 's/one/1/' leak required X-2
  t row-named-malformed-in-two-files-is-refused PASS with_list fixture_says malformed-name 'FAIL mutation:malformed (the name stands in code/mutants.rows line 1 and in scripts/r1-gate/mutants.rows line 1)'
  fixture_tree numeric
  row numeric/scripts/r1-gate/mutants.rows 1 code/a.sh 's/one/1/' leak transitional X-1
  row numeric/code/mutants.rows 1.0 a.sh 's/one/uno/' leak required X-2
  t transitional-row-not-taken-over-by-a-name-of-the-same-number FAIL fixture_run numeric yes
  fixture_tree malformed-alone
  row malformed-alone/scripts/r1-gate/mutants.rows malformed b.sh 's/two/three/' leak required X-1
  t row-named-malformed-runs-as-a-row PASS fixture_says malformed-alone 'PASS mutation:malformed (applies and builds)' yes
  fixture_tree takeover-gate
  row takeover-gate/scripts/r1-gate/mutants.rows inner scripts/r1-gate/b.sh 's/two/2/' leak transitional X-1
  row takeover-gate/scripts/r1-gate/mutants.rows inner b.sh 's/two/3/' leak required X-2
  t transitional-row-on-a-file-beside-the-gate-file-is-refused FAIL with_list fixture_run takeover-gate
  fixture_tree target-dir
  mkdir -p "$gate_scratch/target-dir/code/target"
  row target-dir/scripts/r1-gate/mutants.rows same b.sh 's/two/2/' leak required X-1
  row target-dir/code/target/mutants.rows same c.sh 's/x/y/' leak required X-2
  t rows-file-under-a-directory-named-target-is-read FAIL with_list fixture_run target-dir
  fixture_tree build-dir
  mkdir -p "$gate_scratch/build-dir/target"
  row build-dir/scripts/r1-gate/mutants.rows same b.sh 's/two/2/' leak required X-1
  row build-dir/target/mutants.rows same c.sh 's/x/y/' leak required X-2
  t rows-file-in-the-build-directory-of-the-copy-is-not-read PASS with_list fixture_run build-dir
  fixture_tree numeric-only
  row numeric-only/scripts/r1-gate/mutants.rows 1.0 b.sh 's/two/2/' leak required X-1
  t only-a-name-that-reads-as-the-number-of-another-names-no-row FAIL with_only 1 fixture_run numeric-only yes
  fixture_tree five
  printf 'five\tb.sh\ts/two/2/\tleak\trequired\n' >> "$gate_scratch/five/scripts/r1-gate/mutants.rows"
  t row-with-five-fields FAIL fixture_run five yes
  fixture_tree spaces
  printf 'spaces b.sh s/two/2/ leak required X-1\n' >> "$gate_scratch/spaces/scripts/r1-gate/mutants.rows"
  t row-with-spaces-for-tabs FAIL fixture_run spaces yes
  fixture_tree verdict
  row verdict/scripts/r1-gate/mutants.rows odd b.sh 's/two/2/' 'any FAIL line' required X-1
  t verdict-outside-the-closed-list FAIL fixture_run verdict yes
  fixture_tree state
  row state/scripts/r1-gate/mutants.rows odd b.sh 's/two/2/' leak sometimes X-1
  t state-outside-the-list FAIL fixture_run state yes
  fixture_tree pending
  row pending/scripts/r1-gate/mutants.rows later b.sh 's/two/2/' leak 'pending X-9' X-9
  t pending-row-is-open PASS fixture_says pending 'OPEN mutation:later (X-9)' yes
  t pending-row-fails-strict FAIL fixture_run pending yes yes
  t strict-names-the-pending-row PASS fixture_says pending 'FAIL mutation:later (strict: the row is pending)' yes yes
  fixture_tree retired
  row retired/scripts/r1-gate/mutants.rows gone b.sh 's/two/2/' leak 'retired replaced by the test store::x' X-9
  t retired-row-passes-strict PASS fixture_run retired yes yes
  fixture_tree expected
  row expected/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  printf 'applies\nnever-written\n' > "$gate_scratch/expected/scripts/r1-gate/mutants.expected"
  t expected-name-with-no-row PASS fixture_run expected yes
  t expected-name-with-no-row-fails-strict FAIL fixture_run expected yes yes
  t expected-name-is-named PASS fixture_says expected 'FAIL mutation:never-written (named in mutants.expected' yes yes
  fixture_tree no-expected
  row no-expected/scripts/r1-gate/mutants.rows gone b.sh 's/two/2/' leak 'retired replaced by the test store::x' X-9
  mv "$gate_scratch/no-expected/scripts/r1-gate/mutants.expected" "$gate_scratch/no-expected/expected-moved-away"
  t expected-file-missing-fails-strict FAIL fixture_run no-expected yes yes
  t expected-file-missing-is-named PASS fixture_says no-expected 'FAIL mutation:expected (scripts/r1-gate/mutants.expected is missing, and --strict needs it)' yes yes
  t expected-file-missing-passes-without-strict PASS fixture_run no-expected yes
  fixture_tree unnamed
  row unnamed/scripts/r1-gate/mutants.rows unnamed-row b.sh 's/two/three/' leak required X-1
  t required-row-with-no-expected-name-fails-strict FAIL fixture_run unnamed yes yes
  t required-row-with-no-expected-name-is-named PASS fixture_says unnamed 'FAIL mutation:unnamed-row (strict: a required row of scripts/r1-gate/mutants.rows, and mutants.expected does not name it)' yes yes
  t required-row-with-no-expected-name-passes-without-strict PASS fixture_run unnamed yes
  fixture_tree named
  row named/scripts/r1-gate/mutants.rows named-row b.sh 's/two/three/' leak required X-1
  row named/code/mutants.rows other-row a.sh 's/one/1/' leak 'required on hosted-runner' X-2
  printf 'named-row\nother-row\n' > "$gate_scratch/named/scripts/r1-gate/mutants.expected"
  t required-rows-with-their-expected-names-pass-strict PASS not_ci fixture_run named yes yes
  fixture_tree unnamed-place
  row unnamed-place/scripts/r1-gate/mutants.rows named-row b.sh 's/two/three/' leak required X-1
  row unnamed-place/code/mutants.rows other-row a.sh 's/one/1/' leak 'required on hosted-runner' X-2
  printf 'named-row\n' > "$gate_scratch/unnamed-place/scripts/r1-gate/mutants.expected"
  t required-on-a-place-row-with-no-expected-name-fails-strict FAIL not_ci fixture_run unnamed-place yes yes
  t required-on-a-place-row-is-named-with-its-file PASS not_ci fixture_says unnamed-place 'FAIL mutation:other-row (strict: a required row of code/mutants.rows, and mutants.expected does not name it)' yes yes
  fixture_tree unterminated
  row unterminated/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  printf 'applies\nnever-written' > "$gate_scratch/unterminated/scripts/r1-gate/mutants.expected"
  t expected-name-on-a-last-line-without-its-line-end-fails-strict FAIL fixture_run unterminated yes yes
  fixture_tree no-list-counted
  row no-list-counted/scripts/r1-gate/mutants.rows counted b.sh 's/two/three/' leak required X-1
  mv "$gate_scratch/no-list-counted/scripts/r1-gate/mutants.expected" "$gate_scratch/no-list-counted/expected-moved-away"
  t required-row-is-counted-when-mutants-expected-is-missing PASS fixture_says no-list-counted ', 1 required rows with no expected name' yes
  fixture_tree numeric-expected
  row numeric-expected/scripts/r1-gate/mutants.rows 1.0 b.sh 's/two/2/' leak required X-1
  printf '1.0\n1\n' > "$gate_scratch/numeric-expected/scripts/r1-gate/mutants.expected"
  t expected-name-that-reads-as-the-number-of-a-row-fails-strict FAIL fixture_run numeric-expected yes yes
  fixture_tree empty
  t no-row-at-all FAIL fixture_run empty yes
  fixture_tree missing-file
  row missing-file/scripts/r1-gate/mutants.rows lost gone.sh 's/a/b/' leak required X-1
  t file-not-in-the-copy FAIL fixture_run missing-file yes
  fixture_tree mac
  row mac/scripts/r1-gate/mutants.rows mac-only b.sh 's/two/2/' leak 'required on measuring-mac' X-1
  t measuring-mac-row-under-ci-fails-strict FAIL under_ci fixture_run mac yes yes
  t measuring-mac-row-under-ci-is-not-run PASS under_ci fixture_says mac 'NOT-RUN mutation:mac-only (runs on the measuring Mac)' yes
  fixture_tree input
  row input/scripts/r1-gate/mutants.rows typed b.sh 's/two/2/' leak 'required on hosted-runner' X-1
  printf 'typed\n' > "$gate_scratch/input/scripts/r1-gate/mutants.expected"
  t input-row-here-is-not-run-and-no-fail PASS not_ci fixture_run input yes yes
  t input-row-here-is-named PASS not_ci fixture_says input 'NOT-RUN mutation:typed (needs input; runs on the hosted runner)' yes
  t input-row-runs-under-ci PASS under_ci fixture_says input 'PASS mutation:typed (applies and builds)' yes
  printf 'running 1 test\ntest store::hardening_tests::a_shredded_wrapped_key_is_gone_from_the_raw_database_pages ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 98 filtered out; finished in 0.21s\n' > "$gate_scratch/cargo-failed"
  printf 'running 1 test\n.\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 98 filtered out; finished in 0.21s\n' > "$gate_scratch/cargo-passed"
  printf 'running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out; finished in 0.00s\n' > "$gate_scratch/cargo-none"
  printf 'running 2 tests\n..\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 97 filtered out; finished in 0.21s\n' > "$gate_scratch/cargo-two"
  printf 'error[E0308]: mismatched types\n' > "$gate_scratch/cargo-no-build"
  t cargo-one-test-failed PASS outcome_is failed cargo-failed
  t cargo-one-test-passed PASS outcome_is passed cargo-passed
  t cargo-filter-matched-no-test PASS outcome_is "ran 0 tests" cargo-none
  t cargo-two-tests-ran PASS outcome_is "ran 2 tests" cargo-two
  t cargo-test-did-not-build PASS outcome_is "ran 0 tests" cargo-no-build
  t limit-ends-a-hang FAIL limited 1 sleep 5
  t limit-passes-a-status PASS limited 5 true
  t limit-passes-a-failure FAIL limited 5 false
  for stop_fixture_name in stop-tree stop-twice stop-case stop-group stop-bound stop-build stop-build-apply stop-build-last; do
    fixture_tree "$stop_fixture_name"
    stop_file=b.sh; stop_first='s/two/three/'; stop_second='s/echo two/echo 2/'; stop_verdict='test stand::in'
    case $stop_fixture_name in
      stop-case|stop-group) stub_held_case "$stop_fixture_name"; stop_verdict='test selftest:hold/c1' ;;
      stop-bound) stub_cargo "$stop_fixture_name" 30 ;;
      stop-build*) stub_cc "$stop_fixture_name"; stub_cargo "$stop_fixture_name" 0; stop_file=x.c; stop_first='s/1/2/'; stop_second='s/int/long/' ;;
      *) stub_cargo "$stop_fixture_name" 2 ;;
    esac
    row "$stop_fixture_name/scripts/r1-gate/mutants.rows" held "$stop_file" "$stop_first" "$stop_verdict" required X-1
    [ "$stop_fixture_name" = stop-build-last ] || row "$stop_fixture_name/scripts/r1-gate/mutants.rows" after "$stop_file" "$stop_second" "$stop_verdict" required X-1
  done
  stop_let_end='the running case was let end)'
  t stop-by-term-to-every-process-lets-the-case-end PASS stopped_by stop-tree TERM tree 60 run 143 gone itself 1 "STOPPED mutation:held (signal TERM; $stop_let_end"
  t stop-by-hup-then-term-keeps-the-first-signal PASS stopped_by stop-twice HUP twice 60 run 129 gone itself 1 "STOPPED mutation:held (signal HUP; $stop_let_end"
  t stop-by-term-to-the-runner-ends-a-self-test-case-by-its-trap PASS stopped_by stop-case TERM runner 60 run 143 gone TERM 1 "STOPPED mutation:held (signal TERM; $stop_let_end"
  t stop-by-term-to-the-runner-s-group-ends-a-self-test-case-by-its-trap PASS stopped_by stop-group TERM group 60 run 143 gone TERM 1 "STOPPED mutation:held (signal TERM; $stop_let_end"
  t stop-past-the-bound-kills-the-case PASS stopped_by stop-bound TERM runner 1 run 143 left none 1 'STOPPED mutation:held (signal TERM; the running case did not end within 1 s and was killed)'
  t stop-during-a-build-starts-no-test PASS stopped_by stop-build TERM runner 60 run 143 gone itself 2 'STOPPED mutation:held (signal TERM; no test was running)'
  t stop-during-a-build-starts-no-new-row PASS stopped_by stop-build-apply TERM runner 60 apply-only 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  t stop-during-the-last-build-prints-no-count PASS stopped_by stop-build-last TERM runner 60 apply-only 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  fixture_tree stop-build-group
  stub_cc stop-build-group
  row stop-build-group/scripts/r1-gate/mutants.rows held x.c 's/1/2/' leak required X-1
  t stop-by-term-to-the-group-during-a-build-reports-no-build-failure PASS stopped_by stop-build-group TERM group 60 apply-only 143 left none 1 'STOPPED mutation:held (signal TERM; no test was running)'
  fixture_tree stop-unmutated
  mkdir -p "$gate_scratch/stop-unmutated/crates/x/src"
  printf 'pub fn f() -> i32 { 1 }\n' > "$gate_scratch/stop-unmutated/crates/x/src/lib.rs"
  cp "$DIR/lib.sh" "$gate_scratch/stop-unmutated/scripts/r1-gate/lib.sh"
  stub_cargo stop-unmutated 30
  row stop-unmutated/crates/x/mutants.rows held src/lib.rs 's/1 }/2 }/' 'test stand::in' required X-1
  t stop-by-term-to-the-group-during-the-unmutated-build-reports-no-build-failure PASS with_cargo_home stop-unmutated stopped_by stop-unmutated TERM group 1 apply-only 143 left none 1 'STOPPED mutation:held (signal TERM; the running build of the copy did not end within 1 s and was killed)'
  fixture_tree stop-gate
  stub_gate_hold stop-gate
  row stop-gate/scripts/r1-gate/mutants.rows held b.sh 's/two/three/' 'measured rows' required X-1
  t stop-by-term-to-the-group-during-a-gate-check-reports-no-failure PASS stopped_by stop-gate TERM group 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  fixture_tree stop-gate-unmutated
  mkdir -p "$gate_scratch/stop-gate-unmutated/crates/x/src"
  printf 'pub fn f() -> i32 { 1 }\n' > "$gate_scratch/stop-gate-unmutated/crates/x/src/lib.rs"
  cp "$DIR/lib.sh" "$gate_scratch/stop-gate-unmutated/scripts/r1-gate/lib.sh"
  stub_cargo stop-gate-unmutated 0
  stub_gate_hold stop-gate-unmutated
  row stop-gate-unmutated/crates/x/mutants.rows held src/lib.rs 's/1 }/2 }/' 'test stand::in' required X-1
  t stop-by-term-to-the-group-during-an-unmutated-gate-check-reports-no-failure PASS with_cargo_home stop-gate-unmutated stopped_by stop-gate-unmutated TERM group 60 run 143 gone itself 2 'STOPPED mutation:held (signal TERM; no test was running)'
  for stop_fixture_name in stop-restore stop-restore-last; do
    fixture_tree "$stop_fixture_name"
    stub_cp "$stop_fixture_name"
    row "$stop_fixture_name/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' leak required X-1
    [ "$stop_fixture_name" = stop-restore-last ] || row "$stop_fixture_name/scripts/r1-gate/mutants.rows" after b.sh 's/echo two/echo 2/' leak required X-1
  done
  t stop-while-a-row-ends-starts-no-new-row PASS stopped_by stop-restore TERM runner 60 apply-only 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  t stop-while-the-last-row-ends-prints-no-count PASS stopped_by stop-restore-last TERM runner 60 apply-only 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  stop_cases_by_step
  stop_cases_by_main
  shield_cases
  verifier_cases
  rule_cases
}

verifier_cases() {
  for vf_fixture in then-int then-hup clock; do
    fixture_tree "vf-$vf_fixture"
    stub_gate_until_go "vf-$vf_fixture"
    stub_ps "vf-$vf_fixture"
    row "vf-$vf_fixture/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' 'measured rows' required X-1
  done
  for vf_fixture in then-int then-hup; do
    : > "$gate_scratch/vf-$vf_fixture/bin/ps.slow"
  done
  t vf-stop-by-term-then-int-while-the-runner-waits-for-a-gate-check-changes-nothing PASS stopped_by vf-then-int TERM helper-then-int 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  t vf-stop-by-term-then-hup-while-the-runner-waits-for-a-gate-check-changes-nothing PASS stopped_by vf-then-hup TERM helper-then-hup 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  fixture_tree vf-note
  stub_gate_hold vf-note
  row vf-note/scripts/r1-gate/mutants.rows held b.sh 's/two/three/' 'measured rows' required X-1
  t vf-a-stop-note-of-the-caller-s-environment-is-not-printed PASS vf_with_stop_note stopped_by vf-note TERM runner 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  vf_stub_date vf-clock
  t vf-the-bound-of-a-shielded-step-ends-at-its-second-on-a-made-up-clock PASS vf_clock_reads vf-clock 4 stopped_by vf-clock TERM group 3 run 143 left none 1 'STOPPED mutation:held (signal TERM; the running check-store did not end within 3 s and was killed)'
  fixture_tree vf-fork
  stub_gate_hold vf-fork
  vf_stub_forking vf-fork
  row vf-fork/scripts/r1-gate/mutants.rows held b.sh 's/two/three/' 'measured rows' required X-1
  t vf-the-bound-s-kill-leaves-no-process-that-forks-at-the-read-of-the-table PASS vf_no_fork_after_the_stop vf-fork stopped_by vf-fork TERM group 1 run 143 left none 1 'STOPPED mutation:held (signal TERM; the running check-store did not end within 1 s and was killed)'
}

vf_with_stop_note() {
  stop_note='a note from the environment'
  export stop_note
  "$@"
}

vf_stub_date() {
  vf_date_real=$(command -v date)
  printf '%s\n' "$vf_date_real" > "$gate_scratch/$1/bin/date.real"
  printf '1000\n' > "$gate_scratch/$1/bin/date.now"
  cat > "$gate_scratch/$1/bin/date" <<'STUB'
#!/bin/sh
here=${0%/*}
if [ "$*" = +%s ]; then
  read -r now < "$here/date.now"
  if [ -e "$here/marker" ]; then
    now=$((now + 1))
    printf '%s\n' "$now" > "$here/date.now"
    printf 'read\n' >> "$here/date.reads"
  fi
  printf '%s\n' "$now"
  exit 0
fi
read -r real < "$here/date.real"
exec "$real" "$@"
STUB
  chmod +x "$gate_scratch/$1/bin/date"
}

vf_clock_reads() {
  vf_clock_fixture=$gate_scratch/$1; vf_clock_wanted=$2; shift 2
  "$@" || return 1
  vf_clock_got=0
  [ ! -f "$vf_clock_fixture/bin/date.reads" ] || vf_clock_got=$(wc -l < "$vf_clock_fixture/bin/date.reads" | tr -d ' ')
  [ "$vf_clock_got" -eq "$vf_clock_wanted" ] || { printf 'the runner read the made-up clock %s times while the step held its marker, not %s\n' "$vf_clock_got" "$vf_clock_wanted"; return 1; }
}

vf_stub_forking() {
  vf_ps_real=$(command -v ps)
  printf '%s\n' "$vf_ps_real" > "$gate_scratch/$1/bin/ps.real"
  cat > "$gate_scratch/$1/bin/ps" <<'STUB'
#!/bin/sh
here=${0%/*}
read -r real < "$here/ps.real"
case "$*" in
  *pid=,ppid=*)
    table=$("$real" "$@")
    reads=0
    [ ! -f "$here/reads" ] || read -r reads < "$here/reads"
    reads=$((reads + 1))
    printf '%s\n' "$reads" > "$here/reads"
    : > "$here/fork-request.$reads"
    polls=0
    until [ -e "$here/fork-ack.$reads" ] || [ "$polls" -ge 20 ]; do
      polls=$((polls + 1))
      sleep 0.1
    done
    printf '%s\n' "$table" ;;
  *) exec "$real" "$@" ;;
esac
STUB
  chmod +x "$gate_scratch/$1/bin/ps"
  cat > "$gate_scratch/$1/scripts/r1-gate/check-store.sh" <<'STUB'
#!/bin/sh
here=${0%/*}/../../bin
printf 'store\n' >> "$here/starts"
: > "$here/marker"
exec perl -e '
  my $here = shift;
  my $child = fork();
  my $role = $child ? "step" : "child";
  my $serves = $child ? 3 : 1;
  my %served;
  for (my $poll = 0; $poll < 1200; $poll++) {
    for my $request (glob("$here/fork-request.*")) {
      my ($n) = $request =~ /(\d+)$/;
      next if $served{$n}++ || ($n != $serves && $n != 3);
      open(my $mark, ">", "$here/forked-by-$role.$n"); close($mark);
      my $grandchild = fork();
      if (defined $grandchild && $grandchild == 0) {
        select(undef, undef, undef, 0.05) for 1 .. 1200;
        exit 0;
      }
      open(my $ack, ">", "$here/fork-ack.$n"); close($ack);
    }
    select(undef, undef, undef, 0.05);
  }
' "$here"
STUB
}

vf_group_end() {
  case ${1:-} in ''|*[!0-9]*|0|1) printf 'the case holds no id of its runner\n'; return 1 ;; esac
  vf_group_rest=$(pgrep -g "$1")
  [ -z "$vf_group_rest" ] || kill -s KILL -- "-$1"
}

vf_no_fork_after_the_stop() {
  vf_fork_fixture=$gate_scratch/$1; shift
  "$@"
  vf_fork_status=$?
  vf_group_end "${stop_pid:-}" || return 1
  [ -e "$vf_fork_fixture/bin/forked-by-child.1" ] || { printf 'the child of the stand-in did not fork at the read of the table, so the case shows nothing\n'; return 1; }
  return "$vf_fork_status"
}

shield_cases() {
  helper_kill_after_s=5
  for shield_fixture in term int hup bound again; do
    fixture_tree "stop-shield-$shield_fixture"
    stub_gate_until_go "stop-shield-$shield_fixture"
    stub_ps "stop-shield-$shield_fixture"
    row "stop-shield-$shield_fixture/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' 'measured rows' required X-1
  done
  : > "$gate_scratch/stop-shield-again/bin/ps.slow"
  t stop-by-term-again-while-the-runner-waits-for-a-gate-check-changes-nothing PASS stopped_by stop-shield-again TERM helper-again 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  for shield_signal in TERM INT HUP; do
    stop_line "$shield_signal"
    t "stop-by-$stop_line_lower-as-the-helper-ends-a-run-lets-a-gate-check-end" PASS stopped_by "stop-shield-$stop_line_lower" "$shield_signal" helper 60 run "$stop_line_status" gone itself 1 "STOPPED mutation:held (signal $shield_signal; no test was running)"
  done
  t stop-past-the-bound-kills-a-gate-check-and-names-it PASS stopped_by stop-shield-bound TERM group 1 run 143 left none 1 'STOPPED mutation:held (signal TERM; the running check-store did not end within 1 s and was killed)'
  for shield_check in deletion:2 no-ll-hook:3; do
    bound_holder=${shield_check%:*}
    fixture_tree "stop-shield-bound-$bound_holder"
    stub_gate_holds_at "stop-shield-bound-$bound_holder" "$bound_holder"
    row "stop-shield-bound-$bound_holder/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' 'measured rows' required X-1
    t "stop-past-the-bound-kills-check-$bound_holder-and-names-it" PASS stopped_by "stop-shield-bound-$bound_holder" TERM group 1 run 143 left none "${shield_check#*:}" "STOPPED mutation:held (signal TERM; the running check-$bound_holder did not end within 1 s and was killed)"
  done
  fixture_tree stop-shield-unmutated
  mkdir -p "$gate_scratch/stop-shield-unmutated/crates/x/src"
  printf 'pub fn f() -> i32 { 1 }\n' > "$gate_scratch/stop-shield-unmutated/crates/x/src/lib.rs"
  cp "$DIR/lib.sh" "$gate_scratch/stop-shield-unmutated/scripts/r1-gate/lib.sh"
  stub_cargo stop-shield-unmutated 0
  stub_gate_until_go stop-shield-unmutated
  stub_ps stop-shield-unmutated
  row stop-shield-unmutated/crates/x/mutants.rows held src/lib.rs 's/1 }/2 }/' 'test stand::in' required X-1
  t stop-by-term-as-the-helper-ends-a-run-lets-an-unmutated-gate-check-end PASS with_cargo_home stop-shield-unmutated stopped_by stop-shield-unmutated TERM helper 60 run 143 gone itself 2 'STOPPED mutation:held (signal TERM; no test was running)'
  t a-shielded-step-starts-with-no-signal-and-not-after-one PASS shield_start_rule
  fixture_tree stop-shield-build
  mkdir -p "$gate_scratch/stop-shield-build/crates/x/src"
  printf 'pub fn f() -> i32 { 1 }\n' > "$gate_scratch/stop-shield-build/crates/x/src/lib.rs"
  cp "$DIR/lib.sh" "$gate_scratch/stop-shield-build/scripts/r1-gate/lib.sh"
  stub_cargo stop-shield-build go
  stub_ps stop-shield-build
  stub_gate_hold stop-shield-build
  row stop-shield-build/crates/x/mutants.rows held src/lib.rs 's/1 }/2 }/' 'test stand::in' required X-1
  t stop-by-term-as-the-helper-ends-a-run-lets-the-unmutated-build-end PASS with_cargo_home stop-shield-build stopped_by stop-shield-build TERM helper 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
}

stub_gate_holds_at() {
  mkdir -p "$gate_scratch/$1/bin"
  for stub in store deletion no-ll-hook; do
    # shellcheck disable=SC2016
    printf '#!/bin/sh\nprintf "%s\\n" >> "${0%%/*}/../../bin/starts"\nprintf "PASS %s\\n"\n' "$stub" "$stub" > "$gate_scratch/$1/scripts/r1-gate/check-$stub.sh"
  done
  cat > "$gate_scratch/$1/scripts/r1-gate/check-$2.sh" <<'STUB'
#!/bin/sh
here=${0%/*}/../../bin
me=${0##*/check-}
me=${me%.sh}
printf '%s\n' "$me" >> "$here/starts"
: > "$here/marker"
polls=0
until [ -e "$here/go" ]; do
  polls=$((polls + 1))
  [ "$polls" -lt 300 ] || { printf 'timeout\n' > "$here/ended"; exit 1; }
  sleep 0.1
done
mv "$here/marker" "$here/marker.released"
printf 'itself\n' > "$here/ended"
printf 'PASS %s\n' "$me"
STUB
}

stub_ps() {
  mkdir -p "$gate_scratch/$1/bin"
  stub_ps_real=$(command -v ps)
  printf '%s\n' "$stub_ps_real" > "$gate_scratch/$1/bin/ps.real"
  cat > "$gate_scratch/$1/bin/ps" <<'STUB'
#!/bin/sh
here=${0%/*}
case "$*" in
  *stat=*) printf '%s\n' "$*" >> "$here/ps.polls"; [ ! -e "$here/ps.slow" ] || sleep 1 ;;
esac
read -r real < "$here/ps.real"
exec "$real" "$@"
STUB
  chmod +x "$gate_scratch/$1/bin/ps"
}

polls_of_the_step() {
  step_polls=0
  [ ! -f "$1/bin/ps.polls" ] || step_polls=$(wc -l < "$1/bin/ps.polls" | tr -d ' ')
}

stub_gate_until_go() {
  stub_gate_hold "$1"
  cat > "$gate_scratch/$1/scripts/r1-gate/check-store.sh" <<'STUB'
#!/bin/sh
here=${0%/*}/../../bin
printf 'store\n' >> "$here/starts"
: > "$here/marker"
polls=0
until [ -e "$here/go" ]; do
  polls=$((polls + 1))
  [ "$polls" -lt 300 ] || { printf 'timeout\n' > "$here/ended"; exit 1; }
  sleep 0.1
done
mv "$here/marker" "$here/marker.released"
printf 'itself\n' > "$here/ended"
printf 'PASS store\n'
STUB
}

shield_start_rule() {
  shield_rule_dir=$gate_scratch/shield-start-rule
  mkdir -p "$shield_rule_dir"
  # shellcheck disable=SC2016
  ( stop_signal=''; shield_run check-x sh -c ': > "$1"' sh "$shield_rule_dir/with-no-signal" ) > "$shield_rule_dir.out" 2>&1
  [ -e "$shield_rule_dir/with-no-signal" ] || { printf 'the step did not start with no signal recorded\n'; return 1; }
  # shellcheck disable=SC2016
  ( stop_signal=TERM; stop_status=143; stop_row=held; shield_run check-x sh -c ': > "$1"' sh "$shield_rule_dir/after-a-signal" ) >> "$shield_rule_dir.out" 2>&1
  [ ! -e "$shield_rule_dir/after-a-signal" ] || { printf 'the step started after a recorded signal\n'; return 1; }
}

stop_line() {
  case $1 in
    TERM) stop_line_status=143; stop_line_lower=term ;;
    INT) stop_line_status=130; stop_line_lower=int ;;
    *) stop_line_status=129; stop_line_lower=hup ;;
  esac
}

stop_cases_by_step() {
  for step_fixture in p5-term p5-int p5-hup p5-group; do
    fixture_tree "stop-$step_fixture"
    mkdir -p "$gate_scratch/stop-$step_fixture/crates/x/src"
    printf 'pub fn f() -> i32 { 1 }\n' > "$gate_scratch/stop-$step_fixture/crates/x/src/lib.rs"
    cp "$DIR/lib.sh" "$gate_scratch/stop-$step_fixture/scripts/r1-gate/lib.sh"
    stub_cargo "stop-$step_fixture" 1
    stub_gate_hold "stop-$step_fixture"
    row "stop-$step_fixture/crates/x/mutants.rows" held src/lib.rs 's/1 }/2 }/' 'test stand::in' required X-1
  done
  for step_signal in TERM INT HUP; do
    stop_line "$step_signal"
    t "stop-by-$stop_line_lower-to-the-runner-during-the-unmutated-build-starts-no-gate-check" PASS with_cargo_home "stop-p5-$stop_line_lower" stopped_by "stop-p5-$stop_line_lower" "$step_signal" runner 60 run "$stop_line_status" gone itself 1 "STOPPED mutation:held (signal $step_signal; no test was running)"
  done
  t stop-by-term-to-the-group-during-the-unmutated-build-starts-no-gate-check PASS with_cargo_home stop-p5-group stopped_by stop-p5-group TERM group 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  for step_fixture in p6-term p6-int p6-hup p6-group; do
    fixture_tree "stop-$step_fixture"
    stub_gate_hold "stop-$step_fixture"
    row "stop-$step_fixture/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' 'measured rows' required X-1
  done
  for step_signal in TERM INT HUP; do
    stop_line "$step_signal"
    t "stop-by-$stop_line_lower-to-the-runner-during-a-gate-check-starts-no-next-check" PASS stopped_by "stop-p6-$stop_line_lower" "$step_signal" runner 60 run "$stop_line_status" gone itself 1 "STOPPED mutation:held (signal $step_signal; no test was running)"
  done
  t stop-by-term-to-the-group-during-a-gate-check-starts-no-next-check PASS stopped_by stop-p6-group TERM group 60 run 143 gone itself 1 'STOPPED mutation:held (signal TERM; no test was running)'
  for step_fixture in p7-term p7-int p7-hup; do
    fixture_tree "stop-$step_fixture"
    stub_hold "stop-$step_fixture" perl '*apply-row*'
    row "stop-$step_fixture/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' leak required X-1
  done
  for step_signal in TERM INT HUP; do
    stop_line "$step_signal"
    t "stop-by-$stop_line_lower-to-the-group-during-the-apply-step-prints-no-fail" PASS stopped_by "stop-p7-$stop_line_lower" "$step_signal" group 60 apply-only "$stop_line_status" left none 0 "STOPPED mutation:held (signal $step_signal; no test was running)"
  done
  for step_fixture in p8-int p8-tree-int p8-tree-hup; do
    fixture_tree "stop-$step_fixture"
    stub_cargo "stop-$step_fixture" 2
    row "stop-$step_fixture/scripts/r1-gate/mutants.rows" held b.sh 's/two/three/' 'test stand::in' required X-1
    row "stop-$step_fixture/scripts/r1-gate/mutants.rows" after b.sh 's/echo two/echo 2/' 'test stand::in' required X-1
  done
  t stop-by-int-to-the-runner-lets-the-case-end PASS stopped_by stop-p8-int INT runner 60 run 130 gone itself 1 'STOPPED mutation:held (signal INT; the running case was let end)'
  t stop-by-int-to-every-process-lets-the-case-end PASS stopped_by stop-p8-tree-int INT tree 60 run 130 gone itself 1 'STOPPED mutation:held (signal INT; the running case was let end)'
  t stop-by-hup-to-every-process-lets-the-case-end PASS stopped_by stop-p8-tree-hup HUP tree 60 run 129 gone itself 1 'STOPPED mutation:held (signal HUP; the running case was let end)'
}

stop_cases_by_main() {
  for main_phase in p1 p2 p3 p4 p9; do
    for main_how in term int hup other; do main_fixture "main-$main_phase-$main_how"; done
  done
  for main_how in term int hup other; do
    stub_hold "main-p1-$main_how" sort '*'
    stub_hold "main-p3-$main_how" awk '*malformed*'
    stub_hold "main-p4-$main_how" awk '*mutation:%s*'
    stub_hold "main-p9-$main_how" awk '*ARGV*'
  done
  stub_hold main-p2-term git '*archive*'
  stub_hold main-p2-other git '*archive*'
  stub_hold main-p2-int tar '*'
  stub_hold main-p2-hup cp '*-Rpc*'
  mkdir -p "$gate_scratch/main-p2-hup/target/r1-gate"
  printf 'built\n' > "$gate_scratch/main-p2-hup/target/r1-gate/x"
  no_row='no row had started)'
  for main_signal in TERM INT HUP; do
    stop_line "$main_signal"
    t "stop-by-$stop_line_lower-to-the-runner-before-the-flags-are-read" PASS main_stopped_by "main-p1-$stop_line_lower" "$main_signal" runner "$stop_line_status" "STOPPED mutation (signal $main_signal; $no_row" --list
    t "stop-by-$stop_line_lower-to-the-group-during-the-copy-of-head" PASS main_stopped_by "main-p2-$stop_line_lower" "$main_signal" group "$stop_line_status" "STOPPED mutation (signal $main_signal; $no_row" --list
    t "stop-by-$stop_line_lower-to-the-group-during-the-read-of-the-rows" PASS main_stopped_by "main-p3-$stop_line_lower" "$main_signal" group "$stop_line_status" "STOPPED mutation (signal $main_signal; $no_row" --list
    t "stop-by-$stop_line_lower-to-the-runner-during-the-print-of-the-list" PASS main_stopped_by "main-p4-$stop_line_lower" "$main_signal" runner "$stop_line_status" "STOPPED mutation (signal $main_signal; $no_row" --list
    t "stop-by-$stop_line_lower-to-the-runner-during-the-tail" PASS main_stopped_by "main-p9-$stop_line_lower" "$main_signal" runner "$stop_line_status" "STOPPED mutation:applies (signal $main_signal; no test was running)" --apply-only
  done
  t stop-by-term-to-the-group-before-the-flags-are-read PASS main_stopped_by main-p1-other TERM group 143 "STOPPED mutation (signal TERM; $no_row" --list
  t stop-by-term-to-the-runner-during-the-copy-of-head PASS main_stopped_by main-p2-other TERM runner 143 "STOPPED mutation (signal TERM; $no_row" --list
  t stop-by-term-to-the-runner-during-the-read-of-the-rows PASS main_stopped_by main-p3-other TERM runner 143 "STOPPED mutation (signal TERM; $no_row" --list
  t stop-by-term-to-the-group-during-the-print-of-the-list PASS main_stopped_by main-p4-other TERM group 143 "STOPPED mutation (signal TERM; $no_row" --list
  main_fixture main-step-first
  stub_hold main-step-first awk '*malformed*'
  t stop-by-term-to-a-step-and-then-to-the-runner-ends-as-a-stop PASS main_stopped_by main-step-first TERM step-first 143 "STOPPED mutation (signal TERM; $no_row" --list
  main_fixture main-help
  stub_hold main-help cat '*'
  t stop-by-term-to-the-runner-during-the-help PASS main_stopped_by main-help TERM runner 143 "STOPPED mutation (signal TERM; $no_row" --help
  t stop-by-term-to-the-group-during-the-tail-prints-no-false-fail PASS main_stopped_by main-p9-other TERM group 143 'STOPPED mutation:applies (signal TERM; no test was running)' --apply-only --strict
  main_fixture main-p8
  stub_cargo main-p8 2
  row main-p8/scripts/r1-gate/mutants.rows tested b.sh 's/echo two/echo 2/' 'test stand::in' required X-1
  printf 'applies\ntested\n' > "$gate_scratch/main-p8/scripts/r1-gate/mutants.expected"
  t scratch-is-removed-after-a-stop-of-a-test PASS main_stopped_by main-p8 TERM runner 143 'STOPPED mutation:tested (signal TERM; the running case was let end)' --only tested
  main_fixture main-clean
  t scratch-is-removed-after-a-run-that-ends-0 PASS main_run main-clean 0 '' 0 --apply-only
  main_fixture main-fail
  row main-fail/scripts/r1-gate/mutants.rows stale b.sh 's/no such text/x/' leak required X-1
  printf 'applies\nstale\n' > "$gate_scratch/main-fail/scripts/r1-gate/mutants.expected"
  t scratch-is-removed-after-a-run-with-a-fail-line PASS main_run main-fail 1 'FAIL mutation:stale (the substitution did not apply)' 0 --apply-only
  main_fixture main-keep
  t scratch-is-kept-with-keep-scratch PASS main_run main-keep 0 'scratch mutation: ' 1 --apply-only --keep-scratch
  main_fixture main-export
  row main-export/code/mutants.rows other a.sh 's/one/1/' leak required X-2
  printf 'applies\nother\n' > "$gate_scratch/main-export/scripts/r1-gate/mutants.expected"
  printf 'code/mutants.rows export-ignore\n' > "$gate_scratch/main-export/.gitattributes"
  t rows-file-that-the-export-leaves-out-is-refused PASS main_run main-export 1 'FAIL mutation:rows (the tracked path code/mutants.rows is not a file of the copy of HEAD)' 0 --list
}

rule_cases() {
  t stop-bound-is-60-s PASS stop_bound_is_60
  t no-test-starts-after-a-recorded-signal PASS test_after_a_signal
  t a-step-ended-by-a-signal-that-the-runner-did-not-get-exits-2 PASS step_ended_by_a_signal_of_its_own
  t a-gate-case-that-a-row-runs-gets-remove-scratch PASS case_args_hold tested
  fixture_tree two-takeovers
  row two-takeovers/scripts/r1-gate/mutants.rows shared code/a.sh 's/one/1/' leak transitional X-1
  row two-takeovers/code/mutants.rows shared a.sh 's/one/uno/' leak required X-2
  row two-takeovers/code/mutants.rows shared a.sh 's/one/eins/' leak required X-3
  t transitional-row-with-two-takeovers-is-refused FAIL with_list fixture_run two-takeovers
  fixture_tree unrelated
  mkdir -p "$gate_scratch/unrelated/other"
  printf 'echo x\n' > "$gate_scratch/unrelated/other/x.sh"
  row unrelated/scripts/r1-gate/mutants.rows shared code/a.sh 's/one/1/' leak transitional X-1
  row unrelated/other/mutants.rows shared x.sh 's/x/y/' leak required X-2
  t transitional-row-and-a-row-in-an-unrelated-rows-file-are-refused FAIL with_list fixture_run unrelated
  fixture_tree two-malformed
  printf 'broken\tline\nalso\tbroken\n' >> "$gate_scratch/two-malformed/scripts/r1-gate/mutants.rows"
  t two-malformed-lines-give-the-six-field-fail PASS fixture_says two-malformed 'FAIL mutation:malformed (line 2 of scripts/r1-gate/mutants.rows has no six fields' yes
  fixture_tree linked
  row linked/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  printf 'applies\n' > "$gate_scratch/linked/scripts/r1-gate/mutants.expected"
  row linked/code/real.rows unread a.sh 's/one/1/' leak required X-2
  ln -s real.rows "$gate_scratch/linked/code/mutants.rows"
  t rows-file-that-is-a-symbolic-link-fails-strict FAIL fixture_run linked yes yes
  t rows-file-that-is-a-symbolic-link-is-named PASS with_list fixture_says linked 'FAIL mutation:rows (the tracked path code/mutants.rows is a symbolic link)'
  fixture_tree slashed
  mkdir -p "$gate_scratch/slashed/a\\b"
  row 'slashed/a\b/mutants.rows' other x.sh 's/x/y/' leak required X-2
  t rows-file-whose-path-holds-a-backslash-is-refused PASS with_list fixture_says slashed 'holds a tab, a newline, a backslash or a quote)'
  fixture_tree capital
  mkdir -p "$gate_scratch/capital/other"
  row capital/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  row capital/other/Mutants.rows applies x.sh 's/x/y/' leak required X-2
  t a-file-named-Mutants-rows-is-no-rows-file PASS with_list fixture_run capital
  fixture_tree backslash-name
  row backslash-name/scripts/r1-gate/mutants.rows 'a\b' b.sh 's/two/three/' leak 'retired replaced by the test store::x' X-1
  printf '%s\n' 'a\\b' > "$gate_scratch/backslash-name/scripts/r1-gate/mutants.expected"
  t expected-name-with-two-backslashes-has-no-row-of-one FAIL fixture_run backslash-name yes yes
  fixture_tree budget-strict
  row budget-strict/scripts/r1-gate/mutants.rows first b.sh 's/two/three/' leak required X-1
  row budget-strict/scripts/r1-gate/mutants.rows second b.sh 's/echo two/echo 2/' leak required X-1
  printf 'first\nsecond\n' > "$gate_scratch/budget-strict/scripts/r1-gate/mutants.expected"
  t strict-fails-on-a-row-that-the-budget-left-unrun FAIL with_budget 0 with_only 'first second' fixture_run budget-strict yes yes
  t strict-names-a-row-that-the-budget-left-unrun PASS with_budget 0 with_only 'first second' fixture_says budget-strict 'FAIL mutation:second (strict: the budget left the row unrun)' yes yes
  t the-bound-is-passed-at-it-and-after-it-and-not-before PASS bound_compare
  fixture_tree empty-line
  row empty-line/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  printf 'applies\n\n' > "$gate_scratch/empty-line/scripts/r1-gate/mutants.expected"
  t an-empty-line-of-the-list-is-no-name PASS fixture_run empty-line yes yes
  killed_cases
}

bound_compare() {
  stop_past_bound 59 60 && { printf 'one second under the bound reads as past it\n'; return 1; }
  stop_past_bound 60 60 || { printf 'the bound itself does not read as past it\n'; return 1; }
  stop_past_bound 61 60 || { printf 'one second over the bound does not read as past it\n'; return 1; }
  return 0
}

killed_cases() {
  for killed_fixture in killed-control killed-measured killed-build; do
    fixture_tree "$killed_fixture"
  done
  mkdir -p "$gate_scratch/killed-control/crates/x/src"
  printf 'pub fn f() -> i32 { 1 }\n' > "$gate_scratch/killed-control/crates/x/src/lib.rs"
  cp "$DIR/lib.sh" "$gate_scratch/killed-control/scripts/r1-gate/lib.sh"
  stub_cargo killed-control 0
  stub_killable_store killed-control
  row killed-control/crates/x/mutants.rows held src/lib.rs 's/1 }/2 }/' 'test stand::in' required X-1
  t a-killed-gate-check-of-the-control-pass-is-no-gate-line PASS with_cargo_home killed-control killed_by_case killed-control run 'FAIL step:check-store (ended by signal KILL; the runner got no signal)'
  stub_killable_store killed-measured
  row killed-measured/scripts/r1-gate/mutants.rows held b.sh 's/two/three/' 'measured rows' required X-1
  t a-killed-gate-check-of-a-measured-row-is-no-gate-line PASS killed_by_case killed-measured run 'FAIL step:check-store (ended by signal KILL; the runner got no signal)'
  stub_killable_cc killed-build
  row killed-build/scripts/r1-gate/mutants.rows held x.c 's/1/2/' leak required X-1
  t a-killed-build-of-a-mutant-is-no-build-failure PASS killed_by_case killed-build apply-only 'FAIL step:build of the mutant (ended by signal KILL; the runner got no signal)'
}

stub_killable_store() {
  mkdir -p "$gate_scratch/$1/bin"
  for stub in deletion no-ll-hook; do
    # shellcheck disable=SC2016
    printf '#!/bin/sh\nprintf "%s\\n" >> "${0%%/*}/../../bin/starts"\nprintf "PASS %s\\n"\n' "$stub" "$stub" > "$gate_scratch/$1/scripts/r1-gate/check-$stub.sh"
  done
  cat > "$gate_scratch/$1/scripts/r1-gate/check-store.sh" <<'STUB'
#!/bin/sh
here=${0%/*}/../../bin
printf 'store\n' >> "$here/starts"
printf '%s\n' "$$" > "$here/killme.tmp"
mv "$here/killme.tmp" "$here/killme.pid"
exec sleep 30
STUB
}

stub_killable_cc() {
  mkdir -p "$gate_scratch/$1/bin"
  printf 'int x = 1;\n' > "$gate_scratch/$1/scripts/r1-gate/x.c"
  cat > "$gate_scratch/$1/bin/cc" <<'STUB'
#!/bin/sh
here=${0%/*}
printf '%s\n' "$$" > "$here/killme.tmp"
mv "$here/killme.tmp" "$here/killme.pid"
exec sleep 30
STUB
  chmod +x "$gate_scratch/$1/bin/cc"
}

killed_by_case() {
  killed_fx=$gate_scratch/$1; killed_mode=$2; killed_line=$3
  fixture_commit "$killed_fx"
  perl -e 'setpgrp(0, 0); $SIG{$_} = "DEFAULT" for qw(INT TERM HUP QUIT); exec { $ARGV[0] } @ARGV or exit 127' sh -c 'killed_arg_fixture=$1; killed_arg_mode=$2; set -- --help; . "$0" > /dev/null 2>&1
    PATH=$killed_arg_fixture/bin:$PATH; list=no; apply_only=no; strict=no; only=; budget=
    [ "$killed_arg_mode" = run ] || apply_only=yes
    work=$killed_arg_fixture.work; mkdir -p "$work"; stop_traps; run_rows "$killed_arg_fixture" "$killed_arg_fixture" HEAD' "$DIR/mutation-test.sh" "$killed_fx" "$killed_mode" > "$killed_fx.out" 2>&1 &
  killed_runner=$!
  killed_polls=0
  until [ -s "$killed_fx/bin/killme.pid" ]; do
    killed_polls=$((killed_polls + 1))
    [ "$killed_polls" -lt 300 ] || { kill -s KILL -- "-$killed_runner"; printf 'the stand-in did not start within %s polls of 0.1 s\n' "$killed_polls"; return 1; }
    sleep 0.1
  done
  read -r killed_victim < "$killed_fx/bin/killme.pid"
  kill -s KILL "$killed_victim"
  killed_polls=0
  while kill -0 "$killed_runner" 2> /dev/null; do
    killed_polls=$((killed_polls + 1))
    [ "$killed_polls" -lt 400 ] || { kill -s KILL -- "-$killed_runner"; printf 'the runner did not end within %s polls of 0.1 s\n' "$killed_polls"; return 1; }
    sleep 0.1
  done
  wait "$killed_runner"
  killed_status=$?
  killed_defects=''
  [ "$killed_status" -eq 2 ] || killed_defects="$killed_defects; the runner exited $killed_status, not 2"
  grep -F -x -q -e "$killed_line" "$killed_fx.out" || killed_defects="$killed_defects; the runner printed no line $killed_line"
  ! grep -q '^mutation rows: ' "$killed_fx.out" || killed_defects="$killed_defects; the runner printed its count line"
  ! grep -q '^PASS mutation:' "$killed_fx.out" || killed_defects="$killed_defects; the runner counted a row as caught"
  ! grep -q '^STOPPED ' "$killed_fx.out" || killed_defects="$killed_defects; the runner printed a STOPPED line"
  [ -n "$killed_defects" ] || return 0
  printf '%s\n' "${killed_defects#; }"
  return 1
}

stub_cp() {
  mkdir -p "$gate_scratch/$1/bin"
  cat > "$gate_scratch/$1/bin/cp" <<'STUB'
#!/bin/sh
here=${0%/*}
case $1 in
  *pristine)
    printf 'cp\n' >> "$here/starts"
    : > "$here/marker"
    sleep 2
    mv "$here/marker" "$here/marker.released"
    printf 'itself\n' > "$here/ended" ;;
esac
exec /bin/cp "$@"
STUB
  chmod +x "$gate_scratch/$1/bin/cp"
}

stub_gate_hold() {
  mkdir -p "$gate_scratch/$1/bin"
  for stub in deletion no-ll-hook; do
    # shellcheck disable=SC2016
    printf '#!/bin/sh\nprintf "%s\\n" >> "${0%%/*}/../../bin/starts"\nprintf "PASS %s\\n"\n' "$stub" "$stub" > "$gate_scratch/$1/scripts/r1-gate/check-$stub.sh"
  done
  cat > "$gate_scratch/$1/scripts/r1-gate/check-store.sh" <<'STUB'
#!/bin/sh
here=${0%/*}/../../bin
printf 'store\n' >> "$here/starts"
: > "$here/marker"
sleep 1
mv "$here/marker" "$here/marker.released"
printf 'itself\n' > "$here/ended"
printf 'PASS store\n'
STUB
}

stub_hold() {
  mkdir -p "$gate_scratch/$1/bin"
  stub_hold_real=$(command -v "$2")
  printf '%s\n' "$stub_hold_real" > "$gate_scratch/$1/bin/$2.real"
  [ -z "${3:-}" ] || printf '%s\n' "$3" > "$gate_scratch/$1/bin/$2.when"
  cat > "$gate_scratch/$1/bin/$2" <<'STUB'
#!/bin/sh
here=${0%/*}
tool=${0##*/}
printf '%s %s\n' "$tool" "$*" >> "$here/calls"
read -r real < "$here/$tool.real"
when=''
[ ! -f "$here/$tool.when" ] || read -r when < "$here/$tool.when"
if [ -n "$when" ] && [ ! -e "$here/held" ]; then
  case "$*" in
    $when) printf '%s\n' "$$" > "$here/held"; : > "$here/marker"; sleep 1; mv "$here/marker" "$here/marker.released" ;;
  esac
fi
exec "$real" "$@"
STUB
  chmod +x "$gate_scratch/$1/bin/$2"
}

main_fixture() {
  fixture_tree "$1"
  main_fixture_dir=$gate_scratch/$1
  mkdir -p "$main_fixture_dir/bin" "$main_fixture_dir.tmp" "$main_fixture_dir/scripts/r1-gate/helper"
  cp "$DIR/helper/"* "$main_fixture_dir/scripts/r1-gate/helper/"
  cp "$DIR/lib.sh" "$main_fixture_dir/scripts/r1-gate/lib.sh"
  for main_fixture_tool in git tar cp awk sort perl cmp grep mkdir mktemp; do
    stub_hold "$1" "$main_fixture_tool"
  done
  row "$1/scripts/r1-gate/mutants.rows" applies b.sh 's/two/three/' leak required X-1
  printf 'applies\n' > "$main_fixture_dir/scripts/r1-gate/mutants.expected"
}

main_launch() {
  main_launch_fx=$gate_scratch/$1; shift
  fixture_commit "$main_launch_fx"
  : > "$main_launch_fx/bin/calls"
  perl -e 'setpgrp(0, 0); $SIG{$_} = "DEFAULT" for qw(INT TERM HUP QUIT); exec { $ARGV[0] } @ARGV or exit 127' env GATE_ROOT="$main_launch_fx" CARGO_HOME="$main_launch_fx" TMPDIR="$main_launch_fx.tmp" PATH="$main_launch_fx/bin:$PATH" sh "$DIR/mutation-test.sh" "$@" > "$main_launch_fx.out" 2>&1 &
  main_launch_pid=$!
}

main_ended() {
  main_ended_polls=0
  while kill -0 "$main_launch_pid" 2> /dev/null; do
    main_ended_polls=$((main_ended_polls + 1))
    [ "$main_ended_polls" -lt 600 ] || { kill -s KILL -- "-$main_launch_pid"; printf 'the runner did not end within %s polls of 0.1 s\n' "$main_ended_polls"; return 1; }
    sleep 0.1
  done
  wait "$main_launch_pid"
  main_ended_status=$?
  main_ended_left=$(find "$main_launch_fx.tmp" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
}

main_run() {
  main_run_want=$2; main_run_line=$3; main_run_left=$4; main_run_name=$1; shift 4
  main_launch "$main_run_name" "$@"
  main_ended || return 1
  main_run_defects=''
  [ "$main_ended_status" -eq "$main_run_want" ] || main_run_defects="$main_run_defects; the runner exited $main_ended_status, not $main_run_want"
  [ -z "$main_run_line" ] || grep -F -q -e "$main_run_line" "$main_launch_fx.out" || main_run_defects="$main_run_defects; the runner printed no line with $main_run_line"
  [ "$main_ended_left" -eq "$main_run_left" ] || main_run_defects="$main_run_defects; TMPDIR holds $main_ended_left entries, not $main_run_left"
  [ -n "$main_run_defects" ] || return 0
  printf '%s\n' "${main_run_defects#; }"
  return 1
}

main_stopped_by() {
  main_stop_name=$1; main_stop_signal=$2; main_stop_whom=$3; main_stop_want=$4; main_stop_line=$5; shift 5
  main_launch "$main_stop_name" "$@"
  main_stop_polls=0
  until [ -f "$main_launch_fx/bin/marker" ]; do
    main_stop_polls=$((main_stop_polls + 1))
    [ "$main_stop_polls" -lt 300 ] || { kill -s KILL -- "-$main_launch_pid"; printf 'the held step did not start within %s polls of 0.1 s\n' "$main_stop_polls"; return 1; }
    sleep 0.1
  done
  main_stop_calls=$(wc -l < "$main_launch_fx/bin/calls" | tr -d ' ')
  witness_start
  case $main_stop_whom in
    group) kill -s "$main_stop_signal" -- "-$main_launch_pid" ;;
    step-first) read -r main_stop_step < "$main_launch_fx/bin/held"; kill -s "$main_stop_signal" "$main_stop_step"; sleep 0.5; kill -s "$main_stop_signal" "$main_launch_pid" ;;
    *) kill -s "$main_stop_signal" "$main_launch_pid" ;;
  esac
  main_ended || { witness_end; return 1; }
  sleep 0.2
  witness_end
  main_stop_calls_end=$(wc -l < "$main_launch_fx/bin/calls" | tr -d ' ')
  main_stop_defects=''
  grep -F -x -q -e "$main_stop_line" "$main_launch_fx.out" || main_stop_defects="$main_stop_defects; the runner printed no line $main_stop_line"
  main_stop_count=$(grep -c '^STOPPED ' "$main_launch_fx.out")
  [ "$main_stop_count" -eq 1 ] || main_stop_defects="$main_stop_defects; the runner printed $main_stop_count STOPPED lines"
  [ "$main_ended_status" -eq "$main_stop_want" ] || main_stop_defects="$main_stop_defects; the runner exited $main_ended_status, not $main_stop_want"
  main_stop_fail=$(grep -m 1 -e '^FAIL ' -e '^mutation rows: ' "$main_launch_fx.out")
  [ -z "$main_stop_fail" ] || main_stop_defects="$main_stop_defects; the runner printed $main_stop_fail"
  [ "$main_stop_calls_end" -eq "$main_stop_calls" ] || main_stop_defects="$main_stop_defects; $((main_stop_calls_end - main_stop_calls)) steps started after the signal"
  [ "$main_ended_left" -eq 0 ] || main_stop_defects="$main_stop_defects; TMPDIR holds $main_ended_left entries after the stop"
  [ "$witness_alive" = yes ] || main_stop_defects="$main_stop_defects; the witness process of the case did not outlive the run"
  [ -n "$main_stop_defects" ] || return 0
  printf '%s\n' "${main_stop_defects#; }"
  return 1
}

stop_bound_is_60() {
  [ "$stop_wait_s" -eq 60 ]
}

test_after_a_signal() {
  test_after_dir=$gate_scratch/test-after-a-signal
  mkdir -p "$test_after_dir/work"
  ( work=$test_after_dir/work; tree=$test_after_dir; row_name=held; stop_row=held; stop_signal=TERM; stop_status=143
    # shellcheck disable=SC2016
    test_wait case 30 sh -c ': > "$1/started"' sh "$test_after_dir" ) > "$test_after_dir.out" 2>&1
  test_after_status=$?
  [ "$test_after_status" -eq 143 ] || { printf 'exit %s, not 143\n' "$test_after_status"; return 1; }
  [ ! -e "$test_after_dir/started" ] || { printf 'the test started after the signal\n'; return 1; }
  grep -F -x -q 'STOPPED mutation:held (signal TERM; no test was running)' "$test_after_dir.out" || { printf 'no STOPPED line\n'; return 1; }
}

step_ended_by_a_signal_of_its_own() {
  step_own_out=$gate_scratch/step-ended-by-a-signal.out
  ( stop_row=held; step_check 143 cmp ) > "$step_own_out" 2>&1
  step_own_status=$?
  [ "$step_own_status" -eq 2 ] || { printf 'exit %s, not 2\n' "$step_own_status"; return 1; }
  grep -F -x -q 'FAIL step:cmp (ended by signal TERM; the runner got no signal)' "$step_own_out" || { printf 'no FAIL step line\n'; return 1; }
}

case_args_hold() {
  fixture_run "$1" > /dev/null
  grep -q -e '--remove-scratch' "$gate_scratch/$1/scripts/r1-gate/demo.args"
}

with_cargo_home() {
  CARGO_HOME=$gate_scratch/$1
  export CARGO_HOME
  shift
  "$@"
}

under_ci() {
  CI=true
  "$@"
}

not_ci() {
  CI=''
  "$@"
}

main() {
  mode=run; strict=no; apply_only=no; list=no; only=''; budget=''; main_help=no; main_bad=''
  gate_remove_scratch=yes
  while [ $# -gt 0 ]; do
    case $1 in
      --self-test) mode=self-test ;;
      -v) gate_verbose=yes ;;
      --case) shift; gate_case=${1:-} ;;
      --strict) strict=yes ;;
      --apply-only) apply_only=yes ;;
      --list) list=yes ;;
      --only) shift; [ -n "${1:-}" ] || { main_bad=--only; break; }; only="$only $1" ;;
      --budget) shift; case ${1:-} in ''|*[!0-9]*) main_bad=--budget; break ;; esac; budget=$1 ;;
      --remove-scratch) gate_remove_scratch=yes ;;
      --keep-scratch) gate_remove_scratch=no ;;
      -h|--help) main_help=yes; break ;;
      *) main_bad=$1; break ;;
    esac
    shift
  done
  stop_check
  if [ "$mode" = self-test ] && [ "$main_help" = no ] && [ -z "$main_bad" ]; then
    gate_traps
    selftest_run
    return
  fi
  trap runner_exit EXIT
  if [ "$main_help" = yes ]; then usage; return 0; fi
  case $main_bad in
    '') ;;
    --only|--budget) usage >&2; return 64 ;;
    *) printf 'mutation-test.sh: unknown argument: %s\n' "$main_bad" >&2; usage >&2; return 64 ;;
  esac
  stop_check
  main_base=${TMPDIR:-/tmp}
  gate_scratch=$(mktemp -d "${main_base%/}/r1-gate-$name.XXXXXX")
  step_check $? mktemp || { gate_scratch=''; say FAIL copy "mktemp could not make a scratch directory"; return 2; }
  work=$gate_scratch/work; tree=$gate_scratch/tree
  stop_check
  mkdir "$work" "$tree"
  step_check $? mkdir || return 2
  stop_check
  main_commit=$(git -C "$GATE_ROOT" rev-parse --verify -q 'HEAD^{commit}')
  step_check $? "git rev-parse" || { say FAIL copy "git could not read HEAD"; return 2; }
  stop_check
  git -C "$GATE_ROOT" archive -o "$work/head.tar" "$main_commit"
  step_check $? "git archive" || { say FAIL copy "git archive of HEAD failed"; return 2; }
  stop_check
  tar -xmf "$work/head.tar" -C "$tree"
  step_check $? tar || { say FAIL copy "the archive of HEAD could not be unpacked"; return 2; }
  if [ -d "$GATE_TARGET" ]; then
    stop_check
    mkdir "$tree/target"
    step_check $? mkdir || return 2
    stop_check
    cp -Rpc "$GATE_TARGET" "$tree/target/r1-gate" 2> /dev/null
    if ! step_check $? "copy of the build directory"; then
      stop_check
      cp -Rp "$GATE_TARGET" "$tree/target/r1-gate"
      step_check $? "copy of the build directory" || return 2
    fi
  fi
  run_rows "$tree" "$GATE_ROOT" "$main_commit"
}

main "$@"
