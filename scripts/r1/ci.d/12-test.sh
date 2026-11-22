#!/bin/sh
# jobs: macos windows
# rule: Every test of the workspace passes with the committed lockfile, and the test run leaves no entry in its temp directory. On the macos job a hold test whose secure-input hold the system refuses fails, and the job prints how often the shutdown of each real macOS source answered other than Ok.
# lane: alone
# limit: 1800
set -u
name=12-test
exempt=halvo-consent-test-
slow_list=scripts/ci/slow-tests.txt
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

macos_sources='input foreground screen'

shutdown_lines() {
  [ "$CI_D_JOB" = macos ] || return 0
  for source in $macos_sources; do
    count=$(grep -c "^shutdown source=$source ok=false\$" "$answers")
    printf 'shutdown_not_ok source=%s count=%s\n' "$source" "$count"
  done
}

keychain_items() {
  [ "$CI_D_JOB" = macos ] || return 0
  dump=$(security dump-keychain login.keychain-db 2>/dev/null)
  security_code=$?
  if [ "$security_code" -ne 0 ]; then
    printf 'login-keychain items of com.halvo.capture.dev-fallback %s the tests: unknown, security exited %s\n' "$1" "$security_code"
    return 0
  fi
  count=$(printf '%s\n' "$dump" | grep -c '"svce"<blob>="com\.halvo\.capture\.dev-fallback')
  printf 'login-keychain items of com.halvo.capture.dev-fallback %s the tests: %s\n' "$1" "$count"
}

fast_plan() {
  { [ -f "$slow_list" ] && [ -r "$slow_list" ]; } ||
    fail "$slow_list cannot be read; the form of the merge gate reads from it the test targets, the unit tests and the test names that it skips, and the group form the ones that it runs"
  want_group=
  [ -n "${CI_D_MERGE_GATE:-}" ] || want_group=$CI_D_TEST_GROUP
  group_target_packages=' '
  group_targets=' '
  group_unit_packages=
  group_units=
  group_parts=
  group_part_packages=' '
  group_part_count=0
  group_doc=
  group_unit_names=
  kept_parts=
  kept_lines=
  kept_packages=' '
  kept_count=0
  cr=$(printf '\r')
  skipped_targets=' '
  target_packages=' '
  unit_packages=' '
  part_packages=
  target_excludes=
  unit_excludes=
  name_skips=
  skipped_names=0
  doc_tests=run
  line_number=0
  while read -r kind word line_group line_package _ || [ -n "$kind" ]; do
    line_number=$((line_number + 1))
    kind=${kind%"$cr"}
    word=${word%"$cr"}
    line_group=${line_group%"$cr"}
    line_package=${line_package%"$cr"}
    case $kind in
      '' | '#'*) continue ;;
      doc-tests) line_group=$word ;;
      kept)
        case $word in
          '' | *[!A-Za-z0-9_-]*) fail "$slow_list line $line_number: a kept line names a package in letters, digits, - and _, and then a part of a test name" ;;
        esac
        case $line_group in
          '' | *[!A-Za-z0-9_:]*) fail "$slow_list line $line_number: after its package a kept line names a part of a test name in letters, digits, _ and :" ;;
        esac
        kept_job=both
        case $line_package in
          macos | windows) kept_job=$line_package ;;
        esac
        kept_parts="$kept_parts$word=$line_group "
        kept_lines="$kept_lines$word=$line_group=$kept_job "
        kept_count=$((kept_count + 1))
        case $kept_packages in
          *" $word "*) ;;
          *) kept_packages="$kept_packages$word " ;;
        esac
        continue
        ;;
    esac
    case $line_group in
      [A-Z]) ;;
      *) fail "$slow_list line $line_number names no group: after the name, or after the word doc-tests, a line holds one capital letter, the group in which a slice runs it" ;;
    esac
    if [ "$kind" = doc-tests ]; then
      doc_tests=skipped
      [ "$line_group" != "$want_group" ] || group_doc=yes
      continue
    fi
    case $kind:$word in
      target: | targets: | units: | test: | target:*[!A-Za-z0-9_-]* | targets:*[!A-Za-z0-9_-]* | units:*[!A-Za-z0-9_-]* | test:*[!A-Za-z0-9_:]*)
        fail "$slow_list line $line_number: after its first word a line names a test target or a package in letters, digits, - and _, or a part of a test name in letters, digits, _ and :"
        ;;
      target:*)
        skipped_targets="$skipped_targets$word "
        [ "$line_group" != "$want_group" ] || group_targets="$group_targets$word "
        ;;
      targets:*)
        target_packages="$target_packages$word "
        target_excludes="$target_excludes --exclude $word"
        [ "$line_group" != "$want_group" ] || group_target_packages="$group_target_packages$word "
        ;;
      units:*)
        unit_packages="$unit_packages$word "
        unit_excludes="$unit_excludes --exclude $word"
        if [ "$line_group" = "$want_group" ]; then
          group_unit_packages="$group_unit_packages -p $word"
          group_unit_names="$group_unit_names $word"
          group_units=$((${group_units:-0} + 1))
        fi
        ;;
      test:*)
        name_skips="$name_skips --skip $word"
        skipped_names=$((skipped_names + 1))
        case $line_package in
          '' | *[!A-Za-z0-9_-]*) fail "$slow_list line $line_number: after its group a test line names the package whose unit tests hold the test, in letters, digits, - and _" ;;
        esac
        part_packages="$part_packages$line_package "
        if [ "$line_group" = "$want_group" ]; then
          group_parts="$group_parts$line_package=$word "
          group_part_count=$((group_part_count + 1))
          case $group_part_packages in
            *" $line_package "*) ;;
            *) group_part_packages="$group_part_packages$line_package " ;;
          esac
        fi
        ;;
      *) fail "$slow_list line $line_number begins with another word than target, targets, units, test, kept or doc-tests" ;;
    esac
  done <"$slow_list"
  metadata=$(cargo metadata --no-deps --format-version 1 --locked) || fail "cargo metadata exited $?"
  pairs=$(printf '%s\n' "$metadata" | perl -ne 'my $package = ""; while (/\{"name":"([A-Za-z0-9_-]+)","version":"|\{"kind":\["test"\],"crate_types":\["bin"\],"name":"([A-Za-z0-9_-]+)"|\{"kind":\["(lib)"\]/g) { if (defined $1) { $package = $1; print "package $1\n"; } elsif (defined $2) { print "target $package $2\n"; } else { print "lib $package\n"; } }')
  all_packages=$(printf '%s\n' "$pairs" | sed -n 's/^package //p' | sort -u)
  all_targets=$(printf '%s\n' "$pairs" | sed -n 's/^target [^ ]* //p' | sort -u)
  lib_packages=$(printf '%s\n' "$pairs" | sed -n 's/^lib //p')
  [ -n "$all_targets" ] && [ -n "$all_packages" ] ||
    fail "cargo metadata names no test target or no package in the form that this check reads"
  for package in $target_packages $unit_packages $part_packages $kept_packages; do
    case $nl$all_packages$nl in
      *"$nl$package$nl"*) ;;
      *) fail "$slow_list names the package $package, and the workspace has no package of that name; take the line out" ;;
    esac
  done
  for package in $kept_packages; do
    case $unit_packages in
      *" $package "*) ;;
      *) fail "$slow_list keeps a part of the unit tests of $package, and no units line names that package, so the merge gate runs its unit tests whole; take the kept line out" ;;
    esac
  done
  for target in $skipped_targets; do
    case $nl$all_targets$nl in
      *"$nl$target$nl"*) ;;
      *) fail "$slow_list names the test target $target, and no package of the workspace has a test target of that name; take the line out" ;;
    esac
  done
  kept_targets=
  all_count=0
  kept_targets_count=0
  seen_targets=' '
  while read -r pair_kind package target; do
    [ "$pair_kind" = target ] || continue
    all_count=$((all_count + 1))
    case $target_packages in *" $package "*) continue ;; esac
    case $skipped_targets in *" $target "*) continue ;; esac
    kept_targets_count=$((kept_targets_count + 1))
    case $seen_targets in *" $target "*) continue ;; esac
    seen_targets="$seen_targets$target "
    kept_targets="$kept_targets --test $target"
  done <<EOF
$pairs
EOF
  [ -n "$kept_targets" ] || fail "$slow_list names every test target of the workspace"
  skipped_units=0
  for package in $unit_packages; do
    skipped_units=$((skipped_units + 1))
  done
  if [ -z "$want_group" ]; then
    printf 'form: the merge gate; %s names what this run skips: %s of %s test targets, the unit tests of %s packages, %s parts of a test name; doc-tests: %s; kept of the skipped unit tests: %s parts\n' "$slow_list" "$((all_count - kept_targets_count))" "$all_count" "$skipped_units" "$skipped_names" "$doc_tests" "$kept_count"
    return
  fi
  group_package_names=
  group_package_flags=
  group_named=
  group_owners=
  group_count=0
  seen_targets=' '
  seen_named=' '
  seen_owners=' '
  for package in $group_target_packages; do
    group_package_flags="$group_package_flags -p $package"
  done
  while read -r pair_kind package target; do
    [ "$pair_kind" = target ] || continue
    case $group_target_packages in
      *" $package "*)
        group_count=$((group_count + 1))
        case $seen_targets in
          *" $target "*) ;;
          *)
            seen_targets="$seen_targets$target "
            group_package_names="$group_package_names --test $target"
            ;;
        esac
        continue
        ;;
    esac
    case $group_targets in
      *" $target "*) ;;
      *) continue ;;
    esac
    group_count=$((group_count + 1))
    case $seen_named in
      *" $target "*) ;;
      *)
        seen_named="$seen_named$target "
        group_named="$group_named --test $target"
        ;;
    esac
    case $seen_owners in
      *" $package "*) ;;
      *)
        seen_owners="$seen_owners$package "
        group_owners="$group_owners -p $package"
        ;;
    esac
  done <<EOF
$pairs
EOF
  [ "$group_count" -gt 0 ] || [ -n "$group_units$group_parts$group_doc" ] ||
    fail "$slow_list gives no line the group $want_group; CI_D_TEST_GROUP names a group of that list"
  printf 'form: the group %s of %s, and no other test: %s test targets, the unit tests of %s packages, %s parts of a test name; doc-tests: %s\n' "$want_group" "$slow_list" "$group_count" "${group_units:-0}" "$group_part_count" "${group_doc:-no}"
}

group_one() {
  if [ "$group_step" = build ]; then
    cargo "$@" --no-run
    return
  fi
  # shellcheck disable=SC2086
  TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo "$@" -- $group_names 2>&1
}

unit_flags() {
  for unit_package in "$@"; do
    case $nl$lib_packages$nl in
      *"$nl$unit_package$nl"*)
        printf '%s\n' '--lib --bins'
        return
        ;;
    esac
  done
  printf '%s\n' '--bins'
}

kept_cargo() {
  group_step=$1
  for package in $kept_packages; do
    group_names=
    for part in $kept_parts; do
      case $part in
        "$package="*) group_names="$group_names ${part#*=}" ;;
      esac
    done
    [ "$group_step" = build ] || printf 'kept: the unit tests of %s whose name holds:%s\n' "$package" "$group_names"
    flags=$(unit_flags "$package")
    # shellcheck disable=SC2086
    group_one test --locked -p "$package" $flags || return
  done
}

kept_counts() {
  [ -n "${CI_D_MERGE_GATE:-}" ] || return 0
  zero_package=
  zero_part=
  for kept_line in $kept_lines; do
    package=${kept_line%%=*}
    job=${kept_line##*=}
    part=${kept_line#*=}
    part=${part%=*}
    if [ "$job" != both ] && [ "$job" != "$CI_D_JOB" ]; then
      printf 'kept_not_counted package=%s part=%s (%s runs the part in the %s job alone)\n' "$package" "$part" "$slow_list" "$job"
      continue
    fi
    count=$(tr -d '\r' <"$output/cargo-test.txt" | awk -v package="$package" -v part="$part" '$1 == "kept:" { inside = ($6 == package); next } inside && $1 == "test" && $3 == "..." && $4 == "ok" && index($2, part) { count++ } END { print count + 0 }') ||
      fail "awk exited $? on the output of the kept calls"
    printf 'kept_count package=%s part=%s count=%s\n' "$package" "$part" "$count"
    [ "$count" -gt 0 ] || [ -n "$zero_package" ] || {
      zero_package=$package
      zero_part=$part
    }
  done
  [ -z "$zero_package" ] ||
    fail "$slow_list keeps the unit tests of $zero_package whose name holds $zero_part, and no such test ran in the $CI_D_JOB job; correct the part, write after it the one job that has such tests (macos or windows), or take the kept line out"
}

group_cargo() {
  group_step=$1
  group_names=
  if [ -n "$group_package_names" ]; then
    # shellcheck disable=SC2086
    group_one test --locked $group_package_flags $group_package_names || return
  fi
  if [ -n "$group_named" ]; then
    # shellcheck disable=SC2086
    group_one test --locked $group_owners $group_named || return
  fi
  if [ -n "$group_unit_packages" ]; then
    # shellcheck disable=SC2086
    flags=$(unit_flags $group_unit_names)
    # shellcheck disable=SC2086
    group_one test --locked $group_unit_packages $flags || return
  fi
  for package in $group_part_packages; do
    group_names=
    for part in $group_parts; do
      case $part in
        "$package="*) group_names="$group_names ${part#*=}" ;;
      esac
    done
    flags=$(unit_flags "$package")
    # shellcheck disable=SC2086
    group_one test --locked -p "$package" $flags || return
  done
  [ "$group_step" = run ] && [ -n "$group_doc" ] || return 0
  TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo test --workspace --locked --doc 2>&1
}

build_tests() {
  if [ -n "$want_group" ]; then
    group_cargo build
    return
  fi
  if [ -z "${CI_D_MERGE_GATE:-}" ]; then
    cargo test --workspace --locked --no-run
    return
  fi
  # shellcheck disable=SC2086
  cargo test --workspace $unit_excludes --locked --lib --bins --no-run &&
    cargo test --workspace $target_excludes --locked $kept_targets --no-run &&
    kept_cargo build
}

run_tests() {
  if [ -n "$want_group" ]; then
    group_cargo run
    return
  fi
  if [ -z "${CI_D_MERGE_GATE:-}" ]; then
    TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo test --workspace --locked 2>&1
    return
  fi
  # shellcheck disable=SC2086
  TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo test --workspace $unit_excludes --locked --lib --bins -- $name_skips 2>&1 &&
    TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo test --workspace $target_excludes --locked $kept_targets -- $name_skips 2>&1 &&
    kept_cargo run || return
  [ "$doc_tests" = skipped ] || TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo test --workspace --locked --doc 2>&1
}

line_seconds() {
  if [ -z "${CI_D_MERGE_GATE:-}$want_group" ]; then
    cat
    return
  fi
  perl -e '$| = 1; my $start = time; open(my $file, ">", $ARGV[0]) or exit 2; while (my $line = <STDIN>) { print $line; printf $file "%d %s", time - $start, $line; }' "$CI_D_ARTIFACTS/line-seconds.txt"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  want_group=
  CI_D_TEST_GROUP=${CI_D_TEST_GROUP:-}
  [ -z "${CI_D_MERGE_GATE:-}$CI_D_TEST_GROUP" ] || fast_plan
  build_tests || fail "the test build failed: cargo test --no-run exited $?"
  scratch=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/$name.XXXXXX") || fail "mktemp failed"
  output=$(mktemp -d "${TMPDIR:-/tmp}/$name-output.XXXXXX") || fail "mktemp failed"
  answers=$output/shutdown-answers.txt
  : >"$answers" || fail "the file of the shutdown answers cannot be made"
  if [ "$CI_D_JOB" = macos ]; then
    HALVO_SECURE_INPUT_HOLD_REQUIRED=1
    HALVO_SHUTDOWN_ANSWERS=$answers
    export HALVO_SECURE_INPUT_HOLD_REQUIRED HALVO_SHUTDOWN_ANSWERS
  fi
  keychain_items before
  {
    run_tests
    printf '%s\n' "$?" >"$output/code"
  } | line_seconds | tee "$output/cargo-test.txt"
  code=
  [ ! -s "$output/code" ] || read -r code <"$output/code"
  keychain_items after
  shutdown_lines
  tr -d '\r' <"$output/cargo-test.txt" | grep -E '^ *(Running|Doc-tests) |^test .* \.\.\. (ok|FAILED|ignored)' >"$CI_D_ARTIFACTS/tests.txt" ||
    fail "the list of the tests that cargo test ran cannot be written to its artifacts directory"
  [ "$code" = 0 ] || fail "cargo test exited ${code:-without a status}"
  kept_counts

  left=$(ls -A "$scratch") || fail "the temp directory of the test run cannot be listed"
  total=0
  exempted=0
  if [ -n "$left" ]; then
    total=$(printf '%s\n' "$left" | grep -c '')
    git grep -q -F -e "$exempt" -- '*.rs'
    code=$?
    case $code in
      0)
        exempted=$(printf '%s\n' "$left" | grep -c "^$exempt")
        left=$(printf '%s\n' "$left" | grep -v "^$exempt")
        ;;
      1) ;;
      *) fail "git grep exited $code" ;;
    esac
  fi
  printf 'entries that the test run left in its temp directory: %s, of them exempt: %s\n' "$total" "$exempted"
  if [ "$total" -gt "$exempted" ]; then
    names=$(printf '%s\n' "$left" | tr '\n' ' ')
    fail "the tests left $((total - exempted)) entries in their temp directory: ${names% }"
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  STUB_CALLS=$work/calls.$1
  mkdir -p "$work/artifacts/$1"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_JOB=macos
    CI_D_ARTIFACTS=$work/artifacts/$1
    RUNNER_TEMP=$work/runner-temp
    export PATH CI_D_REPO CI_D_JOB CI_D_ARTIFACTS RUNNER_TEMP STUB_CALLS STUB_BUILD_EXIT STUB_TEST_EXIT STUB_LEAVES STUB_MOVES STUB_KILLS_PARENT STUB_KEPT_RUNS
    unset CI_D_MERGE_GATE CI_D_TEST_GROUP
    if [ "$STUB_MERGE_GATE" = yes ]; then
      CI_D_MERGE_GATE=1
      export CI_D_MERGE_GATE
    fi
    sh "$0" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ -n "${4:-}" ]; then
    tests=
    [ ! -f "$work/artifacts/$1/tests.txt" ] || tests=$(cat "$work/artifacts/$1/tests.txt")
    [ "$tests" = "$4" ] || got=$got-without-the-tests-file
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_BUILD_EXIT=0
  STUB_TEST_EXIT=0
  STUB_LEAVES=
  STUB_MOVES=no
  STUB_KILLS_PARENT=no
  STUB_MERGE_GATE=no
  STUB_KEPT_RUNS=yes
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/runner-temp" "$work/literal-tracked/src" "$work/literal-gone/src"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
[ -f fixture-root ] || exit 98
printf '%s\n' "$*" >>"$STUB_CALLS"
case $* in
  'test --workspace --locked --no-run') exit "$STUB_BUILD_EXIT" ;;
  'test --workspace --locked') ;;
  'metadata --no-deps --format-version 1 --locked')
    printf '%s\n' '{"packages":[{"name":"pkg-a","version":"0.1.0","targets":[{"kind":["lib"],"crate_types":["lib"],"name":"pkg_a"},{"kind":["test"],"crate_types":["bin"],"name":"t1"}]}]}'
    exit 0
    ;;
  'test --workspace --exclude pkg-a --locked --lib --bins --no-run' | 'test --workspace --locked --test t1 --no-run' | 'test --locked -p pkg-a --lib --bins --no-run') exit 0 ;;
  'test --workspace --exclude pkg-a --locked --lib --bins --' | 'test --workspace --locked --test t1 --')
    printf '\nrunning 1 test\ntest tests::kept::elsewhere ... ok\r\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n'
    exit 0
    ;;
  'test --locked -p pkg-a --lib --bins -- tests::kept _windows::tests::')
    [ "$STUB_KEPT_RUNS" = no ] || printf 'test tests::kept::one ... ok\r\n'
    printf 'test tests::kept::two ... ignored, needs a window\r\n'
    exit 0
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
grep -q -e '--no-run' "$STUB_CALLS" || exit 94
[ "${HALVO_SECURE_INPUT_HOLD_REQUIRED:-}" = 1 ] || exit 92
[ -f "${HALVO_SHUTDOWN_ANSWERS:-}" ] || exit 91
printf 'shutdown source=input ok=false\nshutdown source=input ok=true\nshutdown source=screen ok=false\nshutdown source=screen ok=false\nshutdown source=inputs ok=false\n' >>"$HALVO_SHUTDOWN_ANSWERS"
case ${TMPDIR:-} in
  "$RUNNER_TEMP"/?*) ;;
  *) exit 93 ;;
esac
[ "${TMP:-}" = "$TMPDIR" ] || exit 96
[ "${TEMP:-}" = "$TMPDIR" ] || exit 96
left=$(ls -A "$TMPDIR")
[ -z "$left" ] || exit 95
for entry in $STUB_LEAVES; do
  mkdir "$TMPDIR/$entry"
done
[ "$STUB_MOVES" = no ] || mv "$TMPDIR" "$TMPDIR.moved"
[ "$STUB_KILLS_PARENT" = no ] || kill -s KILL "$PPID"
printf '     Running unittests src/lib.rs (target/debug/deps/stub-0123456789abcdef)\n' >&2
printf '\nrunning 3 tests\ntest a::one ... ok\ntest a::two ... ignored, needs a window\r\n'
if [ "$STUB_TEST_EXIT" -ne 0 ]; then
  printf 'test a::three ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out\n\n'
  exit "$STUB_TEST_EXIT"
fi
printf 'test a::three ... ok\n\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n\n'
printf '   Doc-tests stub\n' >&2
printf '\nrunning 1 test\ntest src/lib.rs - one (line 3) ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n'
EOF
  cat >"$work/bin/security" <<'EOF'
#!/bin/sh
[ "$*" = 'dump-keychain login.keychain-db' ] || exit 97
printf '    "svce"<blob>="com.halvo.capture.dev-fallback"\n'
printf '    "svce"<blob>="org.example.other"\n'
printf '    "svce"<blob>="com.halvo.capture.dev-fallback"\n'
EOF
  chmod +x "$work/bin/cargo" "$work/bin/security"
  printf 'let dir = temp_dir().join(format!("halvo-consent-test-{n}"));\n' >"$work/literal-tracked/src/a.rs"
  printf 'let dir = private_temp_dir();\n' >"$work/literal-gone/src/a.rs"
  printf 'The tests left halvo-consent-test-7-0 behind.\n' >"$work/literal-gone/notes.txt"
  for repo in "$work/literal-tracked" "$work/literal-gone"; do
    : >"$repo/fixture-root"
    git -C "$repo" init -q >/dev/null 2>&1
    git -C "$repo" add . >/dev/null 2>&1
  done
  printf 'let dir = "halvo-consent-test-0";\n' >"$work/literal-gone/src/untracked.rs"
  rejected=0
  wrong=
  STUB_BUILD_EXIT=0
  STUB_TEST_EXIT=0
  STUB_LEAVES=
  STUB_MOVES=no
  STUB_KILLS_PARENT=no
  STUB_MERGE_GATE=no
  STUB_KEPT_RUNS=yes
  mkdir -p "$work/merge-gate/scripts/ci"
  : >"$work/merge-gate/fixture-root"
  printf '%s\n' 'units pkg-a A every unit test' 'kept pkg-a tests::kept the tests of both jobs' 'kept pkg-a _windows::tests:: windows the tests of the windows job alone' 'doc-tests A' >"$work/merge-gate/scripts/ci/slow-tests.txt"

  nl='
'
  ran='     Running unittests src/lib.rs (target/debug/deps/stub-0123456789abcdef)'"${nl}test a::one ... ok${nl}test a::two ... ignored, needs a window"
  passed="$ran${nl}test a::three ... ok${nl}   Doc-tests stub${nl}test src/lib.rs - one (line 3) ... ok"
  failed="$ran${nl}test a::three ... FAILED"

  repo=$work/literal-tracked
  check tests-pass PASS "login-keychain items of com.halvo.capture.dev-fallback before the tests: 2${nl}     Running unittests src/lib.rs (target/debug/deps/stub-0123456789abcdef)" "$passed"
  check shutdown-answers-counted PASS "after the tests: 2${nl}shutdown_not_ok source=input count=1${nl}shutdown_not_ok source=foreground count=0${nl}shutdown_not_ok source=screen count=2${nl}"
  STUB_KILLS_PARENT=yes
  check test-status-lost FAIL 'cargo test exited without a status)'
  STUB_BUILD_EXIT=101
  check build-fails FAIL 'the test build failed: cargo test --no-run exited 101'
  STUB_TEST_EXIT=101
  check test-fails FAIL 'cargo test exited 101)' "$failed"
  STUB_TEST_EXIT=137
  check test-killed FAIL 'cargo test exited 137)' "$failed"
  STUB_LEAVES=stray
  check stray-entry FAIL 'the tests left 1 entries in their temp directory: stray)'
  STUB_LEAVES=.tmpAbC
  check stray-dot-entry FAIL 'the tests left 1 entries in their temp directory: .tmpAbC)'
  STUB_MOVES=yes
  check temp-directory-moved-away FAIL 'the temp directory of the test run cannot be listed)'
  STUB_LEAVES='halvo-consent-test-7-0 halvo-consent-test-7-1'
  check consent-directories-while-the-literal-is-tracked PASS 'temp directory: 2, of them exempt: 2'
  STUB_LEAVES='halvo-consent-test-7-0 stray'
  check stray-beside-consent-directories FAIL 'the tests left 1 entries in their temp directory: stray)'
  STUB_LEAVES=old-halvo-consent-test-7-0
  check entry-that-holds-the-literal-inside FAIL 'the tests left 1 entries in their temp directory: old-halvo-consent-test-7-0)'
  repo=$work/literal-gone
  STUB_LEAVES='halvo-consent-test-7-0 halvo-consent-test-7-1'
  check consent-directories-after-the-literal-is-gone FAIL 'the tests left 2 entries in their temp directory: halvo-consent-test-7-0 halvo-consent-test-7-1)'
  repo=$work/merge-gate
  STUB_MERGE_GATE=yes
  check kept-parts-counted PASS "kept_count package=pkg-a part=tests::kept count=1${nl}kept_not_counted package=pkg-a part=_windows::tests:: (scripts/ci/slow-tests.txt runs the part in the windows job alone)${nl}"
  STUB_MERGE_GATE=yes
  STUB_KEPT_RUNS=no
  check kept-part-runs-no-test FAIL "kept_count package=pkg-a part=tests::kept count=0${nl}kept_not_counted package=pkg-a part=_windows::tests:: (scripts/ci/slow-tests.txt runs the part in the windows job alone)${nl}FAIL $name (scripts/ci/slow-tests.txt keeps the unit tests of pkg-a whose name holds tests::kept, and no such test ran in the macos job; correct the part, write after it the one job that has such tests (macos or windows), or take the kept line out)"

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
