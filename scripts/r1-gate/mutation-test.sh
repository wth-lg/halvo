#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=mutation
TAB=$(printf '\t')

usage() {
  cat <<'EOF'
Mutant runner of the R1 gate. It discovers every mutants.rows file of HEAD and
runs its rows in a throwaway copy of HEAD with its own target directory. The
row grammar is in scripts/r1-gate/README.md.

  sh scripts/r1-gate/mutation-test.sh [--strict] [--only NAME]...
      Build the unmutated copy, require PASS from the store, deletion and
      no-ll-hook checks and one passing test for each test verdict, then apply
      each row, build the mutant and judge it by the row's one verdict source.
      PASS mutation:<name> names that source. A mutant that does not build is
      FAIL mutation:<name> (the mutant does not build), never a caught mutant.
      --strict also fails on a pending or transitional row, on a row for the
      measuring Mac that did not run, and on a name of mutants.expected that no
      row file holds or retires.
  sh scripts/r1-gate/mutation-test.sh --apply-only
      Check that every substitution still applies and that its mutant builds.
  sh scripts/r1-gate/mutation-test.sh --list
      Print the rows: name, state, verdict, row file.
  sh scripts/r1-gate/mutation-test.sh --self-test [-v]
      Prove the classifier and the row rules on fixtures.

Exit status: 0 when no line is FAIL (and, with --strict, nothing is open),
1 otherwise, 2 when git or the copy of HEAD failed, 64 on bad usage.
EOF
}

say() { printf '%s mutation:%s (%s)\n' "$1" "$2" "$3"; }

rows_table() {
  find "$1" -name mutants.rows -type f -not -path '*/target/*' > "$2.files" || return 1
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
  if [ "$row_name" = malformed ]; then
    printf 'line %s of %s has no six fields separated by tabs: name, file, substitution, verdict, state, item\n' "$row_line" "$rows_file"; return 1
  fi
  case $row_file in /*|../*|*/../*|*/..)
    printf 'the file %s is not under the directory of %s\n' "$row_file" "$rows_file"; return 1 ;;
  esac
  case $row_verdict in
    'measured '?*|'refused '?*|leak|'test '?*) ;;
    *) printf 'the verdict %s is none of: measured KEY..., refused TEXT, leak, test NAME\n' "$row_verdict"; return 1 ;;
  esac
  case $row_state in
    required|'required on measuring-mac'|'required on hosted-runner'|'required on test-bed'|transitional|control|'pending '?*|'retired '?*) ;;
    *) printf 'the state %s is none of: required, required on PLACE, transitional, control, pending ITEM, retired REASON\n' "$row_state"; return 1 ;;
  esac
  row_twice=$(awk -F '\t' -v n="$row_name" '$1 == n { print $2 }' "$work/twice.tsv")
  [ -z "$row_twice" ] || { printf 'the name stands in %s\n' "$row_twice"; return 1; }
  return 0
}

names_twice() {
  awk -F '\t' '
    $3 == "malformed" { next }
    { n[$3]++; files[$3] = files[$3] (files[$3] == "" ? "" : " and in ") $1
      if ($7 == "transitional") { beside = $4; sub(/\/[^\/]*$/, "", beside); takeover[$3] = beside "/mutants.rows" } else holder[$3] = $1 }
    END { for (k in n) if (n[k] > 1 && !(n[k] == 2 && (k in takeover) && holder[k] == takeover[k])) print k "\t" files[k] }' "$table" > "$work/twice.tsv"
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
    *) return 0 ;;
  esac
}

gate_lines() {
  : > "$work/gate.out"
  for gate_check in store deletion no-ll-hook; do
    GATE_ROOT=$tree sh "$tree/scripts/r1-gate/check-$gate_check.sh" --measure >> "$work/gate.out" 2> "$work/gate.err"
  done
}

test_run() {
  test_name=$1
  case $test_name in
    selftest:*/*)
      test_check=${test_name#selftest:}; test_case=${test_check#*/}; test_check=${test_check%%/*}
      GATE_ROOT=$tree limited 300 sh "$tree/scripts/r1-gate/check-$test_check.sh" --self-test -v --case "$test_case" > "$work/test.out" 2>&1
      test_status=$?
      [ "$test_status" -ne 124 ] || { printf 'hang\n'; return 0; }
      awk -v want="case $test_check/$test_case" '
        index($0, want " want=") == 1 { n++; split($0, part, " "); verdict = (part[3] == "want=" substr(part[4], 5)) ? "passed" : "failed" }
        END { if (n == 1) print verdict; else print "ran " n + 0 " cases" }' "$work/test.out" ;;
    *)
      test_package=${rows_file#crates/}; test_package=${test_package%%/*}
      case $rows_file in crates/*) ;; *) test_package=halvo ;; esac
      ( cd "$tree" && CARGO_TARGET_DIR="$tree/target/r1-gate" limited 900 cargo test -q --locked -p "$test_package" --lib -- --exact "$test_name" ) > "$work/test.out" 2>&1
      test_status=$?
      [ "$test_status" -ne 124 ] || { printf 'hang\n'; return 0; }
      cargo_test_outcome "$work/test.out" ;;
  esac
}

run_rows() {
  tree=$1
  failures=0; opens=0; transitionals=0; unrun=0; caught=0
  table=$work/rows.tsv
  rows_table "$tree" "$table" || { say FAIL rows "the row files of the copy could not be read"; return 2; }
  if [ ! -s "$table" ]; then say FAIL rows "no mutants.rows file holds a row"; return 1; fi
  names_twice || return 2
  if [ "$list" = yes ]; then
    awk -F '\t' '{ printf "mutation:%s\t%s\t%s\t%s\n", $3, $7, $6, $1 }' "$table"
    return 0
  fi
  for only_name in $only; do
    only_hit=$(awk -F '\t' -v n="$only_name" '$3 == n' "$table")
    [ -n "$only_hit" ] || { say FAIL "$only_name" "--only names no row"; failures=$((failures + 1)); }
  done
  built_unmutated=no
  cp "$table" "$work/rows.loop" || return 2
  while IFS="$TAB" read -r rows_file row_line row_name row_file row_subst row_verdict row_state _ <&3; do
    if ! defect=$(row_defect); then
      say FAIL "$row_name" "$defect"; failures=$((failures + 1)); continue
    fi
    case " $only " in "  "|*" $row_name "*) ;; *) continue ;; esac
    case $row_state in
      'retired '*) say RETIRED "$row_name" "${row_state#retired }"; continue ;;
      'pending '*) say OPEN "$row_name" "${row_state#pending }"; opens=$((opens + 1)); continue ;;
      'required on measuring-mac')
        if [ "${CI:-}" = true ]; then say NOT-RUN "$row_name" "runs on the measuring Mac"; unrun=$((unrun + 1)); continue; fi ;;
      'required on hosted-runner')
        if [ "${CI:-}" != true ]; then say NOT-RUN "$row_name" "needs input; runs on the hosted runner"; continue; fi ;;
      'required on test-bed')
        if [ "${GATE_PLACE:-}" != test-bed ]; then say NOT-RUN "$row_name" "needs input; runs on the test bed"; continue; fi ;;
      transitional)
        takeover=${row_file%/*}/mutants.rows
        if [ -f "$tree/$takeover" ]; then
          taken=$(awk -F '\t' -v n="$row_name" -v f="$takeover" '$1 == f && $3 == n' "$table")
          if [ -n "$taken" ]; then say MOVED "$row_name" "$takeover"; continue; fi
          say FAIL "$row_name" "transitional row not taken over by $takeover"; failures=$((failures + 1)); continue
        fi
        transitionals=$((transitionals + 1)) ;;
    esac
    mutated=$(row_path)
    if [ ! -f "$tree/$mutated" ]; then say FAIL "$row_name" "the file $mutated is not in the copy"; failures=$((failures + 1)); continue; fi
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
        outcome=$(test_run "${row_verdict#test }")
        if [ "$outcome" != passed ]; then
          say FAIL "$row_name" "unmutated, the test ${row_verdict#test } must run as one passing test: $outcome"; failures=$((failures + 1)); continue
        fi ;;
      esac
    fi
    cp "$tree/$mutated" "$work/pristine" || return 2
    if ! perl -0pi -e "$row_subst" "$tree/$mutated" 2> "$work/perl.err"; then
      cp "$work/pristine" "$tree/$mutated"
      say FAIL "$row_name" "the substitution is no valid perl"; failures=$((failures + 1)); continue
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
          outcome=$(test_run "${row_verdict#test }")
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
  missing=0
  if [ -f "$tree/scripts/r1-gate/mutants.expected" ]; then
    while IFS= read -r expected_name; do
      [ -n "$expected_name" ] || continue
      expected_row=$(awk -F '\t' -v n="$expected_name" '$3 == n' "$table")
      [ -z "$expected_row" ] || continue
      missing=$((missing + 1))
      [ "$strict" = no ] || say FAIL "$expected_name" "named in mutants.expected, and no row file holds or retires it"
    done < "$tree/scripts/r1-gate/mutants.expected"
  fi
  printf 'mutation rows: %s caught, %s failed, %s open, %s transitional, %s for the measuring Mac not run, %s expected names with no row\n' "$caught" "$failures" "$opens" "$transitionals" "$unrun" "$missing"
  [ "$failures" -eq 0 ] || return 1
  [ "$strict" = no ] && return 0
  [ "$((opens + transitionals + unrun + missing))" -eq 0 ]
}

fixture_tree() {
  made=$gate_scratch/$1
  mkdir -p "$made/scripts/r1-gate" "$made/code"
  printf 'echo one\n' > "$made/code/a.sh"
  printf 'echo two\n' > "$made/scripts/r1-gate/b.sh"
  : > "$made/scripts/r1-gate/mutants.rows"
}

row() {
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$2" "$3" "$4" "$5" "$6" "$7" >> "$gate_scratch/$1"
}

fixture_run() {
  list=no; apply_only=${2:-no}; strict=${3:-no}; only=''
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
  fixture_tree twice
  row twice/scripts/r1-gate/mutants.rows same b.sh 's/two/2/' leak required X-1
  row twice/code/mutants.rows same a.sh 's/one/1/' leak required X-2
  t name-twice FAIL fixture_run twice yes
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
  fixture_tree retired
  row retired/scripts/r1-gate/mutants.rows gone b.sh 's/two/2/' leak 'retired replaced by the test store::x' X-9
  t retired-row-passes-strict PASS fixture_run retired yes yes
  fixture_tree expected
  row expected/scripts/r1-gate/mutants.rows applies b.sh 's/two/three/' leak required X-1
  printf 'applies\nnever-written\n' > "$gate_scratch/expected/scripts/r1-gate/mutants.expected"
  t expected-name-with-no-row PASS fixture_run expected yes
  t expected-name-with-no-row-fails-strict FAIL fixture_run expected yes yes
  t expected-name-is-named PASS fixture_says expected 'FAIL mutation:never-written (named in mutants.expected' yes yes
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
  t input-row-here-is-not-run-and-no-fail PASS not_ci fixture_run input yes yes
  t input-row-here-is-named PASS not_ci fixture_says input 'NOT-RUN mutation:typed (needs input; runs on the hosted runner)' yes
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
  mode=run; strict=no; apply_only=no; list=no; only=''
  while [ $# -gt 0 ]; do
    case $1 in
      --self-test) mode=self-test ;;
      -v) gate_verbose=yes ;;
      --case) shift; gate_case=${1:-} ;;
      --strict) strict=yes ;;
      --apply-only) apply_only=yes ;;
      --list) list=yes ;;
      --only) shift; [ -n "${1:-}" ] || { usage >&2; return 64; }; only="$only $1" ;;
      --remove-scratch) gate_remove_scratch=yes ;;
      -h|--help) usage; return 0 ;;
      *) printf 'mutation-test.sh: unknown argument: %s\n' "$1" >&2; usage >&2; return 64 ;;
    esac
    shift
  done
  gate_traps
  if [ "$mode" = self-test ]; then selftest_run; return; fi
  scratch_root
  work=$gate_scratch/work; tree=$gate_scratch/tree
  mkdir "$work" "$tree" || return 2
  git -C "$GATE_ROOT" archive -o "$work/head.tar" HEAD || { say FAIL copy "git archive of HEAD failed"; return 2; }
  tar -xf "$work/head.tar" -C "$tree" || { say FAIL copy "the archive of HEAD could not be unpacked"; return 2; }
  if [ -d "$GATE_TARGET" ]; then
    mkdir "$tree/target" || return 2
    cp -Rpc "$GATE_TARGET" "$tree/target/r1-gate" 2> /dev/null || cp -Rp "$GATE_TARGET" "$tree/target/r1-gate" || return 2
  fi
  run_rows "$tree"
}

main "$@"
