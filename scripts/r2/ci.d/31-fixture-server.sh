#!/bin/sh
# jobs: macos windows
# rule: The tests that tools/fixture-server/tests.txt lists at HEAD for the job pass under node --test, which gets the listed test files by name, and the site data checker accepts every file of tools/fixture-server/sites/ that HEAD lists and counts at least 4 services for each seed category and 6 of the category other; the check fails when a listed test is renamed, removed, skipped or failing, when a test file of the list is not in HEAD, when a test file at HEAD under tools/fixture-server/ but variants/ is not in the list, when a test passes that the list does not hold, when a line of the list stands twice, when a line of the list or of the removal record tools/fixture-server/tests-removed.txt of the first parent stands in neither of them at HEAD or a line stands in both at HEAD, or when a tracked file differs from HEAD after node --test or after the checker, but it does not hold against code that runs in the job before its reads and writes what it reads (a test file, the list, node on the PATH), a test that keeps its name with another body, a test that leaves the list into the removal record (a landing's diff shows it by name), an untracked file, or the job's environment (ruling S-53).
set -u
name=31-fixture-server
list_path=tools/fixture-server/tests.txt
removed_path=tools/fixture-server/tests-removed.txt
cr=$(printf '\r')
nl='
'
seeds='utility_bill_payment tenant_rent lease_renewal_or_change tenant_application_review email spreadsheet_fill shop_order calendar_cleanup ride_order daily_chart_check'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

number() {
  case $1 in
    '' | *[!0-9]*) return 1 ;;
  esac
  return 0
}

read_list() {
  if ! awk -v cr="$cr" '{ sub(cr "$", "") } seen[$0]++ { twice = 1 } END { exit twice }' "$1"; then
    cause="a line of the test list stands twice"
    return 1
  fi
  tree=$nl
  while IFS= read -r line || [ -n "$line" ]; do
    tree="$tree${line%"$cr"}$nl"
  done <"$3"
  : >"$4"
  test_files=
  required_for_job=
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
    jobs=${line%% *}
    rest=${line#* }
    file=${rest%% *}
    test_name=${rest#* }
    if [ "$rest" = "$line" ] || [ "$test_name" = "$rest" ] || [ -z "$test_name" ]; then
      cause="a line of the test list has no job, file and name"
      return 1
    fi
    case $jobs in
      all | macos | windows) ;;
      *)
        cause="a line of the test list names no job"
        return 1
        ;;
    esac
    case $file in
      */* | .*) cause="a test file of the list is not a file of tools/fixture-server/"; return 1 ;;
      *.test.mjs) ;;
      *) cause="a test file of the list is not a .test.mjs file"; return 1 ;;
    esac
    case " $test_files " in
      *" $file "*) ;;
      *) test_files="$test_files $file" ;;
    esac
    [ "$jobs" = all ] || [ "$jobs" = "$2" ] || continue
    printf '%s\n' "$test_name" >>"$4"
    required_for_job=yes
  done <"$1"
  if [ -z "${required_for_job:-}" ]; then
    cause="the test list names no test for the job"
    return 1
  fi
  for file in $test_files; do
    case $tree in
      *"${nl}tools/fixture-server/$file$nl"*) ;;
      *)
        cause="a test file of the list is not in HEAD"
        return 1
        ;;
    esac
  done
  while IFS= read -r path || [ -n "$path" ]; do
    path=${path%"$cr"}
    case $path in
      tools/fixture-server/variants/*) continue ;;
      tools/fixture-server/*) relative=${path#tools/fixture-server/} ;;
      *) continue ;;
    esac
    is_test_file "$relative" || continue
    case " $test_files " in
      *" $relative "*) ;;
      *)
        cause="a test file at HEAD is not in the test list"
        return 1
        ;;
    esac
  done <"$3"
  return 0
}

is_test_file() {
  case $1 in
    *.js | *.mjs | *.cjs | *.ts | *.mts | *.cts) ;;
    *) return 1 ;;
  esac
  case /$1 in
    */test/*) return 0 ;;
  esac
  base=${1##*/}
  case ${base%.*} in
    *.test | *-test | *_test | test-* | test) return 0 ;;
  esac
  return 1
}

show_or_empty() {
  if git cat-file -e "$1:$2" 2>/dev/null; then
    git show "$1:$2" >"$3" 2>/dev/null || return 1
  else
    : >"$3" || return 1
  fi
  return 0
}

removal_record() {
  show_or_empty HEAD "$removed_path" "$1/removed.txt" || { cause="the removal record at HEAD cannot be read"; return 1; }
  if ! git rev-parse -q --verify 'HEAD^1^{commit}' >/dev/null 2>&1; then
    shallow_file=$(git rev-parse --git-path shallow) || { cause="git cannot name the shallow file of the clone"; return 1; }
    head_id=$(git rev-parse HEAD) || { cause="git cannot name HEAD"; return 1; }
    if [ -f "$shallow_file" ] && grep -q -x -F -e "$head_id" "$shallow_file"; then
      cause="the first parent of HEAD is not in this clone: fetch it (fetch-depth 2 or more on actions/checkout)"
      return 1
    fi
    record_line='removal record: no first parent, nothing lost'
    return 0
  fi
  if ! git cat-file -e "HEAD^1:$list_path" 2>/dev/null; then
    record_line='removal record: the first parent holds no list, nothing lost'
    return 0
  fi
  show_or_empty 'HEAD^1' "$list_path" "$1/parent-list.txt" || { cause="the list of the first parent cannot be read"; return 1; }
  show_or_empty 'HEAD^1' "$removed_path" "$1/parent-removed.txt" || { cause="the removal record of the first parent cannot be read"; return 1; }
  if ! result=$(awk -v cr="$cr" '
    { sub(cr "$", "") }
    $0 == "" { next }
    FILENAME == ARGV[1] || FILENAME == ARGV[2] { if (!($0 in old)) { old[$0] = 1; order[++n] = $0 }; next }
    FILENAME == ARGV[3] { listed[$0] = 1; next }
    { if (!($0 in recorded)) { recorded[$0] = 1; record[++r] = $0 } }
    END {
      for (i = 1; i <= r; i++) if (record[i] in listed) { print "both " record[i]; exit }
      for (i = 1; i <= n; i++) if (!(order[i] in listed) && !(order[i] in recorded)) { print "lost " order[i]; exit }
      printf "ok %d %d\n", n, r
    }
  ' "$1/parent-list.txt" "$1/parent-removed.txt" "$1/tests.txt" "$1/removed.txt"); then
    cause="the lists of HEAD and of its first parent cannot be compared"
    return 1
  fi
  case $result in
    'both '*)
      cause="the line '${result#both }' stands in the list and in the record at HEAD"
      return 1
      ;;
    'lost '*)
      cause="the line '${result#lost }' of the first parent's list or record is neither in the list nor in the record at HEAD"
      return 1
      ;;
  esac
  rest=${result#ok }
  record_line="removal record: lines of the first parent=${rest%% *} recorded=${rest#* } lost=0"
  return 0
}

judge_list() {
  if ! counts=$(awk -v cr="$cr" '
    { sub(cr "$", "") }
    FILENAME == ARGV[1] { need[$0] = 1; n++; next }
    /^ok [0-9]+ - / { test_name = $0; sub(/^ok [0-9]+ - /, "", test_name); passed[test_name] = 1 }
    END {
      for (k in need) if (!(k in passed)) miss++
      for (k in passed) if (!(k in need)) extra++
      printf "%d %d %d\n", n, miss, extra
    }
  ' "$1" "$2"); then
    cause="the test list cannot be compared with the output of node --test"
    return 1
  fi
  listed=${counts%% *}
  rest=${counts#* }
  missing=${rest%% *}
  extra=${rest#* }
  if [ "$missing" != 0 ]; then
    cause="$missing listed tests have no passing line"
    return 1
  fi
  if [ "$extra" != 0 ]; then
    cause="a test passed that the list does not hold"
    return 1
  fi
  return 0
}

judge_tests() {
  t_tests=
  t_pass=
  t_fail=
  t_cancelled=
  t_skipped=
  t_todo=
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
    case $line in
      '# tests '*) t_tests=${line#'# tests '} ;;
      '# pass '*) t_pass=${line#'# pass '} ;;
      '# fail '*) t_fail=${line#'# fail '} ;;
      '# cancelled '*) t_cancelled=${line#'# cancelled '} ;;
      '# skipped '*) t_skipped=${line#'# skipped '} ;;
      '# todo '*) t_todo=${line#'# todo '} ;;
    esac
  done <"$2"
  if [ "$1" -ne 0 ]; then
    cause="node --test exited $1"
    return 1
  fi
  for count in "$t_tests" "$t_pass" "$t_fail" "$t_cancelled" "$t_skipped" "$t_todo"; do
    if ! number "$count"; then
      cause="the output of node --test holds no summary of its tests"
      return 1
    fi
  done
  if [ "$t_pass" -eq 0 ] || [ "$t_tests" -ne "$t_pass" ]; then
    cause="node --test passed $t_pass of $t_tests tests"
    return 1
  fi
  if [ "$t_fail" -ne 0 ] || [ "$t_cancelled" -ne 0 ] || [ "$t_skipped" -ne 0 ] || [ "$t_todo" -ne 0 ]; then
    cause="node --test shows a test failed, cancelled, skipped or todo"
    return 1
  fi
  return 0
}

judge_checker() {
  if [ "$1" -ne 0 ]; then
    refused=
    while IFS= read -r line || [ -n "$line" ]; do
      line=${line%"$cr"}
      case $line in
        'REFUSED '*:*)
          refused=${line#REFUSED }
          refused=${refused%%:*}
          break
          ;;
      esac
    done <"$3"
    case $refused in
      usage | site-data | parameters) cause="the checker refused with $refused" ;;
      *) cause="the checker exited $1" ;;
    esac
    return 1
  fi
  c_files=
  c_services=0
  c_password=0
  c_card=0
  found=' '
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
    case $line in
      'checked files='*) c_files=${line#checked files=} ;;
      'services '*)
        rest=${line#services }
        category=${rest%% *}
        count=${rest#* }
        floor=4
        [ "$category" = other ] && floor=6
        if ! number "$count" || [ "$count" -lt "$floor" ]; then
          cause="the checker counts fewer than $floor services of a category"
          return 1
        fi
        found="$found$category "
        c_services=$((c_services + count))
        ;;
      'password '*) c_password=$((c_password + 1)) ;;
      'card '*) c_card=$((c_card + 1)) ;;
    esac
  done <"$2"
  for category in $seeds other; do
    case $found in
      *" $category "*) ;;
      *)
        cause="the checker prints no count for a category"
        return 1
        ;;
    esac
  done
  if [ "$c_files" != "$4" ] || [ "$c_services" != "$4" ]; then
    cause="the checker read another number of files than the commit lists"
    return 1
  fi
  return 0
}


judge() {
  cause=
  judge_tests "$1" "$2" || return 1
  judge_checker "$3" "$4" "$5" "$6" || return 1
  return 0
}

measure() {
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  job=${CI_D_JOB:-}
  case $job in
    macos | windows) ;;
    *) fail "CI_D_JOB is neither macos nor windows" ;;
  esac
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  if ! command -v node >/dev/null 2>&1; then
    [ "${CI:-}" = true ] && fail "node is not installed"
    printf 'SKIP %s (node is not installed)\n' "$name"
    exit 77
  fi
  art=$CI_D_ARTIFACTS
  git show "HEAD:$list_path" >"$art/tests.txt" 2>/dev/null || fail "the commit holds no list $list_path"
  git ls-tree -r --name-only HEAD -- tools/fixture-server/ >"$art/tree.txt" || fail "git ls-tree of tools/fixture-server failed"
  git ls-tree -r --name-only HEAD -- tools/fixture-server/sites/ >"$art/sites.txt" || fail "git ls-tree of the committed site data failed"
  read_list "$art/tests.txt" "$job" "$art/tree.txt" "$art/required.txt" || fail "$cause"
  set --
  for file in $test_files; do
    set -- "$@" "tools/fixture-server/$file"
  done
  node --test --test-reporter=tap --test-concurrency=1 "$@" >"$art/node-test.txt" 2>&1
  test_code=$?
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after node --test"
  set --
  while IFS= read -r file; do
    set -- "$@" "$file"
  done <"$art/sites.txt"
  [ "$#" -gt 0 ] || fail "the commit holds no site data file"
  files=$#
  node tools/fixture-server/check-sites.mjs "$@" >"$art/check-sites.txt" 2>"$art/check-sites.err"
  check_code=$?
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the checker"
  judge "$test_code" "$art/node-test.txt" "$check_code" "$art/check-sites.txt" "$art/check-sites.err" "$files" || fail "$cause"
  judge_list "$art/required.txt" "$art/node-test.txt" || fail "$cause"
  removal_record "$art" || fail "$cause"
  printf 'node --test: tests=%s pass=%s listed=%s\n' "$t_tests" "$t_pass" "$listed"
  printf 'checker: files=%s password_pages=%s card_pages=%s\n' "$c_files" "$c_password" "$c_card"
  printf '%s\n' "$record_line"
  printf 'PASS %s\n' "$name"
}

summary() {
  printf '# tests %s\n# suites 0\n# pass %s\n# fail %s\n# cancelled %s\n# skipped %s\n# todo %s\n# duration_ms 900.5\n' "$@"
}

counts() {
  printf 'checked files=%s\n' "$1"
  shift
  for category in $seeds other; do
    printf 'services %s %s\n' "$category" "$1"
    shift
  done
  printf 'password sites/a.json sign-in\ncard sites/b.json pay\n'
}

verdict() {
  case $cause in
    *"$3"*) ;;
    *) got=$got-without-the-cause ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

check() {
  if judge "$2" "$work/$3" "$4" "$work/$5" "$work/$6" "$7"; then got=PASS; else got=FAIL; fi
  verdict "$1" "$9" "$8"
}

run_measure() {
  git -C "$repo" checkout -q "$2" || exit 2
  mkdir -p "$work/art/$1" || exit 2
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_JOB=$3
    CI_D_ARTIFACTS=$work/art/$1
    STUB_TAP=$work/$4
    STUB_CHECK=$work/$5
    STUB_TOUCH_TEST=${6:-}
    STUB_TOUCH_CHECK=${7:-}
    STUB_FILES=${8:-}
    export PATH CI_D_REPO CI_D_JOB CI_D_ARTIFACTS STUB_TAP STUB_CHECK STUB_TOUCH_TEST STUB_TOUCH_CHECK STUB_FILES
    sh "$0" 2>&1
  )
  code=$?
  git -C "$repo" checkout -q -- . || exit 2
  case $code:$out in
    *"${nl}PASS $name") got=PASS ;;
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")" | *"${nl}FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  [ "$code" = 0 ] || [ "$got" = FAIL ] || got=exit-$code
  cause=${out##*"FAIL $name ("}
}

make_repo() {
  repo=$work/repo
  mkdir -p "$repo/tools/fixture-server/sites" "$work/bin" || exit 2
  git init -q "$repo" || exit 2
  printf 'all a.test.mjs first test\nall a.test.mjs second test\nall b.test.mjs third test\nwindows b.test.mjs windows test\n' >"$repo/$list_path"
  printf 'a\n' >"$repo/tools/fixture-server/a.test.mjs"
  printf 'b\n' >"$repo/tools/fixture-server/b.test.mjs"
  printf 'h\n' >"$repo/tools/fixture-server/helper.mjs"
  i=0
  while [ "$i" -lt 46 ]; do
    i=$((i + 1))
    printf '{}\n' >"$repo/tools/fixture-server/sites/s$i.json"
  done
  git -C "$repo" add -A && git -C "$repo" commit -q -m base || exit 2
  git -C "$repo" branch -q -M main || exit 2
  git -C "$repo" checkout -q -b gone && git -C "$repo" rm -q tools/fixture-server/b.test.mjs && git -C "$repo" commit -q -m gone || exit 2
  git -C "$repo" checkout -q -b twice main || exit 2
  printf 'all a.test.mjs first test\n' >>"$repo/$list_path"
  git -C "$repo" commit -q -a -m twice || exit 2
  git -C "$repo" checkout -q -b nolist main && git -C "$repo" rm -q "$list_path" && git -C "$repo" commit -q -m nolist || exit 2
  git -C "$repo" checkout -q -b unlistedfile main || exit 2
  printf 'all a.test.mjs first test\nall a.test.mjs second test\n' >"$repo/$list_path"
  git -C "$repo" commit -q -a -m unlistedfile || exit 2
  git -C "$repo" checkout -q -b droppedtest main || exit 2
  printf 'all a.test.mjs second test\nall b.test.mjs third test\nwindows b.test.mjs windows test\n' >"$repo/$list_path"
  git -C "$repo" commit -q -a -m droppedtest || exit 2
  git -C "$repo" checkout -q -b otherjob main || exit 2
  printf 'c\n' >"$repo/tools/fixture-server/c.test.mjs"
  printf 'windows c.test.mjs windows only test\n' >>"$repo/$list_path"
  git -C "$repo" add -A && git -C "$repo" commit -q -m otherjob || exit 2
  removed=$repo/tools/fixture-server/tests-removed.txt
  a_lines='all a.test.mjs first test\nall a.test.mjs second test\n'
  b_lines='all b.test.mjs third test\nwindows b.test.mjs windows test\n'
  for branch in t3 t3record t3partial; do
    git -C "$repo" checkout -q -b "$branch" main && git -C "$repo" rm -q tools/fixture-server/b.test.mjs || exit 2
    printf %b "$a_lines" >"$repo/$list_path"
    case $branch in
      t3record) printf %b "$b_lines" >"$removed" ;;
      t3partial) printf 'all b.test.mjs third test\n' >"$removed" ;;
    esac
    git -C "$repo" add -A && git -C "$repo" commit -q -m "$branch" || exit 2
  done
  git -C "$repo" checkout -q -b both main || exit 2
  printf 'all a.test.mjs first test\n' >"$removed"
  git -C "$repo" add -A && git -C "$repo" commit -q -m both || exit 2
  git -C "$repo" checkout -q -b recorddrop t3record || exit 2
  printf 'all b.test.mjs third test\n' >"$removed"
  git -C "$repo" commit -q -a -m recorddrop || exit 2
  git -C "$repo" checkout -q -b back t3record || exit 2
  printf 'b\n' >"$repo/tools/fixture-server/b.test.mjs"
  printf %b "$a_lines$b_lines" >"$repo/$list_path"
  : >"$removed"
  git -C "$repo" add -A && git -C "$repo" commit -q -m back || exit 2
  git -C "$repo" checkout -q -b nolistchild nolist || exit 2
  printf %b "$a_lines$b_lines" >"$repo/$list_path"
  git -C "$repo" add -A && git -C "$repo" commit -q -m nolistchild || exit 2
  git clone -q --depth 1 --branch t3record "file://$repo" "$work/shallow" || exit 2
  git -C "$repo" checkout -q main || exit 2
  for branch in newfile:c.test.mjs testdir:test/helper.mjs variants:variants/x.test.mjs; do
    mkdir -p "$repo/tools/fixture-server/test" "$repo/tools/fixture-server/variants" || exit 2
    git -C "$repo" checkout -q -b "${branch%%:*}" main || exit 2
    printf 'x\n' >"$repo/tools/fixture-server/${branch#*:}"
    git -C "$repo" add -A && git -C "$repo" commit -q -m "${branch%%:*}" || exit 2
  done
  cat >"$work/bin/node" <<'EOF'
#!/bin/sh
case $1 in
  --test)
    [ "$*" = "--test --test-reporter=tap --test-concurrency=1 ${STUB_FILES:-tools/fixture-server/a.test.mjs tools/fixture-server/b.test.mjs}" ] || { printf 'stub node: got the arguments: %s\n' "$*"; exit 97; }
    cat "$STUB_TAP"
    [ -z "$STUB_TOUCH_TEST" ] || printf 'x\n' >>"$STUB_TOUCH_TEST"
    exit 0
    ;;
  tools/fixture-server/check-sites.mjs)
    [ "$#" = 47 ] || { printf 'stub node: the checker got %s arguments\n' "$#"; exit 96; }
    cat "$STUB_CHECK"
    [ -z "$STUB_TOUCH_CHECK" ] || printf 'x\n' >>"$STUB_TOUCH_CHECK"
    exit 0
    ;;
esac
exit 95
EOF
  chmod +x "$work/bin/node" || exit 2
}

measure_cases() {
  make_repo
  touched=$repo/tools/fixture-server/a.test.mjs
  { printf 'ok 1 - first test\nok 2 - second test\nok 3 - third test\n'; summary 3 3 0 0 0 0; } >"$work/tap-list"
  { printf 'ok 1 - first test\nok 2 - second test\n'; summary 2 2 0 0 0 0; } >"$work/tap-narrowed"
  cp "$work/tap-narrowed" "$work/tap-a" || exit 2
  { printf 'ok 1 - first test\nok 2 - second test\nok 3 - third test renamed\n'; summary 3 3 0 0 0 0; } >"$work/tap-renamed"
  { printf 'ok 1 - first test\nok 2 - second test\nok 3 - third test\nok 4 - fourth test\n'; summary 4 4 0 0 0 0; } >"$work/tap-extra"
  { printf 'ok 1 - first test\nok 2 - second test\nok 3 - third test\nok 4 - windows test\n'; summary 4 4 0 0 0 0; } >"$work/tap-windows"

  run_measure measure-passes main macos tap-list check-ok
  verdict measure-passes PASS 'removal record: no first parent, nothing lost'
  run_measure measure-passes-on-windows main windows tap-windows check-ok
  verdict measure-passes-on-windows PASS ''
  run_measure test-file-deleted-by-a-commit gone macos tap-list check-ok
  verdict test-file-deleted-by-a-commit FAIL 'a test file of the list is not in HEAD'
  run_measure run-narrowed-to-one-file main macos tap-narrowed check-ok
  verdict run-narrowed-to-one-file FAIL '1 listed tests have no passing line'
  run_measure listed-name-without-passing-line main macos tap-renamed check-ok
  verdict listed-name-without-passing-line FAIL '1 listed tests have no passing line'
  run_measure windows-line-without-passing-line main windows tap-list check-ok
  verdict windows-line-without-passing-line FAIL '1 listed tests have no passing line'
  run_measure test-passes-outside-the-list main macos tap-extra check-ok
  verdict test-passes-outside-the-list FAIL 'a test passed that the list does not hold'
  run_measure line-of-the-list-twice twice macos tap-list check-ok
  verdict line-of-the-list-twice FAIL 'a line of the test list stands twice'
  run_measure no-list-in-the-commit nolist macos tap-list check-ok
  verdict no-list-in-the-commit FAIL 'the commit holds no list'
  run_measure test-file-at-head-unlisted unlistedfile macos tap-list check-ok
  verdict test-file-at-head-unlisted FAIL 'a test file at HEAD is not in the test list'
  run_measure one-test-of-a-listed-file-unlisted droppedtest macos tap-list check-ok
  verdict one-test-of-a-listed-file-unlisted FAIL 'a test passed that the list does not hold'
  run_measure new-test-file-unlisted newfile macos tap-list check-ok
  verdict new-test-file-unlisted FAIL 'a test file at HEAD is not in the test list'
  run_measure file-in-a-test-directory-unlisted testdir macos tap-list check-ok
  verdict file-in-a-test-directory-unlisted FAIL 'a test file at HEAD is not in the test list'
  run_measure file-of-the-other-job-is-run-too otherjob macos tap-list check-ok '' '' \
    'tools/fixture-server/a.test.mjs tools/fixture-server/b.test.mjs tools/fixture-server/c.test.mjs'
  verdict file-of-the-other-job-is-run-too PASS ''
  run_measure test-file-under-variants-is-another-runs variants macos tap-list check-ok
  verdict test-file-under-variants-is-another-runs PASS ''
  only_a=tools/fixture-server/a.test.mjs
  run_measure t3-file-and-its-lines-taken-out t3 macos tap-a check-ok '' '' "$only_a"
  verdict t3-file-and-its-lines-taken-out FAIL "the line 'all b.test.mjs third test' of the first parent"
  run_measure t3-lines-into-the-record t3record macos tap-a check-ok '' '' "$only_a"
  verdict t3-lines-into-the-record PASS 'removal record: lines of the first parent=4 recorded=2 lost=0'
  run_measure t3-one-line-missing-from-the-record t3partial macos tap-a check-ok '' '' "$only_a"
  verdict t3-one-line-missing-from-the-record FAIL "the line 'windows b.test.mjs windows test' of the first parent"
  run_measure line-in-the-record-and-the-list both macos tap-list check-ok
  verdict line-in-the-record-and-the-list FAIL "the line 'all a.test.mjs first test' stands in the list and in the record"
  run_measure line-out-of-the-record-not-into-the-list recorddrop macos tap-a check-ok '' '' "$only_a"
  verdict line-out-of-the-record-not-into-the-list FAIL "the line 'windows b.test.mjs windows test' of the first parent"
  run_measure lines-back-from-the-record back macos tap-list check-ok
  verdict lines-back-from-the-record PASS 'removal record: lines of the first parent=4 recorded=0 lost=0'
  run_measure parent-without-a-list nolistchild macos tap-list check-ok
  verdict parent-without-a-list PASS 'removal record: the first parent holds no list, nothing lost'
  kept=$repo
  repo=$work/shallow
  run_measure parent-not-in-a-shallow-clone t3record macos tap-a check-ok '' '' "$only_a"
  repo=$kept
  verdict parent-not-in-a-shallow-clone FAIL 'fetch it'
  run_measure services-count-not-the-file-count main macos tap-list check-services-count
  verdict services-count-not-the-file-count FAIL 'another number of files'
  run_measure tracked-file-changed-by-the-tests main macos tap-list check-ok "$touched"
  verdict tracked-file-changed-by-the-tests FAIL 'a tracked file differs from HEAD after node --test'
  run_measure tracked-file-changed-by-the-checker main macos tap-list check-ok '' "$touched"
  verdict tracked-file-changed-by-the-checker FAIL 'a tracked file differs from HEAD after the checker'
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  ok_tap='ok 1 - site data: the three fixtures are of C11.1'
  { printf 'TAP version 13\n%s\n# services other 6\n# tests 9\n' "$ok_tap"; summary 62 62 0 0 0 0; } >"$work/tap-ok"
  { printf 'TAP version 13%s\n%s%s\n' "$cr" "$ok_tap" "$cr"; summary "62$cr" "62$cr" "0$cr" "0$cr" "0$cr" "0$cr"; } >"$work/tap-crlf"
  { printf 'not ok 2 - server\n'; summary 62 61 1 0 0 0; } >"$work/tap-failed"
  summary 62 61 1 0 0 0 >"$work/tap-fail-count"
  summary 0 0 0 0 0 0 >"$work/tap-none"
  summary 62 61 0 0 1 0 >"$work/tap-skipped"
  summary 62 62 0 0 1 0 >"$work/tap-skipped-counted"
  summary 62 62 0 1 0 0 >"$work/tap-cancelled"
  summary 62 62 0 0 0 1 >"$work/tap-todo"
  summary 62 62 1 0 0 0 >"$work/tap-failed-counted"
  summary 63 62 0 0 0 0 >"$work/tap-uncounted"
  printf 'TAP version 13\n%s\n' "$ok_tap" >"$work/tap-cut"
  counts 46 4 4 4 4 4 4 4 4 4 4 6 >"$work/check-ok"
  counts 46 4 4 4 4 4 4 4 4 4 4 6 | tr '\n' '\r' >"$work/check-cr-only"
  counts 47 4 4 4 4 4 4 4 4 4 5 6 >"$work/check-other-ok"
  counts 45 4 4 4 4 4 4 4 4 4 3 6 >"$work/check-seed-short"
  counts 45 4 4 4 4 4 4 4 4 4 4 5 >"$work/check-other-short"
  counts 46 4 4 4 4 4 4 4 4 4 4 6 | sed '/^services email /d' >"$work/check-category-missing"
  counts 47 4 4 4 4 4 4 4 4 4 4 6 >"$work/check-file-count"
  counts 46 4 4 4 4 4 4 4 4 4 4 7 >"$work/check-services-count"
  : >"$work/empty"
  printf 'REFUSED usage: usage: node check-sites.mjs [--parameters FILE] SITE_DATA...\n' >"$work/err-usage"
  printf 'REFUSED site-data: tools/fixture-server/sites/a.json\n' >"$work/err-site-data"
  printf 'REFUSED parameters: docs/r2/category-parameters.md\n' >"$work/err-parameters"
  printf 'checked files=46\nparameters names=10 missing=1\nmissing sites/a.json email folder\n' >"$work/check-missing"
  printf 'node:internal/modules/run_main:1\nError: boom\n' >"$work/err-crash"

  check passes 0 tap-ok 0 check-ok empty 46 '' PASS
  check passes-with-crlf-line-ends 0 tap-crlf 0 check-ok empty 46 '' PASS
  check passes-with-more-services-than-the-floor 0 tap-ok 0 check-other-ok empty 47 '' PASS
  check tests-fail 1 tap-failed 0 check-ok empty 46 'node --test exited 1' FAIL
  check exit-0-with-a-failed-count 0 tap-fail-count 0 check-ok empty 46 'node --test passed 61 of 62 tests' FAIL
  check no-test-ran 0 tap-none 0 check-ok empty 46 'node --test passed 0 of 0 tests' FAIL
  check a-test-skipped 0 tap-skipped 0 check-ok empty 46 'node --test passed 61 of 62 tests' FAIL
  check a-test-skipped-and-counted-as-passed 0 tap-skipped-counted 0 check-ok empty 46 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-test-cancelled 0 tap-cancelled 0 check-ok empty 46 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-test-todo 0 tap-todo 0 check-ok empty 46 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-failed-count-beside-all-passed 0 tap-failed-counted 0 check-ok empty 46 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-test-uncounted 0 tap-uncounted 0 check-ok empty 46 'node --test passed 62 of 63 tests' FAIL
  check summary-cut 0 tap-cut 0 check-ok empty 46 'holds no summary' FAIL
  check checker-refuses-usage 0 tap-ok 2 empty err-usage 46 'the checker refused with usage' FAIL
  check checker-refuses-site-data 0 tap-ok 2 empty err-site-data 46 'the checker refused with site-data' FAIL
  check checker-refuses-parameters 0 tap-ok 2 empty err-parameters 46 'the checker refused with parameters' FAIL
  check checker-finds-a-name-without-row 0 tap-ok 1 check-missing empty 46 'the checker exited 1' FAIL
  check checker-crashes 0 tap-ok 1 empty err-crash 46 'the checker exited 1' FAIL
  check checker-without-line-ends 0 tap-ok 0 check-cr-only empty 46 'prints no count for a category' FAIL
  check seed-category-below-4 0 tap-ok 0 check-seed-short empty 45 'fewer than 4 services' FAIL
  check other-below-6 0 tap-ok 0 check-other-short empty 45 'fewer than 6 services' FAIL
  check category-missing 0 tap-ok 0 check-category-missing empty 46 'prints no count for a category' FAIL
  check checker-read-another-count 0 tap-ok 0 check-file-count empty 46 'another number of files' FAIL
  check services-sum-not-the-file-count 0 tap-ok 0 check-services-count empty 46 'another number of files' FAIL

  measure_cases

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  rm -rf "$work"
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
