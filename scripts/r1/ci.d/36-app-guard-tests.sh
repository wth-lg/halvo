#!/bin/sh
# jobs: macos windows
# rule: The job's own run of src-tauri/tests/page_guard.rs passes the five tests on the check's list by name, with no test ignored, failed, missing or outside that list and the app's four modules, with one number in its running line, its ok lines and its result line, and with no build of the file to zero tests, and tauri's resolved features in a normal build of halvo on any target name neither unstable nor test, nor does a tauri configuration of the app at HEAD hold build.features.
set -u
name=36-app-guard-tests
guard_tests='a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing
a_call_whose_origin_header_is_no_address_is_refused
an_address_of_the_app_host_with_a_password_and_no_user_name_is_refused
the_web_view_main_moved_into_another_window_gets_no_command_of_the_unit_and_deletes_nothing
a_capability_handed_to_the_context_macro_reaches_no_command_of_the_unit_from_another_window'
app_modules='app
commands
launch
tray'
test_command='test --workspace --locked --test page_guard'
tree_command='tree -p halvo -e normal --target all -f {p} {f} --prefix none --locked'
esc=$(printf '\033')

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

broke() {
  printf '%s\n' "$1"
  broken="$broken${broken:+; }$1"
}

tests_decide() {
  tr -d '\r' <"$2" | sed "s/${esc}\\[[0-9;]*m//g" >"$2.lf" || broke "rule 1: the output of the test run cannot be read"
  tr -d '\r' <"$3" | sed "s/${esc}\\[[0-9;]*m//g" >"$3.lf" || broke "rule 1: the cargo output of the test run cannot be read"
  [ "$1" = 0 ] || broke "rule 1: cargo $test_command exited $1"
  runs=$(grep -c -E '^[[:space:]]*(Running|Doc-tests) ' "$3.lf")
  guard_runs=$(grep -c -E '^[[:space:]]*Running tests[/\\]page_guard\.rs \(' "$3.lf")
  if [ "$runs" != 1 ] || [ "$guard_runs" != 1 ]; then
    broke "rule 1: the run started $runs test binaries, $guard_runs of them that of tests/page_guard.rs, and it must start that one alone"
  fi
  counts=$(sed -n 's/^running \([0-9][0-9]*\) tests\{0,1\}$/\1/p' "$2.lf")
  case $counts in
    0) broke "rule 3: tests/page_guard.rs was built to 0 tests in the job ${CI_D_JOB:-}" ;;
    '' | *[!0-9]*) broke "rule 1: the run printed no single line running N tests" ;;
    *) ;;
  esac
  passed=0
  for test in $guard_tests; do
    if grep -q -x "test $test \.\.\. ok" "$2.lf"; then
      passed=$((passed + 1))
    elif grep -q -E "^test $test \.\.\. ignored" "$2.lf"; then
      broke "rule 1: $test is ignored"
    elif grep -q -E "^test $test \.\.\. " "$2.lf"; then
      broke "rule 1: $test does not pass"
    else
      broke "rule 1: $test is missing"
    fi
  done
  oks=$(grep -c -E '^test [^ ]+ \.\.\. ok$' "$2.lf")
  results=$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed; 0 failed; 0 ignored;.*$/\1/p' "$2.lf")
  case $results in
    '' | *[!0-9]*) results=none ;;
    *) ;;
  esac
  case $counts in
    '' | *[!0-9]*) said=none ;;
    *) said=$counts ;;
  esac
  if [ "$said" != "$oks" ] || [ "$results" != "$oks" ]; then
    broke "rule 1: the run's three counts are not one number: running N tests gives $said, ok lines give $oks, the result line gives $results"
  fi
  others=$(grep -E '^test .* \.\.\. ' "$2.lf" | grep -c -v -E '^test [^ ]+ \.\.\. ok$')
  [ "$others" = 0 ] || broke "rule 1: $others tests of the run do not pass"
  unlisted=$(GUARD_TESTS="$guard_tests" APP_MODULES="$app_modules" awk '
    BEGIN {
      n = split(ENVIRON["GUARD_TESTS"], names, "\n"); for (i = 1; i <= n; i++) known[names[i]] = 1
      n = split(ENVIRON["APP_MODULES"], names, "\n"); for (i = 1; i <= n; i++) modules[names[i]] = 1
    }
    /^test [^ ]+ \.\.\. / {
      if ($2 in known) next
      cut = index($2, "::")
      if (cut > 0 && (substr($2, 1, cut - 1) in modules)) next
      print $2
    }
  ' "$2.lf")
  for test in $unlisted; do
    case $test in
      *::*) broke "rule 1: $test of tests/page_guard.rs stands in a module that is not on app_modules, so write it at the top level of the file and add it to guard_tests, or, when the file brings in one more module of the app by a path attribute, add that module's name to app_modules" ;;
      *) broke "rule 1: $test of tests/page_guard.rs is not in the list of this check, so add it to guard_tests" ;;
    esac
  done
  listed=$(printf '%s\n' "$guard_tests" | grep -c .)
  distinct=$(printf '%s\n' "$guard_tests" | grep . | sort -u | grep -c -v ':')
  if [ "$listed" != 5 ] || [ "$distinct" != 5 ]; then
    broke "rule 1: the list guard_tests of this check holds $listed names, $distinct of them distinct and with no path, and it must hold the five guard tests of the file, each once, at its top level"
  fi
  printf 'tests of page_guard.rs passed by name: %s of %s\n' "$passed" "$listed"
}

tree_decide() {
  [ "$1" = 0 ] || broke "rule 2: cargo $tree_command exited $1"
  tr -d '\r' <"$2" >"$2.lf" || broke "rule 2: the output of cargo tree cannot be read"
  lines=$(grep -c -E '^tauri v[0-9]' "$2.lf")
  [ "$lines" -gt 0 ] || broke "rule 2: cargo tree names no package tauri"
  named=$(awk '
    $1 == "tauri" && $2 ~ /^v[0-9]/ {
      last = $NF
      if (last == "(*)") last = $(NF - 1)
      if (last ~ /^v[0-9]/ || last ~ /\)$/) next
      n = split(last, features, ",")
      for (i = 1; i <= n; i++) if (features[i] == "unstable" || features[i] == "test") print features[i]
    }
  ' "$2.lf" | sort -u | tr '\n' ' ')
  [ -z "$named" ] || broke "rule 2: tauri's resolved features in a normal build name ${named% }"
  printf 'tauri lines in the normal tree on every target: %s\n' "$lines"
}

config_kind() {
  lower=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')
  case $lower in
    tauri.conf.json | tauri.*.conf.json) printf 'json\n' ;;
    *.conf.json5 | tauri.toml | tauri.*.toml) printf 'other\n' ;;
    *) printf 'none\n' ;;
  esac
}

config_decide() {
  [ -f "$1/tauri.conf.json" ] || broke "rule 2: src-tauri/tauri.conf.json is missing"
  for file in "$1"/*; do
    [ -f "$file" ] || continue
    base=${file##*/}
    kind=$(config_kind "$base")
    case $kind in
      json)
        verdict=$(node -e '
          const fs = require("fs");
          let config;
          try { config = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); } catch (e) { console.log("unreadable"); process.exit(0); }
          const build = config !== null && typeof config === "object" ? config.build : undefined;
          console.log(build !== null && typeof build === "object" && Object.prototype.hasOwnProperty.call(build, "features") ? "features" : "none");
        ' "$file")
        case $verdict in
          none) ;;
          features) broke "rule 2: src-tauri/$base holds build.features" ;;
          unreadable) broke "rule 2: src-tauri/$base is no JSON that node reads" ;;
          *) broke "rule 2: node did not read src-tauri/$base" ;;
        esac
        ;;
      other)
        broke "rule 2: src-tauri/$base is a form of tauri configuration that this check does not read, so write it as JSON"
        ;;
      *) ;;
    esac
  done
}

tracked_changes() {
  git status --porcelain --untracked-files=no >"$work/status.$1" || fail "git status failed $1 cargo ran"
  [ -s "$work/status.$1" ] || return 0
  case $1 in
    before) fail "a tracked file differs from HEAD before cargo ran, so commit or stash the change, then run the check again" ;;
    *) broke "a tracked file differs from HEAD after cargo ran: a test or a build of the run wrote a tracked file, so read git status and mend what wrote it" ;;
  esac
}

measure() {
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  for tool in cargo node; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      [ "${CI:-}" = true ] && fail "$tool is not installed"
      printf 'SKIP %s (%s is not installed)\n' "$name" "$tool"
      exit 77
    fi
  done
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  broken=
  tracked_changes before
  mkdir "$work/config" || fail "the directory of the configuration cannot be made"
  configs=$(git ls-tree --name-only HEAD src-tauri/) || fail "git ls-tree failed"
  for path in $configs; do
    kind=$(config_kind "${path##*/}")
    [ "$kind" = none ] || git show "HEAD:$path" >"$work/config/${path##*/}" || fail "git show HEAD:$path failed"
  done
  config_decide "$work/config"
  cargo test --workspace --locked --test page_guard >"$work/test.out" 2>"$work/test.err"
  code=$?
  cp "$work/test.out" "$CI_D_ARTIFACTS/page-guard-test.txt" || fail "the output of the test run cannot be saved"
  cp "$work/test.err" "$CI_D_ARTIFACTS/page-guard-cargo.txt" || fail "the cargo output of the test run cannot be saved"
  tr -d '\r' <"$work/test.out" | grep -E '^(running [0-9]|test )'
  tests_decide "$code" "$work/test.out" "$work/test.err"
  cargo tree -p halvo -e normal --target all -f '{p} {f}' --prefix none --locked >"$work/tree.out" 2>"$work/tree.err"
  code=$?
  cp "$work/tree.out" "$CI_D_ARTIFACTS/tree-normal.txt" || fail "the output of cargo tree cannot be saved"
  tree_decide "$code" "$work/tree.out"
  tracked_changes after
  [ -z "$broken" ] || fail "$broken"
  printf 'PASS %s\n' "$name"
  exit 0
}

passing_test_out() {
  cat <<'EOF'

running 12 tests
test launch::tests::check_page_takes_an_absolute_config_dir_and_a_bound_in_seconds ... ok
test launch::tests::each_exit_has_its_code ... ok
test a_call_whose_origin_header_is_no_address_is_refused ... ok
test an_address_of_the_app_host_with_a_password_and_no_user_name_is_refused ... ok
test a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing ... ok
test launch::tests::an_unknown_or_an_extra_argument_is_a_usage_error ... ok
test launch::tests::check_page_refuses_a_missing_repeated_relative_or_bad_option ... ok
test launch::tests::an_argument_that_is_not_unicode_is_a_usage_error ... ok
test a_capability_handed_to_the_context_macro_reaches_no_command_of_the_unit_from_another_window ... ok
test launch::tests::no_argument_runs_the_app_and_version_alone_prints_the_version ... ok
test tray::tests::the_indicator_reflects_paused_and_running ... ok
test the_web_view_main_moved_into_another_window_gets_no_command_of_the_unit_and_deletes_nothing ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

EOF
}

passing_test_err() {
  cat <<'EOF'
   Compiling halvo v0.0.0 (/work/halvo/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 9.75s
     Running tests/page_guard.rs (target/debug/deps/page_guard-6a2c1b0f9e8d7c6b)
EOF
}

passing_tree() {
  cat <<'EOF'
halvo v0.0.0 (/work/halvo/src-tauri)
halvo-capture v0.0.0 (/work/halvo/crates/halvo-capture)
serde v1.0.228 derive,serde_derive,std
tauri v2.12.5 common-controls-v6,compression,dbus,default,dynamic-acl,tauri-runtime-wry,tray-icon,webkit2gtk,webview2-com,wry,x11
tauri-runtime v2.10.1
tauri-runtime-wry v2.10.1 common-controls-v6,x11
tauri-utils v2.8.3 brotli,build,cargo_metadata,compression,resources,schema,walkdir
tauri v2.12.5 common-controls-v6,compression,dbus,default,dynamic-acl,tauri-runtime-wry,tray-icon,webkit2gtk,webview2-com,wry,x11 (*)
EOF
}

passing_config() {
  cat <<'EOF'
{
  "productName": "Halvo",
  "identifier": "com.halvo.desktop",
  "build": {
    "frontendDist": "../ui/app"
  },
  "app": {
    "windows": [{ "label": "main", "create": false }]
  }
}
EOF
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

judge() {
  if [ -s "$work/$2.stderr" ]; then
    got="standard error: $(head -n 1 "$work/$2.stderr")"
  elif [ -z "$broken" ]; then
    got=PASS
  else
    case "$broken; " in
      *"$1"*) got="FAIL $1" ;;
      *) got="FAIL other: $broken" ;;
    esac
  fi
}

check_tests() {
  passing_test_out >"$work/$1.base.out" || self_fail "the fixture cannot be written"
  passing_test_err >"$work/$1.base.err" || self_fail "the fixture cannot be written"
  sed "$4" "$work/$1.base.out" >"$work/$1.out" || self_fail "the test output of $1 cannot be made"
  sed "$5" "$work/$1.base.err" >"$work/$1.err" || self_fail "the cargo output of $1 cannot be made"
  broken=
  tests_decide "$3" "$work/$1.out" "$work/$1.err" >"$work/$1.log" 2>"$work/$1.stderr"
  judge "${2#FAIL }" "$1"
  verdict "$1" "$2" "$got"
}

add_line() {
  AT="$2" TEXT="$3" awk '
    { print }
    (ENVIRON["AT"] ~ /^[0-9]+$/ && NR == ENVIRON["AT"] + 0) || (ENVIRON["AT"] !~ /^[0-9]+$/ && $0 ~ ENVIRON["AT"]) { print ENVIRON["TEXT"] }
  ' "$1"
}

check_tests_with_list() {
  kept=$guard_tests
  guard_tests=$(printf '%s\n' "$kept" | grep -v -x -F -e "$5")
  check_tests "$1" "$2" "$3" "$4" "$keep"
  guard_tests=$kept
}

check_tests_adding() {
  passing_test_out >"$work/$1.base.out" || self_fail "the fixture cannot be written"
  passing_test_err >"$work/$1.base.err" || self_fail "the fixture cannot be written"
  if [ "$4" = out ]; then
    made=$work/$1.out
    sed "$5" "$work/$1.base.out" >"$work/$1.pre" || self_fail "the test output of $1 cannot be made"
    cp "$work/$1.base.err" "$work/$1.err" || self_fail "the cargo output of $1 cannot be made"
  else
    made=$work/$1.err
    sed "$5" "$work/$1.base.err" >"$work/$1.pre" || self_fail "the cargo output of $1 cannot be made"
    cp "$work/$1.base.out" "$work/$1.out" || self_fail "the test output of $1 cannot be made"
  fi
  add_line "$work/$1.pre" "$6" "$7" >"$made" || self_fail "the fixture of $1 cannot be made"
  whole=$(grep -c -x -F -e "$7" "$made")
  lines=$(wc -l <"$made")
  if [ "$whole" != 1 ] || [ "$((lines))" != "$8" ]; then
    self_fail "the fixture of $1 was not made as written"
  fi
  broken=
  tests_decide "$3" "$work/$1.out" "$work/$1.err" >"$work/$1.log" 2>"$work/$1.stderr"
  judge "${2#FAIL }" "$1"
  verdict "$1" "$2" "$got"
}

check_tree() {
  passing_tree >"$work/$1.base" || self_fail "the fixture cannot be written"
  sed "$4" "$work/$1.base" >"$work/$1.tree" || self_fail "the tree of $1 cannot be made"
  broken=
  tree_decide "$3" "$work/$1.tree" >"$work/$1.log" 2>"$work/$1.stderr"
  judge "${2#FAIL }" "$1"
  verdict "$1" "$2" "$got"
}

check_config() {
  mkdir -p "$work/$1" || self_fail "the directory of $1 cannot be made"
  if [ -n "$3" ]; then
    passing_config >"$work/$1.base" || self_fail "the fixture cannot be written"
    sed "$4" "$work/$1.base" >"$work/$1/$3" || self_fail "the configuration of $1 cannot be made"
  fi
  if [ -n "${5:-}" ]; then
    passing_config >"$work/$1/$5" || self_fail "the second file of $1 cannot be made"
    [ -z "${6:-}" ] || sed "$6" "$work/$1/$5" >"$work/$1/$5.new" || self_fail "the second file of $1 cannot be changed"
    [ -z "${6:-}" ] || mv "$work/$1/$5.new" "$work/$1/$5" || self_fail "the second file of $1 cannot be changed"
  fi
  broken=
  config_decide "$work/$1" >"$work/$1.log" 2>"$work/$1.stderr"
  judge "${2#FAIL }" "$1"
  verdict "$1" "$2" "$got"
}

real_tree() {
  broken=
  (cd "$work/copy" && cargo tree -p halvo -e normal --target all -f '{p} {f}' --prefix none --locked) >"$work/$1.tree" 2>"$work/$1.tree.err"
  code=$?
  tree_decide "$code" "$work/$1.tree" >"$work/$1.log" 2>"$work/$1.stderr"
  judge "${2#FAIL }" "$1"
  verdict "$1" "$2" "$got"
}

stub_cargo() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_CALLS"
case $* in
  'test --workspace --locked --test page_guard')
    cat "$STUB_TEST_ERR" >&2
    cat "$STUB_TEST_OUT"
    [ "$STUB_WRITES" = no ] || printf 'changed by a test\n' >>tracked.txt
    exit "$STUB_TEST_EXIT"
    ;;
  'tree -p halvo -e normal --target all -f {p} {f} --prefix none --locked')
    cat "$STUB_TREE"
    exit 0
    ;;
  *)
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo" || self_fail "the stub cannot be made executable"
}

fixture_repo() {
  mkdir -p "$work/$1/src-tauri" || self_fail "the repository $1 cannot be made"
  passing_config >"$work/$1/src-tauri/tauri.conf.json" || self_fail "the configuration of $1 cannot be written"
  [ -z "$2" ] || sed "$2" "$work/$1/src-tauri/tauri.conf.json" >"$work/$1/src-tauri/tauri.conf.json.new" || self_fail "the configuration of $1 cannot be changed"
  [ -z "$2" ] || mv "$work/$1/src-tauri/tauri.conf.json.new" "$work/$1/src-tauri/tauri.conf.json" || self_fail "the configuration of $1 cannot be changed"
  printf 'tracked\n' >"$work/$1/tracked.txt" || self_fail "the tracked file of $1 cannot be written"
  git -C "$work/$1" init -q || self_fail "git init failed in $1"
  git -C "$work/$1" add . || self_fail "git add failed in $1"
  git -C "$work/$1" commit -q -m "fixture $1" || self_fail "git commit failed in $1"
}

run_measure() {
  mkdir -p "$work/artifacts.$1" || self_fail "the artifacts directory of $1 cannot be made"
  : >"$work/calls.$1"
  out=$(PATH="$work/bin:$PATH" CI_D_REPO="$work/$3" CI_D_ARTIFACTS="$work/artifacts.$1" CI_D_JOB=macos \
    STUB_CALLS="$work/calls.$1" STUB_TEST_OUT="$work/$4.out" STUB_TEST_ERR="$work/$4.err" STUB_TREE="$work/$5" \
    STUB_TEST_EXIT="${6:-0}" STUB_WRITES="${7:-no}" sh "$self" 2>&1)
  code=$?
  printf '%s\n' "$out" >"$work/$1.log"
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      case "${last%)}; " in
        *"${2#FAIL }"*) got="FAIL ${2#FAIL }" ;;
        *) got="FAIL other: $last" ;;
      esac
      ;;
    *) got=exit-$code ;;
  esac
  calls=$(cat "$work/calls.$1")
  want_calls="$test_command$nl$tree_command"
  [ "$code" != 0 ] || [ "$calls" = "$want_calls" ] || got="$got, other cargo calls"
  [ "${8:-}" != no-cargo ] || [ -z "$calls" ] || got="$got, cargo was called"
  verdict "$1" "$2" "$got"
}

self_test() {
  [ -n "${CI_D_REPO:-}" ] || self_fail "CI_D_REPO is not set"
  self=$0
  case $self in
    /*) ;;
    *) self="$PWD/$self" ;;
  esac
  command -v cargo >/dev/null 2>&1 || self_fail "cargo is not installed"
  command -v node >/dev/null 2>&1 || self_fail "node is not installed"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  mkdir "$work/bin" || self_fail "the stub directory cannot be made"
  rejected=0
  wrong=
  nl='
'
  cr=$(printf '\r')
  keep='s/^//'

  check_tests all-pass PASS 0 "$keep" "$keep"
  check_tests crlf-and-windows-paths PASS 0 "s/\$/$cr/" "s|Running tests/|Running tests\\\\|; s|target/debug/deps/|target\\\\debug\\\\deps\\\\|; s/\$/$cr/"
  check_tests_adding another-module-test-passes PASS 0 out 's/^running 12 tests$/running 13 tests/; s/^test result: ok\. 12 passed;/test result: ok. 13 passed;/' '^test tray::' 'test commands::tests::a_new_test_of_a_module ... ok' 18
  check_tests_adding a-test-of-the-module-app-passes PASS 0 out 's/^running 12 tests$/running 13 tests/; s/^test result: ok\. 12 passed;/test result: ok. 13 passed;/' '^test tray::' 'test app::tests::a_test_of_the_app_module ... ok' 18
  check_tests_adding a-test-in-a-module-of-the-file "FAIL rule 1: more::a_guard_test_in_a_module of tests/page_guard.rs stands in a module that is not on app_modules" 0 out 's/^running 12 tests$/running 13 tests/; s/^test result: ok\. 12 passed;/test result: ok. 13 passed;/' '^test tray::' 'test more::a_guard_test_in_a_module ... ok' 18
  check_tests_adding a-test-two-modules-deep "FAIL rule 1: more::deeper::a_guard_test_two_modules_deep of tests/page_guard.rs stands in a module that is not on app_modules" 0 out 's/^running 12 tests$/running 13 tests/; s/^test result: ok\. 12 passed;/test result: ok. 13 passed;/' '^test tray::' 'test more::deeper::a_guard_test_two_modules_deep ... ok' 18
  check_tests_adding a-module-that-begins-like-an-app-module "FAIL rule 1: tray_more::tests::a_test_of_a_module_named_like_tray of tests/page_guard.rs stands in a module that is not on app_modules" 0 out 's/^running 12 tests$/running 13 tests/; s/^test result: ok\. 12 passed;/test result: ok. 13 passed;/' '^test tray::' 'test tray_more::tests::a_test_of_a_module_named_like_tray ... ok' 18
  check_tests_adding m33-built-to-zero-tests "FAIL rule 3: tests/page_guard.rs was built to 0 tests in the job" 0 out '/^running 12 tests$/,/^$/d; /^test /d' 1 'running 0 tests' 3
  check_tests_adding m33-names-missing "FAIL rule 1: a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing is missing" 0 out '/^running 12 tests$/,/^$/d; /^test /d' 1 'running 0 tests' 3
  check_tests one-guard-test-ignored "FAIL rule 1: a_call_whose_origin_header_is_no_address_is_refused is ignored" 0 's/^test a_call_whose_origin_header_is_no_address_is_refused \.\.\. ok$/test a_call_whose_origin_header_is_no_address_is_refused ... ignored/' "$keep"
  check_tests one-guard-test-ignored-with-a-reason "FAIL rule 1: a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing is ignored" 0 's/^test a_child_web_view\(.*\) \.\.\. ok$/test a_child_web_view\1 ... ignored, needs a window/' "$keep"
  check_tests one-guard-test-failed "FAIL rule 1: an_address_of_the_app_host_with_a_password_and_no_user_name_is_refused does not pass" 101 's/^test an_address_of\(.*\) \.\.\. ok$/test an_address_of\1 ... FAILED/' "$keep"
  check_tests one-guard-test-missing "FAIL rule 1: the_web_view_main_moved_into_another_window_gets_no_command_of_the_unit_and_deletes_nothing is missing" 0 '/^test the_web_view_main_moved/d; s/running 12 tests/running 11 tests/' "$keep"
  check_tests one-guard-test-renamed "FAIL rule 1: a_capability_handed_to_the_context_macro_reaches_no_command_of_the_unit_from_another_window is missing" 0 's/^test a_capability_handed_to_the_context_macro_reaches/test a_capability_given_to_the_context_macro_reaches/' "$keep"
  check_tests guard-test-only-under-a-module-path "FAIL rule 1: a_call_whose_origin_header_is_no_address_is_refused is missing" 0 's/^test a_call_whose_origin/test tests::a_call_whose_origin/' "$keep"
  check_tests guard-test-ok-with-a-suffix "FAIL rule 1: a_call_whose_origin_header_is_no_address_is_refused does not pass" 0 's/^test a_call_whose_origin_header_is_no_address_is_refused \.\.\. ok$/& (61 s)/' "$keep"
  check_tests_adding a-test-of-the-file-not-in-the-list "FAIL rule 1: a_new_guard_test_of_the_file of tests/page_guard.rs is not in the list" 0 out "$keep" '^test tray::' 'test a_new_guard_test_of_the_file ... ok' 18
  check_tests a-module-test-ignored "FAIL rule 1: 1 tests of the run do not pass" 0 's/^test tray::tests::the_indicator_reflects_paused_and_running \.\.\. ok$/test tray::tests::the_indicator_reflects_paused_and_running ... ignored/' "$keep"
  check_tests cargo-exits-101 "FAIL rule 1: cargo test --workspace --locked --test page_guard exited 101" 101 "$keep" "$keep"
  check_tests no-output "FAIL rule 1: the run started 0 test binaries, 0 of them" 0 'd' 'd'
  check_tests another-file-ran "FAIL rule 1: the run started 1 test binaries, 0 of them" 0 "$keep" 's|tests/page_guard\.rs|tests/page_guard_old.rs|'
  check_tests_adding a-second-binary-ran "FAIL rule 1: the run started 2 test binaries, 1 of them" 0 err "$keep" 'Running tests' '     Running unittests src/main.rs (target/debug/deps/halvo-0123456789abcdef)' 4
  check_tests_adding two-running-lines "FAIL rule 1: the run printed no single line running N tests" 0 out "$keep" '^running 12 tests$' 'running 3 tests' 18
  check_tests cargo-colour-on-standard-error PASS 0 "$keep" "s/^\\( *\\)\\([A-Z][a-z]*\\) /\\1${esc}[1m${esc}[92m\\2${esc}[0m /"
  check_tests a-run-that-ends-before-its-result-line "FAIL rule 1: the run's three counts are not one number: running N tests gives 12, ok lines give 11, the result line gives none" 0 '/^test launch::tests::each_exit_has_its_code /d; /^test result:/d' "$keep"
  check_tests_adding one-more-ok-line-than-the-run-counts "FAIL rule 1: the run's three counts are not one number: running N tests gives 12, ok lines give 13, the result line gives 12" 0 out "$keep" '^test tray::' 'test launch::tests::a_line_that_no_test_of_the_run_printed ... ok' 18
  check_tests_with_list a-list-of-four-guard-tests "FAIL rule 1: the list guard_tests of this check holds 4 names" 0 '/^test a_call_whose_origin_header_is_no_address_is_refused /d; s/^running 12 tests$/running 11 tests/; s/^test result: ok\. 12 passed;/test result: ok. 11 passed;/' 'a_call_whose_origin_header_is_no_address_is_refused'
  check_tests a-running-line-that-counts-one-less "FAIL rule 1: the run's three counts are not one number: running N tests gives 11, ok lines give 12, the result line gives 12" 0 's/^running 12 tests$/running 11 tests/' "$keep"
  check_tests a-result-line-that-counts-one-more "FAIL rule 1: the run's three counts are not one number: running N tests gives 12, ok lines give 12, the result line gives 13" 0 's/^test result: ok\. 12 passed;/test result: ok. 13 passed;/' "$keep"
  check_tests a-result-line-with-an-ignored-test "FAIL rule 1: the run's three counts are not one number: running N tests gives 12, ok lines give 12, the result line gives none" 0 's/^test result: ok\. 12 passed; 0 failed; 0 ignored;/test result: ok. 12 passed; 0 failed; 1 ignored;/' "$keep"
  check_tests no-running-line "FAIL rule 1: the run started 0 test binaries, 0 of them" 0 "$keep" '/Running /d'

  check_tree tree-passes PASS 0 "$keep"
  check_tree tree-crlf PASS 0 "s/\$/$cr/"
  check_tree tauri-with-unstable "FAIL rule 2: tauri's resolved features in a normal build name unstable;" 0 's/^\(tauri v2.12.5 [^ ]*\)tray-icon,/\1tray-icon,unstable,/'
  check_tree tauri-with-test "FAIL rule 2: tauri's resolved features in a normal build name test;" 0 's/^\(tauri v2.12.5 \)common-controls-v6,/\1common-controls-v6,test,/'
  check_tree tauri-with-unstable-only-first "FAIL rule 2: tauri's resolved features in a normal build name unstable;" 0 's/^\(tauri v2.12.5 \)\(.*,x11\)$/\1unstable,\2/'
  check_tree a-feature-named-unstable-something PASS 0 's/^\(tauri v2.12.5 \)common-controls-v6,/\1common-controls-v6,unstable-later,/'
  check_tree unstable-on-another-package-alone PASS 0 's/^tauri-runtime-wry v2.10.1 common-controls-v6,x11$/tauri-runtime-wry v2.10.1 common-controls-v6,unstable,x11/'
  check_tree tauri-with-test-and-unstable "FAIL rule 2: tauri's resolved features in a normal build name test unstable;" 0 's/^\(tauri v2.12.5 \)common-controls-v6,/\1common-controls-v6,unstable,test,/'
  check_tree unstable-only-on-the-repeated-tauri-line "FAIL rule 2: tauri's resolved features in a normal build name unstable;" 0 's/^\(tauri v2.12.5 \)\(.*\) (\*)$/\1unstable,\2 (*)/'
  check_tree tauri-from-a-path-source "FAIL rule 2: tauri's resolved features in a normal build name unstable;" 0 's|^tauri v2.12.5 common-controls-v6,|tauri v2.12.5 (/work/tauri/crates/tauri) common-controls-v6,unstable,|'
  check_tree tauri-from-a-git-source "FAIL rule 2: tauri's resolved features in a normal build name test;" 0 's|^tauri v2.12.5 common-controls-v6,|tauri v2.12.5 (https://github.com/tauri-apps/tauri?rev=1a2b3c#1a2b3c4d) common-controls-v6,test,|'
  check_tree tauri-from-a-path-source-with-no-feature PASS 0 's|^tauri v2.12.5 common-controls-v6,.*,x11$|tauri v2.12.5 (/work/tauri/crates/tauri)|'
  check_tree no-tauri-line "FAIL rule 2: cargo tree names no package tauri" 0 '/^tauri v/d'
  check_tree cargo-tree-exits-101 "FAIL rule 2: cargo tree -p halvo -e normal --target all -f {p} {f} --prefix none --locked exited 101" 101 "$keep"

  check_config config-passes PASS tauri.conf.json "$keep"
  check_config config-with-build-features "FAIL rule 2: src-tauri/tauri.conf.json holds build.features" tauri.conf.json 's|^    "frontendDist": "../ui/app"$|    "frontendDist": "../ui/app",\
    "features": ["tauri/unstable"]|'
  check_config config-with-empty-build-features "FAIL rule 2: src-tauri/tauri.conf.json holds build.features" tauri.conf.json 's|^    "frontendDist": "../ui/app"$|    "frontendDist": "../ui/app", "features": []|'
  check_config config-with-features-outside-build PASS tauri.conf.json 's|^  "identifier": "com.halvo.desktop",$|&\
  "features": ["unstable"],|'
  check_config windows-config-with-build-features "FAIL rule 2: src-tauri/tauri.windows.conf.json holds build.features" tauri.conf.json "$keep" tauri.windows.conf.json 's|^    "frontendDist": "../ui/app"$|    "frontendDist": "../ui/app", "features": ["unstable"]|'
  check_config config-as-json5 "FAIL rule 2: src-tauri/tauri.macos.conf.json5 is a form of tauri configuration that this check does not read" tauri.conf.json "$keep" tauri.macos.conf.json5
  check_config config-as-tauri-toml "FAIL rule 2: src-tauri/Tauri.toml is a form of tauri configuration that this check does not read" tauri.conf.json "$keep" Tauri.toml
  check_config platform-config-in-other-case "FAIL rule 2: src-tauri/Tauri.macos.conf.json holds build.features" tauri.conf.json "$keep" Tauri.macos.conf.json 's|^    "frontendDist": "../ui/app"$|    "frontendDist": "../ui/app", "features": ["unstable"]|'
  check_config platform-toml-in-lower-case "FAIL rule 2: src-tauri/tauri.macos.toml is a form of tauri configuration that this check does not read" tauri.conf.json "$keep" tauri.macos.toml
  check_config json5-in-upper-case "FAIL rule 2: src-tauri/tauri.windows.conf.JSON5 is a form of tauri configuration that this check does not read" tauri.conf.json "$keep" tauri.windows.conf.JSON5
  check_config config-not-json "FAIL rule 2: src-tauri/tauri.conf.json is no JSON that node reads" tauri.conf.json 's/^{$/{,/'
  check_config config-missing "FAIL rule 2: src-tauri/tauri.conf.json is missing" '' "$keep"

  mkdir "$work/copy" || self_fail "the copy cannot be made"
  git -C "$CI_D_REPO" archive --format=tar HEAD -- Cargo.toml Cargo.lock src-tauri crates >"$work/copy.tar" || self_fail "git archive of HEAD failed"
  (cd "$work/copy" && tar -x -f -) <"$work/copy.tar" || self_fail "the copy of HEAD cannot be unpacked"
  real_tree workspace-at-head PASS
  sed '/^resolver = /d' "$work/copy/Cargo.toml" >"$work/copy/Cargo.toml.new" || self_fail "the resolver line cannot be dropped"
  mv "$work/copy/Cargo.toml.new" "$work/copy/Cargo.toml" || self_fail "the resolver line cannot be dropped"
  ! grep -q '^resolver' "$work/copy/Cargo.toml" || self_fail "the copy still holds a resolver line"
  real_tree workspace-without-its-resolver-line "FAIL rule 2: tauri's resolved features in a normal build name test unstable;"

  stub_cargo
  passing_tree >"$work/tree.ok" || self_fail "the fixture cannot be written"
  passing_tree | sed 's/^\(tauri v2.12.5 \)common-controls-v6,/\1common-controls-v6,test,unstable,/' >"$work/tree.unstable" || self_fail "the fixture cannot be written"
  fixture_repo repo ''
  fixture_repo repo-with-build-features 's|^    "frontendDist": "../ui/app"$|    "frontendDist": "../ui/app", "features": ["unstable"]|'
  run_measure measure-passes PASS repo all-pass tree.ok
  run_measure measure-m33 "FAIL rule 3: tests/page_guard.rs was built to 0 tests in the job macos" repo m33-built-to-zero-tests tree.ok
  run_measure measure-ignored "FAIL rule 1: a_call_whose_origin_header_is_no_address_is_refused is ignored" repo one-guard-test-ignored tree.ok
  run_measure measure-test-run-fails "FAIL rule 1: cargo test --workspace --locked --test page_guard exited 101" repo one-guard-test-failed tree.ok 101
  run_measure measure-unstable-in-the-tree "FAIL rule 2: tauri's resolved features in a normal build name test unstable;" repo all-pass tree.unstable
  run_measure measure-build-features "FAIL rule 2: src-tauri/tauri.conf.json holds build.features" repo-with-build-features all-pass tree.ok
  run_measure measure-a-test-writes-the-checkout 'FAIL a tracked file differs from HEAD after cargo ran' repo all-pass tree.ok 0 yes
  printf 'changed before the check\n' >>"$work/repo/tracked.txt" || self_fail "the tracked file cannot be changed"
  run_measure measure-on-a-changed-checkout 'FAIL a tracked file differs from HEAD before cargo ran' repo all-pass tree.ok 0 no no-cargo

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (unknown argument)\n' "$name"
    exit 64
    ;;
esac
