#!/bin/sh
# jobs: macos windows
# lane: free
# rule: On a tag ref (GITHUB_REF_TYPE=tag) no tracked file under crates/halvo-capture/src at HEAD holds a statement that creates the search index events_fts (CREATE VIRTUAL TABLE, with or without IF NOT EXISTS, in any letter case, across line breaks and string continuations, the name quoted or after a schema), which store-4 takes out; every other ref passes, an absent one among them, and so does a HEAD without that directory, while a link or a submodule under it, or a file that git or awk cannot read, fails on every ref.
set -u
name=54-tag-needs-store4
src=crates/halvo-capture/src
store=$src/store/db.rs
statement='CREATE VIRTUAL TABLE IF NOT EXISTS events_fts'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

creates() {
  LC_ALL=C awk '
    { text = text sep tolower($0); sep = "\n" }
    END {
      line = 1
      while (match(text, /create([ \t\r\n]|\\[ntr]|\\\n)+virtual([ \t\r\n]|\\[ntr]|\\\n)+table([ \t\r\n]|\\[ntr]|\\\n)+(if([ \t\r\n]|\\[ntr]|\\\n)+not([ \t\r\n]|\\[ntr]|\\\n)+exists([ \t\r\n]|\\[ntr]|\\\n)+)?([a-z_][a-z0-9_]*([ \t\r\n]|\\\n)*[.]([ \t\r\n]|\\\n)*)?(\\?["\047`[])?events_fts([^a-z0-9_]|$)/)) {
        before = substr(text, 1, RSTART - 1)
        print line + gsub(/\n/, "\n", before)
        taken = substr(text, 1, RSTART + RLENGTH - 1)
        line += gsub(/\n/, "\n", taken)
        text = substr(text, RSTART + RLENGTH)
      }
    }'
}

measure() {
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_COMMON_DIR CDPATH
  unset GIT_GLOB_PATHSPECS GIT_NOGLOB_PATHSPECS GIT_ICASE_PATHSPECS GIT_LITERAL_PATHSPECS
  GIT_NO_REPLACE_OBJECTS=1
  export GIT_NO_REPLACE_OBJECTS
  root=${CI_D_REPO:-$(pwd)}
  case $root in
    /*|[A-Za-z]:*) ;;
    *) root=$(pwd)/$root ;;
  esac
  if [ "${GITHUB_REF_TYPE:-}" = tag ]; then tag_ref=true; else tag_ref=false; fi
  head=$(git -C "$root" rev-parse --verify -q 'HEAD^{commit}') || fail "git cannot read HEAD of the checkout"
  root=$(git -C "$root" rev-parse --show-toplevel) || fail "git cannot read the top of the checkout"
  tree=$head
  for part in crates halvo-capture src; do
    object=$(git -C "$root" rev-parse --verify -q "$tree:$part") || {
      printf 'tag ref: %s, %s at HEAD: absent, statements that create the index: 0\n' "$tag_ref" "$src"
      printf 'PASS %s\n' "$name"
      exit 0
    }
    kind=$(git -C "$root" cat-file -t "$object") || fail "git cannot read the part $part of $src at HEAD"
    [ "$kind" = tree ] || fail "the part $part of $src at HEAD is a $kind, not a directory that git can read"
    tree=$object
  done
  listing=$(git -C "$root" ls-tree -r --full-tree "$tree") || fail "git cannot read the tree $src at HEAD"
  files=$(printf '%s\n' "$listing" | awk 'NF { n++ } END { print n + 0 }') || fail "awk cannot count the files under $src at HEAD"
  other=$(printf '%s\n' "$listing" | awk -F '\t' 'NF && $1 !~ /^100(644|755) blob / { print $2; exit }') || fail "awk cannot read the listing of $src at HEAD"
  [ -z "$other" ] || fail "$src/$other at HEAD is a link or a submodule, which this check cannot read"
  objects=$(printf '%s\n' "$listing" | awk 'NF { print $3 }') || fail "awk cannot read the listing of $src at HEAD"
  kinds=$(printf '%s\n' "$objects" | git -C "$root" cat-file --batch-check='%(objecttype)') || fail "git cannot read the files under $src at HEAD"
  unread=$(printf '%s\n' "$kinds" | awk 'NF && $0 != "blob" { n++ } END { print n + 0 }') || fail "awk cannot read what git says of the files under $src at HEAD"
  [ "$unread" = 0 ] || fail "git cannot read $unread files under $src at HEAD"
  found=0
  candidates=$(git -C "$root" grep -l -i -e virtual "$tree") || found=$?
  [ "$found" -le 1 ] || fail "git cannot read the files under $src at HEAD"
  read_files=0
  count=0
  first=
  while IFS= read -r candidate; do
    [ -n "$candidate" ] || continue
    path=${candidate#"$tree":}
    text=$(git -C "$root" cat-file blob "$tree:$path") || fail "git cannot read $src/$path at HEAD"
    lines=$(printf '%s\n' "$text" | creates) || fail "awk cannot read $src/$path at HEAD"
    here=$(printf '%s\n' "$lines" | awk 'NF { n++ } END { print n + 0 }') || fail "awk cannot count the statements in $src/$path"
    read_files=$((read_files + 1))
    if [ "$here" -gt 0 ]; then
      count=$((count + here))
      [ -n "$first" ] || first="$src/$path line ${lines%%[!0-9]*}"
    fi
  done <<EOF
$candidates
EOF
  if [ "$count" -gt 0 ] && [ "$tag_ref" = true ]; then
    if [ -n "${GITHUB_REF_NAME:-}" ]; then refused="the tag $GITHUB_REF_NAME is refused"; else refused='a tag of no name is refused'; fi
    fail "$refused: $first holds a statement that creates the search index events_fts ($count under $src at HEAD in all), the index that keeps words of a removed event; store-4 takes it out before any tag"
  fi
  where=
  [ "$count" = 0 ] || where=", the first at $first"
  printf 'tag ref: %s, %s at HEAD: present, files: %s, files read for a create: %s, statements that create the index: %s%s\n' "$tag_ref" "$src" "$files" "$read_files" "$count" "$where"
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
  add_file() {
    mkdir -p "$repo/${1%/*}" || exit 2
    printf '%s\n' "$2" >"$repo/$1"
    git -C "$repo" add -A && git -C "$repo" commit -q -m "Add $1" || exit 2
  }
  run_case() {
    case_name=$1 want=$2 dir=$3 want_text=$4
    shift 4
    status=0
    (
      unset GITHUB_REF_TYPE GITHUB_REF_NAME CI_D_REPO GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE CDPATH
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
  tag='GITHUB_REF_TYPE=tag'
  name_v='GITHUB_REF_NAME=v1.2.4'
  fixture with "$with_statement"
  fixture without "$without_statement"
  on_line_4="$store line 4 holds a statement that creates the search index events_fts"
  run_case tag-with-the-statement FAIL "$work/with" "the tag v1.2.4 is refused: $on_line_4 (1 under $src at HEAD in all)" "$tag" "$name_v"
  got=FAIL
  grep -q -F -e 'the ref is' "$work/out-tag-with-the-statement" || got=PASS
  verdict cause-names-the-refused-tag-and-no-ref PASS "$got"
  run_case tag-without-the-statement PASS "$work/without" 'statements that create the index: 0' "$tag" "$name_v"
  run_case branch-with-the-statement PASS "$work/with" "statements that create the index: 1, the first at $store line 4" GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main
  run_case merge-ref-of-a-pull-request-with-the-statement PASS "$work/with" 'tag ref: false' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=12/merge
  run_case no-ref-with-the-statement PASS "$work/with" 'tag ref: false'
  run_case empty-ref-type-with-the-statement PASS "$work/with" 'tag ref: false' GITHUB_REF_TYPE= "$name_v"
  run_case tag-without-a-name-with-the-statement FAIL "$work/with" 'a tag of no name is refused: ' "$tag" GITHUB_REF_NAME=
  run_case root-from-ci-d-repo FAIL "$work" "$on_line_4" "$tag" "$name_v" "CI_D_REPO=$work/with"
  run_case git-dir-of-another-repository-in-the-environment FAIL "$work/with" "$on_line_4" "$tag" "$name_v" "GIT_DIR=$work/without/.git"
  run_case run-from-a-subdirectory-of-the-checkout FAIL "$work/with/crates/halvo-capture" "$on_line_4" "$tag" "$name_v"
  run_case pathspec-variables-on-a-tag FAIL "$work/with" "$on_line_4" "$tag" "$name_v" GIT_GLOB_PATHSPECS=1 GIT_ICASE_PATHSPECS=1
  run_case pathspec-variables-on-a-branch PASS "$work/with" 'statements that create the index: 1' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main GIT_GLOB_PATHSPECS=1 GIT_ICASE_PATHSPECS=1

  fixture moved-to-another-file "$without_statement"
  add_file "$src/store/schema.rs" "$with_statement"
  run_case tag-with-the-statement-in-another-file-under-src FAIL "$repo" "$src/store/schema.rs line 4 holds" "$tag" "$name_v"
  without_if=$(printf '%s\n' "$with_statement" | sed 's/TABLE IF NOT EXISTS events_fts/TABLE events_fts/')
  fixture without-if-not-exists "$without_if"
  run_case tag-with-the-statement-without-if-not-exists FAIL "$repo" "$on_line_4" "$tag" "$name_v"
  fixture lower-case-in-another-file "$without_statement"
  add_file "$src/lib.rs" 'const INDEX: &str = "create virtual table events_fts using fts5(foreground_app)";'
  run_case tag-with-the-statement-in-lower-case-in-another-file FAIL "$repo" "$src/lib.rs line 1 holds" "$tag" "$name_v"
  continued=$(printf '%s\n' "$with_statement" | sed 's/IF NOT EXISTS events_fts USING/IF NOT EXISTS \\\
                 events_fts USING/')
  fixture split-by-a-continuation "$continued"
  run_case tag-with-the-statement-split-over-two-lines-by-a-continuation FAIL "$repo" "$on_line_4" "$tag" "$name_v"
  fixture split-over-two-lines "$without_statement"
  add_file "$src/store/schema.rs" 'const SCHEMA: &str = r#"
CREATE VIRTUAL TABLE
    IF NOT EXISTS events_fts USING fts5(foreground_app);
"#;'
  run_case tag-with-the-statement-split-over-two-lines FAIL "$repo" "$src/store/schema.rs line 2 holds" "$tag" "$name_v"
  quoted=$(printf '%s\n' "$with_statement" | sed 's/EXISTS events_fts USING/EXISTS \\"events_fts\\" USING/')
  fixture quoted-name "$quoted"
  run_case tag-with-the-name-quoted FAIL "$repo" "$on_line_4" "$tag" "$name_v"
  schema=$(printf '%s\n' "$with_statement" | sed 's/EXISTS events_fts USING/EXISTS main.events_fts USING/')
  fixture schema-prefix "$schema"
  run_case tag-with-a-schema-before-the-name FAIL "$repo" "$on_line_4" "$tag" "$name_v"
  fixture two-statements "$with_statement"
  add_file "$src/store/schema.rs" "$without_if"
  run_case tag-with-two-statements FAIL "$repo" "(2 under $src at HEAD in all)" "$tag" "$name_v"

  fixture names-that-create-nothing "$without_statement
const SCHEMA_TABLES: [&str; 3] = [\"segment_keys\", \"events\", \"events_fts\"];
// events_fts is the search index of foreground_app; a virtual table that store-4 takes out"
  add_file "$src/store/expiry.rs" "        \"DELETE FROM events_fts WHERE rowid IN \\"
  add_file "$src/store/formal_tests.rs" 'const EVENTS_INDEX: &str = "events_fts";
const EVENTS_INDEX_SET_ASIDE: &str = "events_fts_set_aside";'
  add_file crates/halvo-capture/tests/index.rs "const SCHEMA: &str = \"$statement USING fts5(foreground_app);\";"
  run_case tag-with-the-name-in-places-that-create-nothing PASS "$repo" 'statements that create the index: 0' "$tag" "$name_v"
  fixture mutants-rows-pattern "$without_statement"
  add_file "$src/store/mutants.rows" "formal-second-wrap-in-another-table	db.rs	s/(                 $statement USING fts5)/x/"
  run_case tag-with-the-statement-in-a-pattern-of-mutants-rows FAIL "$repo" "$src/store/mutants.rows line 1 holds" "$tag" "$name_v"

  fixture absent absent
  run_case tag-on-a-tree-without-the-capture-source PASS "$work/absent" "$src at HEAD: absent" "$tag" "$name_v"
  fixture src-without-db-rs absent
  add_file "$src/lib.rs" 'pub fn virtual_key() {}'
  run_case tag-on-a-capture-source-without-db-rs PASS "$repo" 'statements that create the index: 0' "$tag" "$name_v"

  fixture only-in-the-work-tree "$without_statement"
  printf '%s\n' "$with_statement" >"$work/only-in-the-work-tree/$store"
  run_case tag-with-the-statement-in-the-work-tree-alone PASS "$work/only-in-the-work-tree" 'statements that create the index: 0' "$tag" "$name_v"
  fixture committed-and-removed-from-the-work-tree "$with_statement"
  printf '%s\n' "$without_statement" >"$work/committed-and-removed-from-the-work-tree/$store"
  run_case tag-with-the-statement-committed-alone FAIL "$work/committed-and-removed-from-the-work-tree" "$on_line_4" "$tag" "$name_v"
  fixture replaced-blob "$with_statement"
  repo=$work/replaced-blob
  committed=$(git -C "$repo" rev-parse "HEAD:$store") || exit 2
  replacement=$(printf '%s\n' "$without_statement" | git -C "$repo" hash-object -w --stdin) || exit 2
  git -C "$repo" replace "$committed" "$replacement" || exit 2
  run_case tag-with-the-statement-under-a-replace-ref FAIL "$repo" "$on_line_4" "$tag" "$name_v"
  mkdir -p "$work/cdpath/repo" "$work/relative" || exit 2
  git init -q "$work/cdpath/repo" && git -C "$work/cdpath/repo" commit -q --allow-empty -m 'A repository that CDPATH finds' || exit 2
  fixture relative/repo "$with_statement"
  run_case relative-root-under-an-exported-cdpath FAIL "$work/relative" "$on_line_4" "$tag" "$name_v" CI_D_REPO=repo "CDPATH=$work/cdpath"

  lower=$(printf '%s\n' "$with_statement" | tr '[:upper:]' '[:lower:]')
  fixture lower-case "$lower"
  run_case tag-with-the-statement-in-lower-case FAIL "$work/lower-case" "$on_line_4" "$tag" "$name_v"
  spaced=$(printf '%s\n' "$with_statement" | sed 's/CREATE VIRTUAL TABLE IF NOT EXISTS events_fts/CREATE  VIRTUAL	TABLE 	IF  NOT	 EXISTS  events_fts/')
  fixture more-blanks "$spaced"
  run_case tag-with-more-blanks-in-the-statement FAIL "$work/more-blanks" "$on_line_4" "$tag" "$name_v"
  at_end=$(printf '%s\n' "$with_statement" | sed 's/events_fts USING fts5(foreground_app);",/events_fts/')
  fixture name-at-the-end-of-the-line "$at_end"
  run_case tag-with-the-statement-at-the-end-of-a-line FAIL "$work/name-at-the-end-of-the-line" "$on_line_4" "$tag" "$name_v"
  other_table=$(printf '%s\n' "$with_statement" | sed 's/events_fts USING/events_fts_set_aside USING/')
  fixture another-table "$other_table"
  run_case tag-with-a-table-of-a-longer-name PASS "$work/another-table" 'statements that create the index: 0' "$tag" "$name_v"

  missing=0123456789abcdef0123456789abcdef01234567
  fixture unreadable absent
  repo=$work/unreadable
  git -C "$repo" update-index --add --cacheinfo "100644,$missing,$store" || exit 2
  tree=$(git -C "$repo" write-tree --missing-ok) || exit 2
  commit=$(git -C "$repo" commit-tree "$tree" -p HEAD -m 'A db.rs whose blob is missing') || exit 2
  git -C "$repo" update-ref HEAD "$commit" || exit 2
  run_case tag-with-a-file-that-git-cannot-read FAIL "$repo" 'git cannot read' "$tag" "$name_v"
  run_case branch-with-a-file-that-git-cannot-read FAIL "$repo" 'git cannot read' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main

  link_case() {
    fixture "$1" absent
    repo=$work/$1
    add_file crates/halvo-capture/old-store/db.rs "$with_statement"
    target=$(printf '%s' "$3" | git -C "$repo" hash-object -w --stdin) || exit 2
    git -C "$repo" update-index --add --cacheinfo "120000,$target,$2" || exit 2
    tree=$(git -C "$repo" write-tree) || exit 2
    commit=$(git -C "$repo" commit-tree "$tree" -p HEAD -m 'A link') || exit 2
    git -C "$repo" update-ref HEAD "$commit" || exit 2
  }
  link_case db-rs-as-a-link "$store" ../../old-store/db.rs
  run_case db-rs-that-is-a-link FAIL "$repo" 'is a link or a submodule' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main
  link_case store-as-a-link "$src/store" ../old-store
  run_case store-directory-that-is-a-link FAIL "$repo" "$src/store at HEAD is a link or a submodule" "$tag" "$name_v"
  link_case src-as-a-link "$src" old-store
  run_case capture-source-that-is-a-link FAIL "$repo" 'the part src of' "$tag" "$name_v"

  mkdir -p "$work/bad-awk" || exit 2
  printf '%s\n' '#!/bin/sh' 'exit 2' >"$work/bad-awk/awk"
  chmod +x "$work/bad-awk/awk" || exit 2
  run_case awk-that-fails FAIL "$work/with" 'awk cannot' "$tag" "$name_v" "PATH=$work/bad-awk:$PATH"

  mkdir -p "$work/no-repository" || exit 2
  run_case outside-a-repository FAIL "$work/no-repository" 'git cannot read HEAD' GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main "GIT_CEILING_DIRECTORIES=$work"

  status=0
  (
    GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main CI_D_REPO=$work/without
    export GITHUB_REF_TYPE GITHUB_REF_NAME CI_D_REPO
    cd "$work/with" && GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.4 CI_D_REPO=$work/with sh "$self"
  ) >"$work/out-call-of-release-sh" 2>&1 || status=$?
  got=PASS
  [ "$status" = 1 ] && grep -q -F -e "the tag v1.2.4 is refused: $on_line_4" "$work/out-call-of-release-sh" && got=FAIL
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
