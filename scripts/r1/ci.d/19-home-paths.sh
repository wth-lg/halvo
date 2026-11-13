#!/bin/sh
# jobs: macos windows
# rule: scripts/check-home-paths.sh passes its self-test and finds no added line with a home path that scripts/home-path-allow.txt does not allow, and no message with a home path, in the commits of the head that the default branch does not hold, or in the head commit when the default branch holds it or the checkout does not, and in the tag of a run that a pushed tag started, and the list holds no entry that allows nothing in HEAD (sh scripts/check-home-paths.sh --stale); it does not refuse an entry of the list for the account that runs it, as the pre-push hook does, because the account of a hosted runner is no person's.
set -u
name=19-home-paths

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

make_scratch() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail 'mktemp failed'
  trap 'rm -rf "$scratch"' EXIT
  trap 'exit 2' INT TERM HUP
}

has_commit() {
  git rev-parse -q --verify "$1^{commit}" >/dev/null 2>&1
}

scan_head() {
  repo=$1
  cd "$repo" || fail "cannot enter $repo"
  [ -f scripts/check-home-paths.sh ] || fail 'scripts/check-home-paths.sh is missing'
  tip=HEAD
  tag=not-a-tag-run
  case ${GITHUB_REF:-} in
    refs/tags/?*)
      type=$(git cat-file -t "$GITHUB_REF" 2>/dev/null) || type=
      if [ "$type" = tag ]; then
        tip=$GITHUB_REF
        tag='read'
      else
        tag=ref-without-a-tag-object
      fi
      ;;
  esac
  if has_commit refs/remotes/origin/main && ! git merge-base --is-ancestor HEAD refs/remotes/origin/main; then
    kind=since-the-default-branch
    range=refs/remotes/origin/main..$tip
  elif has_commit HEAD^1; then
    kind=head-commit
    range="$tip^!"
  else
    kind=root-commit
    range=$tip
  fi
  sh scripts/check-home-paths.sh "$range" >"$scratch/out" 2>"$scratch/err" </dev/null
  code=$?
  counts=$(sed -n 's/.*commits read \([0-9]*\), lines with a home path \([0-9]*\)\.$/commits=\1 lines=\2/p' "$scratch/out" "$scratch/err") || counts=
  printf 'range=%s tag=%s %s exit=%s\n' "$kind" "$tag" "${counts:-commits=unread lines=unread}" "$code"
  sh scripts/check-home-paths.sh --stale >"$scratch/stale" 2>&1 </dev/null
  stale_code=$?
  stale=$(sed -n 's/.*entries read \([0-9]*\), stale \([0-9]*\)\.$/entries=\1 stale=\2/p' "$scratch/stale") || stale=
  printf 'list %s exit=%s\n' "${stale:-entries=unread stale=unread}" "$stale_code"
  case $code in
    0) ;;
    1) fail "an added line or a message holds a home path that scripts/home-path-allow.txt does not allow; run sh scripts/check-home-paths.sh '$range' on a clone of this head" ;;
    *) fail "sh scripts/check-home-paths.sh could not check the range (exit $code)" ;;
  esac
  case $stale_code in
    0) printf 'PASS %s\n' "$name" ;;
    1) fail "an entry of scripts/home-path-allow.txt allows nothing in HEAD; run sh scripts/check-home-paths.sh --stale" ;;
    *) fail "sh scripts/check-home-paths.sh --stale could not check the list (exit $stale_code)" ;;
  esac
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  make_scratch
  scan_head "$repo"
}

in_fixture() {
  fixture_dir=$scratch/$1
  shift
  git -C "$fixture_dir" -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath="$scratch/no-hooks" "$@"
}

copy_scripts() {
  for file in check-home-paths.sh commit-range.sh; do
    cp "$repo/scripts/$file" "$1/scripts/$file" || exit 2
  done
  : >"$1/scripts/home-path-allow.txt" || exit 2
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/scripts" "$scratch/no-hooks" || exit 2
  git init -q "$dir" || exit 2
  copy_scripts "$dir"
  printf 'plain\n' >"$dir/notes.txt"
  in_fixture "$1" add -A . || exit 2
  in_fixture "$1" commit -q -m 'first' || exit 2
}

add_commit() {
  printf '%s\n' "$2" >>"$scratch/$1/notes.txt"
  in_fixture "$1" add -A . || exit 2
  in_fixture "$1" commit -q -m "$3" || exit 2
}

expect() {
  want=$1
  case_name=$2
  (scan_head "$scratch/$case_name") >"$scratch/$case_name.out" 2>&1
  code=$?
  last=$(tail -n 1 "$scratch/$case_name.out") || last=
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  if [ -n "${3:-}" ] && ! grep -qF -- "$3" "$scratch/$case_name.out"; then got=$got-without-$3; fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  make_scratch
  unset GITHUB_REF
  wrong=
  rejected=0
  u=Users

  sh "$repo/scripts/check-home-paths.sh" --self-test >"$scratch/predicate.out" 2>&1
  code=$?
  grep -E '^case check-home-paths/[^ ]* (left out|not run)|^seconds check-home-paths/|^case check-home-paths/(file-name-with-a-tab-listed-by-its-quoted-name|added-entry-with-home-unset|scan-ended-by-term|path-cut-at-its-piece-passes) ' "$scratch/predicate.out"
  predicate=$(sed -n "s/^PASS self-test:check-home-paths (\([0-9]*\) failing fixtures rejected)\$/\1/p" "$scratch/predicate.out") || predicate=
  cut_held=$(grep -c '^case check-home-paths/path-cut-at-its-piece-passes want=1 got=1$' "$scratch/predicate.out") || cut_held=0
  if [ "$code" = 0 ] && [ -n "$predicate" ] && [ "$cut_held" = 1 ]; then
    printf 'case %s/predicate-self-test want=PASS got=PASS (%s failing fixtures rejected)\n' "$name" "$predicate"
  else
    printf 'case %s/predicate-self-test want=PASS got=exit-%s (lines of path-cut-at-its-piece-passes with want=1 got=1: %s)\n' "$name" "$code" "$cut_held"
    grep '^FAIL self-test:check-home-paths (wrong verdict in:' "$scratch/predicate.out"
    wrong="$wrong predicate-self-test"
    predicate=0
  fi

  fixture clean-head
  add_commit clean-head 'more plain text' 'second'
  expect PASS clean-head ' tag=not-a-tag-run '

  fixture home-path-in-the-head-commit
  add_commit home-path-in-the-head-commit "see /$u/zoe/x" 'second'
  expect FAIL home-path-in-the-head-commit

  git init -q "$scratch/home-path-in-the-root-commit" || exit 2
  mkdir -p "$scratch/home-path-in-the-root-commit/scripts" "$scratch/no-hooks" || exit 2
  copy_scripts "$scratch/home-path-in-the-root-commit"
  printf 'see /%s/zoe/x\n' "$u" >"$scratch/home-path-in-the-root-commit/notes.txt"
  in_fixture home-path-in-the-root-commit add -A . || exit 2
  in_fixture home-path-in-the-root-commit commit -q -m 'root' || exit 2
  expect FAIL home-path-in-the-root-commit

  fixture home-path-below-a-clean-head
  first=$(git -C "$scratch/home-path-below-a-clean-head" rev-parse HEAD) || exit 2
  add_commit home-path-below-a-clean-head "see /$u/zoe/x" 'second'
  add_commit home-path-below-a-clean-head 'plain again' 'third'
  git -C "$scratch/home-path-below-a-clean-head" update-ref refs/remotes/origin/main "$first" || exit 2
  expect FAIL home-path-below-a-clean-head

  fixture head-on-the-default-branch
  add_commit head-on-the-default-branch "see /$u/zoe/x" 'second'
  add_commit head-on-the-default-branch 'plain again' 'third'
  git -C "$scratch/head-on-the-default-branch" update-ref refs/remotes/origin/main HEAD || exit 2
  expect PASS head-on-the-default-branch

  fixture head-on-the-default-branch-adds-the-line
  add_commit head-on-the-default-branch-adds-the-line "see /$u/zoe/x" 'second'
  git -C "$scratch/head-on-the-default-branch-adds-the-line" update-ref refs/remotes/origin/main HEAD || exit 2
  expect FAIL head-on-the-default-branch-adds-the-line

  fixture scanner-missing
  add_commit scanner-missing 'more plain text' 'second'
  mv "$scratch/scanner-missing/scripts/check-home-paths.sh" "$scratch/scanner-aside.sh" || exit 2
  expect FAIL scanner-missing

  fixture home-path-in-a-commit-message
  add_commit home-path-in-a-commit-message 'more plain text' "see /$u/zoe/x"
  expect FAIL home-path-in-a-commit-message

  fixture home-path-in-the-message-of-the-pushed-tag
  add_commit home-path-in-the-message-of-the-pushed-tag 'more plain text' 'second'
  in_fixture home-path-in-the-message-of-the-pushed-tag tag -a -m "built in /$u/zoe" v1 || exit 2
  GITHUB_REF=refs/tags/v1
  export GITHUB_REF
  expect FAIL home-path-in-the-message-of-the-pushed-tag ' tag=read '
  git clone -q --no-local "$scratch/home-path-in-the-message-of-the-pushed-tag" "$scratch/tag-fetched-whole" || exit 2
  expect FAIL tag-fetched-whole ' tag=read '
  git clone -q --no-local "$scratch/home-path-in-the-message-of-the-pushed-tag" "$scratch/tag-fetched-as-its-commit" || exit 2
  git -C "$scratch/tag-fetched-as-its-commit" update-ref refs/tags/v1 'refs/tags/v1^{commit}' || exit 2
  expect PASS tag-fetched-as-its-commit ' tag=ref-without-a-tag-object '
  fixture plain-message-of-the-pushed-tag
  add_commit plain-message-of-the-pushed-tag 'more plain text' 'second'
  in_fixture plain-message-of-the-pushed-tag tag -a -m 'built in /Users/<name>' v1 || exit 2
  expect PASS plain-message-of-the-pushed-tag ' tag=read '
  unset GITHUB_REF

  fixture stale-entry-in-the-list
  printf 'gone.txt\t/%s/zoe\tmade up, for a file that is gone\n' "$u" >>"$scratch/stale-entry-in-the-list/scripts/home-path-allow.txt"
  add_commit stale-entry-in-the-list 'more plain text' 'second'
  expect FAIL stale-entry-in-the-list

  fixture listed-line-and-its-entry
  printf 'notes.txt\t/%s/zoe\tmade up\n' "$u" >>"$scratch/listed-line-and-its-entry/scripts/home-path-allow.txt"
  add_commit listed-line-and-its-entry "see /$u/zoe/x" 'second'
  expect PASS listed-line-and-its-entry

  fixture list-malformed-in-the-commit
  printf 'x\tnot a path\treason\n' >>"$scratch/list-malformed-in-the-commit/scripts/home-path-allow.txt"
  add_commit list-malformed-in-the-commit 'more plain text' 'second'
  expect FAIL list-malformed-in-the-commit


  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$((rejected + predicate))"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
