#!/bin/sh
# jobs: macos windows
# rule: When the range from HEAD's first parent to HEAD changes a file under crates/halvo-sqlcipher, a copy of the tree whose crates/halvo-sqlcipher/build.rs has the statements of the rows rebuilt-page-gap-not-zeroed and edited-page-gap-not-zeroed of crates/halvo-sqlcipher/mutants.rows applied as those rows stand gives, for each drawn workload of page_gap::unmended, the rows by table and by index and the integrity answer of the build of the tree, the build of the tree gives no line of the search, and the copy gives lines of it.
set -u
name=55-sqlcipher-unmended
probe=page_gap::unmended::the_drawn_workloads_print_their_rows_their_integrity_and_the_lines_of_the_search
rows_file=crates/halvo-sqlcipher/mutants.rows
build_file=crates/halvo-sqlcipher/build.rs
statements='rebuilt-page-gap-not-zeroed edited-page-gap-not-zeroed'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

run_probe() {
  (cd "$1" && CARGO_TARGET_DIR=$2 cargo test --locked -p halvo-sqlcipher --lib -- --exact "$probe" --nocapture) >"$3" 2>&1
}

compare() {
  grep '^UNMENDED-PROBE ' "$1" | grep -v ' search_lines=' >"$work/mended.rows"
  grep '^UNMENDED-PROBE ' "$2" | grep -v ' search_lines=' >"$work/unmended.rows"
  [ -s "$work/mended.rows" ] || fail "the build of the tree printed no workload"
  cmp -s "$work/mended.rows" "$work/unmended.rows" ||
    fail "the rows by table and by index or the integrity answer differ between the build of the tree and the copy"
  seeds=$(grep -c ' search_lines=' "$1")
  [ "$seeds" -gt 0 ] || fail "the build of the tree printed no count of the search's lines"
  [ "$(grep -c ' search_lines=' "$2")" -eq "$seeds" ] || fail "the two builds printed the search's lines of different workloads"
  mended=$(sed -n 's/^UNMENDED-PROBE .* search_lines=\([0-9][0-9]*\)$/\1/p' "$1" | awk '{ s += $1 } END { print s + 0 }')
  unmended=$(sed -n 's/^UNMENDED-PROBE .* search_lines=\([0-9][0-9]*\)$/\1/p' "$2" | awk '{ s += $1 } END { print s + 0 }')
  printf 'workloads %s rows_equal yes search_lines_of_the_tree %s search_lines_of_the_copy %s\n' "$seeds" "$mended" "$unmended"
  [ "$mended" -eq 0 ] || fail "the build of the tree gives $mended lines of the search"
  [ "$unmended" -gt 0 ] || fail "the copy without the statements gives no line of the search: the workloads do not reach the gap"
}

measure() {
  root=$1
  given=${2:-}
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  git -C "$root" diff --quiet HEAD -- || fail "the checkout differs from HEAD before the run"
  parent=$(git -C "$root" rev-parse -q --verify 'HEAD^1') || fail "HEAD has no first parent in this checkout, so the range cannot be read"
  if git -C "$root" diff --quiet "$parent" HEAD -- crates/halvo-sqlcipher; then
    printf 'the range from the first parent to HEAD changes no file under crates/halvo-sqlcipher: no build is made\n'
    printf 'PASS %s\n' "$name"
    exit 0
  fi
  git -C "$root" show "HEAD:$rows_file" >"$work/rows" 2>/dev/null || fail "HEAD holds no $rows_file"
  mkdir "$work/copy" || fail "mkdir failed"
  git -C "$root" archive HEAD | tar -x -C "$work/copy" || fail "the copy of HEAD failed"
  for row in $statements; do
    statement=$(awk -F '\t' -v n="$row" '$1 == n { c++; s = $3; f = $2 } END { if (c == 1 && f == "build.rs") print s }' "$work/rows")
    [ -n "$statement" ] || fail "the row $row does not stand once on build.rs in $rows_file"
    cp "$work/copy/$build_file" "$work/before" || fail "cp failed"
    perl -0pi -e "$statement" "$work/copy/$build_file" || fail "perl failed on the statement of the row $row"
    cmp -s "$work/before" "$work/copy/$build_file" && fail "the statement of the row $row does not apply to $build_file"
    printf 'applied the statement of the row %s\n' "$row"
  done
  if [ -n "$given" ]; then
    cp "$given/mended.out" "$work/mended.out" && cp "$given/unmended.out" "$work/unmended.out" || fail "the case gives no probe output"
  else
    run_probe "$root" "${CARGO_TARGET_DIR:-$root/target}" "$work/mended.out" || fail "the probe failed on the build of the tree"
    run_probe "$work/copy" "$work/target" "$work/unmended.out" || fail "the probe failed on the copy without the statements"
  fi
  if [ -n "${CI_D_ARTIFACTS:-}" ]; then
    cp "$work/mended.out" "$work/unmended.out" "$CI_D_ARTIFACTS/" 2>/dev/null || true
  fi
  git -C "$root" diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the run"
  compare "$work/mended.out" "$work/unmended.out"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(CI_D_REPO=$work/$1 sh "$0" --case "$work/$1.probe" 2>&1)
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

TAB=$(printf '\t')
good_rows="rebuilt-page-gap-not-zeroed${TAB}build.rs${TAB}s/ memset\\(pCellptr, 0, 9\\);//${TAB}test a${TAB}required${TAB}x
edited-page-gap-not-zeroed${TAB}build.rs${TAB}s/ memset\\(pBegin, 0, 9\\);//${TAB}test b${TAB}required${TAB}x"
good_build='const A: &str = " memset(pCellptr, 0, 9); memset(pBegin, 0, 9);";'

in_repo() {
  repo_dir=$work/$1
  shift
  git -C "$repo_dir" -c commit.gpgsign=false -c core.hooksPath="$work/no-hooks" "$@"
}

repo() {
  mkdir -p "$work/$1/crates/halvo-sqlcipher" "$work/$1/src" || exit 2
  printf '%s\n' "$2" >"$work/$1/$rows_file" || exit 2
  printf '%s\n' "$3" >"$work/$1/$build_file" || exit 2
  printf 'one\n' >"$work/$1/src/other.txt" || exit 2
  git -C "$work/$1" init -q || exit 2
  git -C "$work/$1" add -A || exit 2
  in_repo "$1" commit -q -m one || exit 2
  case $4 in
    sqlcipher) printf '%s\n' "$3 " >"$work/$1/$build_file" || exit 2 ;;
    other) printf 'two\n' >"$work/$1/src/other.txt" || exit 2 ;;
    none) return 0 ;;
  esac
  git -C "$work/$1" add -A || exit 2
  in_repo "$1" commit -q -m two || exit 2
}

probe_text() {
  for seed in 1 2; do
    printf 'UNMENDED-PROBE seed=%s table=t rows=%s digest=00000000000000aa\n' "$seed" "$1"
    printf 'UNMENDED-PROBE seed=%s integrity=%s\n' "$seed" "$2"
    printf 'UNMENDED-PROBE seed=%s search_lines=%s\n' "$seed" "$3"
  done
}

outputs() {
  mkdir -p "$work/$1.probe" || exit 2
  probe_text $2 >"$work/$1.probe/mended.out" || exit 2
  probe_text $3 >"$work/$1.probe/unmended.out" || exit 2
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  repo equal-with-lines "$good_rows" "$good_build" sqlcipher
  outputs equal-with-lines '5 ok 0' '5 ok 7'
  check equal-with-lines PASS 'workloads 2 rows_equal yes search_lines_of_the_tree 0 search_lines_of_the_copy 14'

  repo no-line-of-the-copy "$good_rows" "$good_build" sqlcipher
  outputs no-line-of-the-copy '5 ok 0' '5 ok 0'
  check no-line-of-the-copy FAIL 'the copy without the statements gives no line of the search'

  repo rows-differ "$good_rows" "$good_build" sqlcipher
  outputs rows-differ '5 ok 0' '4 ok 7'
  check rows-differ FAIL 'the rows by table and by index or the integrity answer differ'

  repo integrity-differs "$good_rows" "$good_build" sqlcipher
  outputs integrity-differs '5 ok 0' '5 malformed 7'
  check integrity-differs FAIL 'the rows by table and by index or the integrity answer differ'

  repo lines-of-the-tree "$good_rows" "$good_build" sqlcipher
  outputs lines-of-the-tree '5 ok 3' '5 ok 7'
  check lines-of-the-tree FAIL 'the build of the tree gives 6 lines of the search'

  repo no-workload "$good_rows" "$good_build" sqlcipher
  mkdir -p "$work/no-workload.probe" || exit 2
  printf 'test result: ok\n' >"$work/no-workload.probe/mended.out" || exit 2
  printf 'test result: ok\n' >"$work/no-workload.probe/unmended.out" || exit 2
  check no-workload FAIL 'the build of the tree printed no workload'

  repo range-outside "$good_rows" "$good_build" other
  check range-outside PASS 'changes no file under crates/halvo-sqlcipher: no build is made'

  repo no-parent "$good_rows" "$good_build" none
  check no-parent FAIL 'HEAD has no first parent in this checkout'

  repo row-missing "$(printf '%s\n' "$good_rows" | head -n 1)" "$good_build" sqlcipher
  outputs row-missing '5 ok 0' '5 ok 7'
  check row-missing FAIL 'the row edited-page-gap-not-zeroed does not stand once on build.rs'

  repo row-twice "$good_rows
$good_rows" "$good_build" sqlcipher
  outputs row-twice '5 ok 0' '5 ok 7'
  check row-twice FAIL 'the row rebuilt-page-gap-not-zeroed does not stand once on build.rs'

  repo statement-not-applying "$good_rows" 'const A: &str = "nothing to take out";' sqlcipher
  outputs statement-not-applying '5 ok 0' '5 ok 7'
  check statement-not-applying FAIL 'the statement of the row rebuilt-page-gap-not-zeroed does not apply'

  repo dirty "$good_rows" "$good_build" sqlcipher
  printf 'changed\n' >"$work/dirty/src/other.txt" || exit 2
  outputs dirty '5 ok 0' '5 ok 7'
  check dirty FAIL 'the checkout differs from HEAD before the run'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --case) measure "$CI_D_REPO" "${2:?}" ;;
  '') measure "$CI_D_REPO" ;;
  *) exit 64 ;;
esac
