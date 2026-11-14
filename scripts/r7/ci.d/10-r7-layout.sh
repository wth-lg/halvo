#!/bin/sh
# jobs: macos windows
# rule: Every tracked file under a root of scripts/r7/layout.txt (backend/ and formal/r7-ledger/) matches one of its patterns, which names the owner of each, and every pattern there matches a tracked file.
# lane: free
set -u
name=10-r7-layout
layout=scripts/r7/layout.txt
tab=$(printf '\t')

check_in() {
  repo=$1
  work=$2
  problems=$3
  : >"$problems" || return 2
  cd "$repo" || return 2
  if [ ! -f "$layout" ]; then
    printf 'layout: %s is no regular file\n' "$layout" >>"$problems"
    counts='roots=0 patterns=0 files=0 strays=0 unmatched=0'
    return 1
  fi
  last=$(tail -c 1 "$layout")
  [ -s "$layout" ] && [ -z "$last" ] || printf 'layout: no LF at its end\n' >>"$problems"
  tr -d '\r' <"$layout" >"$work/layout" || return 2
  awk -F "$tab" -v roots="$work/roots" -v patterns="$work/patterns" -v problems="$problems" '
    function problem(why) { printf "layout line %d: %s\n", NR, why >>problems }
    NF != 2 || $1 == "" || $2 == "" { problem("not two fields split by one tab"); next }
    $1 == "root" {
      if ($2 !~ /\/$/) { problem("a root ends with /"); next }
      if ($2 in root_seen) { problem("root twice"); next }
      root_seen[$2] = 1
      root[++nroots] = $2
      print $2 >roots
      next
    }
    {
      if ($1 !~ /^([0-9]+\.[0-9]+|F\.[0-9]+|S[0-9]+|P[0-9]+)$/) { problem("owner is no unit or phase of the plan"); next }
      if ($2 in seen) { problem("pattern twice"); next }
      seen[$2] = 1
      pending[NR] = $2
      owner[NR] = $1
    }
    END {
      if (nroots == 0) printf "layout: no root line\n" >>problems
      for (n in pending) {
        under = 0
        for (r = 1; r <= nroots; r++) if (substr(pending[n], 1, length(root[r])) == root[r]) under = 1
        if (!under) printf "layout line %d: pattern under no root\n", n >>problems
        else print owner[n] "\t" pending[n] >patterns
      }
    }
  ' "$work/layout" || return 2
  : >>"$work/roots"
  : >>"$work/patterns"
  : >"$work/files"
  while IFS= read -r root; do
    git -c core.quotepath=off ls-files -- "$root" >>"$work/files" || return 2
  done <"$work/roots"
  : >"$work/matches"
  unmatched=0
  while IFS="$tab" read -r owner pattern; do
    git -c core.quotepath=off ls-files -- ":(glob)$pattern" >"$work/one" || return 2
    if [ ! -s "$work/one" ]; then
      unmatched=$((unmatched + 1))
      printf 'pattern matches no tracked file: %s %s\n' "$owner" "$pattern" >>"$problems"
      continue
    fi
    awk -v p="$pattern" -v o="$owner" '{ print length(p) "\t" o "\t" p "\t" $0 }' "$work/one" >>"$work/matches" || return 2
  done <"$work/patterns"
  awk -F "$tab" -v owners="$work/owners" -v problems="$problems" '
    FILENAME == ARGV[1] {
      if (!($4 in len) || $1 + 0 > len[$4]) { len[$4] = $1 + 0; who[$4] = $2 }
      next
    }
    { files++; if ($0 in who) print who[$0] "\t" $0 >owners; else { strays++; printf "stray: %s\n", $0 >>problems } }
    END { printf "%d %d\n", files, strays }
  ' "$work/matches" "$work/files" >"$work/tally" || return 2
  read -r files strays <"$work/tally" || return 2
  roots=$(awk 'END { print NR }' "$work/roots")
  patterns=$(awk 'END { print NR }' "$work/patterns")
  counts="roots=$roots patterns=$patterns files=$files strays=$strays unmatched=$unmatched"
  [ "$files" -gt 0 ] || printf 'no tracked file under the roots\n' >>"$problems"
  [ ! -s "$problems" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  check_in "$repo" "$work" "$artifacts/problems.txt"
  code=$?
  [ ! -f "$work/owners" ] || cp "$work/owners" "$artifacts/owners.txt"
  found=$(awk 'END { print NR }' "$artifacts/problems.txt")
  printf '%s problems=%s\n' "${counts:-unmeasured}" "$found"
  case $code in
    0) printf 'PASS %s\n' "$name" ;;
    1) printf 'FAIL %s (%s problems; the list is problems.txt among the artifacts)\n' "$name" "$found"; exit 1 ;;
    *) printf 'FAIL %s (setup)\n' "$name"; exit 1 ;;
  esac
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/scripts/r7" "$dir/backend/src/a" "$dir/formal/r7-ledger" "$dir/other" || exit 2
  git -C "$dir" init -q || exit 2
  printf 'x\n' >"$dir/backend/one.json"
  printf 'x\n' >"$dir/backend/src/a/two.ts"
  printf 'x\n' >"$dir/formal/r7-ledger/m.tla"
  printf 'x\n' >"$dir/other/free.txt"
  printf 'root\tbackend/\nroot\tformal/r7-ledger/\n0.1\tbackend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/**\n' >"$dir/$layout"
}

stage() {
  git -C "$1" add -A . || exit 2
}

expect() {
  want=$1
  case_name=$2
  work=$(mktemp -d "$scratch/work.XXXXXX") || exit 2
  (check_in "$scratch/$case_name" "$work" "$work/problems") >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  if [ "$got" = PASS ] && [ -n "${3:-}" ] && ! grep -q -x -F "$3" "$work/owners"; then
    got=PASS-with-another-owner
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  stage "$scratch/good"
  expect PASS good

  fixture stray-file
  printf 'x\n' >"$scratch/stray-file/backend/loose.md"
  stage "$scratch/stray-file"
  expect FAIL stray-file

  fixture stray-below-the-first-level
  mkdir -p "$scratch/stray-below-the-first-level/backend/deep/er" || exit 2
  printf 'x\n' >"$scratch/stray-below-the-first-level/backend/deep/er/loose.md"
  stage "$scratch/stray-below-the-first-level"
  expect FAIL stray-below-the-first-level

  fixture most-specific-pattern-owns
  printf 'P2\tbackend/src/a/two.ts\n' >>"$scratch/most-specific-pattern-owns/$layout"
  stage "$scratch/most-specific-pattern-owns"
  expect PASS most-specific-pattern-owns "P2${tab}backend/src/a/two.ts"

  fixture star-does-not-cross-a-slash
  printf 'root\tbackend/\nroot\tformal/r7-ledger/\n0.1\tbackend/one.json\nP4\tbackend/src/*\nP4\tformal/r7-ledger/**\n' >"$scratch/star-does-not-cross-a-slash/$layout"
  stage "$scratch/star-does-not-cross-a-slash"
  expect FAIL star-does-not-cross-a-slash

  fixture stray-in-the-model
  printf 'x\n' >"$scratch/stray-in-the-model/formal/r7-ledger/x.cfg"
  printf 'root\tbackend/\nroot\tformal/r7-ledger/\n0.1\tbackend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/m.tla\n' >"$scratch/stray-in-the-model/$layout"
  stage "$scratch/stray-in-the-model"
  expect FAIL stray-in-the-model

  fixture untracked-file-is-not-read
  stage "$scratch/untracked-file-is-not-read"
  printf 'x\n' >"$scratch/untracked-file-is-not-read/backend/loose.md"
  expect PASS untracked-file-is-not-read

  fixture pattern-matches-nothing
  printf 'P5\tbackend/src/metering/**\n' >>"$scratch/pattern-matches-nothing/$layout"
  stage "$scratch/pattern-matches-nothing"
  expect FAIL pattern-matches-nothing

  fixture pattern-under-no-root
  printf 'P2\tother/**\n' >>"$scratch/pattern-under-no-root/$layout"
  stage "$scratch/pattern-under-no-root"
  expect FAIL pattern-under-no-root

  fixture owner-is-no-unit
  printf 'owner\tbackend/src/a/two.ts\n' >>"$scratch/owner-is-no-unit/$layout"
  stage "$scratch/owner-is-no-unit"
  expect FAIL owner-is-no-unit

  fixture three-fields
  printf '0.1\tbackend/src/a/two.ts\textra\n' >>"$scratch/three-fields/$layout"
  stage "$scratch/three-fields"
  expect FAIL three-fields

  fixture root-twice
  printf 'root\tbackend/\n' >>"$scratch/root-twice/$layout"
  stage "$scratch/root-twice"
  expect FAIL root-twice

  fixture pattern-twice
  printf 'P4\tbackend/src/**\n' >>"$scratch/pattern-twice/$layout"
  stage "$scratch/pattern-twice"
  expect FAIL pattern-twice

  fixture space-for-a-tab
  printf 'root\tbackend/\nroot\tformal/r7-ledger/\n0.1 backend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/**\n' >"$scratch/space-for-a-tab/$layout"
  stage "$scratch/space-for-a-tab"
  expect FAIL space-for-a-tab

  fixture no-root
  printf '0.1\tbackend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/**\n' >"$scratch/no-root/$layout"
  stage "$scratch/no-root"
  expect FAIL no-root

  fixture root-without-slash
  printf 'root\tbackend\nroot\tformal/r7-ledger/\n0.1\tbackend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/**\n' >"$scratch/root-without-slash/$layout"
  stage "$scratch/root-without-slash"
  expect FAIL root-without-slash

  fixture crlf-checkout
  printf 'root\tbackend/\r\nroot\tformal/r7-ledger/\n0.1\tbackend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/**\n' >"$scratch/crlf-checkout/$layout"
  stage "$scratch/crlf-checkout"
  expect PASS crlf-checkout

  fixture no-lf-at-the-end
  printf 'root\tbackend/\nroot\tformal/r7-ledger/\n0.1\tbackend/one.json\nP4\tbackend/src/**\nP4\tformal/r7-ledger/**' >"$scratch/no-lf-at-the-end/$layout"
  stage "$scratch/no-lf-at-the-end"
  expect FAIL no-lf-at-the-end

  fixture no-layout
  mv "$scratch/no-layout/$layout" "$scratch/no-layout/layout.moved" || exit 2
  stage "$scratch/no-layout"
  expect FAIL no-layout

  fixture no-file-under-the-roots
  printf 'root\tbackend/\nroot\tformal/r7-ledger/\n' >"$scratch/no-file-under-the-roots/$layout"
  git -C "$scratch/no-file-under-the-roots" add "$layout" other || exit 2
  expect FAIL no-file-under-the-roots

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) printf 'FAIL %s (usage)\n' "$name"; exit 1 ;;
esac
