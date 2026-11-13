#!/bin/sh
# jobs: macos windows
# rule: Each check of the backend's gate, backend/gate/checks/NN-name.sh, has one stamped record docs/r7/gate/NN-name.txt of the line that the gate printed for it, its UTC stamp, the 1-minute load, and the trees of backend/ and of the model formal/r7-ledger/ that it ran on, which are their trees at HEAD; docs/r7/gate/ holds no other file.
# lane: free
set -u
name=22-r7-backend-records
records=docs/r7/gate

records_in() {
  repo=$1
  rev=HEAD
  work=$2
  problems=$3
  : >"$problems" || return 2
  cd "$repo" || return 2
  tree=$(git rev-parse --verify --quiet "$rev:backend")
  model=$(git rev-parse --verify --quiet "$rev:formal/r7-ledger")
  if [ -z "$tree" ] || [ -z "$model" ]; then
    printf 'git names no tree of backend/ or of formal/r7-ledger/ at %s\n' "$rev" >>"$problems"
    counts='checks=0 records=0'
    return 1
  fi
  git -c core.quotepath=off ls-files -- 'backend/gate/checks/' >"$work/checks" || return 2
  git -c core.quotepath=off ls-files -- "$records/" >"$work/records" || return 2
  awk -v tree="$tree" -v records="$records" -v problems="$problems" -v work="$work" '
    function problem(why) { printf "%s\n", why >>problems; bad++ }
    FILENAME == ARGV[1] {
      n = split($0, part, "/")
      file = part[n]
      if (n != 4 || file !~ /^[0-9][0-9]-[a-z0-9-]+\.sh$/) { problem("not a check file: " $0); next }
      id = substr(file, 1, length(file) - 3)
      want[records "/" id ".txt"] = substr(id, 4)
      checks++
      next
    }
    {
      if (!($0 in want)) { problem("a record of no check: " $0); next }
      have[$0] = 1
    }
    END {
      for (r in want) {
        if (!(r in have)) { problem("no record: " r); continue }
        print r "\t" want[r] >(work "/pairs")
      }
      printf "%d\n", checks >(work "/checks-count")
      exit 0
    }
  ' "$work/checks" "$work/records" || return 2
  : >>"$work/pairs"
  stale=0
  formed=0
  pass=0
  fail=0
  tab=$(printf '\t')
  while IFS="$tab" read -r record check; do
    tr -d '\r' <"$record" >"$work/record" || return 2
    awk -v check="$check" -v tree="$tree" -v model="$model" -v record="$record" '
      NR == 1 { ok1 = $0 ~ ("^line: (PASS|FAIL) " check ": .") }
      NR == 2 { ok2 = $0 ~ /^stamp: [0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-5][0-9]Z$/ }
      NR == 3 { ok3 = $0 ~ /^load: [0-9]+(\.[0-9]+)?$/ }
      NR == 4 { ok4 = $0 ~ /^tree: [0-9a-f]+$/; named = substr($0, 7) }
      NR == 5 { ok5 = $0 ~ /^model: [0-9a-f]+$/; named_model = substr($0, 8) }
      END {
        if (NR != 5 || !ok1 || !ok2 || !ok3 || !ok4 || !ok5) { print "malformed"; exit }
        if (named != tree || named_model != model) { print "stale"; exit }
        print "ok"
      }
    ' "$work/record" >"$work/verdict" || return 2
    verdict=$(cat "$work/verdict")
    case $verdict in
      ok)
        formed=$((formed + 1))
        case $(sed -n '1s/^line: \([A-Z]*\) .*/\1/p' "$work/record") in
          PASS) pass=$((pass + 1)) ;;
          *) fail=$((fail + 1)) ;;
        esac
        ;;
      stale)
        stale=$((stale + 1))
        printf 'names another tree than HEAD:backend or HEAD:formal/r7-ledger: %s\n' "$record" >>"$problems"
        ;;
      *) printf 'malformed: %s\n' "$record" >>"$problems" ;;
    esac
  done <"$work/pairs"
  checks=$(cat "$work/checks-count")
  listed=$(awk 'END { print NR }' "$work/records")
  counts="checks=$checks records=$listed well-formed=$formed stale=$stale pass-lines=$pass fail-lines=$fail"
  [ "$checks" -gt 0 ] || printf 'git lists no check under backend/gate/checks/\n' >>"$problems"
  [ ! -s "$problems" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  records_in "$repo" "$work" "$artifacts/problems.txt"
  code=$?
  found=$(awk 'END { print NR }' "$artifacts/problems.txt")
  printf '%s problems=%s\n' "${counts:-unmeasured}" "$found"
  case $code in
    0) printf 'PASS %s\n' "$name" ;;
    1) printf 'FAIL %s (%s problems; the list is problems.txt among the artifacts; stamp the records again with sh scripts/r7/stamp-gate-records.sh)\n' "$name" "$found"; exit 1 ;;
    *) printf 'FAIL %s (setup)\n' "$name"; exit 1 ;;
  esac
}

record() {
  printf 'line: %s\nstamp: 2026-11-12T10:00:00Z\nload: 3.25\ntree: TREE\nmodel: MODEL\n' "$2" >"$1"
}

commit() {
  git -C "$1" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "$2" >/dev/null || exit 2
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/gate/checks" "$dir/formal/r7-ledger" "$dir/$records" || exit 2
  git -C "$dir" init -q || exit 2
  printf 'echo\n' >"$dir/backend/gate/checks/10-subscription.sh"
  printf 'echo\n' >"$dir/backend/gate/checks/95-model.sh"
  printf 'older\n' >"$dir/backend/other.txt"
  printf 'older\n' >"$dir/formal/r7-ledger/m.tla"
  git -C "$dir" add -A backend formal || exit 2
  commit "$dir" 'An older backend and model'
  printf 'x\n' >"$dir/backend/other.txt"
  printf 'x\n' >"$dir/formal/r7-ledger/m.tla"
  record "$dir/$records/10-subscription.txt" 'PASS subscription: active=1'
  record "$dir/$records/95-model.txt" 'FAIL model: states=unmeasured; first failure: no tool'
}

seal() {
  dir=$scratch/$1
  git -C "$dir" add -A backend formal || exit 2
  tree=$(git -C "$dir" write-tree) || exit 2
  backend=$(git -C "$dir" rev-parse "$tree:backend") || exit 2
  model=$(git -C "$dir" rev-parse "$tree:formal/r7-ledger") || exit 2
  for file in "$dir/$records"/*.txt; do
    [ -f "$file" ] || continue
    sed -e "s/^tree: TREE$/tree: $backend/" -e "s/^model: MODEL$/model: $model/" "$file" >"$file.sealed" && mv "$file.sealed" "$file" || exit 2
  done
  git -C "$dir" add -A . || exit 2
  commit "$dir" 'The records of the backend and model of this commit'
}

expect() {
  want=$1
  case_name=$2
  work=$(mktemp -d "$scratch/work.XXXXXX") || exit 2
  (records_in "$scratch/$case_name" "$work" "$work/problems") >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 || {
    printf 'FAIL self-test:%s (the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.)\n' "$name"
    exit 1
  }
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  seal good
  expect PASS good

  fixture staged-change-that-head-does-not-hold
  seal staged-change-that-head-does-not-hold
  printf 'y\n' >>"$scratch/staged-change-that-head-does-not-hold/backend/other.txt"
  git -C "$scratch/staged-change-that-head-does-not-hold" add -A . || exit 2
  expect PASS staged-change-that-head-does-not-hold

  fixture record-with-crlf
  seal record-with-crlf
  cr=$(printf '\r')
  sed "s/\$/$cr/" "$scratch/record-with-crlf/$records/95-model.txt" >"$scratch/record-with-crlf/crlf" || exit 2
  mv "$scratch/record-with-crlf/crlf" "$scratch/record-with-crlf/$records/95-model.txt" || exit 2
  git -C "$scratch/record-with-crlf" add -A . || exit 2
  expect PASS record-with-crlf

  fixture no-record
  mv "$scratch/no-record/$records/95-model.txt" "$scratch/no-record/95-model.moved" || exit 2
  seal no-record
  expect FAIL no-record

  fixture file-that-is-no-check
  mkdir -p "$scratch/file-that-is-no-check/backend/gate/checks/old" || exit 2
  printf 'echo\n' >"$scratch/file-that-is-no-check/backend/gate/checks/old/10-subscription.sh"
  seal file-that-is-no-check
  expect FAIL file-that-is-no-check

  fixture record-of-no-check
  record "$scratch/record-of-no-check/$records/50-concurrency.txt" 'PASS concurrency: runs=1 of 1'
  seal record-of-no-check
  expect FAIL record-of-no-check

  fixture other-file-in-the-records
  printf 'notes\n' >"$scratch/other-file-in-the-records/$records/README.md"
  seal other-file-in-the-records
  expect FAIL other-file-in-the-records

  fixture tree-of-an-older-backend
  seal tree-of-an-older-backend
  printf 'y\n' >>"$scratch/tree-of-an-older-backend/backend/other.txt"
  git -C "$scratch/tree-of-an-older-backend" add -A . || exit 2
  commit "$scratch/tree-of-an-older-backend" 'A later change that the records do not name'
  expect FAIL tree-of-an-older-backend

  fixture tree-of-an-older-model
  seal tree-of-an-older-model
  printf 'y\n' >>"$scratch/tree-of-an-older-model/formal/r7-ledger/m.tla"
  git -C "$scratch/tree-of-an-older-model" add -A . || exit 2
  commit "$scratch/tree-of-an-older-model" 'A later change that the records do not name'
  expect FAIL tree-of-an-older-model

  fixture record-without-the-model
  sed '/^model: /d' "$scratch/record-without-the-model/$records/95-model.txt" >"$scratch/record-without-the-model/x" || exit 2
  mv "$scratch/record-without-the-model/x" "$scratch/record-without-the-model/$records/95-model.txt" || exit 2
  seal record-without-the-model
  expect FAIL record-without-the-model

  fixture line-of-a-check-whose-name-starts-the-same
  record "$scratch/line-of-a-check-whose-name-starts-the-same/$records/95-model.txt" 'PASS models: states=1'
  seal line-of-a-check-whose-name-starts-the-same
  expect FAIL line-of-a-check-whose-name-starts-the-same

  fixture line-of-another-check
  record "$scratch/line-of-another-check/$records/95-model.txt" 'PASS schema: seed=1'
  seal line-of-another-check
  expect FAIL line-of-another-check

  fixture line-that-is-a-skip
  record "$scratch/line-that-is-a-skip/$records/95-model.txt" 'SKIP model: left out by --except 95'
  seal line-that-is-a-skip
  expect FAIL line-that-is-a-skip

  fixture stamp-without-zone
  sed 's/^stamp: .*/stamp: 2026-11-12T10:00:00/' "$scratch/stamp-without-zone/$records/95-model.txt" >"$scratch/stamp-without-zone/x" || exit 2
  mv "$scratch/stamp-without-zone/x" "$scratch/stamp-without-zone/$records/95-model.txt" || exit 2
  seal stamp-without-zone
  expect FAIL stamp-without-zone

  fixture load-not-a-number
  sed 's/^load: .*/load: high/' "$scratch/load-not-a-number/$records/95-model.txt" >"$scratch/load-not-a-number/x" || exit 2
  mv "$scratch/load-not-a-number/x" "$scratch/load-not-a-number/$records/95-model.txt" || exit 2
  seal load-not-a-number
  expect FAIL load-not-a-number

  fixture record-without-a-tree
  sed '/^tree: /d' "$scratch/record-without-a-tree/$records/95-model.txt" >"$scratch/record-without-a-tree/x" || exit 2
  mv "$scratch/record-without-a-tree/x" "$scratch/record-without-a-tree/$records/95-model.txt" || exit 2
  seal record-without-a-tree
  expect FAIL record-without-a-tree

  fixture record-with-a-fifth-line
  printf 'note: more\n' >>"$scratch/record-with-a-fifth-line/$records/95-model.txt"
  seal record-with-a-fifth-line
  expect FAIL record-with-a-fifth-line

  fixture untracked-record-is-not-read
  mv "$scratch/untracked-record-is-not-read/$records/95-model.txt" "$scratch/untracked-record-is-not-read/95-model.moved" || exit 2
  seal untracked-record-is-not-read
  record "$scratch/untracked-record-is-not-read/$records/95-model.txt" 'PASS model: states=1'
  expect FAIL untracked-record-is-not-read

  fixture no-checks
  mv "$scratch/no-checks/backend/gate" "$scratch/no-checks/gate.moved" || exit 2
  mv "$scratch/no-checks/$records" "$scratch/no-checks/records.moved" || exit 2
  seal no-checks
  expect FAIL no-checks

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
