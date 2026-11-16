#!/bin/sh
# jobs: macos windows
# lane: free
# rule: On a tag ref (GITHUB_REF_TYPE=tag) the committed crates/halvo-capture/src/store/db.rs at HEAD holds no statement CREATE VIRTUAL TABLE IF NOT EXISTS events_fts, the search index that store-4 takes out; every other ref passes, an absent one among them, and so does a HEAD without that file, while a db.rs that HEAD holds and git cannot read as a file fails on every ref.
set -u
name=54-tag-needs-store4
store=crates/halvo-capture/src/store/db.rs
statement='CREATE VIRTUAL TABLE IF NOT EXISTS events_fts'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

statement_line() {
  printf '%s\n' "$1" | LC_ALL=C awk '{ text = tolower($0) } text ~ /create[ \t]+virtual[ \t]+table[ \t]+if[ \t]+not[ \t]+exists[ \t]+events_fts([^a-z0-9_]|$)/ { print NR; exit }'
}

measure() {
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_COMMON_DIR
  root=${CI_D_REPO:-$(pwd)}
  cd "$root" || fail "the root of the checkout cannot be entered"
  if [ "${GITHUB_REF_TYPE:-}" = tag ]; then tag_ref=true; else tag_ref=false; fi
  head=$(git rev-parse --verify -q 'HEAD^{commit}') || fail "git cannot read HEAD of the checkout"
  entry=$(git ls-tree --full-tree "$head" -- "$store") || fail "git ls-tree cannot read the tree of HEAD"
  if [ -z "$entry" ]; then
    printf 'tag ref: %s, %s at HEAD: absent, index statement: false\n' "$tag_ref" "$store"
    printf 'PASS %s\n' "$name"
    exit 0
  fi
  mode=${entry%% *}
  kind=${entry#* }
  object=${kind#* }
  object=${object%%"	"*}
  kind=${kind%% *}
  case $mode:$kind in
    100644:blob|100755:blob) ;;
    *) fail "$store at HEAD is a $kind of mode $mode, not a file that git can read" ;;
  esac
  text=$(git cat-file blob "$object" 2>/dev/null) || fail "git cannot read $store at HEAD"
  line=$(statement_line "$text")
  if [ -z "$line" ]; then
    printf 'tag ref: %s, %s at HEAD: present, index statement: false\n' "$tag_ref" "$store"
    printf 'PASS %s\n' "$name"
    exit 0
  fi
  if [ "$tag_ref" = true ]; then
    if [ -n "${GITHUB_REF_NAME:-}" ]; then refused="the tag $GITHUB_REF_NAME is refused"; else refused='a tag of no name is refused'; fi
    fail "$refused: $store at HEAD holds $statement on line $line, the search index that keeps words of a removed event, and store-4 takes that statement out before any tag"
  fi
  printf 'tag ref: false, %s at HEAD: present, index statement: true, on line %s\n' "$store" "$line"
  printf 'PASS %s\n' "$name"
}

self_test() {
  self_dir=$(dirname "$0") || exit 2
  self_dir=$(cd "$self_dir" && pwd) || exit 2
  self=$self_dir/${0##*/}
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  with_statement='impl Schema {
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY);\n\
                 CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(foreground_app);",
            )
}'
  without_statement='impl Schema {
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY);",
            )
            .execute_batch("INSERT INTO events_fts(events_fts, rank) VALUES(1, 1);")
            .execute_batch("DROP TABLE events_fts;")
}'
  fixture() {
    repo=$work/$1
    mkdir -p "$repo" || exit 2
    git init -q "$repo" || exit 2
    printf 'a fixture of %s\n' "$name" >"$repo/README.md"
    if [ "$2" != absent ]; then
      mkdir -p "$repo/crates/halvo-capture/src/store" || exit 2
      printf '%s\n' "$2" >"$repo/$store"
    fi
    git -C "$repo" add -A && git -C "$repo" commit -q -m fixture || exit 2
  }
  run_case() {
    case_name=$1 want=$2 dir=$3 want_text=$4
    shift 4
    status=0
    (
      unset GITHUB_REF_TYPE GITHUB_REF_NAME CI_D_REPO GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
      cd "$dir" && env "$@" sh "$self"
    ) >"$work/out-$case_name" 2>&1 || status=$?
    last=$(awk 'NF { last = $0 } END { print last }' "$work/out-$case_name")
    case $status:$last in
      "0:PASS $name") got=PASS ;;
      "1:FAIL $name ("*")") got=FAIL ;;
      *) got=exit-$status ;;
    esac
    grep -q -F -e "$want_text" "$work/out-$case_name" || got=$got-without-the-text
    verdict "$case_name" "$want" "$got"
  }
  fixture with "$with_statement"
  fixture without "$without_statement"
  on_line_4="$statement on line 4"
  run_case tag-with-the-statement FAIL "$work/with" "the tag v1.2.4 is refused: $store at HEAD holds $on_line_4" GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  got=FAIL
  grep -q -F -e 'the ref is' "$work/out-tag-with-the-statement" || got=PASS
  verdict cause-names-the-refused-tag-and-no-ref PASS "$got"
  run_case tag-without-the-statement PASS "$work/without" 'index statement: false' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  run_case branch-with-the-statement PASS "$work/with" 'index statement: true' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main
  run_case merge-ref-of-a-pull-request-with-the-statement PASS "$work/with" 'tag ref: false' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=12/merge
  run_case no-ref-with-the-statement PASS "$work/with" 'tag ref: false'
  run_case empty-ref-type-with-the-statement PASS "$work/with" 'tag ref: false' GITHUB_REF_TYPE= GITHUB_REF_NAME=v1.2.4
  run_case tag-without-a-name-with-the-statement FAIL "$work/with" 'a tag of no name is refused: ' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=
  run_case root-from-ci-d-repo FAIL "$work" "$on_line_4" GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4 "CI_D_REPO=$work/with"
  run_case git-dir-of-another-repository-in-the-environment FAIL "$work/with" "$on_line_4" GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4 "GIT_DIR=$work/without/.git"

  fixture name-in-other-files "$without_statement"
  repo=$work/name-in-other-files
  printf '%s\n' "formal-second-wrap-in-another-table	db.rs	s/(                 $statement USING fts5)/x/" >"$repo/crates/halvo-capture/src/store/mutants.rows"
  mkdir -p "$repo/crates/halvo-capture/tests" || exit 2
  printf '%s\n' "const SCHEMA: &str = \"$statement USING fts5(foreground_app);\";" >"$repo/crates/halvo-capture/tests/index.rs"
  printf '%s\n' "        \"DELETE FROM events_fts WHERE rowid IN (1)\"," >"$repo/crates/halvo-capture/src/store/expiry.rs"
  git -C "$repo" add -A && git -C "$repo" commit -q -m 'The name in other files' || exit 2
  run_case tag-with-the-name-in-other-files-alone PASS "$repo" 'index statement: false' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4

  fixture absent absent
  run_case tag-on-a-tree-without-db-rs PASS "$work/absent" 'at HEAD: absent' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4

  fixture only-in-the-work-tree "$without_statement"
  printf '%s\n' "$with_statement" >"$work/only-in-the-work-tree/$store"
  run_case tag-with-the-statement-in-the-work-tree-alone PASS "$work/only-in-the-work-tree" 'index statement: false' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  fixture committed-and-removed-from-the-work-tree "$with_statement"
  printf '%s\n' "$without_statement" >"$work/committed-and-removed-from-the-work-tree/$store"
  run_case tag-with-the-statement-committed-alone FAIL "$work/committed-and-removed-from-the-work-tree" "$on_line_4" GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4

  lower=$(printf '%s\n' "$with_statement" | tr '[:upper:]' '[:lower:]')
  fixture lower-case "$lower"
  run_case tag-with-the-statement-in-lower-case FAIL "$work/lower-case" 'on line 4' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  spaced=$(printf '%s\n' "$with_statement" | sed 's/CREATE VIRTUAL TABLE IF NOT EXISTS events_fts/CREATE  VIRTUAL	TABLE 	IF  NOT	 EXISTS  events_fts/')
  fixture more-blanks "$spaced"
  run_case tag-with-more-blanks-in-the-statement FAIL "$work/more-blanks" 'on line 4' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  at_end=$(printf '%s\n' "$with_statement" | sed 's/events_fts USING fts5(foreground_app);",/events_fts/')
  fixture name-at-the-end-of-the-line "$at_end"
  run_case tag-with-the-statement-at-the-end-of-a-line FAIL "$work/name-at-the-end-of-the-line" 'on line 4' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  other_table=$(printf '%s\n' "$with_statement" | sed 's/events_fts USING/events_fts_set_aside USING/')
  fixture another-table "$other_table"
  run_case tag-with-a-table-of-a-longer-name PASS "$work/another-table" 'index statement: false' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4

  missing=0123456789abcdef0123456789abcdef01234567
  fixture unreadable absent
  repo=$work/unreadable
  git -C "$repo" update-index --add --cacheinfo "100644,$missing,$store" || exit 2
  tree=$(git -C "$repo" write-tree --missing-ok) || exit 2
  commit=$(git -C "$repo" commit-tree "$tree" -p HEAD -m 'A db.rs whose blob is missing') || exit 2
  git -C "$repo" update-ref HEAD "$commit" || exit 2
  run_case tag-with-a-db-rs-that-git-cannot-read FAIL "$repo" 'git cannot read' GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4
  run_case branch-with-a-db-rs-that-git-cannot-read FAIL "$repo" 'git cannot read' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main

  fixture symbolic-link absent
  repo=$work/symbolic-link
  target=$(printf 'schema.rs' | git -C "$repo" hash-object -w --stdin) || exit 2
  git -C "$repo" update-index --add --cacheinfo "120000,$target,$store" || exit 2
  tree=$(git -C "$repo" write-tree) || exit 2
  commit=$(git -C "$repo" commit-tree "$tree" -p HEAD -m 'A db.rs that is a symbolic link') || exit 2
  git -C "$repo" update-ref HEAD "$commit" || exit 2
  run_case db-rs-that-is-a-symbolic-link FAIL "$repo" 'not a file that git can read' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main

  mkdir -p "$work/no-repository" || exit 2
  run_case outside-a-repository FAIL "$work/no-repository" 'git cannot read HEAD' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main "GIT_CEILING_DIRECTORIES=$work"

  status=0
  (
    GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main CI_D_REPO=$work/without
    export GITHUB_REF_TYPE GITHUB_REF_NAME CI_D_REPO
    cd "$work/with" && GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4 CI_D_REPO=$work/with sh "$self"
  ) >"$work/out-call-of-release-sh" 2>&1 || status=$?
  got=PASS
  [ "$status" = 1 ] && grep -q -F -e "the tag v1.2.4 is refused: $store at HEAD holds $on_line_4" "$work/out-call-of-release-sh" && got=FAIL
  verdict call-of-release-sh-over-a-branch-ref-and-a-foreign-root FAIL "$got"
  status=0
  (
    GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4 CI_D_REPO=$work/with
    export GITHUB_REF_TYPE GITHUB_REF_NAME CI_D_REPO
    cd "$work/without" && GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4 CI_D_REPO=$work/without sh "$self"
  ) >"$work/out-call-of-release-sh-in-a-tree-without" 2>&1 || status=$?
  got=FAIL
  [ "$status" = 0 ] && grep -q -x -F -e "PASS $name" "$work/out-call-of-release-sh-in-a-tree-without" && got=PASS
  verdict call-of-release-sh-in-a-tree-without-the-statement-under-a-foreign-root PASS "$got"

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) printf 'usage: sh %s [--self-test]\n' "scripts/r1/ci.d/$name.sh" >&2; exit 64 ;;
esac
