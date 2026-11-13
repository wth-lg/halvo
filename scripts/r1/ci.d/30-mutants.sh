#!/bin/sh
# jobs: macos
# rule: Every substitution of a mutants.rows file still applies and builds, and a push to main runs mutants in an order that the commit decides, starts no row after budget_s seconds and fails when a row that ran is not caught.
# lane: alone
set -u
name=30-mutants
budget_s=240
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
  out=$(TMPDIR=$scratch sh scripts/r1-gate/mutation-test.sh "$@" --remove-scratch 2>&1)
  code=$?
  printf '%s\n' "$out"
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
  rows=$(TMPDIR=$scratch sh scripts/r1-gate/mutation-test.sh --list --remove-scratch 2>&1) ||
    fail "mutation-test.sh --list exited $?"
  printf '%s\n' "$rows" >"$scratch/list" || exit 2
  awk -F '\t' 'sub(/^mutation:/, "", $1) { print $1 }' "$scratch/list" >"$scratch/listed" || exit 2
  sort -u -o "$scratch/names" "$scratch/listed" || exit 2
  mkdir "$scratch/order" || exit 2
  m=0
  while IFS= read -r row; do
    m=$((m + 1))
    printf '%s %s' "$commit" "$row" >"$scratch/order/$m" || exit 2
  done <"$scratch/names"
  [ "$m" -gt 0 ] || fail "mutation-test.sh --list printed no row"
  (cd "$scratch/order" && shasum -a 256 -- *) >"$scratch/hashes" || fail "shasum failed"
  awk 'NR == FNR { row[FNR] = $0; next } { print $1, row[$2] }' "$scratch/names" "$scratch/hashes" >"$scratch/keyed" || exit 2
  sort -o "$scratch/ordered" "$scratch/keyed" || exit 2
  set --
  while read -r _ row; do
    set -- "$@" --only "$row"
  done <"$scratch/ordered"
  label="mutation-test.sh --budget $budget_s"
  out=$(TMPDIR=$scratch sh scripts/r1-gate/mutation-test.sh --remove-scratch --budget "$budget_s" "$@" 2>&1)
  code=$?
  printf '%s\n' "$out"
  printf '%s\n' "$out" >"$scratch/full" || exit 2
  awk -v m="$m" '
    /^[A-Z-]+ mutation:[^ ]+ \(/ { word[$2] = $1; next }
    /^seconds mutation:[^ ]+ [0-9]+$/ {
      if (word[$2] == "PASS" || word[$2] == "FAIL") { printf "row %s %s %s s\n", $2, word[$2], $3; n++ }
    }
    END { printf "ran %d of %d rows\n", n, m }' "$scratch/full" >"$scratch/ran" || exit 2
  cat "$scratch/ran" || exit 2
  ran=$(sed -n 's/^ran \([0-9]*\) of .*/\1/p' "$scratch/ran")
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
    unset GITHUB_EVENT_NAME GITHUB_REF GITHUB_SHA
    if [ -n "$event" ]; then
      GITHUB_EVENT_NAME=$event
      GITHUB_REF=$ref
      export GITHUB_EVENT_NAME GITHUB_REF
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
  sha=$commit1
  script=$0
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
  git -C "$dir" init -q && git -C "$dir" add -A || exit 2
  tree=$(git -C "$dir" write-tree) || exit 2
  head=$(printf 'tree %s\nauthor 30-mutants fixture <fixture@example.invalid> 1 +0000\ncommitter 30-mutants fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$tree" | git -C "$dir" hash-object -t commit -w --stdin) || exit 2
  git -C "$dir" update-ref HEAD "$head" || exit 2
}

# shellcheck disable=SC2016
runner() {
  mkdir -p "$work/$1/scripts/r1-gate" "$work/$1/tmp" || exit 2
  printf 'printf "call %%s\\n" "$*" >>"$STUB_CALLS"\nmkdir -p "$TMPDIR/r1-gate-mutation.stub"\n%s\n' "$2" >"$work/$1/scripts/r1-gate/mutation-test.sh"
}

# shellcheck disable=SC2016
self_test() {
  make_scratch
  work=$scratch
  repo=$(cd "$(dirname "$0")/../../.." && pwd) || exit 2
  only='[ "${1:-}" = --apply-only ] || exit 9'
  applies='case ${1:-} in --apply-only) printf "PASS mutation:shredded-key-survives (applies and builds)\n"; exit 0 ;; esac'
  apply_only='call --apply-only --remove-scratch'
  commit1=1111111111111111111111111111111111111111
  commit2=2222222222222222222222222222222222222222
  commit3=3333333333333333333333333333333333333333
  listing='case ${1:-} in --list) printf "mutation:%s\\trequired\\tleak\\trows\\n" shredded-key-survives checkpoint-not-verified key-row-deleted segment-roundtrip-broken delete-all-destroys-nothing; exit 0 ;; esac'
  twice='case ${1:-} in --list) printf "mutation:%s\\trequired\\tleak\\trows\\n" shredded-key-survives checkpoint-not-verified key-row-deleted segment-roundtrip-broken delete-all-destroys-nothing; printf "mutation:shredded-key-survives\\ttransitional\\tleak\\tother\\n"; exit 0 ;; esac'
  lists="call --apply-only --remove-scratch${nl}call --list --remove-scratch"
  budgeted="$lists${nl}call --remove-scratch --budget 240"
  order1="$budgeted --only segment-roundtrip-broken --only delete-all-destroys-nothing --only shredded-key-survives --only checkpoint-not-verified --only key-row-deleted"
  order2="$budgeted --only key-row-deleted --only delete-all-destroys-nothing --only checkpoint-not-verified --only shredded-key-survives --only segment-roundtrip-broken"
  order3="$budgeted --only segment-roundtrip-broken --only delete-all-destroys-nothing --only checkpoint-not-verified --only shredded-key-survives --only key-row-deleted"
  caught='PASS mutation:segment-roundtrip-broken (test store::x: the one test failed)\nseconds mutation:segment-roundtrip-broken 31\nNOT-RUN mutation:delete-all-destroys-nothing (the budget of 240 s is used)'
  counted='mutation rows: 1 caught, 0 failed, 0 open, 0 transitional, 0 for the measuring Mac not run, 0 expected names with no row'
  rejected=0
  wrong=
  event=push
  ref=refs/heads/a-branch
  sha=$commit1
  script=$0

  runner rows-apply "$applies; exit 9"
  check rows-apply PASS '' "$apply_only"
  runner anchor-moved "$only; printf \"PASS mutation:checkpoint-not-verified (applies and builds)\\nFAIL mutation:shredded-key-survives (the substitution did not apply)\\n\"; exit 1"
  check anchor-moved FAIL 'mutation-test.sh --apply-only exited 1: FAIL mutation:shredded-key-survives (the substitution did not apply))' "$apply_only"
  runner runner-exits-2 "$only; printf \"PASS mutation:shredded-key-survives (applies and builds)\\n\"; exit 2"
  check runner-exits-2 FAIL 'mutation-test.sh --apply-only exited 2: it printed no FAIL line)' "$apply_only"
  runner fail-line-with-exit-0 "$only; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\""
  check fail-line-with-exit-0 FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL mutation:shredded-key-survives' "$apply_only"
  runner other-fail-line-with-exit-0 "$only; printf \"PASS mutation:shredded-key-survives (applies and builds)\\nFAIL unmutated:store (gate said: FAIL store)\\n\""
  check other-fail-line-with-exit-0 FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL unmutated:store' "$apply_only"
  runner fail-line-on-standard-error "$only; printf \"PASS mutation:checkpoint-not-verified (applies and builds)\\n\"; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\" >&2"
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
  check push-to-main-runs-rows-in-the-order-of-the-commit PASS "row mutation:segment-roundtrip-broken PASS 31 s${nl}ran 1 of 5 rows${nl}PASS $name" "$order1"
  runner push-to-main-same-commit-same-rows "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-same-commit-same-rows PASS 'ran 1 of 5 rows' "$order1"
  runner push-to-main-second-commit-other-rows "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  sha=$commit2
  check push-to-main-second-commit-other-rows PASS 'ran 1 of 5 rows' "$order2"
  runner push-to-main-third-commit-other-rows "$applies; $listing; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  sha=$commit3
  check push-to-main-third-commit-other-rows PASS 'ran 1 of 5 rows' "$order3"
  runner push-to-main-name-in-two-row-files "$applies; $twice; printf \"$caught\\n$counted\\n\""
  ref=refs/heads/main
  check push-to-main-name-in-two-row-files PASS 'ran 1 of 5 rows' "$order1"
  if command -v dash >/dev/null 2>&1; then
    runner term-under-dash-leaves-no-directory "case \${1:-} in --apply-only) : >\"\$STUB_CALLS.started\"; sleep 2; printf \"PASS mutation:shredded-key-survives (applies and builds)\\n\" ;; esac"
    signalled term-under-dash-leaves-no-directory
  fi
  real_runner real-runner-rows-and-count
  ref=refs/heads/main
  check real-runner-rows-and-count PASS "PASS $name" '' "row mutation:gamma-gone PASS * s${nl}row mutation:alpha-gone PASS N s${nl}row mutation:beta-gone PASS * s${nl}ran 3 of 4 rows"
  real_runner real-runner-first-row-only
  sed 's/^budget_s=.*/budget_s=1/' "$0" >"$work/real-runner-first-row-only/30-mutants.sh" || exit 2
  script=$work/real-runner-first-row-only/30-mutants.sh
  ref=refs/heads/main
  sha=$commit3
  check real-runner-first-row-only PASS "PASS $name" '' "row mutation:alpha-gone PASS N s${nl}ran 1 of 4 rows"
  runner push-to-main-row-not-caught "$applies; $listing; printf \"FAIL mutation:segment-roundtrip-broken (test store::x: passed)\\nseconds mutation:segment-roundtrip-broken 31\\nmutation rows: 0 caught, 1 failed\\n\"; exit 1"
  ref=refs/heads/main
  check push-to-main-row-not-caught FAIL "row mutation:segment-roundtrip-broken FAIL 31 s${nl}ran 1 of 5 rows${nl}FAIL $name (mutation-test.sh --budget 240 exited 1: FAIL mutation:segment-roundtrip-broken (test store::x: passed))" "$order1"
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
