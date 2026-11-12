#!/bin/sh
# jobs: windows
# rule: The tests of halvo-schema-core and halvo-schema pass, each test that scripts/r2/20-schema-tests.txt lists passes under its test target and stands, not ignored, in the list of the test binary that the test run built, and export-names writes on two runs the names file that the crate commits as tests/golden/names.txt.
set -u
name=20-schema
golden=crates/halvo-schema/tests/golden/names.txt
required=scripts/r2/20-schema-tests.txt
work=
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

clean_up() {
  [ -z "$work" ] || rm -rf "$work"
}

trap clean_up EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || fail "no repository"
  fi
  cd "$repo" || fail "the repository root cannot be entered"
}

required_gap() {
  awk -v pkg="$1" -v list="$required" '
    { sub(/\r$/, "") }
    FILENAME == list {
      if (NF == 0) next
      if (NF != 3) {
        if (bad == "") bad = "the line " FNR " of " list " is not <package> <test target> <test name>"
        next
      }
      if ($1 == pkg) { n++; target[n] = $2; test[n] = $3 }
      next
    }
    $1 == "Running" || $1 == "Doc-tests" {
      at = ""
      if ($1 == "Running") { at = $2; gsub(/\\/, "/", at); ran[at] = 1 }
      next
    }
    at != "" && /^test .* \.\.\. ok$/ { ok[at, substr($0, 6, length($0) - 12)] = 1 }
    END {
      if (bad != "") { print bad; exit }
      if (n == 0) { print "the list " list " names no test of " pkg; exit }
      for (i = 1; i <= n; i++) {
        if (!(target[i] in ran)) { print "the test target " target[i] " of " pkg " did not run"; exit }
        if (!((target[i], test[i]) in ok)) { print "the test " test[i] " of " target[i] " of " pkg " did not pass"; exit }
      }
    }
  ' "$required" "$2"
}

required_targets() {
  awk -v pkg="$1" '
    NF == 3 && $1 == pkg && !seen[$2]++ { print $2 }
  ' "$required"
}

listing() {
  if [ "$4" = ignored ]; then
    call="cargo test --locked -p $1 --test $3 -- --list --ignored"
    cargo test --locked -p "$1" --test "$3" -- --list --ignored >"$5.txt" 2>"$5.err"
  else
    call="cargo test --locked -p $1 --test $3 -- --list"
    cargo test --locked -p "$1" --test "$3" -- --list >"$5.txt" 2>"$5.err"
  fi
  code=$?
  [ "$code" -eq 0 ] || fail "$call exited $code"
  gap=$(
    awk -v target="$2" -v call="$call" '
      $1 == "Compiling" { compiled = 1 }
      $1 == "Running" { at = $2; gsub(/\\/, "/", at); if (at == target) ran = 1 }
      END {
        if (!ran) print call " did not run " target
        else if (compiled) print call " compiled, so the test run did not build " target
      }
    ' "$5.err"
  ) || fail "the cargo lines of $call cannot be read"
  [ -z "$gap" ] || fail "$gap"
}

listed_gap() {
  awk -v pkg="$1" -v target="$2" -v list="$required" -v all="$3" '
    { sub(/\r$/, "") }
    FILENAME == list { if (NF == 3 && $1 == pkg && $2 == target) { n++; test[n] = $3 }; next }
    FILENAME == all { listed[$0] = 1; next }
    { ignored[$0] = 1 }
    END {
      for (i = 1; i <= n; i++) {
        if (!((test[i] ": test") in listed)) { print "the test " test[i] " of " target " of " pkg " is not in the list of its test binary"; exit }
        if ((test[i] ": test") in ignored) { print "the test " test[i] " of " target " of " pkg " is ignored"; exit }
      }
      print "listed " n
    }
  ' "$required" "$3" "$4"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  [ -f "$required" ] || fail "$required is missing"

  for package in halvo-schema-core halvo-schema; do
    cargo test --locked -p "$package" >"$artifacts/cargo-test-$package.txt" 2>&1
    code=$?
    [ "$code" -eq 0 ] || fail "cargo test --locked -p $package exited $code"
    passed=$(grep -c '\.\.\. ok' "$artifacts/cargo-test-$package.txt")
    printf 'cargo test --locked -p %s: tests passed %s\n' "$package" "$passed"
    gap=$(required_gap "$package" "$artifacts/cargo-test-$package.txt") || fail "the log of $package cannot be read"
    [ -z "$gap" ] || fail "$gap"
    targets=$(required_targets "$package") || fail "the list $required cannot be read"
    for target in $targets; do
      test_name=${target##*/}
      test_name=${test_name%.rs}
      at=$artifacts/list-$package-$test_name
      listing "$package" "$target" "$test_name" all "$at"
      listing "$package" "$target" "$test_name" ignored "$at-ignored"
      result=$(listed_gap "$package" "$target" "$at.txt" "$at-ignored.txt") || fail "the lists of $target of $package cannot be read"
      case $result in
        "listed "*) printf 'cargo test --locked -p %s --test %s -- --list: required names listed %s, ignored 0\n' "$package" "$test_name" "${result#listed }" ;;
        *) fail "$result" ;;
      esac
    done
  done
  listed=$(grep -c '[^[:space:]]' "$required")
  printf 'required tests ok=%s\n' "$listed"

  [ -f "$golden" ] || fail "$golden is missing"
  lines=$(grep -c '' "$golden")
  for run in 1 2; do
    cargo run -q --locked -p halvo-schema --bin export-names -- --out "$work/names.$run.txt" \
      >"$artifacts/export-names.$run.out" 2>"$artifacts/export-names.$run.err"
    code=$?
    [ "$code" -eq 0 ] || fail "export-names run $run exited $code"
    printed=$(cat "$artifacts/export-names.$run.out") || fail "the output of export-names run $run cannot be read"
    [ "$printed" = "names $lines" ] || fail "export-names run $run did not print the one line names $lines"
    cmp -s "$work/names.$run.txt" "$golden" ||
      fail "the names file of export-names run $run differs from $golden"
  done
  printf 'export-names runs=2 lines=%s same_as_golden=yes same_on_second_run=yes\n' "$lines"
  printf 'PASS %s\n' "$name"
}

note() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

check() {
  : >"$work/runs"
  mkdir "$work/tmp/$1" || self_fail "the temporary directory of the fixture $1 cannot be made"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_ARTIFACTS=${art:-$work/artifacts/$1}
    STUB_RUNS=$work/runs
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_RUNS STUB_CORE_LOG STUB_SCHEMA_LOG STUB_CORE_EXIT STUB_TEST_EXIT STUB_RUN_EXIT STUB_RUN2_EXIT STUB_OUT STUB_FIRST STUB_SECOND STUB_LISTS STUB_SIGNAL
    TMPDIR=$work/tmp/$1 sh "$self" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  left=$(ls -A "$work/tmp/$1") || self_fail "the temporary directory of the fixture $1 cannot be read"
  [ -z "$left" ] || got=$got-and-a-temporary-directory-left
  note "$1" "$2" "$got"
  STUB_LISTS=$work/lists
  STUB_SIGNAL=
  STUB_CORE_LOG=$work/core.log
  STUB_SCHEMA_LOG=$work/schema.log
  STUB_CORE_EXIT=0
  STUB_TEST_EXIT=0
  STUB_RUN_EXIT=0
  STUB_RUN2_EXIT=0
  STUB_OUT='names 3'
  STUB_FIRST=$names
  STUB_SECOND=$names
  art=
}

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

make_repo() {
  mkdir -p "$1/crates/halvo-schema/tests/golden" "$1/scripts/r2" || self_fail "the fixture directories cannot be made"
  [ "$2" = no-golden ] || printf '%s\n' "$names" >"$1/$golden" || self_fail "the fixture names file cannot be written"
  [ "$3" = none ] || printf '%s' "$3" >"$1/$required" || self_fail "the fixture list cannot be written"
}

log() {
  printf '%b' "$2" >"$work/$1.log" || self_fail "the fixture log $1 cannot be written"
}

stub_listing() {
  printf '%s\n' "$5" >"$1/$2-$3-$4.code" || self_fail "the fixture listing $2 $3 $4 cannot be written"
  printf '%b' "$6" >"$1/$2-$3-$4.err" || self_fail "the fixture listing $2 $3 $4 cannot be written"
  printf '%b' "$7" >"$1/$2-$3-$4.txt" || self_fail "the fixture listing $2 $3 $4 cannot be written"
}

lists() {
  mkdir -p "$1" || self_fail "the fixture lists $1 cannot be made"
  e=$2
  s=$3
  done_line="    Finished \`test\` profile [unoptimized + debuginfo] target(s) in 0.05s$e"
  at_notice="$done_line     Running tests${s}notice.rs (target${s}debug${s}deps${s}notice-1$4)$e"
  at_lock="$done_line     Running tests${s}locked_files.rs (target${s}debug${s}deps${s}locked_files-1$4)$e"
  at_later="$done_line     Running tests${s}later_file_builds.rs (target${s}debug${s}deps${s}later_file_builds-1$4)$e"
  stub_listing "$1" halvo-schema-core notice all 0 "$at_notice" "the_notice_is_the_one_row_of_the_public_suffix_list: test${e}the_notice_the_manifest_and_the_lock_name_one_version_of_psl: test$e${e}2 tests, 0 benchmarks$e"
  stub_listing "$1" halvo-schema-core notice ignored 0 "$at_notice" "0 tests, 0 benchmarks$e"
  stub_listing "$1" halvo-schema locked_files all 0 "$at_lock" "both_manifests_are_the_fixed_files: test${e}each_lib_rs_holds_its_form: test${e}the_slow_one: test$e${e}3 tests, 0 benchmarks$e"
  stub_listing "$1" halvo-schema locked_files ignored 0 "$at_lock" "the_slow_one: test$e${e}1 test, 0 benchmarks$e"
  stub_listing "$1" halvo-schema later_file_builds all 0 "$at_later" "a_later_file_that_gives_a_core_type_code_of_its_own_does_not_compile: test$e${e}1 test, 0 benchmarks$e"
  stub_listing "$1" halvo-schema later_file_builds ignored 0 "$at_later" "0 tests, 0 benchmarks$e"
}

lists_of() {
  cp -R "$work/lists" "$work/lists-$1" || self_fail "the fixture lists of $1 cannot be made"
  STUB_LISTS=$work/lists-$1
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  repo=$work/repo
  mkdir -p "$work/bin" "$work/artifacts" "$work/tmp" || self_fail "the fixture directories cannot be made"
  lists "$work/lists" '\n' / ''
  lists "$work/lists-windows" '\r\n' "\\\\" .exe
  STUB_LISTS=$work/lists
  STUB_SIGNAL=
  names='accessibility_text
active
address_field'
  core_line='halvo-schema-core tests/notice.rs the_notice_is_the_one_row_of_the_public_suffix_list'
  lock_line='halvo-schema tests/locked_files.rs each_lib_rs_holds_its_form'
  later_line='halvo-schema tests/later_file_builds.rs a_later_file_that_gives_a_core_type_code_of_its_own_does_not_compile'
  list="$core_line
$lock_line
$later_line
"
  make_repo "$repo" golden "$list"
  unit='     Running unittests src/lib.rs (target/debug/deps/halvo_schema-1)\ntest golden_names ... ok\n'
  lock='     Running tests/locked_files.rs (target/debug/deps/locked_files-1)\n'
  later='     Running tests/later_file_builds.rs (target/debug/deps/later_file_builds-1)\ntest a_later_file_that_gives_a_core_type_code_of_its_own_does_not_compile ... ok\n'
  docs='   Doc-tests halvo_schema\n'
  log core '     Running unittests src/lib.rs (target/debug/deps/halvo_schema_core-1)\ntest ids::tests::shown ... ok\n     Running tests/notice.rs (target/debug/deps/notice-1)\ntest the_notice_is_the_one_row_of_the_public_suffix_list ... ok\n   Doc-tests halvo_schema_core\n'
  log schema "$unit${lock}test each_lib_rs_holds_its_form ... ok\n$later$docs"
  log core-no-targets '     Running unittests src/lib.rs (target/debug/deps/halvo_schema_core-1)\ntest ids::tests::shown ... ok\n   Doc-tests halvo_schema_core\n'
  log no-targets "$unit$docs"
  log ignored "$unit${lock}test each_lib_rs_holds_its_form ... ignored\n$later$docs"
  log other-file "$unit     Running tests/other.rs (target/debug/deps/locked_files-1)\ntest each_lib_rs_holds_its_form ... ok\n$later$docs"
  log other-target "$unit${lock}test both_manifests_are_the_fixed_files ... ok\n${later}test each_lib_rs_holds_its_form ... ok\n$docs"
  log longer-name "$unit${lock}test each_lib_rs_holds_its_form_twin ... ok\n$later$docs"
  log indented "$unit${lock} test each_lib_rs_holds_its_form ... ok\n$later$docs"
  forged="$unit     Running tests/coarse.rs (target/debug/deps/coarse-1)\n     Running tests/locked_files.rs (target/debug/deps/locked_files-0)\ntest each_lib_rs_holds_its_form ... ok\ntest forged_from_another_target ... ok\n"
  log forged "$forged$later$docs"
  log empty-target "$forged$later${lock}$docs"
  log forged-ignored "$forged$later${lock}test each_lib_rs_holds_its_form ... ignored\n$docs"
  log two-names "$unit${lock}test both_manifests_are_the_fixed_files ... ok\ntest each_lib_rs_holds_its_form ... ok\n$later$docs"
  log both-packages "$unit     Running tests/notice.rs (target/debug/deps/notice-2)\ntest the_schema_notice_holds ... ok\n$later$docs"
  log windows-core '     Running unittests src\\lib.rs (target\\debug\\deps\\halvo_schema_core-1.exe)\r\n     Running tests\\notice.rs (target\\debug\\deps\\notice-1.exe)\r\ntest the_notice_is_the_one_row_of_the_public_suffix_list ... ok\r\n'
  log windows-schema '     Running tests\\locked_files.rs (target\\debug\\deps\\locked_files-1.exe)\r\ntest each_lib_rs_holds_its_form ... ok\r\n     Running tests\\later_file_builds.rs (target\\debug\\deps\\later_file_builds-1.exe)\r\ntest a_later_file_that_gives_a_core_type_code_of_its_own_does_not_compile ... ok\r\n'
  STUB_CORE_LOG=$work/core.log
  STUB_SCHEMA_LOG=$work/schema.log
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-schema-core')
    cat "$STUB_CORE_LOG"
    exit "$STUB_CORE_EXIT"
    ;;
  'test --locked -p halvo-schema')
    cat "$STUB_SCHEMA_LOG"
    [ -z "$STUB_SIGNAL" ] || kill -s "$STUB_SIGNAL" "$PPID"
    exit "$STUB_TEST_EXIT"
    ;;
  'test --locked -p '*' --test '*' -- --list' | 'test --locked -p '*' --test '*' -- --list --ignored')
    kind=all
    [ "$#" -eq 8 ] || kind=ignored
    [ "$#" -eq 8 ] || [ "$#" -eq 9 ] || exit 97
    listing=$STUB_LISTS/$4-$6-$kind
    [ -f "$listing.code" ] || exit 97
    cat "$listing.err" >&2
    cat "$listing.txt"
    exit "$(cat "$listing.code")"
    ;;
  'run -q --locked -p halvo-schema --bin export-names -- --out '*)
    shift 9
    [ "$#" -eq 1 ] || exit 97
    printf 'x\n' >>"$STUB_RUNS"
    if [ "$(grep -c '' "$STUB_RUNS")" -eq 1 ]; then
      content=$STUB_FIRST
      status=$STUB_RUN_EXIT
    else
      content=$STUB_SECOND
      status=$STUB_RUN2_EXIT
    fi
    [ "$content" = none ] || printf '%s\n' "$content" >"$1"
    [ -z "$STUB_OUT" ] || printf '%s\n' "$STUB_OUT"
    exit "$status"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo" || self_fail "the stub cargo cannot be made executable"
  rejected=0
  wrong=
  STUB_CORE_EXIT=0
  STUB_TEST_EXIT=0
  STUB_RUN_EXIT=0
  STUB_RUN2_EXIT=0
  STUB_OUT='names 3'
  STUB_FIRST=$names
  STUB_SECOND=$names

  check all-hold PASS "cargo test --locked -p halvo-schema-core: tests passed 2
cargo test --locked -p halvo-schema-core --test notice -- --list: required names listed 1, ignored 0
cargo test --locked -p halvo-schema: tests passed 3
cargo test --locked -p halvo-schema --test locked_files -- --list: required names listed 1, ignored 0
cargo test --locked -p halvo-schema --test later_file_builds -- --list: required names listed 1, ignored 0
required tests ok=3
export-names runs=2 lines=3 same_as_golden=yes same_on_second_run=yes
PASS $name"
  STUB_CORE_EXIT=101
  check core-tests-fail FAIL "FAIL $name (cargo test --locked -p halvo-schema-core exited 101)"
  STUB_TEST_EXIT=101
  check tests-fail FAIL "FAIL $name (cargo test --locked -p halvo-schema exited 101)"
  STUB_RUN_EXIT=2
  STUB_OUT=
  check export-names-refuses FAIL "FAIL $name (export-names run 1 exited 2)"
  STUB_RUN2_EXIT=2
  check second-run-refuses FAIL "FAIL $name (export-names run 2 exited 2)"
  STUB_FIRST='accessibility_text
active'
  check file-lacks-a-name FAIL "FAIL $name (the names file of export-names run 1 differs from $golden)"
  STUB_SECOND='accessibility_text
address_field
active'
  check second-run-differs FAIL "FAIL $name (the names file of export-names run 2 differs from $golden)"
  STUB_FIRST=none
  check no-file-written FAIL "FAIL $name (the names file of export-names run 1 differs from $golden)"
  STUB_OUT='names 2'
  check count-differs FAIL "FAIL $name (export-names run 1 did not print the one line names 3)"
  STUB_OUT='names 3
names 3'
  check two-lines-printed FAIL "FAIL $name (export-names run 1 did not print the one line names 3)"
  STUB_OUT=
  check nothing-printed FAIL "FAIL $name (export-names run 1 did not print the one line names 3)"

  STUB_SCHEMA_LOG=$work/no-targets.log
  check targets-not-built FAIL "FAIL $name (the test target tests/locked_files.rs of halvo-schema did not run)"
  STUB_CORE_LOG=$work/core-no-targets.log
  check core-targets-not-built FAIL "FAIL $name (the test target tests/notice.rs of halvo-schema-core did not run)"
  STUB_SCHEMA_LOG=$work/ignored.log
  check ignored FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema did not pass)"
  STUB_SCHEMA_LOG=$work/other-file.log
  check name-given-to-another-file FAIL "FAIL $name (the test target tests/locked_files.rs of halvo-schema did not run)"
  STUB_SCHEMA_LOG=$work/other-target.log
  check ok-under-another-target FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema did not pass)"
  STUB_SCHEMA_LOG=$work/longer-name.log
  check longer-name FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema did not pass)"
  STUB_SCHEMA_LOG=$work/indented.log
  check not-a-whole-line FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema did not pass)"

  lists_of w1
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 0 "$lock" 'forged: test\n\n1 test, 0 benchmarks\n'
  check w1-a-test-writes-the-ok-lines FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of w2
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 0 "$lock" '0 tests, 0 benchmarks\n'
  STUB_SCHEMA_LOG=$work/empty-target.log
  check w2-an-empty-target-and-lines-from-another FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of ignored
  stub_listing "$STUB_LISTS" halvo-schema locked_files ignored 0 "$lock" 'each_lib_rs_holds_its_form: test\nthe_slow_one: test\n\n2 tests, 0 benchmarks\n'
  STUB_SCHEMA_LOG=$work/forged-ignored.log
  check required-name-ignored FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is ignored)"
  lists_of not-built
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 0 "   Compiling halvo-schema v0.0.0 (crates/halvo-schema)\n$lock" 'both_manifests_are_the_fixed_files: test\neach_lib_rs_holds_its_form: test\n\n2 tests, 0 benchmarks\n'
  STUB_SCHEMA_LOG=$work/forged.log
  check target-the-run-did-not-build FAIL "FAIL $name (cargo test --locked -p halvo-schema --test locked_files -- --list compiled, so the test run did not build tests/locked_files.rs)"
  lists_of no-target
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 101 "error: no test target named \`locked_files\` in \`halvo-schema\` package\n" ''
  STUB_SCHEMA_LOG=$work/forged.log
  check listing-refused FAIL "FAIL $name (cargo test --locked -p halvo-schema --test locked_files -- --list exited 101)"
  lists_of ignored-refused
  stub_listing "$STUB_LISTS" halvo-schema later_file_builds ignored 2 "$later" ''
  check ignored-listing-refused FAIL "FAIL $name (cargo test --locked -p halvo-schema --test later_file_builds -- --list --ignored exited 2)"
  lists_of other-file
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 0 '     Running tests/coarse.rs (target/debug/deps/locked_files-1)\n' 'both_manifests_are_the_fixed_files: test\neach_lib_rs_holds_its_form: test\n\n2 tests, 0 benchmarks\n'
  check listing-of-another-file FAIL "FAIL $name (cargo test --locked -p halvo-schema --test locked_files -- --list did not run tests/locked_files.rs)"
  lists_of quiet
  stub_listing "$STUB_LISTS" halvo-schema later_file_builds ignored 0 '' '0 tests, 0 benchmarks\n'
  check listing-without-cargo-lines FAIL "FAIL $name (cargo test --locked -p halvo-schema --test later_file_builds -- --list --ignored did not run tests/later_file_builds.rs)"
  lists_of prefix
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 0 "$lock" 'each_lib_rs_holds_its_form_twin: test\n\n1 test, 0 benchmarks\n'
  check listed-name-longer FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of module
  stub_listing "$STUB_LISTS" halvo-schema locked_files all 0 "$lock" 'tests::each_lib_rs_holds_its_form: test\n\n1 test, 0 benchmarks\n'
  check listed-name-in-a-module FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of core
  stub_listing "$STUB_LISTS" halvo-schema-core notice all 0 "     Running tests/notice.rs (target/debug/deps/notice-1)\n" 'the_notice_the_manifest_and_the_lock_name_one_version_of_psl: test\n\n1 test, 0 benchmarks\n'
  check core-name-not-listed FAIL "FAIL $name (the test the_notice_is_the_one_row_of_the_public_suffix_list of tests/notice.rs of halvo-schema-core is not in the list of its test binary)"
  STUB_LISTS=$work/lists-windows
  stub_listing "$STUB_LISTS" halvo-schema locked_files ignored 0 '     Running tests\\locked_files.rs (target\\debug\\deps\\locked_files-1.exe)\r\n' 'each_lib_rs_holds_its_form: test\r\n\r\n1 test, 0 benchmarks\r\n'
  STUB_SCHEMA_LOG=$work/forged-ignored.log
  check windows-ignored FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is ignored)"
  lists "$work/lists-windows" '\r\n' "\\\\" .exe
  for signal in HUP INT TERM; do
    STUB_SIGNAL=$signal
    case $signal in
      HUP) want=exit-129 ;;
      INT) want=exit-130 ;;
      TERM) want=exit-143 ;;
    esac
    check "signal-$signal" "$want" "cargo test --locked -p halvo-schema-core: tests passed 2"
  done
  repo=$work/crlf
  cr=$(printf '\r')
  make_repo "$repo" golden "$core_line$cr
$lock_line$cr
$later_line$cr
"
  STUB_CORE_LOG=$work/windows-core.log
  STUB_SCHEMA_LOG=$work/windows-schema.log
  STUB_LISTS=$work/lists-windows
  check windows-form PASS "required tests ok=3"
  repo=$work/two-names
  make_repo "$repo" golden "$core_line
$lock_line
halvo-schema tests/locked_files.rs both_manifests_are_the_fixed_files
$later_line
"
  STUB_SCHEMA_LOG=$work/two-names.log
  check two-names-of-one-target PASS "cargo test --locked -p halvo-schema --test locked_files -- --list: required names listed 2, ignored 0
cargo test --locked -p halvo-schema --test later_file_builds -- --list: required names listed 1, ignored 0
required tests ok=4"
  repo=$work/both-packages
  make_repo "$repo" golden "$core_line
halvo-schema tests/notice.rs the_schema_notice_holds
$later_line
"
  lists_of both-packages
  stub_listing "$STUB_LISTS" halvo-schema notice all 0 "     Running tests/notice.rs (target/debug/deps/notice-2)\n" 'the_schema_notice_holds: test\n\n1 test, 0 benchmarks\n'
  stub_listing "$STUB_LISTS" halvo-schema notice ignored 0 "     Running tests/notice.rs (target/debug/deps/notice-2)\n" '0 tests, 0 benchmarks\n'
  STUB_SCHEMA_LOG=$work/both-packages.log
  check a-target-path-in-both-packages PASS "cargo test --locked -p halvo-schema --test notice -- --list: required names listed 1, ignored 0"
  repo=$work/no-core
  make_repo "$repo" golden "$lock_line
$later_line
"
  check no-test-of-a-package FAIL "FAIL $name (the list $required names no test of halvo-schema-core)"
  repo=$work/two-fields
  make_repo "$repo" golden "$core_line
halvo-schema each_lib_rs_holds_its_form
"
  check list-line-out-of-form FAIL "FAIL $name (the line 2 of $required is not <package> <test target> <test name>)"
  repo=$work/no-list
  make_repo "$repo" golden none
  check list-missing FAIL "FAIL $name ($required is missing)"
  repo=$work/no-golden
  make_repo "$repo" no-golden "$list"
  check golden-missing FAIL "FAIL $name ($golden is missing)"
  repo=$work/missing
  check repository-missing FAIL "FAIL $name (the repository root cannot be entered)"
  repo=$work/repo
  : >"$work/a-file" || self_fail "the fixture file cannot be made"
  art=$work/a-file/artifacts
  check artifacts-not-made FAIL "FAIL $name (the artifacts directory cannot be made)"

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    note unknown-argument FAIL FAIL
  else
    note unknown-argument FAIL "exit-$code"
  fi
  mkdir "$work/victim" || self_fail "the fixture directory victim cannot be made"
  : >"$work/victim/kept" || self_fail "the fixture file victim/kept cannot be made"
  victim=$(work=$work/victim CI_D_REPO=$work/missing sh "$self" 2>&1)
  code=$?
  last=$(printf '%s\n' "$victim" | tail -n 1)
  if [ "$code:$last" = "1:FAIL $name (the repository root cannot be entered)" ] && [ -f "$work/victim/kept" ]; then
    note work-from-the-environment FAIL FAIL
  else
    note work-from-the-environment FAIL "exit-$code-or-the-directory-removed"
  fi
  help=$(sh "$self" --help 2>&1)
  code=$?
  case $code:$help in
    "0:usage: sh scripts/r2/ci.d/20-schema.sh [--self-test | --help]"*) note help PASS PASS ;;
    *) note help PASS "exit-$code" ;;
  esac

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

usage() {
  cat <<'EOF'
usage: sh scripts/r2/ci.d/20-schema.sh [--self-test | --help]
Runs the tests of halvo-schema-core and halvo-schema. For each test that
scripts/r2/20-schema-tests.txt names, it requires the ok line under its test
target in the test run, the name in the list of the target's test binary
(cargo test --locked -p <package> --test <target> -- --list), not in its
list of ignored tests, and no compile in either listing. Then export-names
must write the golden names file on two runs. --self-test runs the checks
against a stub cargo.
EOF
}

case ${1:-} in
  --self-test) self_test ;;
  --help) usage ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
