#!/bin/sh
# jobs: macos windows
# rule: No tracked file under crates/halvo-capture/src/store holds the word ANALYZE or a PRAGMA optimize, in any letter case, so that no statistics table holds values of the store's columns.
# lane: free
set -u
name=39-store-no-analyze
scope=crates/halvo-capture/src/store

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

statements() {
  awk '
    FNR == 1 { file = FILENAME; sub(/^.*\/\.\//, "", file) }
    {
      line = tolower($0)
      sub(/\r$/, "", line)
      if (line ~ /(^|[^a-z0-9_])analyze([^a-z0-9_]|$)/) print file ":" FNR ": ANALYZE"
      if (line ~ /pragma[[:space:]]+([a-z0-9_]+[[:space:]]*\.[[:space:]]*)?optimize([^a-z0-9_]|$)/) print file ":" FNR ": PRAGMA optimize"
    }
  ' "$@"
}

measure() {
  root=$1
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  git -C "$root" -c core.quotePath=false ls-files -- "$scope" >"$work/files" || fail "git ls-files failed in $root"
  set --
  while IFS= read -r file; do
    set -- "$@" "$root/./$file"
  done <"$work/files"
  files=$#
  if [ "$files" -eq 0 ]; then
    fail "git ls-files lists no tracked file under $scope"
  fi
  statements "$@" >"$work/found" || fail "awk failed"
  problems=0
  while IFS= read -r place; do
    printf 'problem %s: the store runs no ANALYZE and no PRAGMA optimize; take the statement out\n' "$place"
    problems=$((problems + 1))
  done <"$work/found"
  printf 'files %s problems %s\n' "$files" "$problems"
  if [ "$problems" -ne 0 ]; then
    fail "$problems statements that gather statistics"
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(CI_D_REPO=$work/$1 sh "$0" 2>&1)
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
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

fixture() {
  mkdir -p "$work/$1/$scope" || exit 2
  printf '%s\n' "$3" >"$work/$1/$scope/$2" || exit 2
  git -C "$work/$1" init -q || exit 2
  git -C "$work/$1" add -- "$scope/$2" || exit 2
}

clean='const SCHEMA: &str = "CREATE TABLE daily_totals (day INTEGER NOT NULL);";
fn analyzed_by_hand() {}
const NOTE: &str = "the analyzer of the logs";'

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture clean db.rs "$clean"
  check clean PASS 'files 1 problems 0'

  fixture analyze db.rs 'conn.execute_batch("ANALYZE;")?;'
  check analyze FAIL "problem $scope/db.rs:1: ANALYZE"

  fixture analyze-lower db.rs 'conn.execute_batch("analyze segment_keys")?;'
  check analyze-lower FAIL "problem $scope/db.rs:1: ANALYZE"

  fixture analyze-mixed expiry.rs 'let sql = "Analyze";'
  check analyze-mixed FAIL "problem $scope/expiry.rs:1: ANALYZE"

  fixture optimize db.rs 'conn.execute_batch("PRAGMA optimize;")?;'
  check optimize FAIL "problem $scope/db.rs:1: PRAGMA optimize"

  fixture optimize-schema db.rs 'conn.execute_batch("pragma main.optimize(0x10002)")?;'
  check optimize-schema FAIL "problem $scope/db.rs:1: PRAGMA optimize"

  fixture optimize-spaced db.rs 'conn.execute_batch("PRAGMA   main . optimize")?;'
  check optimize-spaced FAIL "problem $scope/db.rs:1: PRAGMA optimize"

  crlf=$(printf 'fn a() {}\r\nconst S: &str = "ANALYZE";\r') || exit 2
  fixture crlf db.rs "$crlf"
  check crlf FAIL "problem $scope/db.rs:2: ANALYZE"

  fixture deeper hardening_tests.rs "$clean"
  mkdir -p "$work/deeper/$scope/test_support" || exit 2
  printf '%s\n' 'conn.execute_batch("ANALYZE")' >"$work/deeper/$scope/test_support/pages.rs" || exit 2
  git -C "$work/deeper" add -- "$scope/test_support/pages.rs" || exit 2
  check deeper FAIL "problem $scope/test_support/pages.rs:1: ANALYZE"

  fixture outside db.rs "$clean"
  mkdir -p "$work/outside/crates/halvo-sqlcipher/tests" || exit 2
  printf '%s\n' 'conn.execute_batch("ANALYZE; PRAGMA optimize;")' >"$work/outside/crates/halvo-sqlcipher/tests/a.rs" || exit 2
  git -C "$work/outside" add -- crates/halvo-sqlcipher/tests/a.rs || exit 2
  check outside PASS 'files 1 problems 0'

  fixture untracked db.rs "$clean"
  printf '%s\n' 'conn.execute_batch("ANALYZE")' >"$work/untracked/$scope/scratch.rs" || exit 2
  check untracked PASS 'files 1 problems 0'

  mkdir -p "$work/no-store-file" || exit 2
  git -C "$work/no-store-file" init -q || exit 2
  check no-store-file FAIL "git ls-files lists no tracked file under $scope"

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure "$CI_D_REPO" ;;
  *) exit 64 ;;
esac
