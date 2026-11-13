#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=mutation
TAB=$(printf '\t')
stop_wait_s=60
stop_signal=''

usage() {
  cat <<'EOF'
Mutant runner of the R1 gate. It discovers every mutants.rows file of HEAD and
runs its rows in a throwaway copy of HEAD with its own target directory. The
row grammar is in scripts/r1-gate/README.md.

  sh scripts/r1-gate/mutation-test.sh [--strict] [--budget SECONDS] [--only NAME]...
      Build the unmutated copy, require PASS from the store, deletion and
      no-ll-hook checks and one passing test for each test verdict, then apply
      each row, build the mutant and judge it by the row's one verdict source.
      PASS mutation:<name> names that source. A mutant that does not build is
      FAIL mutation:<name> (the mutant does not build), never a caught mutant.
      --strict also fails on a pending or transitional row, on a row for the
      measuring Mac that did not run, on a name of mutants.expected that no
      row file holds or retires, and on a required row, of any place, whose
      name mutants.expected lacks.
      --only runs the named rows in the order given, each name once.
      --budget starts no row once SECONDS have passed since the first row
      started, lets a running row end, prints NOT-RUN mutation:<name> (the
      budget of SECONDS s is used) for each row that it did not start, and
      prints seconds mutation:<name> <n> after each row that it started.
  sh scripts/r1-gate/mutation-test.sh --apply-only
      Check that every substitution still applies and that its mutant builds.
  sh scripts/r1-gate/mutation-test.sh --list
      Print the rows: name, state, verdict, row file.
  Every mode but --self-test first refuses a row name that stands in more than
  one row, unless one of the two is a transitional row and the other the row
  that takes it over, and names each place of the name.
  sh scripts/r1-gate/mutation-test.sh --self-test [-v]
      Prove the classifier and the row rules on fixtures.

TERM, INT or HUP stops a run: the runner starts no new row and no new test,
sends the running test's process group TERM, waits up to 60 s for the test to
end, prints STOPPED mutation:<row> (signal <name>; <what became of the test>)
and exits with 143, 130 or 129. A cargo test runs with the three signals
ignored, so that its case runs to its end and its own guard removes what it
made; a gate self-test case ends its case by its own trap.

Exit status: 0 when no line is FAIL (and, with --strict, nothing is open),
1 otherwise, 2 when git or the copy of HEAD failed, 64 on bad usage, and
143, 130 or 129 after a stop by TERM, INT or HUP.
EOF
}

say() { printf '%s mutation:%s (%s)\n' "$1" "$2" "$3"; }

rows_table() {
  find "$1" -name mutants.rows -type f -not -path "$1/target/*" > "$2.files" || return 1
  LC_ALL=C sort "$2.files" > "$2.sorted" || return 1
  : > "$2"
  while IFS= read -r rows_path; do
    awk -F '\t' -v file="${rows_path#"$1"/}" 'BEGIN { OFS = "\t" }
      /^[[:space:]]*$/ { next }
      { empty = 0; for (i = 1; i <= NF; i++) if ($i == "") empty = 1
        if (NF != 6 || empty) print file, NR, "malformed", "-", "-", "-", "-", "-"
        else print file, NR, $1, $2, $3, $4, $5, $6 }' "$rows_path" >> "$2" || return 1
  done < "$2.sorted"
}

row_path() {
  case $row_state in
    transitional|control) printf '%s\n' "$row_file" ;;
    *) row_dir=${rows_file%/*}
       [ "$row_dir" != "$rows_file" ] || row_dir=.
       printf '%s/%s\n' "$row_dir" "$row_file" ;;
  esac
}

row_defect() {
  if [ "$row_name" = malformed ] && [ "$row_state" = - ]; then
    printf 'line %s of %s has no six fields separated by tabs: name, file, substitution, verdict, state, item\n' "$row_line" "$rows_file"; return 1
  fi
  case $row_file in /*|../*|*/../*|*/..)
    printf 'the file %s is not under the directory of %s\n' "$row_file" "$rows_file"; return 1 ;;
  esac
  case $row_verdict in
    'measured '?*|'refused '?*|leak|'test '?*) ;;
    *) printf 'the verdict %s is none of: measured KEY..., refused TEXT, leak, test NAME\n' "$row_verdict"; return 1 ;;
  esac
  if [ "$row_state" = transitional ] && [ "$rows_file" != scripts/r1-gate/mutants.rows ]; then
    printf 'a transitional row stands in scripts/r1-gate/mutants.rows, not in %s\n' "$rows_file"; return 1
  fi
  case $row_state in
    required|'required on measuring-mac'|'required on hosted-runner'|'required on test-bed'|transitional|control|'pending '?*|'retired '?*) ;;
    *) printf 'the state %s is none of: required, required on PLACE, transitional, control, pending ITEM, retired REASON\n' "$row_state"; return 1 ;;
  esac
  return 0
}

names_twice() {
  awk -F '\t' -v gate=scripts/r1-gate/mutants.rows '
    $3 == "malformed" && $7 == "-" { next }
    { n[$3]++; files[$3] = files[$3] (files[$3] == "" ? "" : " and in ") $1 " line " $2
      if ($7 == "transitional" && $1 == gate) { beside = $4; sub(/\/[^\/]*$/, "", beside); takeover[$3] = beside "/mutants.rows" } else holder[$3] = $1 }
    END { for (k in n) if (n[k] > 1 && !(n[k] == 2 && (k in takeover) && holder[k] == takeover[k] && takeover[k] != gate)) print k "\t" files[k] }' "$table" > "$work/twice.unsorted" || return 2
  LC_ALL=C sort "$work/twice.unsorted" > "$work/twice.tsv" || return 2
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
  GATE_ROOT=$tree sh -c '. "$1/scripts/r1-gate/lib.sh"; gate_build || { printf "%s\n" "$gate_build_error"; exit 1; }' sh "$tree"
}

mutant_builds() {
  if needs_cargo "$1"; then tree_build > "$work/build.out" 2>&1; return; fi
  case $1 in
    *.sh) sh -n "$tree/$1" > "$work/build.out" 2>&1 ;;
    *.c) cc -fsyntax-only "$tree/$1" > "$work/build.out" 2>&1 ;;
    *.pl) perl -c "$tree/$1" > "$work/build.out" 2>&1 ;;
    *.swift) swiftc -parse "$tree/$1" > "$work/build.out" 2>&1 ;;
    *) return 1 ;;
  esac
}

buildable() {
  needs_cargo "$1" && return 0
  case $1 in *.sh|*.c|*.pl|*.swift) return 0 ;; esac
  return 1
}

gate_lines() {
  : > "$work/gate.out"
  { git init -q "$tree" && git -C "$tree" add -A; } > "$work/gate.err" 2>&1 || { printf 'FAIL copy (git could not stage the copy)\n' > "$work/gate.out"; return 1; }
  for gate_check in store deletion no-ll-hook; do
    env GATE_ROOT="$tree" sh "$tree/scripts/r1-gate/check-$gate_check.sh" --measure >> "$work/gate.out" 2> "$work/gate.err"
  done
}

stop_traps() {
  trap 'stop_on TERM 143' TERM
  trap 'stop_on INT 130' INT
  trap 'stop_on HUP 129' HUP
}

stop_on() {
  [ -z "$stop_signal" ] || return 0
  stop_signal=$1; stop_status=$2
}

stop_now() {
  if [ -n "$1" ]; then say STOPPED "$1" "signal $stop_signal; $2"
  else printf 'STOPPED mutation (signal %s; no row had started)\n' "$stop_signal"; fi
  exit "$stop_status"
}

test_ended() {
  case $(ps -o stat= -p "$1" 2> /dev/null) in ''|Z*) return 0 ;; esac
  return 1
}

test_wait() {
  test_kind=$1; test_limit=$2; shift 2
  [ -z "$stop_signal" ] || stop_now "$row_name" "no test was running"
  case $test_kind in
    cargo) ( cd "$tree" && trap '' TERM INT HUP && exec perl "$GATE_LIMIT" "$test_limit" "$@" ) > "$work/test.out" 2>&1 & ;;
    *) ( trap '' TERM INT HUP && exec perl "$GATE_LIMIT" "$test_limit" perl -e '$SIG{$_} = "DEFAULT" for qw(TERM INT HUP); exec { $ARGV[0] } @ARGV or exit 127' "$@" ) > "$work/test.out" 2>&1 & ;;
  esac
  test_pid=$!
  wait "$test_pid"
  test_status=$?
  [ -n "$stop_signal" ] || return 0
  test_group=$(pgrep -P "$test_pid")
  [ -z "$test_group" ] || kill -s TERM -- "-$test_group" 2> /dev/null
  stop_deadline=$(date +%s)
  stop_deadline=$((stop_deadline + stop_wait_s))
  until test_ended "$test_pid"; do
    stop_clock=$(date +%s)
    if [ "$stop_clock" -ge "$stop_deadline" ]; then
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
      test_wait case 300 env GATE_ROOT="$tree" sh "$tree/scripts/r1-gate/check-$test_check.sh" --self-test -v --case "$test_case"
      [ "$test_status" -ne 124 ] || { outcome=hang; return 0; }
      outcome=$(awk -v want="case $test_check/$test_case" '
        index($0, want " want=") == 1 { n++; split($0, part, " "); verdict = (part[3] == "want=" substr(part[4], 5)) ? "passed" : "failed" }
        END { if (n == 1) print verdict; else print "ran " n + 0 " cases" }' "$work/test.out") ;;
    *)
      test_package=${rows_file#crates/}; test_package=${test_package%%/*}
      case $rows_file in crates/*) ;; *) test_package=halvo ;; esac
      test_wait cargo 900 env CARGO_TARGET_DIR="$tree/target/r1-gate" cargo test -q --locked -p "$test_package" --lib -- --exact "$test_name"
      [ "$test_status" -ne 124 ] || { outcome=hang; return 0; }
      outcome=$(cargo_test_outcome "$work/test.out") ;;
  esac
}

row_seconds() {
  [ -n "$timed_name" ] || return 0
  printf 'seconds mutation:%s %s\n' "$timed_name" "$((row_now - timed_start))"
  timed_name=''
}

run_rows() {
  tree=$1
  failures=0; opens=0; transitionals=0; unrun=0; caught=0
  table=$work/rows.tsv
  rows_table "$tree" "$table" || { say FAIL rows "the row files of the copy could not be read"; return 2; }
  if [ ! -s "$table" ]; then say FAIL rows "no mutants.rows file holds a row"; return 1; fi
  names_twice
  case $? in 0) ;; 3) return 1 ;; *) return 2 ;; esac
  if [ "$list" = yes ]; then
    awk -F '\t' '{ printf "mutation:%s\t%s\t%s\t%s\n", $3, $7, $6, $1 }' "$table"
    return 0
  fi
  for only_name in $only; do
    only_hit=$(awk -F '\t' -v n="$only_name" '$3 == n ""' "$table")
    [ -n "$only_hit" ] || { say FAIL "$only_name" "--only names no row"; failures=$((failures + 1)); }
  done
  built_unmutated=no
  if [ -n "$only" ]; then
    : > "$work/rows.loop"
    only_seen=' '
    for only_name in $only; do
      case $only_seen in *" $only_name "*) continue ;; esac
      only_seen="$only_seen$only_name "
      awk -F '\t' -v n="$only_name" '$3 == n ""' "$table" >> "$work/rows.loop" || return 2
    done
    awk -F '\t' -v only=" $only " 'index(only, " " $3 " ") == 0' "$table" >> "$work/rows.loop" || return 2
  else
    cp "$table" "$work/rows.loop" || return 2
  fi
  budget_start=''; timed_name=''; last_row=''
  while IFS="$TAB" read -r rows_file row_line row_name row_file row_subst row_verdict row_state _ <&3; do
    [ -z "$stop_signal" ] || stop_now "$last_row" "no test was running"
    last_row=$row_name
    if ! defect=$(row_defect); then
      say FAIL "$row_name" "$defect"; failures=$((failures + 1)); continue
    fi
    case " $only " in "  "|*" $row_name "*) ;; *) continue ;; esac
    if [ -n "$budget" ]; then
      row_now=$(date +%s)
      row_seconds
      if [ -z "$budget_start" ]; then
        budget_start=$row_now
      elif [ "$((row_now - budget_start))" -ge "$budget" ]; then
        say NOT-RUN "$row_name" "the budget of $budget s is used"; continue
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
          taken=$(awk -F '\t' -v n="$row_name" -v f="$takeover" '$1 == f "" && $3 == n ""' "$table")
          if [ -n "$taken" ]; then say MOVED "$row_name" "$takeover"; continue; fi
          say FAIL "$row_name" "transitional row not taken over by $takeover"; failures=$((failures + 1)); continue
        fi
        transitionals=$((transitionals + 1))
        [ "$strict" = no ] || say FAIL "$row_name" "strict: a transitional row, and no $takeover holds it yet" ;;
    esac
    mutated=$(row_path)
    if [ ! -f "$tree/$mutated" ]; then say FAIL "$row_name" "the file $mutated is not in the copy"; failures=$((failures + 1)); continue; fi
    if ! buildable "$mutated"; then say FAIL "$row_name" "the runner has no build for $mutated: a row mutates a file that cargo builds, or a .sh, .c, .pl or .swift file"; failures=$((failures + 1)); continue; fi
    if needs_cargo "$mutated" && [ "$built_unmutated" = no ]; then
      if ! tree_build > "$work/unmutated.out" 2>&1; then
        first=$(head -1 "$work/unmutated.out")
        printf 'FAIL unmutated:build (%s)\n' "$first"; return 1
      fi
      built_unmutated=yes
      if [ "$apply_only" = no ]; then
        gate_lines
        for gate_check in store deletion no-ll-hook; do
          gate_line=$(grep -E "^[A-Z-]+ $gate_check( |\$)" "$work/gate.out")
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
    cp "$tree/$mutated" "$work/pristine" || return 2
    if ! applied=$(ROW_SUBST=$row_subst perl "$DIR/helper/apply-row.pl" "$tree/$mutated" 2> "$work/perl.err"); then
      cp "$work/pristine" "$tree/$mutated"
      say FAIL "$row_name" "${applied:-the substitution could not be applied}"; failures=$((failures + 1)); continue
    fi
    if cmp -s "$work/pristine" "$tree/$mutated"; then
      say FAIL "$row_name" "the substitution did not apply"; failures=$((failures + 1)); continue
    fi
    if mutant_builds "$mutated"; then builds=yes; else builds=no; fi
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
          if gate_line=$(mutant_verdict "$row_verdict" "$work/gate.out"); then say PASS "$row_name" "$row_verdict: gate said: $gate_line"; caught=$((caught + 1))
          else
            gate_line=$(tr '\n' ' ' < "$work/gate.out")
            say FAIL "$row_name" "$row_verdict not shown; gate said: $gate_line"; failures=$((failures + 1))
          fi ;;
      esac
    fi
    cp "$work/pristine" "$tree/$mutated" || return 2
  done 3< "$work/rows.loop"
  [ -z "$stop_signal" ] || stop_now "$last_row" "no test was running"
  if [ -n "$budget" ]; then
    row_now=$(date +%s)
    row_seconds
  fi
  missing=0; unnamed=0; expected_list=$tree/scripts/r1-gate/mutants.expected
  if [ -f "$expected_list" ]; then
    while IFS= read -r expected_name || [ -n "$expected_name" ]; do
      [ -n "$expected_name" ] || continue
      expected_row=$(awk -F '\t' -v n="$expected_name" '$3 == n ""' "$table")
      [ -z "$expected_row" ] || continue
      missing=$((missing + 1))
      [ "$strict" = no ] || say FAIL "$expected_name" "named in mutants.expected, and no row file holds or retires it"
    done < "$expected_list"
  else
    expected_list=/dev/null
    [ "$strict" = no ] || { say FAIL expected "scripts/r1-gate/mutants.expected is missing, and --strict needs it"; failures=$((failures + 1)); }
  fi
  awk -F '\t' 'FILENAME == ARGV[1] { if ($0 != "") named[$0] = 1; next }
    $7 ~ /^required/ && !($3 in named) { print $3 "\t" $1 }' "$expected_list" "$table" > "$work/unnamed.tsv" || return 2
  while IFS="$TAB" read -r unnamed_name unnamed_file; do
    unnamed=$((unnamed + 1))
    [ "$strict" = no ] || [ "$expected_list" = /dev/null ] || say FAIL "$unnamed_name" "strict: a required row of $unnamed_file, and mutants.expected does not name it"
  done < "$work/unnamed.tsv"
  printf 'mutation rows: %s caught, %s failed, %s open, %s transitional, %s for the measuring Mac not run, %s expected names with no row, %s required rows with no expected name\n' "$caught" "$failures" "$opens" "$transitionals" "$unrun" "$missing" "$unnamed"
  [ "$failures" -eq 0 ] || return 1
  [ "$strict" = no ] && return 0
  [ "$((opens + transitionals + unrun + missing + unnamed))" -eq 0 ]
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

fixture_run() {
  list=${fixture_list:-no}; apply_only=${2:-no}; strict=${3:-no}; only=${fixture_only:-}; budget=${fixture_budget:-}
  work=$gate_scratch/work-$1
  mkdir -p "$work"
  run_rows "$gate_scratch/$1"
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
if [ "$hold" -gt 0 ]; then
  : > "$here/marker"
  sleep "$hold"
  mv "$here/marker" "$here/marker.released"
fi
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

stopped_by() {
  stop_fixture=$gate_scratch/$1; stop_signal_sent=$2; stop_whom=$3; stop_bound=$4; stop_mode=$5
  stop_status_wanted=$6; stop_marker_wanted=$7; stop_ended_wanted=$8; stop_starts_wanted=$9; shift 9; stop_line_wanted=$1
  perl -e 'setpgrp(0, 0); exec { $ARGV[0] } @ARGV or exit 127' sh -c 'stop_arg_fixture=$1; stop_arg_bound=$2; stop_arg_mode=$3; set -- --self-test --case no-such-case; . "$0" > /dev/null 2>&1
    PATH=$stop_arg_fixture/bin:$PATH; stop_wait_s=$stop_arg_bound; list=no; apply_only=no; strict=no; only=; budget=
    [ "$stop_arg_mode" = run ] || apply_only=yes
    work=$stop_arg_fixture.work; mkdir -p "$work"; stop_traps; run_rows "$stop_arg_fixture"' "$DIR/mutation-test.sh" "$stop_fixture" "$stop_bound" "$stop_mode" > "$stop_fixture.out" 2>&1 &
  stop_pid=$!
  stop_polls=0
  until [ -f "$stop_fixture/bin/marker" ]; do
    stop_polls=$((stop_polls + 1))
    [ "$stop_polls" -lt 300 ] || { kill -s KILL -- "-$stop_pid"; printf 'the stand-in did not start within %s polls of 0.1 s\n' "$stop_polls"; return 1; }
    sleep 0.1
  done
  case $stop_whom in
    tree) for stop_child in $(descendants_of "$stop_pid"); do kill -s "$stop_signal_sent" "$stop_child" 2> /dev/null; done
      kill -s "$stop_signal_sent" "$stop_pid" ;;
    group) kill -s "$stop_signal_sent" -- "-$stop_pid" ;;
    twice) kill -s "$stop_signal_sent" "$stop_pid"; sleep 0.5; kill -s TERM "$stop_pid" ;;
    *) kill -s "$stop_signal_sent" "$stop_pid" ;;
  esac
  stop_polls=0
  while kill -0 "$stop_pid" 2> /dev/null; do
    stop_polls=$((stop_polls + 1))
    [ "$stop_polls" -lt 400 ] || { kill -s KILL -- "-$stop_pid"; printf 'the runner did not end within %s polls of 0.1 s after the signal\n' "$stop_polls"; return 1; }
    sleep 0.1
  done
  wait "$stop_pid"
  stop_status=$?
  sleep 0.2
  stop_starts=$(wc -l < "$stop_fixture/bin/starts" | tr -d ' ')
  stop_left=$(pgrep -f "$stop_fixture/" | wc -l | tr -d ' ')
  stop_marker=gone
  [ ! -e "$stop_fixture/bin/marker" ] || stop_marker=left
  stop_ended=none
  [ ! -f "$stop_fixture/bin/ended" ] || read -r stop_ended < "$stop_fixture/bin/ended"
  stop_defects=''
  grep -F -x -q -e "$stop_line_wanted" "$stop_fixture.out" || stop_defects="$stop_defects; the runner printed no line $stop_line_wanted"
  [ "$stop_status" -eq "$stop_status_wanted" ] || stop_defects="$stop_defects; the runner exited $stop_status, not $stop_status_wanted"
  [ "$stop_marker" = "$stop_marker_wanted" ] || stop_defects="$stop_defects; the marker of the stand-in is $stop_marker, not $stop_marker_wanted"
  [ "$stop_ended" = "$stop_ended_wanted" ] || stop_defects="$stop_defects; the stand-in ended by $stop_ended, not by $stop_ended_wanted"
  [ "$stop_starts" -eq "$stop_starts_wanted" ] || stop_defects="$stop_defects; the stand-ins started $stop_starts times, not $stop_starts_wanted"
  [ "$stop_left" -eq 0 ] || stop_defects="$stop_defects; $stop_left processes of the stand-ins are left"
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
  mode=run; strict=no; apply_only=no; list=no; only=''; budget=''
  while [ $# -gt 0 ]; do
    case $1 in
      --self-test) mode=self-test ;;
      -v) gate_verbose=yes ;;
      --case) shift; gate_case=${1:-} ;;
      --strict) strict=yes ;;
      --apply-only) apply_only=yes ;;
      --list) list=yes ;;
      --only) shift; [ -n "${1:-}" ] || { usage >&2; return 64; }; only="$only $1" ;;
      --budget) shift; case ${1:-} in ''|*[!0-9]*) usage >&2; return 64 ;; esac; budget=$1 ;;
      --remove-scratch) gate_remove_scratch=yes ;;
      -h|--help) usage; return 0 ;;
      *) printf 'mutation-test.sh: unknown argument: %s\n' "$1" >&2; usage >&2; return 64 ;;
    esac
    shift
  done
  gate_traps
  if [ "$mode" = self-test ]; then selftest_run; return; fi
  stop_traps
  scratch_root
  work=$gate_scratch/work; tree=$gate_scratch/tree
  mkdir "$work" "$tree" || return 2
  git -C "$GATE_ROOT" archive -o "$work/head.tar" HEAD || { say FAIL copy "git archive of HEAD failed"; return 2; }
  tar -xmf "$work/head.tar" -C "$tree" || { say FAIL copy "the archive of HEAD could not be unpacked"; return 2; }
  if [ -d "$GATE_TARGET" ]; then
    mkdir "$tree/target" || return 2
    cp -Rpc "$GATE_TARGET" "$tree/target/r1-gate" 2> /dev/null || cp -Rp "$GATE_TARGET" "$tree/target/r1-gate" || return 2
  fi
  run_rows "$tree"
}

main "$@"
