#!/bin/sh
# jobs: macos windows
# rule: the page tests run and none fails; zero tests is a failure
# lane: free
set -u
name=30-app-page-test
pattern='ui/tests/**/*.test.js'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

verdict() {
  tap_file=$1
  node_code=$2
  count=$(sed -n 's/^# tests \([0-9][0-9]*\)$/\1/p' "$tap_file" | tail -n 1)
  passed=$(sed -n 's/^# pass \([0-9][0-9]*\)$/\1/p' "$tap_file" | tail -n 1)
  failed=$(sed -n 's/^# fail \([0-9][0-9]*\)$/\1/p' "$tap_file" | tail -n 1)
  printf 'page tests: %s, passed: %s, failed: %s, node exit: %s\n' "${count:--}" "${passed:--}" "${failed:--}" "$node_code"
  [ -n "$count" ] || fail "node printed no line # tests"
  [ "$node_code" -eq 0 ] || fail "node --test exited $node_code"
  [ "$count" -gt 0 ] || fail "node ran 0 tests"
  [ "${failed:-0}" -eq 0 ] || fail "node reported $failed failed tests"
}

run_tests() {
  root=$1
  out_dir=$2
  command -v node >/dev/null 2>&1 || fail "node is not on the PATH"
  (cd "$root" && node --test --test-reporter=tap "$pattern") >"$out_dir/page-test.tap" 2>"$out_dir/page-test.stderr"
  verdict "$out_dir/page-test.tap" "$?"
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  run_tests "$repo" "$artifacts"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the page tests"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(cd "$work/$1" && verdict_only=$3 sh "$self" --fixture "$work/$1" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$4"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

fixture_tree() {
  mkdir -p "$work/$1/ui/tests/page" "$work/$1/out"
}

self_test() {
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture_tree passing
  printf 'require("node:test")("one", () => {});\n' >"$work/passing/ui/tests/page/one.test.js"
  fixture_tree failing
  printf 'require("node:test")("one", () => { throw new Error("no"); });\n' >"$work/failing/ui/tests/page/one.test.js"
  fixture_tree empty
  printf 'not a test file\n' >"$work/empty/ui/tests/page/notes.txt"
  fixture_tree tests-zero
  printf '# tests 0\n# pass 0\n# fail 0\n' >"$work/tests-zero/out/page-test.tap"
  fixture_tree no-count-line
  printf 'ok 1 - one\n' >"$work/no-count-line/out/page-test.tap"
  fixture_tree count-with-a-nonzero-exit
  printf '# tests 3\n# pass 3\n# fail 0\n' >"$work/count-with-a-nonzero-exit/out/page-test.tap"

  check passing PASS '' 'page tests: 1, passed: 1, failed: 0, node exit: 0'
  check failing FAIL '' 'node --test exited 1)'
  check empty FAIL '' 'node ran 0 tests)'
  check tests-zero FAIL 0 'node ran 0 tests)'
  check no-count-line FAIL 0 'node printed no line # tests)'
  check count-with-a-nonzero-exit FAIL 1 'node --test exited 1)'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

fixture() {
  root=$1
  if [ -n "${verdict_only:-}" ]; then
    verdict "$root/out/page-test.tap" "$verdict_only"
  else
    run_tests "$root" "$root/out"
  fi
  printf 'PASS %s\n' "$name"
}

case ${1:-} in
  --self-test) self_test ;;
  --fixture) fixture "$2" ;;
  '') measure ;;
  *) exit 64 ;;
esac
