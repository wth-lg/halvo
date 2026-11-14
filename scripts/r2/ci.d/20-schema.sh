#!/bin/sh
# jobs: windows
# rule: Before cargo runs, the check fails when scripts/r2/20-schema-tests.txt at HEAD holds a byte other than a printable ASCII character, a space, a tab, a carriage return or a line feed, a carriage return before the end of a line, a line that is not three fields, a line whose package is neither halvo-schema-core nor halvo-schema, or a line that stands twice; then the tests of halvo-schema-core and halvo-schema pass, export-names writes on two runs the names file that the crate commits as tests/golden/names.txt, and the check fails when a test that the list names is renamed, removed, ignored, filtered out or left unbuilt, when its target is pointed at another file, when a test prints the lines that the check reads, or when a tracked file differs from HEAD after the tests, but it does not hold against code of the job that writes a test binary or a tool on the PATH before the check reads them, a tool that a step of the workflow itself puts earlier on the PATH (the step Prerequisites copies cargo-deny and cargo-release into $CARGO_HOME/bin, the directory of cargo that the driver puts first), a test that keeps its name with another body, a required target with its test harness switched off, code that runs before main in a required test file, or a cargo configuration with a runner or a compiler wrapper (ruling S-53).
set -u
name=20-schema
golden=crates/halvo-schema/tests/golden/names.txt
required=scripts/r2/20-schema-tests.txt
work=
newline='
'
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

list_fault() {
  awk -v list="$required" '
    bad != "" { next }
    {
      sub(/\r$/, "")
      if (index($0, "\r")) { bad = "the line " FNR " of " list " holds a carriage return before its end"; next }
      if (NF == 0) next
      if (NF != 3) { bad = "the line " FNR " of " list " is not <package> <test target> <test name>"; next }
      if ($1 != "halvo-schema-core" && $1 != "halvo-schema") {
        bad = "the line " FNR " of " list " names the package " $1 ", which is neither halvo-schema-core nor halvo-schema"
        next
      }
      if (($1, $2, $3) in listed) { bad = "the line " FNR " of " list " stands twice: " $1 " " $2 " " $3; next }
      listed[$1, $2, $3] = 1
    }
    END { if (bad != "") print bad }
  ' "$list_file"
}

required_gap() {
  awk -v pkg="$1" -v file="$list_file" -v list="$required" '
    { sub(/\r$/, "") }
    FILENAME == file {
      if (NF == 3 && $1 == pkg) { n++; target[n] = $2; test[n] = $3 }
      next
    }
    $1 == "Running" || $1 == "Doc-tests" {
      at = ""
      if ($1 == "Running") { at = $2; gsub(/\\/, "/", at); ran[at] = 1 }
      next
    }
    at != "" && /^test .* \.\.\. ok$/ { ok[at, substr($0, 6, length($0) - 12)] = 1 }
    END {
      if (n == 0) { print "the list " list " names no test of " pkg; exit }
      for (i = 1; i <= n; i++) {
        if (!(target[i] in ran)) { print "the test target " target[i] " of " pkg " did not run"; exit }
        if (!((target[i], test[i]) in ok)) { print "the test " test[i] " of " target[i] " of " pkg " did not pass"; exit }
      }
    }
  ' "$list_file" "$2"
}

required_targets() {
  awk -v pkg="$1" '
    { sub(/\r$/, "") }
    NF == 3 && $1 == pkg && !seen[$2]++ { print $2 }
  ' "$list_file"
}

built_binary() {
  awk -v target="$1" '
    function field(key, value) {
      if (!match($0, "\"" key "\":\"[^\"]*\"")) return ""
      value = substr($0, RSTART + length(key) + 4, RLENGTH - length(key) - 5)
      gsub(/\\\\/, "/", value)
      return value
    }
    index($0, "{\"reason\":\"compiler-artifact\",") != 1 { next }
    {
      dir = field("manifest_path")
      sub(/\/[^\/]*$/, "", dir)
      if (stale == "" && index($0, "\"fresh\":true") == 0) {
        crate = dir
        sub(/.*\//, "", crate)
        stale = "the target " field("name") " of " crate
      }
      if (field("src_path") != dir "/" target) next
      n++
      binary = field("executable")
    }
    END {
      if (stale != "") print "compiled " stale ", so code of the workspace ran while its messages were written"
      else if (n != 1) print "names " (n + 0) " test targets built from " target ", not one"
      else if (binary == "") print "names no test binary built from " target
      else print "binary " binary
    }
  ' "$2"
}

listed_gap() {
  awk -v pkg="$1" -v target="$2" -v list="$list_file" -v all="$3" '
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
  ' "$list_file" "$3" "$4"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  list_file=$work/required-at-head.txt
  git show "HEAD:$required" >"$list_file" 2>"$work/required-at-head.err" || fail "$required is missing"
  LC_ALL=C tr -d '\011\012\015\040-\176' <"$list_file" >"$work/required-other-bytes" ||
    fail "the list $required cannot be read"
  [ ! -s "$work/required-other-bytes" ] ||
    fail "$required at HEAD holds a byte other than a printable ASCII character, a space, a tab, a carriage return or a line feed"
  fault=$(list_fault) || fail "the list $required cannot be read"
  [ -z "$fault" ] || fail "$fault"

  for package in halvo-schema-core halvo-schema; do
    cargo test --locked -p "$package" >"$artifacts/cargo-test-$package.txt" 2>&1
    code=$?
    [ "$code" -eq 0 ] || fail "cargo test --locked -p $package exited $code"
    passed=$(grep -c '\.\.\. ok' "$artifacts/cargo-test-$package.txt")
    printf 'cargo test --locked -p %s: tests passed %s\n' "$package" "$passed"
    gap=$(required_gap "$package" "$artifacts/cargo-test-$package.txt") || fail "the log of $package cannot be read"
    [ -z "$gap" ] || fail "$gap"
    build="cargo test --locked -p $package --no-run --message-format=json"
    cargo test --locked -p "$package" --no-run --message-format=json >"$artifacts/build-$package.json" 2>"$artifacts/build-$package.err"
    code=$?
    [ "$code" -eq 0 ] || fail "$build exited $code"
    targets=$(required_targets "$package") || fail "the list $required cannot be read"
    for target in $targets; do
      result=$(built_binary "$target" "$artifacts/build-$package.json") || fail "the messages of $build cannot be read"
      case $result in
        "binary "*) binary=${result#binary } ;;
        *) fail "$build $result" ;;
      esac
      test_name=${target##*/}
      at=$artifacts/list-$package-${test_name%.rs}
      "$binary" --list >"$at.txt" 2>"$at.err"
      code=$?
      [ "$code" -eq 0 ] || fail "the test binary of $target of $package exited $code on --list"
      "$binary" --list --ignored >"$at-ignored.txt" 2>"$at-ignored.err"
      code=$?
      [ "$code" -eq 0 ] || fail "the test binary of $target of $package exited $code on --list --ignored"
      result=$(listed_gap "$package" "$target" "$at.txt" "$at-ignored.txt") || fail "the lists of $target of $package cannot be read"
      case $result in
        "listed "*) printf 'the test binary of %s of %s: required names listed %s, ignored 0\n' "$target" "$package" "${result#listed }" ;;
        *) fail "$result" ;;
      esac
    done
  done
  listed=$(grep -c '[^[:space:]]' "$list_file")
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
  changed=$(git diff --name-only HEAD --) || fail "git diff --name-only HEAD failed"
  [ -z "$changed" ] || fail "the tracked file ${changed%%"$newline"*} differs from HEAD after the test runs"
  printf 'tracked files that differ from HEAD: 0\n'
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
  : >"$work/calls"
  mkdir "$work/tmp/$1" || self_fail "the temporary directory of the fixture $1 cannot be made"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_ARTIFACTS=${art:-$work/artifacts/$1}
    STUB_RUNS=$work/runs
    STUB_CALLS=$work/calls
    if [ -n "${fixture_locale:-}" ]; then
      LC_ALL=$fixture_locale
      export LC_ALL
    fi
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_RUNS STUB_CALLS STUB_CORE_LOG STUB_SCHEMA_LOG STUB_CORE_EXIT STUB_TEST_EXIT STUB_RUN_EXIT STUB_RUN2_EXIT STUB_OUT STUB_FIRST STUB_SECOND STUB_LISTS STUB_SIGNAL STUB_HEAD STUB_DIFF
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
  STUB_HEAD=
  STUB_DIFF=
  fixture_locale=
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

stub_list() {
  printf '%s\n' "$4" >"$1/$2-$3.code" || self_fail "the fixture list $2 $3 cannot be written"
  printf '%b' "$5" >"$1/$2-$3.txt" || self_fail "the fixture list $2 $3 cannot be written"
}

stub_binary() {
  cat >"$1/$2" <<'EOF' || self_fail "the fixture test binary $2 cannot be written"
#!/bin/sh
case $* in
  '--list') kind=all ;;
  '--list --ignored') kind=ignored ;;
  *) exit 97 ;;
esac
cat "$0-$kind.txt"
exit "$(cat "$0-$kind.code")"
EOF
  chmod +x "$1/$2" || self_fail "the fixture test binary $2 cannot be made executable"
}

message() {
  printf '%s%b' "$1" "$eol" >>"$json" || self_fail "the fixture messages $json cannot be written"
}

artifact() {
  crate=$root${sep}crates$sep$1
  message "{\"reason\":\"compiler-artifact\",\"package_id\":\"path+file://$crate#0.0.0\",\"manifest_path\":\"$crate${sep}Cargo.toml\",\"target\":{\"kind\":[\"test\"],\"crate_types\":[\"bin\"],\"name\":\"$2\",\"src_path\":\"$crate$sep$3\",\"edition\":\"2021\",\"doc\":false,\"doctest\":false,\"test\":true},\"profile\":{\"test\":true},\"features\":[],\"filenames\":[],\"executable\":$4,\"fresh\":$5}"
}

messages_of() {
  json=$1/$2.json
  printf '%s\n' "$3" >"$1/$2.code" || self_fail "the fixture messages of $2 cannot be written"
  : >"$json" || self_fail "the fixture messages of $2 cannot be written"
  message '{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0.229","manifest_path":"/c/serde/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"serde","src_path":"/c/serde/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"test":false},"features":[],"filenames":[],"executable":null,"fresh":true}'
}

usual_schema_targets() {
  artifact halvo-schema locked_files "tests${sep}locked_files.rs" '"@DIR@/halvo-schema-locked_files"' "$1"
  artifact halvo-schema later_file_builds "tests${sep}later_file_builds.rs" '"@DIR@/halvo-schema-later_file_builds"' true
  artifact halvo-schema notice "tests${sep}notice.rs" '"@DIR@/halvo-schema-notice"' true
}

lists() {
  mkdir -p "$1" || self_fail "the fixture lists $1 cannot be made"
  eol=$2
  root=$3
  sep=$4
  for binary in halvo-schema-core-notice halvo-schema-locked_files halvo-schema-later_file_builds halvo-schema-notice; do
    stub_binary "$1" "$binary"
  done
  stub_list "$1" halvo-schema-core-notice all 0 "the_notice_is_the_one_row_of_the_public_suffix_list: test${eol}the_notice_the_manifest_and_the_lock_name_one_version_of_psl: test$eol${eol}2 tests, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-core-notice ignored 0 "0 tests, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-locked_files all 0 "both_manifests_are_the_fixed_files: test${eol}each_lib_rs_holds_its_form: test${eol}the_slow_one: test$eol${eol}3 tests, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-locked_files ignored 0 "the_slow_one: test$eol${eol}1 test, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-later_file_builds all 0 "a_later_file_that_gives_a_core_type_code_of_its_own_does_not_compile: test$eol${eol}1 test, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-later_file_builds ignored 0 "0 tests, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-notice all 0 "the_schema_notice_holds: test$eol${eol}1 test, 0 benchmarks$eol"
  stub_list "$1" halvo-schema-notice ignored 0 "0 tests, 0 benchmarks$eol"
  messages_of "$1" halvo-schema-core 0
  artifact halvo-schema-core notice "tests${sep}notice.rs" '"@DIR@/halvo-schema-core-notice"' true
  message '{"reason":"build-finished","success":true}'
  messages_of "$1" halvo-schema 0
  usual_schema_targets true
  message '{"reason":"build-finished","success":true}'
}

lists_of() {
  cp -R "$work/lists" "$work/lists-$1" || self_fail "the fixture lists of $1 cannot be made"
  STUB_LISTS=$work/lists-$1
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  repo=$work/repo
  mkdir -p "$work/bin" "$work/artifacts" "$work/tmp" || self_fail "the fixture directories cannot be made"
  lists "$work/lists-windows" '\r\n' "C:\\\\r" "\\\\"
  lists "$work/lists" '\n' /r /
  STUB_LISTS=$work/lists
  STUB_SIGNAL=
  STUB_HEAD=
  STUB_DIFF=
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
printf '%s\n' "$*" >>"$STUB_CALLS"
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
  'test --locked -p halvo-schema-core --no-run --message-format=json' | 'test --locked -p halvo-schema --no-run --message-format=json')
    sed "s|@DIR@|$STUB_LISTS|g" "$STUB_LISTS/$4.json"
    exit "$(cat "$STUB_LISTS/$4.code")"
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
  cat >"$work/bin/git" <<'EOF'
#!/bin/sh
case $* in
  'show HEAD:scripts/r2/20-schema-tests.txt')
    if [ "$STUB_HEAD" = fails ]; then
      printf "fatal: path 'scripts/r2/20-schema-tests.txt' does not exist in 'HEAD'\n" >&2
      exit 128
    elif [ -n "$STUB_HEAD" ]; then
      cat "$STUB_HEAD"
    elif [ -f scripts/r2/20-schema-tests.txt ]; then
      cat scripts/r2/20-schema-tests.txt
    else
      printf "fatal: path 'scripts/r2/20-schema-tests.txt' does not exist in 'HEAD'\n" >&2
      exit 128
    fi
    ;;
  'diff --name-only HEAD --')
    [ "$STUB_DIFF" != fails ] || exit 128
    printf '%s' "$STUB_DIFF"
    ;;
  *)
    printf 'stub git: got the arguments: %s\n' "$*" >&2
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/git" || self_fail "the stub git cannot be made executable"
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
the test binary of tests/notice.rs of halvo-schema-core: required names listed 1, ignored 0
cargo test --locked -p halvo-schema: tests passed 3
the test binary of tests/locked_files.rs of halvo-schema: required names listed 1, ignored 0
the test binary of tests/later_file_builds.rs of halvo-schema: required names listed 1, ignored 0
required tests ok=3
export-names runs=2 lines=3 same_as_golden=yes same_on_second_run=yes
tracked files that differ from HEAD: 0
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
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 0 'forged: test\n\n1 test, 0 benchmarks\n'
  check w1-a-test-writes-the-ok-lines FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of w2
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 0 '0 tests, 0 benchmarks\n'
  STUB_SCHEMA_LOG=$work/empty-target.log
  check w2-an-empty-target-and-lines-from-another FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of ignored
  stub_list "$STUB_LISTS" halvo-schema-locked_files ignored 0 'each_lib_rs_holds_its_form: test\nthe_slow_one: test\n\n2 tests, 0 benchmarks\n'
  STUB_SCHEMA_LOG=$work/forged-ignored.log
  check required-name-ignored FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is ignored)"
  build="cargo test --locked -p halvo-schema --no-run --message-format=json"
  lists_of not-built
  messages_of "$STUB_LISTS" halvo-schema 0
  usual_schema_targets false
  message '{"reason":"build-finished","success":true}'
  STUB_SCHEMA_LOG=$work/forged.log
  check target-the-run-did-not-build FAIL "FAIL $name ($build compiled the target locked_files of halvo-schema, so code of the workspace ran while its messages were written)"
  lists_of macro-prints-artifacts
  messages_of "$STUB_LISTS" halvo-schema 0
  message '{"reason":"compiler-artifact","package_id":"forged","manifest_path":"/r/crates/halvo-schema/Cargo.toml","target":{"kind":["test"],"crate_types":["bin"],"name":"locked_files","src_path":"/r/crates/halvo-schema/tests/locked_files.rs","edition":"2021","doc":false,"doctest":false,"test":true},"profile":{"test":true},"features":[],"filenames":[],"executable":"@DIR@/halvo-schema-locked_files","fresh":true}'
  message '{"reason":"compiler-artifact","package_id":"path+file:///r/crates/zz#0.0.0","manifest_path":"/r/crates/zz/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"zz","src_path":"/r/crates/zz/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"test":false},"features":[],"filenames":[],"executable":null,"fresh":false}'
  artifact halvo-schema later_file_builds tests/later_file_builds.rs '"@DIR@/halvo-schema-later_file_builds"' true
  message '{"reason":"build-finished","success":true}'
  STUB_SCHEMA_LOG=$work/forged.log
  check a-macro-prints-artifacts-while-a-crate-compiles FAIL "FAIL $name ($build compiled the target zz of zz, so code of the workspace ran while its messages were written)"
  lists_of build-refused
  messages_of "$STUB_LISTS" halvo-schema 101
  check build-refused FAIL "FAIL $name ($build exited 101)"
  lists_of list-refused
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 101 ''
  check binary-refuses-the-list FAIL "FAIL $name (the test binary of tests/locked_files.rs of halvo-schema exited 101 on --list)"
  lists_of ignored-refused
  stub_list "$STUB_LISTS" halvo-schema-later_file_builds ignored 2 ''
  check binary-refuses-the-ignored-list FAIL "FAIL $name (the test binary of tests/later_file_builds.rs of halvo-schema exited 2 on --list --ignored)"
  lists_of list-exits-1
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 1 'both_manifests_are_the_fixed_files: test\neach_lib_rs_holds_its_form: test\n\n2 tests, 0 benchmarks\n'
  check binary-exits-1-on-the-list FAIL "FAIL $name (the test binary of tests/locked_files.rs of halvo-schema exited 1 on --list)"
  lists_of ignored-exits-1
  stub_list "$STUB_LISTS" halvo-schema-locked_files ignored 1 '0 tests, 0 benchmarks\n'
  check binary-exits-1-on-the-ignored-list FAIL "FAIL $name (the test binary of tests/locked_files.rs of halvo-schema exited 1 on --list --ignored)"
  for form in other-file no-messages starts-like replayed-warning stand-in leading-directory; do
    lists_of "$form"
    messages_of "$STUB_LISTS" halvo-schema 0
    case $form in
      other-file) artifact halvo-schema locked_files tests/coarse.rs '"@DIR@/halvo-schema-locked_files"' true ;;
      starts-like) artifact halvo-schema locked_files 'tests/locked_files.rs (fake)/t.rs' '"@DIR@/halvo-schema-locked_files"' true ;;
      replayed-warning)
        message '{"reason":"compiler-message","package_id":"path+file:///r/crates/halvo-schema#0.0.0","manifest_path":"/r/crates/halvo-schema/Cargo.toml","target":{"kind":["test"],"crate_types":["bin"],"name":"locked_files","src_path":"/r/crates/halvo-schema/forge/stand_in.rs","edition":"2021","doc":false,"doctest":false,"test":true},"message":{"rendered":"warning: use of deprecated function forged: forged\n     Running tests/locked_files.rs (target/debug/deps/locked_files-0)\n{\"reason\":\"compiler-artifact\",\"manifest_path\":\"/r/crates/halvo-schema/Cargo.toml\",\"src_path\":\"/r/crates/halvo-schema/tests/locked_files.rs\",\"executable\":\"@DIR@/halvo-schema-locked_files\",\"fresh\":true}\n","level":"warning"}}'
        artifact halvo-schema locked_files forge/stand_in.rs '"@DIR@/halvo-schema-locked_files"' true
        ;;
      stand-in) artifact halvo-schema locked_files tests/stand_in.rs '"@DIR@/halvo-schema-locked_files"' true ;;
      leading-directory) artifact halvo-schema locked_files stand/tests/locked_files.rs '"@DIR@/halvo-schema-locked_files"' true ;;
    esac
    [ "$form" = no-messages ] || artifact halvo-schema later_file_builds tests/later_file_builds.rs '"@DIR@/halvo-schema-later_file_builds"' true
    [ "$form" = no-messages ] || message '{"reason":"build-finished","success":true}'
    STUB_SCHEMA_LOG=$work/forged.log
    check "target-$form" FAIL "FAIL $name ($build names 0 test targets built from tests/locked_files.rs, not one)"
  done
  lists_of two-targets
  messages_of "$STUB_LISTS" halvo-schema 0
  usual_schema_targets true
  artifact halvo-schema locked_twin tests/locked_files.rs '"@DIR@/halvo-schema-locked_files"' true
  check two-targets-from-one-file FAIL "FAIL $name ($build names 2 test targets built from tests/locked_files.rs, not one)"
  lists_of no-binary
  messages_of "$STUB_LISTS" halvo-schema 0
  artifact halvo-schema locked_files tests/locked_files.rs null true
  artifact halvo-schema later_file_builds tests/later_file_builds.rs '"@DIR@/halvo-schema-later_file_builds"' true
  check no-test-binary FAIL "FAIL $name ($build names no test binary built from tests/locked_files.rs)"
  lists_of prefix
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 0 'each_lib_rs_holds_its_form_twin: test\n\n1 test, 0 benchmarks\n'
  check listed-name-longer FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of module
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 0 'tests::each_lib_rs_holds_its_form: test\n\n1 test, 0 benchmarks\n'
  check listed-name-in-a-module FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  lists_of core
  stub_list "$STUB_LISTS" halvo-schema-core-notice all 0 'the_notice_the_manifest_and_the_lock_name_one_version_of_psl: test\n\n1 test, 0 benchmarks\n'
  check core-name-not-listed FAIL "FAIL $name (the test the_notice_is_the_one_row_of_the_public_suffix_list of tests/notice.rs of halvo-schema-core is not in the list of its test binary)"
  cp -R "$work/lists-windows" "$work/lists-windows-ignored" || self_fail "the fixture lists of windows-ignored cannot be made"
  STUB_LISTS=$work/lists-windows-ignored
  stub_list "$STUB_LISTS" halvo-schema-locked_files ignored 0 'each_lib_rs_holds_its_form: test\r\n\r\n1 test, 0 benchmarks\r\n'
  STUB_SCHEMA_LOG=$work/forged-ignored.log
  check windows-ignored FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is ignored)"
  STUB_DIFF='scripts/r2/20-schema-tests.txt
crates/halvo-schema/Cargo.toml
'
  check tracked-file-differs-after-the-tests FAIL "FAIL $name (the tracked file scripts/r2/20-schema-tests.txt differs from HEAD after the test runs)"
  STUB_DIFF='crates/halvo-schema/tests/golden/names.txt
'
  check tracked-file-of-a-crate-differs FAIL "FAIL $name (the tracked file crates/halvo-schema/tests/golden/names.txt differs from HEAD after the test runs)"
  STUB_HEAD=fails
  check list-not-at-head FAIL "FAIL $name ($required is missing)"
  STUB_DIFF=fails
  check git-diff-fails FAIL "FAIL $name (git diff --name-only HEAD failed)"
  lists_of warning
  messages_of "$STUB_LISTS" halvo-schema 0
  message '{"reason":"compiler-message","package_id":"path+file:///r/crates/halvo-schema#0.0.0","manifest_path":"/r/crates/halvo-schema/Cargo.toml","target":{"kind":["test"],"crate_types":["bin"],"name":"locked_files","src_path":"/r/crates/halvo-schema/tests/locked_files.rs","edition":"2021","doc":false,"doctest":false,"test":true},"message":{"rendered":"warning: unused variable","level":"warning"}}'
  usual_schema_targets true
  message '{"reason":"build-finished","success":true}'
  check a-warning-of-a-required-target PASS "the test binary of tests/locked_files.rs of halvo-schema: required names listed 1, ignored 0"
  printf '%s' "$list" >"$work/head-list.txt" || self_fail "the fixture list at HEAD cannot be written"
  repo=$work/list-rewritten
  make_repo "$repo" golden "$core_line
$later_line
"
  STUB_HEAD=$work/head-list.txt
  STUB_SCHEMA_LOG=$work/no-targets.log
  check list-read-from-head FAIL "FAIL $name (the test target tests/locked_files.rs of halvo-schema did not run)"
  lists_of targets-from-head
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 0 'forged: test\n\n1 test, 0 benchmarks\n'
  STUB_HEAD=$work/head-list.txt
  check targets-read-from-head FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
  repo=$work/repo
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
  check two-names-of-one-target PASS "the test binary of tests/locked_files.rs of halvo-schema: required names listed 2, ignored 0
the test binary of tests/later_file_builds.rs of halvo-schema: required names listed 1, ignored 0
required tests ok=4"
  repo=$work/both-packages
  make_repo "$repo" golden "$core_line
halvo-schema tests/notice.rs the_schema_notice_holds
$later_line
"
  STUB_SCHEMA_LOG=$work/both-packages.log
  check a-target-path-in-both-packages PASS "the test binary of tests/notice.rs of halvo-schema: required names listed 1, ignored 0"
  repo=$work/space-before-cr
  make_repo "$repo" golden "$core_line
$lock_line $cr
$later_line
"
  lists_of space-before-cr
  stub_list "$STUB_LISTS" halvo-schema-locked_files all 0 'forged: test\n\n1 test, 0 benchmarks\n'
  check list-line-with-a-space-before-cr FAIL "FAIL $name (the test each_lib_rs_holds_its_form of tests/locked_files.rs of halvo-schema is not in the list of its test binary)"
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
  repo=$work/line-twice
  make_repo "$repo" golden "$core_line
$lock_line
$later_line
$lock_line
"
  check list-line-twice FAIL "FAIL $name (the line 4 of $required stands twice: $lock_line)"
  repo=$work/third-package
  make_repo "$repo" golden "$core_line
$lock_line
halvo-schemx tests/locked_files.rs never_checked_anywhere
$later_line
"
  check list-line-of-a-third-package FAIL "FAIL $name (the line 3 of $required names the package halvo-schemx, which is neither halvo-schema-core nor halvo-schema)"
  repo=$work/prefixed-package
  make_repo "$repo" golden "${list}halvo-schema-extra tests/locked_files.rs each_lib_rs_holds_its_form
"
  check list-line-of-a-package-that-begins-with-a-crate-name FAIL "FAIL $name (the line 4 of $required names the package halvo-schema-extra, which is neither halvo-schema-core nor halvo-schema)"
  repo=$work/out-of-form-first
  make_repo "$repo" golden "$core_line
halvo-schema each_lib_rs_holds_its_form
halvo-schemx tests/locked_files.rs never_checked_anywhere
"
  check the-first-fault-of-the-list-is-named FAIL "FAIL $name (the line 2 of $required is not <package> <test target> <test name>)"
  bytes="$required at HEAD holds a byte other than a printable ASCII character, a space, a tab, a carriage return or a line feed"
  repo=$work/nul-line
  make_repo "$repo" golden "$list"
  printf '\000halvo-account tests/accounts.rs every_account_is_checked\n' >>"$repo/$required" ||
    self_fail "the fixture list with a NUL byte cannot be written"
  check list-line-led-by-a-nul FAIL "FAIL $name ($bytes)"
  repo=$work/nul-in-name
  make_repo "$repo" golden "$core_line
$later_line
"
  printf 'halvo-schema tests/locked_files.rs each_lib_rs_holds_its_form\000_and_more\n' >>"$repo/$required" ||
    self_fail "the fixture list with a NUL byte cannot be written"
  check nul-inside-a-test-name FAIL "FAIL $name ($bytes)"
  repo=$work/not-utf-8
  make_repo "$repo" golden "$list"
  printf 'halvo-schema tests/locked_files.rs each_lib_rs_holds_its_form\377\n' >>"$repo/$required" ||
    self_fail "the fixture list with a byte that is no UTF-8 cannot be written"
  fixture_locale=en_US.UTF-8
  check a-byte-that-is-no-utf-8-under-a-utf-8-locale FAIL "FAIL $name ($bytes)"
  repo=$work/package-first
  make_repo "$repo" golden "$core_line
halvo-schemx tests/locked_files.rs never_checked_anywhere
halvo-schema each_lib_rs_holds_its_form
"
  check the-first-fault-is-named-when-the-package-comes-first FAIL "FAIL $name (the line 2 of $required names the package halvo-schemx, which is neither halvo-schema-core nor halvo-schema)"
  repo=$work/third-package
  check a-third-package-is-refused-before-cargo FAIL "FAIL $name (the line 3 of $required names the package halvo-schemx, which is neither halvo-schema-core nor halvo-schema)"
  note a-third-package-is-refused-with-no-call-of-cargo 0 "$(grep -c '' "$work/calls")"
  repo=$work/cr-inside
  make_repo "$repo" golden "$core_line
"
  printf 'halvo-sch\015ema tests/locked_files.rs each_lib_rs_holds_its_form\n' >>"$repo/$required" ||
    self_fail "the fixture list with a carriage return inside a line cannot be written"
  check a-carriage-return-inside-a-line FAIL "FAIL $name (the line 2 of $required holds a carriage return before its end)"
  note a-refusal-prints-no-carriage-return 0 "$(printf '%s' "$out" | LC_ALL=C tr -cd '\015' | wc -c | tr -d ' ')"
  repo=$work/tab-and-tilde
  make_repo "$repo" golden "$core_line
"
  printf 'halvo-schema\011tests/locked_files.rs\011each_lib_rs_holds_its_form\n%s\nhalvo-schema tests/later_file_builds.rs a\176b\n' "$later_line" >>"$repo/$required" ||
    self_fail "the fixture list with a tab and a tilde cannot be written"
  check a-tab-between-fields-and-a-tilde-in-a-name-pass-the-byte-rule FAIL "FAIL $name (the test a~b of tests/later_file_builds.rs of halvo-schema did not pass)"
  repo=$work/byte-177
  make_repo "$repo" golden "$list"
  printf 'halvo-schema tests/locked_files.rs each_lib\177rs\n' >>"$repo/$required" ||
    self_fail "the fixture list with the byte 0177 cannot be written"
  check the-byte-0177-is-refused FAIL "FAIL $name ($bytes)"
  repo=$work/byte-037
  make_repo "$repo" golden "$list"
  printf 'halvo-schema tests/locked_files.rs each_lib\037rs\n' >>"$repo/$required" ||
    self_fail "the fixture list with the byte 037 cannot be written"
  check the-byte-037-is-refused FAIL "FAIL $name ($bytes)"
  repo=$work/byte-013
  make_repo "$repo" golden "$list"
  printf 'halvo-schema tests/locked_files.rs each_lib\013rs\n' >>"$repo/$required" ||
    self_fail "the fixture list with the byte 013 cannot be written"
  check the-byte-013-is-refused FAIL "FAIL $name ($bytes)"
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
  kept=$work/victim
  victim=$(work=$kept CI_D_REPO=$kept/missing sh "$self" 2>&1)
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
Reads scripts/r2/20-schema-tests.txt from HEAD and, before cargo runs, refuses
a byte in it other than a printable ASCII character, a space, a tab, a carriage
return or a line feed, then a carriage return before the end of a line, a line
that is not three fields, a line whose package is neither halvo-schema-core nor
halvo-schema, and a line that stands twice, naming the first fault of the list.
A line can require only a test of an integration test target, a file under
tests/ of its package; a unit test or a doc test cannot be required. Then it
runs the tests of halvo-schema-core and halvo-schema.
For each test that the list names, it
requires the ok line under its test target in the test run, and the name in
the list of the one test binary that cargo's messages
(cargo test --locked -p <package> --no-run --message-format=json) name for
the target's file, built by the test run and not in its list of ignored
tests. Then export-names must write the golden names file on two runs, and
no tracked file may differ from HEAD. It trusts the job around it: code of
the job that runs before its reads, and a tool earlier on the PATH, whether
code of the job wrote it there or a step of the workflow itself put it there
(ruling S-53). --self-test runs the checks against a stub cargo and a stub
git.
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
