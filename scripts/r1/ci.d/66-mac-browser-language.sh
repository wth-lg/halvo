#!/bin/sh
# jobs: macos
# rule: On a hosted Mac, one ignored test of halvo-capture starts the runner's Chrome and Edge in three states each (started plainly, started with the argument -AppleLanguages for German, and started under German written into the browser's own settings domain, which is put back), opens a private window in each on pages of its own on 127.0.0.1, and prints for each of the twelve windows the files ending in .pak that the browser's process holds open and has mapped, the names of the menu bar's own items, the title that the capture's own read gives and what the title rules say of the process today; the check PASSes where every browser started and the title read was not refused, and FAILs where the read is refused, a browser does not start, or the log holds fewer lines of a state than the count line names (each line is read from its MBL on, so that a line behind libtest's own text counts); it runs on a hosted runner alone, read by one run by hand on a probe branch that never lands (browser-language-told).
set -u
name=66-mac-browser-language
test_name=macos::foreground::tests::a_real_chrome_and_edge_print_the_language_that_each_process_tells
test_file=crates/halvo-capture/src/macos/foreground.rs

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

guard_stands() {
  awk -v fn="fn ${test_name##*::}()" '
    index($0, fn) { found = 1; next }
    found && NF { print; exit }
  ' "$1" | grep -q 'std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")'
}

mbl_lines() {
  awk '{ i = index($0, "MBL "); if (i) print substr($0, i) }' "$1"
}

lines_short() {
  count=$(sed -n 's/^MBL lines=\([0-9]*\) .*/\1/p' "$1" | head -n 1)
  [ -n "$count" ] || { printf 'the probe printed no count line\n'; return 0; }
  states=$(grep -c '^MBL browser=' "$1")
  if [ "$states" -lt "$count" ]; then
    printf 'the log holds %s lines of a state and the count line names %s\n' "$states" "$count"
    return 0
  fi
  [ "$count" -eq 12 ] || { printf 'the count line names %s lines, not twelve\n' "$count"; return 0; }
  return 1
}

measure() {
  [ "${GITHUB_ACTIONS:-}" = true ] || fail "it runs on a hosted runner alone"
  root=$(git rev-parse --show-toplevel) || exit 2
  cd "$root" || exit 2
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  guard_stands "$test_file" || fail "the probe's first line is not its guard"
  out=$(mktemp "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  HALVO_HOSTED_BROWSER_LANGUAGE=1 cargo test --locked -p halvo-capture --lib -- --ignored --exact --nocapture --test-threads=1 "$test_name" >"$out" 2>&1
  code=$?
  mbl=$(mktemp "${TMPDIR:-/tmp}/$name.mbl.XXXXXX") || exit 2
  mbl_lines "$out" >"$mbl"
  cat "$mbl"
  if [ "$code" -ne 0 ]; then
    grep -E '^(test result|thread .* panicked at crates/|error)|^(assertion|  left|  right)' "$out"
    fail "the probe exited $code"
  fi
  if reason=$(lines_short "$mbl"); then
    fail "$reason"
  fi
  printf 'PASS %s\n' "$name"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  wrong=
  rejected=0
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  printf '    fn %s() {\n        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")\n' "${test_name##*::}" >"$work/guarded.rs"
  got=FAIL
  guard_stands "$work/guarded.rs" && got=PASS
  verdict guard-on-the-first-line PASS "$got"
  printf '    fn %s() {\n        let port = mbl_serve();\n' "${test_name##*::}" >"$work/unguarded.rs"
  got=FAIL
  guard_stands "$work/unguarded.rs" && got=PASS
  verdict no-guard-on-the-first-line FAIL "$got"
  all_lines() {
    printf 'test %s ... \n' "$test_name"
    printf '%s\n' "$1"
    i=1
    while [ "$i" -le "$2" ]; do
      printf 'MBL browser=chrome state=s%s title="x"\n' "$i"
      i=$((i + 1))
    done
    printf 'MBL lines=12 read=12 refused=0 no_title=0 not_started=0\n'
  }
  all_lines 'test x ... MBL browser=chrome state=plain title="x"' 11 >"$work/prefixed.log"
  mbl_lines "$work/prefixed.log" >"$work/prefixed.mbl"
  got=PASS
  lines_short "$work/prefixed.mbl" >/dev/null && got=FAIL
  verdict a-line-behind-a-prefix-counts PASS "$got"
  all_lines '' 11 >"$work/eleven.log"
  mbl_lines "$work/eleven.log" >"$work/eleven.mbl"
  got=PASS
  lines_short "$work/eleven.mbl" >/dev/null && got=FAIL
  verdict eleven-lines-and-a-count-of-twelve FAIL "$got"
  got=FAIL
  (GITHUB_ACTIONS=false measure) >"$work/off.out" 2>&1 && got=PASS
  grep -q 'runs on a hosted runner alone' "$work/off.out" || got=PASS
  verdict measure-off-a-hosted-runner FAIL "$got"
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
    printf 'usage: sh scripts/r1/ci.d/%s.sh [--self-test]\n' "$name" >&2
    exit 64
    ;;
esac
