#!/bin/sh
# jobs: macos windows
# rule: The tests of tools/fixture-server pass with none failed, skipped, cancelled or todo, the site data checker accepts every committed file of tools/fixture-server/sites/, and it counts at least 4 services for each seed category and 6 of the category other.
set -u
name=31-fixture-server
cr=$(printf '\r')
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
  if [ "$7" -ne 0 ]; then
    cause="a tracked file differs from HEAD after the commands"
    return 1
  fi
  return 0
}

measure() {
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  if ! command -v node >/dev/null 2>&1; then
    [ "${CI:-}" = true ] && fail "node is not installed"
    printf 'SKIP %s (node is not installed)\n' "$name"
    exit 77
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  git ls-tree -r --name-only HEAD -- tools/fixture-server/sites/ >"$work/sites.txt" || fail "git ls-tree of the committed site data failed"
  set --
  while IFS= read -r file; do
    set -- "$@" "$file"
  done <"$work/sites.txt"
  [ "$#" -gt 0 ] || fail "the commit holds no site data file"
  files=$#
  node --test --test-reporter=tap tools/fixture-server/ >"$CI_D_ARTIFACTS/node-test.txt" 2>&1
  test_code=$?
  node tools/fixture-server/check-sites.mjs "$@" >"$CI_D_ARTIFACTS/check-sites.txt" 2>"$CI_D_ARTIFACTS/check-sites.err"
  check_code=$?
  git diff --quiet HEAD --
  dirty=$?
  judge "$test_code" "$CI_D_ARTIFACTS/node-test.txt" "$check_code" "$CI_D_ARTIFACTS/check-sites.txt" "$CI_D_ARTIFACTS/check-sites.err" "$files" "$dirty" || fail "$cause"
  printf 'node --test: tests=%s pass=%s\n' "$t_tests" "$t_pass"
  printf 'checker: files=%s password_pages=%s card_pages=%s\n' "$c_files" "$c_password" "$c_card"
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

check() {
  if judge "$2" "$work/$3" "$4" "$work/$5" "$work/$6" "$7" "$8"; then got=PASS; else got=FAIL; fi
  case $cause in
    *"$9"*) ;;
    *) got=$got-without-the-cause ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "${10}" "$got"
  if [ "$got" != "${10}" ]; then
    wrong="$wrong $1"
  elif [ "${10}" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
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
  : >"$work/empty"
  printf 'REFUSED usage: usage: node check-sites.mjs [--parameters FILE] SITE_DATA...\n' >"$work/err-usage"
  printf 'REFUSED site-data: tools/fixture-server/sites/a.json\n' >"$work/err-site-data"
  printf 'REFUSED parameters: docs/r2/category-parameters.md\n' >"$work/err-parameters"
  printf 'checked files=46\nparameters names=10 missing=1\nmissing sites/a.json email folder\n' >"$work/check-missing"
  printf 'node:internal/modules/run_main:1\nError: boom\n' >"$work/err-crash"

  check passes 0 tap-ok 0 check-ok empty 46 0 '' PASS
  check passes-with-crlf-line-ends 0 tap-crlf 0 check-ok empty 46 0 '' PASS
  check passes-with-more-services-than-the-floor 0 tap-ok 0 check-other-ok empty 47 0 '' PASS
  check tests-fail 1 tap-failed 0 check-ok empty 46 0 'node --test exited 1' FAIL
  check exit-0-with-a-failed-count 0 tap-fail-count 0 check-ok empty 46 0 'node --test passed 61 of 62 tests' FAIL
  check no-test-ran 0 tap-none 0 check-ok empty 46 0 'node --test passed 0 of 0 tests' FAIL
  check a-test-skipped 0 tap-skipped 0 check-ok empty 46 0 'node --test passed 61 of 62 tests' FAIL
  check a-test-skipped-and-counted-as-passed 0 tap-skipped-counted 0 check-ok empty 46 0 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-test-cancelled 0 tap-cancelled 0 check-ok empty 46 0 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-test-todo 0 tap-todo 0 check-ok empty 46 0 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-failed-count-beside-all-passed 0 tap-failed-counted 0 check-ok empty 46 0 'shows a test failed, cancelled, skipped or todo' FAIL
  check a-test-uncounted 0 tap-uncounted 0 check-ok empty 46 0 'node --test passed 62 of 63 tests' FAIL
  check summary-cut 0 tap-cut 0 check-ok empty 46 0 'holds no summary' FAIL
  check checker-refuses-usage 0 tap-ok 2 empty err-usage 46 0 'the checker refused with usage' FAIL
  check checker-refuses-site-data 0 tap-ok 2 empty err-site-data 46 0 'the checker refused with site-data' FAIL
  check checker-refuses-parameters 0 tap-ok 2 empty err-parameters 46 0 'the checker refused with parameters' FAIL
  check checker-finds-a-name-without-row 0 tap-ok 1 check-missing empty 46 0 'the checker exited 1' FAIL
  check checker-crashes 0 tap-ok 1 empty err-crash 46 0 'the checker exited 1' FAIL
  check checker-without-line-ends 0 tap-ok 0 check-cr-only empty 46 0 'prints no count for a category' FAIL
  check seed-category-below-4 0 tap-ok 0 check-seed-short empty 45 0 'fewer than 4 services' FAIL
  check other-below-6 0 tap-ok 0 check-other-short empty 45 0 'fewer than 6 services' FAIL
  check category-missing 0 tap-ok 0 check-category-missing empty 46 0 'prints no count for a category' FAIL
  check checker-read-another-count 0 tap-ok 0 check-file-count empty 46 0 'another number of files' FAIL
  check tracked-file-changed 0 tap-ok 0 check-ok empty 46 1 'a tracked file differs from HEAD' FAIL

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
