#!/bin/sh
# jobs: windows
# rule: The tests of halvo-schema-core and halvo-schema pass, each test that scripts/r2/20-schema-tests.txt lists passes under its test target, and export-names writes on two runs the names file that the crate commits as tests/golden/names.txt.
set -u
name=20-schema
golden=crates/halvo-schema/tests/golden/names.txt
required=scripts/r2/20-schema-tests.txt
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

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
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_ARTIFACTS=${art:-$work/artifacts/$1}
    STUB_RUNS=$work/runs
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_RUNS STUB_CORE_LOG STUB_SCHEMA_LOG STUB_CORE_EXIT STUB_TEST_EXIT STUB_RUN_EXIT STUB_RUN2_EXIT STUB_OUT STUB_FIRST STUB_SECOND
    sh "$self" 2>&1
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
  note "$1" "$2" "$got"
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
  printf "$2" >"$work/$1.log" || self_fail "the fixture log $1 cannot be written"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  repo=$work/repo
  mkdir -p "$work/bin" "$work/artifacts" || self_fail "the fixture directories cannot be made"
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
    exit "$STUB_TEST_EXIT"
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
cargo test --locked -p halvo-schema: tests passed 3
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
  repo=$work/crlf
  cr=$(printf '\r')
  make_repo "$repo" golden "$core_line$cr
$lock_line$cr
$later_line$cr
"
  STUB_CORE_LOG=$work/windows-core.log
  STUB_SCHEMA_LOG=$work/windows-schema.log
  check windows-form PASS "required tests ok=3"
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

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
