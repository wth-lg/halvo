#!/bin/sh
# jobs: macos windows
# rule: An ignore attribute on a test carries a reason string.
# lane: free
set -u
name=30-store-ignore-reason

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

bare_ignores() {
  awk '
    FNR == 1 { file = FILENAME; sub(/^.*\/\.\//, "", file) }
    {
      line = $0
      sub(/\r$/, "", line)
      bare = line ~ /#\[[[:space:]]*ignore[[:space:]]*\]/
      empty = line ~ /#\[[[:space:]]*ignore[[:space:]]*=[[:space:]]*""[[:space:]]*\]/
      in_cfg_attr = line ~ /cfg_attr[[:space:]]*\(.*[,(][[:space:]]*ignore[[:space:]]*[,)]/
      if (bare || empty || in_cfg_attr) print file ":" FNR
    }
  ' "$@"
}

measure() {
  root=$1
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  git -C "$root" -c core.quotePath=false ls-files -- '*.rs' >"$work/files" || fail "git ls-files failed in $root"
  set --
  while IFS= read -r file; do
    set -- "$@" "$root/./$file"
  done <"$work/files"
  files=$#
  if [ "$files" -eq 0 ]; then
    fail "git ls-files lists no tracked .rs file"
  fi
  bare_ignores "$@" >"$work/found" || fail "awk failed"
  problems=0
  while IFS= read -r place; do
    printf 'problem %s: an ignore attribute without a reason string; write #[ignore = "<why it is ignored>"]\n' "$place"
    problems=$((problems + 1))
  done <"$work/found"
  printf 'files %s problems %s\n' "$files" "$problems"
  if [ "$problems" -ne 0 ]; then
    fail "$problems ignore attributes without a reason string"
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
  mkdir -p "$work/$1/src" || exit 2
  printf '%s\n' "$3" >"$work/$1/src/$2" || exit 2
  git -C "$work/$1" init -q || exit 2
  git -C "$work/$1" add -- "src/$2" || exit 2
}

with_reason='#[test]
#[ignore = "run with: cargo test -- --ignored"]
fn slow() {}

#[cfg_attr(windows, ignore = "no FIFO on this system")]
#[test]
fn fifo() {}'

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture reasons-given lib.rs "$with_reason"
  check reasons-given PASS 'files 1 problems 0'

  fixture bare-ignore lib.rs '#[test]
#[ignore]
fn slow() {}'
  check bare-ignore FAIL 'problem src/lib.rs:2: an ignore attribute without a reason string'

  fixture spaced-bare-ignore lib.rs '#[test] #[ ignore ] fn slow() {}'
  check spaced-bare-ignore FAIL 'problem src/lib.rs:1: an ignore attribute without a reason string'

  crlf=$(printf '#[test]\r\n#[ignore]\r\nfn slow() {}\r') || exit 2
  fixture crlf-bare-ignore lib.rs "$crlf"
  check crlf-bare-ignore FAIL 'problem src/lib.rs:2: an ignore attribute without a reason string'

  fixture empty-reason lib.rs '#[test]
#[ignore = ""]
fn slow() {}'
  check empty-reason FAIL 'problem src/lib.rs:2: an ignore attribute without a reason string'

  fixture bare-ignore-in-cfg-attr lib.rs '#[cfg_attr(windows, ignore)]
#[test]
fn fifo() {}'
  check bare-ignore-in-cfg-attr FAIL 'problem src/lib.rs:1: an ignore attribute without a reason string'

  fixture untracked-bare-ignore lib.rs "$with_reason"
  printf '#[test]\n#[ignore]\nfn slow() {}\n' >"$work/untracked-bare-ignore/src/scratch.rs" || exit 2
  check untracked-bare-ignore PASS 'files 1 problems 0'

  mkdir -p "$work/no-rust-file" || exit 2
  git -C "$work/no-rust-file" init -q || exit 2
  check no-rust-file FAIL 'git ls-files lists no tracked .rs file'

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
