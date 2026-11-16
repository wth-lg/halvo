#!/bin/sh
# jobs: macos
# rule: On every pull request and push each row of a mutants.rows file that a run would apply here still applies and its mutant compiles (cargo check for a cargo row, cargo build for a row on a file named build.rs or on a path with a part named halvo-sqlcipher, sh -n, cc, perl or swiftc for the others), except a row on code that the macOS build does not compile; a push to main also builds and runs mutants in the sorted order of the row names rotated to start at the first-parent commit count of main times the stride, modulo the row count, prints within how many pushes every row starts while each push runs at least the stride's number of rows and, after a push that ran fewer, how many rows of its turn were not started, starts no row after budget_s seconds, prints how many rows ran, how many of those it built a mutant for (built mutation:<name>, printed only when the build compiled the mutated file, read from the build's dep-info for a cargo row; a row that fails before its build, or whose mutant does not build, is not built) and, apart, how many no compile of the build read (not-compiled mutation:<name>), and fails when a row that ran is not caught; the whole list is built and run against its verdicts only in a full run on the measuring Mac.
# lane: alone
set -u
name=30-mutants
budget_s=240
stride=6
nl='
'

scratch=

remove_scratch() {
  [ -z "$scratch" ] || rm -rf "$scratch"
}

trap remove_scratch EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

make_scratch() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
}

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

run_rows() {
  label=$1
  shift
  started=$(date +%s) || exit 2
  out=$(TMPDIR=$scratch sh scripts/r1-gate/mutation-test.sh "$@" --remove-scratch 2>&1)
  code=$?
  ended=$(date +%s) || exit 2
  printf '%s\n' "$out"
  printf 'phase apply-only %s s\n' "$((ended - started))"
  first=$(printf '%s\n' "$out" | sed -n '/^FAIL /{p;q;}')
  [ "$code" -eq 0 ] || fail "$label exited $code: ${first:-it printed no FAIL line}"
  [ -z "$first" ] || fail "$label exited 0 and printed: $first"
  case $nl$out in
    *"${nl}PASS mutation:"*) ;;
    *) fail "$label printed no PASS mutation line" ;;
  esac
}

budget_run() {
  commit=${GITHUB_SHA:-}
  case $commit in *[!0-9a-f]*) commit= ;; esac
  [ "${#commit}" -eq 40 ] || fail "GITHUB_SHA is not a commit id of 40 hex digits"
  shallow=$(git rev-parse --is-shallow-repository 2>/dev/null)
  [ "$shallow" = false ] || fail "the checkout is shallow, so git cannot count the first-parent commits of $commit"
  rows=$(TMPDIR=$scratch sh scripts/r1-gate/mutation-test.sh --list --remove-scratch 2>&1) ||
    fail "mutation-test.sh --list exited $?"
  printf '%s\n' "$rows" >"$scratch/list" || exit 2
  awk -F '\t' 'sub(/^mutation:/, "", $1) { print $1 }' "$scratch/list" >"$scratch/listed" || exit 2
  sort -u -o "$scratch/names" "$scratch/listed" || exit 2
  m=$(awk 'END { print NR }' "$scratch/names") || exit 2
  [ "$m" -gt 0 ] || fail "mutation-test.sh --list printed no row"
  pushes=$(git rev-list --count --first-parent "$commit" 2>/dev/null)
  case $pushes in '' | *[!0-9]*) fail "git could not count the first-parent commits of $commit" ;; esac
  start=$((pushes * stride % m))
  within=$(((m + stride - 1) / stride))
  plural=es
  [ "$within" -ne 1 ] || plural=
  awk -v s="$start" 'NR > s' "$scratch/names" >"$scratch/ordered" || exit 2
  awk -v s="$start" 'NR <= s' "$scratch/names" >>"$scratch/ordered" || exit 2
  printf 'order: from row %d of %d at %d first-parent commits; every row starts within %d push%s while each push runs at least %d rows\n' "$((start + 1))" "$m" "$pushes" "$within" "$plural" "$stride"
  set --
  while IFS= read -r row; do
    set -- "$@" --only "$row"
  done <"$scratch/ordered"
  label="mutation-test.sh --budget $budget_s"
  started=$(date +%s) || exit 2
  out=$(TMPDIR=$scratch sh scripts/r1-gate/mutation-test.sh --remove-scratch --budget "$budget_s" "$@" 2>&1)
  code=$?
  ended=$(date +%s) || exit 2
  printf '%s\n' "$out"
  printf 'phase budget %s s\n' "$((ended - started))"
  printf '%s\n' "$out" >"$scratch/full" || exit 2
  awk -v m="$m" '
    /^built mutation:[^ ]+$/ { built_row[$2] = 1; next }
    /^not-compiled mutation:[^ ]+ \(/ { unread_row[$2] = 1; next }
    /^[A-Z-]+ mutation:[^ ]+ \(/ { word[$2] = $1; next }
    /^seconds mutation:[^ ]+ [0-9]+$/ {
      if (word[$2] == "PASS" || word[$2] == "FAIL") { printf "row %s %s %s s\n", $2, word[$2], $3; n++; if ($2 in built_row) built++; if ($2 in unread_row) unread++ }
    }
    END { printf "ran %d of %d rows\nbuilt %d of %d rows\nnot compiled by the build %d of %d rows\n", n, m, built, m, unread, m }' "$scratch/full" >"$scratch/ran" || exit 2
  cat "$scratch/ran" || exit 2
  ran=$(sed -n 's/^ran \([0-9]*\) of .*/\1/p' "$scratch/ran")
  turn=$stride
  [ "$m" -ge "$turn" ] || turn=$m
  if [ "${ran:-0}" -lt "$turn" ]; then
    left=$((turn - ${ran:-0}))
    ran_words="${ran:-0} rows"; after=them
    [ "${ran:-0}" -ne 1 ] || { ran_words='1 row'; after=it; }
    left_words="$left rows"
    [ "$left" -ne 1 ] || left_words='1 row'
    printf "order: this push ran %s, fewer than the stride %d, so the %s after %s up to the next push's start were not started in this turn\n" "$ran_words" "$stride" "$left_words" "$after"
  fi
  first=$(sed -n '/^FAIL /{p;q;}' "$scratch/full")
  [ "$code" -eq 0 ] || fail "$label exited $code: ${first:-it printed no FAIL line}"
  [ -z "$first" ] || fail "$label exited 0 and printed: $first"
  grep -q '^mutation rows: ' "$scratch/full" || fail "$label printed no count line"
  [ "$ran" -gt 0 ] || fail "$label ran no row"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  make_scratch
  case $budget_s in
    '' | *[!0-9]*) fail "the budget budget_s=$budget_s is not a number of seconds" ;;
  esac
  [ "$budget_s" -ge 1 ] || fail "the budget budget_s=$budget_s is below 1 second"
  [ -f scripts/r1-gate/mutation-test.sh ] || fail "scripts/r1-gate/mutation-test.sh is missing"
  grep -q -e '--apply-only' scripts/r1-gate/mutation-test.sh ||
    fail "scripts/r1-gate/mutation-test.sh takes no --apply-only"
  run_rows 'mutation-test.sh --apply-only' --apply-only
  if [ "${GITHUB_EVENT_NAME:-}" = push ] && [ "${GITHUB_REF:-}" = refs/heads/main ]; then
    budget_run
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    CI_D_REPO=$work/$1
    STUB_CALLS=$work/$1/calls
    TMPDIR=$work/$1/tmp
    export CI_D_REPO STUB_CALLS TMPDIR
    if [ -n "$dates" ]; then
      PATH=$work/$dates/bin:$PATH
      export PATH
    fi
    unset GITHUB_EVENT_NAME GITHUB_REF GITHUB_SHA
    if [ -n "$event" ]; then
      GITHUB_EVENT_NAME=$event
      GITHUB_REF=$ref
      export GITHUB_EVENT_NAME GITHUB_REF
    fi
    if [ "$sha" = auto ]; then
      sha=
      [ ! -f "$work/$1/head.id" ] || read -r sha <"$work/$1/head.id"
    fi
    if [ -n "$sha" ]; then
      GITHUB_SHA=$sha
      export GITHUB_SHA
    fi
    sh "$script" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  calls=
  [ ! -f "$work/$1/calls" ] || calls=$(cat "$work/$1/calls")
  [ "$calls" = "$4" ] || got=$got-with-other-calls
  left=$(find "$work/$1/tmp" -mindepth 1 -maxdepth 1)
  [ -z "$left" ] || got=$got-with-leftovers
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ -n "${5:-}" ]; then
    rows=$(printf '%s\n' "$out" | awk '
      /^row mutation:/ { if ($2 == "mutation:alpha-gone") $4 = ($4 > 0 ? "N" : "0"); else $4 = "*"; print }
      /^ran [0-9]+ of [0-9]+ rows$/ { print }')
    [ "$rows" = "$5" ] || got=$got-with-other-rows
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  event=push
  ref=refs/heads/a-branch
  sha=auto
  script=$0
  dates=
}

signalled() {
  dir=$work/$1
  (
    CI_D_REPO=$dir
    STUB_CALLS=$dir/calls
    TMPDIR=$dir/tmp
    export CI_D_REPO STUB_CALLS TMPDIR
    unset GITHUB_EVENT_NAME GITHUB_REF GITHUB_SHA
    exec dash "$0"
  ) >"$dir/out" 2>&1 &
  pid=$!
  i=0
  while [ ! -f "$dir/calls.started" ] && [ "$i" -lt 100 ]; do
    sleep 0.1
    i=$((i + 1))
  done
  kill -TERM "$pid"
  wait "$pid"
  got=exit-$?
  left=$(find "$dir/tmp" -mindepth 1 -maxdepth 1)
  [ -z "$left" ] || got=$got-with-leftovers
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" exit-143 "$got"
  [ "$got" = exit-143 ] || wrong="$wrong $1"
}

# shellcheck disable=SC2016
real_runner() {
  dir=$work/$1
  mkdir -p "$dir/scripts/r1-gate/helper" "$dir/tmp" || exit 2
  cp "$repo/scripts/r1-gate/mutation-test.sh" "$repo/scripts/r1-gate/lib.sh" "$dir/scripts/r1-gate/" || exit 2
  cp "$repo/scripts/r1-gate/helper/"* "$dir/scripts/r1-gate/helper/" || exit 2
  printf 'echo alpha\necho beta\necho gamma\n' >"$dir/scripts/r1-gate/target.sh"
  printf '%s\n' '#!/bin/sh' 'while [ $# -gt 0 ]; do [ "$1" != --case ] || w=$2; shift; done' \
    '[ "$w" != alpha ] || sleep 1' \
    'if grep -qx "echo $w" "$GATE_ROOT/scripts/r1-gate/target.sh"; then got=PASS; else got=FAIL; fi' \
    'printf "case fx/%s want=PASS got=%s\n" "$w" "$got"' >"$dir/scripts/r1-gate/check-fx.sh"
  for gone in alpha beta gamma; do
    printf '%s-gone\ttarget.sh\ts/echo %s\\n//\ttest selftest:fx/%s\trequired\tX-1\n' "$gone" "$gone" "$gone"
  done >"$dir/scripts/r1-gate/mutants.rows"
  printf 'later-row\ttarget.sh\ts/echo beta/echo b/\tleak\tpending X-2\tX-2\n' >>"$dir/scripts/r1-gate/mutants.rows"
  : >"$dir/scripts/r1-gate/mutants.expected"
  chain "$1" 1
}

chain() {
  dir=$work/$1
  git -C "$dir" init -q && git -C "$dir" add -A || exit 2
  tree=$(git -C "$dir" write-tree) || exit 2
  head=
  i=0
  while [ "$i" -lt "$2" ]; do
    i=$((i + 1))
    parent=
    [ -z "$head" ] || parent="parent $head$nl"
    head=$(printf 'tree %s\n%sauthor 30-mutants fixture <fixture@example.invalid> %s +0000\ncommitter 30-mutants fixture <fixture@example.invalid> %s +0000\n\nfixture %s\n' "$tree" "$parent" "$i" "$i" "$i" | git -C "$dir" hash-object -t commit -w --stdin) || exit 2
  done
  git -C "$dir" update-ref HEAD "$head" || exit 2
  printf '%s\n' "$head" >"$dir/head.id" || exit 2
}

# shellcheck disable=SC2016
runner() {
  mkdir -p "$work/$1/scripts/r1-gate" "$work/$1/tmp" || exit 2
  printf 'printf "call %%s\\n" "$*" >>"$STUB_CALLS"\nmkdir -p "$TMPDIR/r1-gate-mutation.stub"\n%s\n' "$2" >"$work/$1/scripts/r1-gate/mutation-test.sh"
  chain "$1" 1
}

# shellcheck disable=SC2016
stand_in_date() {
  mkdir -p "$work/$1/bin" || exit 2
  dates=$1
  shift
  printf '%s\n' "$@" >"$work/$dates/bin/date.values" || exit 2
  printf '%s\n' '#!/bin/sh' 'here=${0%/*}' 'n=0' '[ ! -f "$here/date.n" ] || read -r n <"$here/date.n"' 'n=$((n + 1))' 'printf "%s\n" "$n" >"$here/date.n"' 'sed -n "${n}p" "$here/date.values"' >"$work/$dates/bin/date" || exit 2
  chmod +x "$work/$dates/bin/date" || exit 2
}

# shellcheck disable=SC2016
self_test() {
  make_scratch
  work=$scratch
  repo=$(cd "$(dirname "$0")/../../.." && pwd) || exit 2
  only='[ "${1:-}" = --apply-only ] || exit 9'
  applies='case ${1:-} in --apply-only) printf "PASS mutation:shredded-key-survives (applies and type-checks)\n"; exit 0 ;; esac'
  apply_only='call --apply-only --remove-scratch'
  listing='case ${1:-} in --list) printf "mutation:%s\\trequired\\tleak\\trows\\n" shredded-key-survives checkpoint-not-verified key-row-deleted segment-roundtrip-broken delete-all-destroys-nothing; exit 0 ;; esac'
  twice='case ${1:-} in --list) printf "mutation:%s\\trequired\\tleak\\trows\\n" shredded-key-survives checkpoint-not-verified key-row-deleted segment-roundtrip-broken delete-all-destroys-nothing; printf "mutation:shredded-key-survives\\ttransitional\\tleak\\tother\\n"; exit 0 ;; esac'
  lists="call --apply-only --remove-scratch${nl}call --list --remove-scratch"
  budgeted="$lists${nl}call --remove-scratch --budget 240"
  order1="$budgeted --only delete-all-destroys-nothing --only key-row-deleted --only segment-roundtrip-broken --only shredded-key-survives --only checkpoint-not-verified"
  order2="$budgeted --only key-row-deleted --only segment-roundtrip-broken --only shredded-key-survives --only checkpoint-not-verified --only delete-all-destroys-nothing"
  order3="$budgeted --only segment-roundtrip-broken --only shredded-key-survives --only checkpoint-not-verified --only delete-all-destroys-nothing --only key-row-deleted"
  order4="$budgeted --only shredded-key-survives --only checkpoint-not-verified --only delete-all-destroys-nothing --only key-row-deleted --only segment-roundtrip-broken"
  order5="$budgeted --only checkpoint-not-verified --only delete-all-destroys-nothing --only key-row-deleted --only segment-roundtrip-broken --only shredded-key-survives"
  thirteen='case ${1:-} in --list) for r in r09 r02 r13 r05 r11 r01 r07 r12 r03 r10 r06 r08 r04; do printf "mutation:%s\\trequired\\tleak\\trows\\n" "$r"; done; exit 0 ;; esac'
  order13="$budgeted --only r07 --only r08 --only r09 --only r10 --only r11 --only r12 --only r13 --only r01 --only r02 --only r03 --only r04 --only r05 --only r06"
  caught='built mutation:segment-roundtrip-broken\nPASS mutation:segment-roundtrip-broken (test store::x: the one test failed)\nseconds mutation:segment-roundtrip-broken 31\nNOT-RUN mutation:delete-all-destroys-nothing (the budget of 240 s is used)'
  counted='mutation rows: 1 caught, 0 failed, 0 open, 0 transitional, 0 for the measuring Mac not run, 0 expected names with no row'
  rejected=0
  wrong=
  dates=
  event=push
  ref=refs/heads/a-branch
  sha=auto
  script=$0

  runner rows-apply "$applies; exit 9"
  check rows-apply PASS '' "$apply_only"
  runner rows-type-checked-and-timed "$applies; exit 9"
  check rows-type-checked-and-timed PASS "PASS mutation:shredded-key-survives (applies and type-checks)${nl}phase apply-only " "$apply_only"
  runner anchor-moved "$only; printf \"PASS mutation:checkpoint-not-verified (applies and type-checks)\\nFAIL mutation:shredded-key-survives (the substitution did not apply)\\n\"; exit 1"
  check anchor-moved FAIL 'mutation-test.sh --apply-only exited 1: FAIL mutation:shredded-key-survives (the substitution did not apply))' "$apply_only"
  runner rows-apply-timed-by-the-clock "$applies; exit 9"
  stand_in_date rows-apply-timed-by-the-clock 1000 1007
  check rows-apply-timed-by-the-clock PASS "${nl}phase apply-only 7 s${nl}" "$apply_only"
  runner anchor-moved-is-timed-by-the-clock "$only; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\"; exit 1"
  stand_in_date anchor-moved-is-timed-by-the-clock 1000 1007
  check anchor-moved-is-timed-by-the-clock FAIL "${nl}phase apply-only 7 s${nl}FAIL $name (mutation-test.sh --apply-only exited 1" "$apply_only"
  runner runner-exits-2 "$only; printf \"PASS mutation:shredded-key-survives (applies and type-checks)\\n\"; exit 2"
  check runner-exits-2 FAIL 'mutation-test.sh --apply-only exited 2: it printed no FAIL line)' "$apply_only"
  runner fail-line-with-exit-0 "$only; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\""
  check fail-line-with-exit-0 FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL mutation:shredded-key-survives' "$apply_only"
  runner other-fail-line-with-exit-0 "$only; printf \"PASS mutation:shredded-key-survives (applies and type-checks)\\nFAIL unmutated:store (gate said: FAIL store)\\n\""
  check other-fail-line-with-exit-0 FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL unmutated:store' "$apply_only"
  runner fail-line-on-standard-error "$only; printf \"PASS mutation:checkpoint-not-verified (applies and type-checks)\\n\"; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\" >&2"
  check fail-line-on-standard-error FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL mutation:shredded-key-survives' "$apply_only"
  runner no-row "$only; exit 0"
  check no-row FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner only-other-lines "$only; printf \"PASS unmutated:store\\n\""
  check only-other-lines FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner rows-all-pending "$only; printf \"OPEN mutation:shredded-key-survives (no work)\\n\"; printf \"scratch mutation: /tmp/r1-gate-mutation.x\\n\" >&2"
  check rows-all-pending FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner scratch-line-only "$only; printf \"scratch mutation: /tmp/r1-gate-mutation.x\\n\" >&2"
  check scratch-line-only FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner runner-without-the-flag 'printf "PASS mutation:shredded-key-survives (gate said: FAIL deletion)\n"'
  check runner-without-the-flag FAIL 'scripts/r1-gate/mutation-test.sh takes no --apply-only)' ''
  mkdir -p "$work/runner-missing/tmp"
  check runner-missing FAIL 'scripts/r1-gate/mutation-test.sh is missing)' ''
  runner run-outside-github-actions "$applies; exit 9"
  event=
  check run-outside-github-actions PASS '' "$apply_only"

  runner push-to-main-runs-rows-in-the-order-of-the-commit "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-runs-rows-in-the-order-of-the-commit PASS "row mutation:segment-roundtrip-broken PASS 31 s${nl}ran 1 of 5 rows${nl}built 1 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 1 row, fewer than the stride 6, so the 4 rows after it up to the next push's start were not started in this turn${nl}PASS $name" "$order1"
  runner push-to-main-times-the-budget-run "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-times-the-budget-run PASS "${nl}phase budget " "$order1"
  runner push-to-main-mutant-that-does-not-build "$applies; $listing; printf \"FAIL mutation:segment-roundtrip-broken (the mutant does not build)\\nseconds mutation:segment-roundtrip-broken 31\\nmutation rows: 0 caught, 1 failed\\n\"; exit 1"
  ref=refs/heads/main
  check push-to-main-mutant-that-does-not-build FAIL "ran 1 of 5 rows${nl}built 0 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 1 row, fewer than the stride 6, so the 4 rows after it up to the next push's start were not started in this turn${nl}FAIL $name" "$order1"
  runner push-to-main-control-row-is-not-built "$applies; $listing; printf \"PASS mutation:segment-roundtrip-broken (control: the mutant does not build, and the runner did not count it as caught)\\nseconds mutation:segment-roundtrip-broken 12\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-control-row-is-not-built PASS "ran 1 of 5 rows${nl}built 0 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 1 row, fewer than the stride 6, so the 4 rows after it up to the next push's start were not started in this turn${nl}PASS $name" "$order1"
  runner push-to-main-same-commit-same-rows "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-same-commit-same-rows PASS 'order: from row 2 of 5 at 1 first-parent commits; every row starts within 1 push while each push runs at least 6 rows' "$order1"
  runner push-to-main-second-commit-other-rows "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  chain push-to-main-second-commit-other-rows 2
  check push-to-main-second-commit-other-rows PASS 'ran 1 of 5 rows' "$order2"
  runner push-to-main-third-commit-other-rows "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  chain push-to-main-third-commit-other-rows 3
  check push-to-main-third-commit-other-rows PASS 'ran 1 of 5 rows' "$order3"
  runner push-to-main-shallow-checkout-is-refused "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  chain push-to-main-shallow-checkout-is-refused 4
  cp "$work/push-to-main-shallow-checkout-is-refused/head.id" "$work/push-to-main-shallow-checkout-is-refused/.git/shallow" || exit 2
  check push-to-main-shallow-checkout-is-refused FAIL 'the checkout is shallow, so git cannot count the first-parent commits of ' "$apply_only"
  runner push-to-main-counts-apart-a-row-that-no-compile-reads "$applies; $listing; printf \"not-compiled mutation:segment-roundtrip-broken (no compile of the build reads crates/x/tests/t.rs)\\nPASS mutation:segment-roundtrip-broken (test store::x: the one test failed)\\nseconds mutation:segment-roundtrip-broken 31\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-counts-apart-a-row-that-no-compile-reads PASS "ran 1 of 5 rows${nl}built 0 of 5 rows${nl}not compiled by the build 1 of 5 rows${nl}order: this push ran 1 row" "$order1"
  runner push-to-main-says-the-rows-that-a-short-push-left-unstarted "$applies; $thirteen; printf \"built mutation:r07\\nPASS mutation:r07 (test store::x: the one test failed)\\nseconds mutation:r07 3\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-says-the-rows-that-a-short-push-left-unstarted PASS "${nl}order: this push ran 1 row, fewer than the stride 6, so the 5 rows after it up to the next push's start were not started in this turn${nl}" "$order13"
  six_ran='built mutation:r07\\nPASS mutation:r07 (x)\\nseconds mutation:r07 3\\nbuilt mutation:r08\\nPASS mutation:r08 (x)\\nseconds mutation:r08 3\\nbuilt mutation:r09\\nPASS mutation:r09 (x)\\nseconds mutation:r09 3\\nbuilt mutation:r10\\nPASS mutation:r10 (x)\\nseconds mutation:r10 3\\nbuilt mutation:r11\\nPASS mutation:r11 (x)\\nseconds mutation:r11 3\\nbuilt mutation:r12\\nPASS mutation:r12 (x)\\nseconds mutation:r12 3'
  runner push-to-main-that-ran-the-stride-says-no-row-was-left "$applies; $thirteen; printf \"$six_ran\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-that-ran-the-stride-says-no-row-was-left PASS "ran 6 of 13 rows${nl}built 6 of 13 rows${nl}not compiled by the build 0 of 13 rows${nl}PASS $name" "$order13"
  runner push-to-main-consecutive-counts-start-at-different-rows-4 "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  chain push-to-main-consecutive-counts-start-at-different-rows-4 4
  check push-to-main-consecutive-counts-start-at-different-rows-4 PASS 'order: from row 5 of 5 at 4 first-parent commits;' "$order4"
  runner push-to-main-consecutive-counts-start-at-different-rows-5 "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  chain push-to-main-consecutive-counts-start-at-different-rows-5 5
  check push-to-main-consecutive-counts-start-at-different-rows-5 PASS 'order: from row 1 of 5 at 5 first-parent commits;' "$order5"
  runner push-to-main-prints-within-how-many-pushes-every-row-starts "$applies; $thirteen; printf \"built mutation:r07\\nPASS mutation:r07 (test store::x: the one test failed)\\nseconds mutation:r07 3\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-prints-within-how-many-pushes-every-row-starts PASS "${nl}order: from row 7 of 13 at 1 first-parent commits; every row starts within 3 pushes while each push runs at least 6 rows${nl}" "$order13"
  runner push-to-main-name-in-two-row-files "$applies; $twice; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-name-in-two-row-files PASS 'ran 1 of 5 rows' "$order1"
  if command -v dash >/dev/null 2>&1; then
    runner term-under-dash-leaves-no-directory "case \${1:-} in --apply-only) : >\"\$STUB_CALLS.started\"; sleep 2; printf \"PASS mutation:shredded-key-survives (applies and type-checks)\\n\" ;; esac"
    signalled term-under-dash-leaves-no-directory
  fi
  real_runner real-runner-rows-and-count
  ref=refs/heads/main
  check real-runner-rows-and-count PASS "PASS $name" '' "row mutation:gamma-gone PASS * s${nl}row mutation:alpha-gone PASS N s${nl}row mutation:beta-gone PASS * s${nl}ran 3 of 4 rows"
  real_runner real-runner-first-row-only
  sed 's/^budget_s=.*/budget_s=1/' "$0" >"$work/real-runner-first-row-only/30-mutants.sh" || exit 2
  script=$work/real-runner-first-row-only/30-mutants.sh
  ref=refs/heads/main
  chain real-runner-first-row-only 2
  check real-runner-first-row-only PASS "PASS $name" '' "row mutation:alpha-gone PASS N s${nl}ran 1 of 4 rows"
  runner push-to-main-row-that-fails-before-its-build-is-not-built "$applies; $listing; printf \"FAIL mutation:segment-roundtrip-broken (the substitution did not apply)\\nseconds mutation:segment-roundtrip-broken 3\\nmutation rows: 0 caught, 1 failed\\n\"; exit 1"
  ref=refs/heads/main
  check push-to-main-row-that-fails-before-its-build-is-not-built FAIL "ran 1 of 5 rows${nl}built 0 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 1 row, fewer than the stride 6, so the 4 rows after it up to the next push's start were not started in this turn${nl}FAIL $name" "$order1"
  runner push-to-main-built-line-of-a-row-that-did-not-run-is-not-counted "$applies; $listing; printf \"built mutation:key-row-deleted\\n$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-built-line-of-a-row-that-did-not-run-is-not-counted PASS "ran 1 of 5 rows${nl}built 1 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 1 row, fewer than the stride 6, so the 4 rows after it up to the next push's start were not started in this turn${nl}PASS $name" "$order1"
  real_runner real-runner-counts-the-rows-it-built
  printf 'delta-gone\ttarget.sh\ts/echo alpha/echo alpha2/\ttest selftest:fx/delta\trequired\tX-1\n' >>"$work/real-runner-counts-the-rows-it-built/scripts/r1-gate/mutants.rows"
  chain real-runner-counts-the-rows-it-built 1
  ref=refs/heads/main
  check real-runner-counts-the-rows-it-built FAIL "ran 4 of 5 rows${nl}built 3 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 4 rows, fewer than the stride 6, so the 1 row after them up to the next push's start were not started in this turn${nl}FAIL $name" ''
  runner push-to-main-row-not-caught "$applies; $listing; printf \"built mutation:segment-roundtrip-broken\\nFAIL mutation:segment-roundtrip-broken (test store::x: passed)\\nseconds mutation:segment-roundtrip-broken 31\\nmutation rows: 0 caught, 1 failed\\n\"; exit 1"
  ref=refs/heads/main
  check push-to-main-row-not-caught FAIL "row mutation:segment-roundtrip-broken FAIL 31 s${nl}ran 1 of 5 rows${nl}built 1 of 5 rows${nl}not compiled by the build 0 of 5 rows${nl}order: this push ran 1 row, fewer than the stride 6, so the 4 rows after it up to the next push's start were not started in this turn${nl}FAIL $name (mutation-test.sh --budget 240 exited 1: FAIL mutation:segment-roundtrip-broken (test store::x: passed))" "$order1"
  runner push-to-main-budget-is-timed-by-the-clock "$applies; $listing; printf \"FAIL mutation:segment-roundtrip-broken (test store::x: passed)\\nseconds mutation:segment-roundtrip-broken 31\\nmutation rows: 0 caught, 1 failed\\n\"; exit 1"
  ref=refs/heads/main
  stand_in_date push-to-main-budget-is-timed-by-the-clock 1000 1007 2000 2042
  check push-to-main-budget-is-timed-by-the-clock FAIL "${nl}phase budget 42 s${nl}" "$order1"
  runner push-to-main-not-caught-with-exit-0 "$applies; $listing; printf \"FAIL mutation:segment-roundtrip-broken (test store::x: passed)\\nseconds mutation:segment-roundtrip-broken 31\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-not-caught-with-exit-0 FAIL 'mutation-test.sh --budget 240 exited 0 and printed: FAIL mutation:segment-roundtrip-broken' "$order1"
  runner push-to-main-no-count-line "$applies; $listing; printf \"$caught\\n\""
  ref=refs/heads/main
  check push-to-main-no-count-line FAIL 'mutation-test.sh --budget 240 printed no count line)' "$order1"
  runner push-to-main-no-row-ran "$applies; $listing; printf \"NOT-RUN mutation:segment-roundtrip-broken (the budget of 240 s is used)\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-no-row-ran FAIL 'mutation-test.sh --budget 240 ran no row)' "$order1"
  runner push-to-main-list-fails "$applies; case \${1:-} in --list) exit 2 ;; esac; exit 9"
  ref=refs/heads/main
  check push-to-main-list-fails FAIL 'mutation-test.sh --list exited 2)' "$lists"
  runner push-to-main-without-a-commit "$applies; $listing; exit 9"
  ref=refs/heads/main
  sha=
  check push-to-main-without-a-commit FAIL 'GITHUB_SHA is not a commit id of 40 hex digits)' 'call --apply-only --remove-scratch'
  runner push-to-main-commit-not-hex "$applies; $listing; exit 9"
  ref=refs/heads/main
  sha=gggggggggggggggggggggggggggggggggggggggg
  check push-to-main-commit-not-hex FAIL 'GITHUB_SHA is not a commit id of 40 hex digits)' 'call --apply-only --remove-scratch'
  runner budget-below-one "$applies; exit 9"
  sed 's/^budget_s=.*/budget_s=0/' "$0" >"$work/budget-below-one/30-mutants.sh" || exit 2
  script=$work/budget-below-one/30-mutants.sh
  check budget-below-one FAIL 'the budget budget_s=0 is below 1 second)' ''
  runner budget-not-a-number "$applies; exit 9"
  sed 's/^budget_s=.*/budget_s=4x/' "$0" >"$work/budget-not-a-number/30-mutants.sh" || exit 2
  script=$work/budget-not-a-number/30-mutants.sh
  check budget-not-a-number FAIL 'the budget budget_s=4x is not a number of seconds)' ''
  runner push-to-a-branch-that-ends-in-main "$applies; exit 9"
  ref=refs/heads/team/main
  check push-to-a-branch-that-ends-in-main PASS '' "$apply_only"
  runner pull-request-into-main "$applies; exit 9"
  event=pull_request
  ref=refs/heads/main
  check pull-request-into-main PASS '' "$apply_only"

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
