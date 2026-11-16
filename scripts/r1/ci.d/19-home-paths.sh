#!/bin/sh
# jobs: macos windows
# rule: scripts/check-home-paths.sh passes its self-test and finds no added line with a home path that scripts/home-path-allow.txt does not allow, and no message with a home path, in the commits of the head that the default branch does not hold, or, on a push to the default branch in a clone that is not shallow, in the commits of the push as the event file that GITHUB_EVENT_PATH names gives its before commit (range=the-push, before..HEAD; range=every-commit-of-the-head when that commit is all zeros or not in the clone; FAIL when the variable names no file or the before commit is no object id of 40 or 64 lower-case hexadecimal characters), or else in the head commit when the default branch holds it or the checkout does not, and in the message of the tag of a run that a pushed tag started, beside the head's commits (tag=read from the checkout; when the checkout's ref names a commit, tag=fetched-and-read from origin under refs/ci-d/19-home-paths/tag-of-the-run, that ref deleted again, or tag=remote-holds-a-commit; FAIL with tag=remote-holds-another-commit when the tag moved, tag=remote-holds-no-such-ref when origin no longer holds it, and tag=remote-could-not-be-asked when the ask or the fetch fails; tag=not-a-tag-run otherwise); beside the copy of this branch, the copies of scripts/check-home-paths.sh and scripts/commit-range.sh, with the exempt commit of scripts/check-commit-messages.sh, that the default branch holds, taken from the commit that refs/remotes/origin/<default branch> names by its object id (the default branch of the event file that GITHUB_EVENT_PATH names, else main), scan the same range and tag with the list of the scanned commit, and the run FAILs when that copy finds a path, prints no count or reads 0 commits where git lists one, when the commit holds no such scanner or range file, and in a full clone of a workflow run without that ref (copy-of-the-default-branch not-run in a shallow clone without it, as the windows job's pull request runs are, and outside a workflow run); and FAIL when the scan or --stale prints no count or the scan reads 0 commits where git lists one, and the list holds no entry that allows nothing in HEAD (sh scripts/check-home-paths.sh --stale); it does not refuse an entry of the list for the account that runs it, as the pre-push hook does, because the account of a hosted runner is no person's.
# lane: free
set -u
name=19-home-paths
own_tag_ref=refs/ci-d/19-home-paths/tag-of-the-run

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

make_scratch() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail 'mktemp failed'
  trap 'rm -rf "$scratch"' EXIT
  trap 'exit 2' INT TERM HUP
}

drop_own_tag_ref() {
  git update-ref -d "$own_tag_ref" 2>/dev/null
  return 0
}

tag_from_the_remote() {
  if ! git ls-remote origin "$GITHUB_REF" >"$scratch/ls-remote" 2>"$scratch/ls-remote-error" </dev/null; then
    tag=remote-could-not-be-asked
    tag_cause="git ls-remote could not ask origin for $GITHUB_REF, so the tag's message was not read; run the job again"
    return 1
  fi
  remote_object=$(awk -F '\t' -v ref="$GITHUB_REF" '$2 == ref { print $1 }' "$scratch/ls-remote") || remote_object=
  if [ -z "$remote_object" ]; then
    tag=remote-holds-no-such-ref
    tag_cause="origin holds no $GITHUB_REF any more, so the message of the tag that started this run cannot be read, and no rerun reads it; whoever still holds that tag can read it with git cat-file -p $GITHUB_REF"
    return 1
  fi
  if [ "$remote_object" = "$head_commit" ]; then
    tag=remote-holds-a-commit
    return 0
  fi
  trap 'drop_own_tag_ref; exit 2' INT TERM HUP
  fetch_done=0
  git -c maintenance.auto=false -c gc.auto=0 fetch -q --no-tags --no-write-fetch-head --refmap= origin "+$GITHUB_REF:$own_tag_ref" >"$scratch/fetch" 2>&1 </dev/null && fetch_done=1
  type=$(git cat-file -t "$own_tag_ref" 2>/dev/null) || type=
  fetched_commit=$(git rev-parse -q --verify "$own_tag_ref^{commit}" 2>/dev/null) || fetched_commit=
  case $fetch_done:$type in
    1:tag | 1:commit) ;;
    *)
      drop_own_tag_ref
      tag=remote-could-not-be-asked
      tag_cause="origin lists $GITHUB_REF, and git fetch could not fetch it, so the tag's message was not read; run the job again"
      return 1
      ;;
  esac
  if [ "$fetched_commit" != "$head_commit" ]; then
    drop_own_tag_ref
    tag=remote-holds-another-commit
    tag_cause="origin's $GITHUB_REF names another commit now than the one that this run checked out, so the tag moved after the run started and the message of the tag that started it cannot be read; the run that the newer push of the tag started reads the newer tag"
    return 1
  fi
  tag_to_read=$own_tag_ref
  tag=fetched-and-read
}

has_commit() {
  git rev-parse -q --verify "$1^{commit}" >/dev/null 2>&1
}

event_program='
let event;
try {
  event = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
} catch (error) {
  process.exit(3);
}
const field = (object, key) => (object !== null && typeof object === "object" && typeof object[key] === "string" ? object[key] : "");
const repository = event !== null && typeof event === "object" ? event.repository : null;
process.stdout.write(field(repository, "default_branch") + "\n" + field(event, "before") + "\n");
'

read_the_event() {
  default_branch=main
  event_before=
  [ -n "${GITHUB_EVENT_PATH:-}" ] || return 0
  command -v node >/dev/null 2>&1 || fail 'node is not on PATH, and the check reads the event file that GITHUB_EVENT_PATH names with it, to learn the default branch and the commits of a push; set Node.js up before this step'
  node -e "$event_program" "$GITHUB_EVENT_PATH" >"$scratch/event" 2>/dev/null </dev/null || fail 'the event file that GITHUB_EVENT_PATH names cannot be read as JSON, so the check cannot tell the default branch or the commits of a push; run the job again'
  default_branch=$(sed -n 1p "$scratch/event") || default_branch=
  event_before=$(sed -n 2p "$scratch/event") || event_before=
  if [ -z "$default_branch" ] || ! git check-ref-format "refs/heads/$default_branch"; then
    fail 'the event file that GITHUB_EVENT_PATH names gives no default branch that git reads as a branch name (repository.default_branch), so the check cannot take the copy of the default branch; run the job again'
  fi
}

take_the_copy_of_the_default_branch() {
  default_ref=refs/remotes/origin/$default_branch
  default_copy=
  default_note=
  if ! default_commit=$(git rev-parse -q --verify "$default_ref^{commit}" 2>/dev/null); then
    if [ "$shallow" = true ]; then
      default_note="not-run (no $default_ref in this shallow clone)"
      return 0
    fi
    [ -z "${GITHUB_EVENT_PATH:-}" ] || fail "$default_ref is not in this clone, so the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that the default branch holds cannot scan beside the copy of this branch; the checkout step needs fetch-depth: 0"
    default_note="not-run (no $default_ref in this clone)"
    return 0
  fi
  mkdir -p "$scratch/default/scripts" || fail 'mkdir failed'
  for file in check-home-paths.sh commit-range.sh; do
    git cat-file blob "$default_commit:scripts/$file" >"$scratch/default/scripts/$file" 2>/dev/null </dev/null ||
      fail "$default_ref holds no scripts/$file, so the copy of the default branch cannot scan beside the copy of this branch; restore the file on the default branch"
  done
  git cat-file blob "$default_commit:scripts/check-commit-messages.sh" >"$scratch/default/scripts/check-commit-messages.sh" 2>/dev/null </dev/null ||
    : >"$scratch/default/scripts/check-commit-messages.sh"
  default_copy=$scratch/default/scripts/check-home-paths.sh
}

range_of_the_push() {
  [ -n "${GITHUB_EVENT_PATH:-}" ] || fail "this run is a push to $default_branch, and GITHUB_EVENT_PATH names no event file, so the check cannot read which commits the push added; run it in the workflow, which sets the variable"
  before_form=hexadecimal
  case $event_before in
    '' | *[!0123456789abcdef]*) before_form=other ;;
  esac
  case ${#event_before} in
    40 | 64) ;;
    *) before_form=other ;;
  esac
  [ "$before_form" = hexadecimal ] || fail "the event file that GITHUB_EVENT_PATH names gives no before commit of 40 or 64 lower-case hexadecimal characters for this push to $default_branch, so the check cannot read which commits the push added; run the job again"
  case $event_before in
    *[!0]*)
      if has_commit "$event_before"; then
        kind=the-push
        range=$event_before..HEAD
        return 0
      fi
      ;;
  esac
  kind=every-commit-of-the-head
  range=HEAD
}

counts_of() {
  sed -n 's/.*commits read \([0-9]*\), lines with a home path \([0-9]*\)\.$/commits=\1 lines=\2/p' "$@"
}

scan_head() {
  repo=$1
  cd "$repo" || fail "cannot enter $repo"
  [ -f scripts/check-home-paths.sh ] || fail 'scripts/check-home-paths.sh is missing'
  head_commit=$(git rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null) || fail 'HEAD names no commit'
  read_the_event
  shallow=$(git rev-parse --is-shallow-repository 2>/dev/null) || shallow=unknown
  take_the_copy_of_the_default_branch
  tag=not-a-tag-run
  tag_to_read=
  tag_cause=
  case ${GITHUB_REF:-} in
    refs/tags/?*)
      type=$(git cat-file -t "$GITHUB_REF" 2>/dev/null) || type=
      if [ "$type" = tag ]; then
        tag_to_read=$GITHUB_REF
        tag='read'
      else
        tag_from_the_remote
      fi
      ;;
  esac
  if [ "$shallow" != true ] && [ "${GITHUB_EVENT_NAME:-}" = push ] && [ "${GITHUB_REF:-}" = "refs/heads/$default_branch" ]; then
    range_of_the_push
  elif has_commit "$default_ref" && ! git merge-base --is-ancestor HEAD "$default_ref"; then
    kind=since-the-default-branch
    range=$default_ref..HEAD
  elif has_commit HEAD^1; then
    kind=head-commit
    range='HEAD^!'
  else
    kind=root-commit
    range=HEAD
  fi
  sh scripts/check-home-paths.sh "$range" ${tag_to_read:+"$tag_to_read"} >"$scratch/out" 2>"$scratch/err" </dev/null
  code=$?
  default_code=
  if [ -n "$default_copy" ]; then
    sh "$default_copy" "$range" ${tag_to_read:+"$tag_to_read"} >"$scratch/default-out" 2>"$scratch/default-err" </dev/null
    default_code=$?
  fi
  drop_own_tag_ref
  listed=$(git rev-list --count "$range" 2>/dev/null) || listed=0
  counts=$(counts_of "$scratch/out" "$scratch/err") || counts=
  printf 'range=%s tag=%s %s exit=%s\n' "$kind" "$tag" "${counts:-commits=unread lines=unread}" "$code"
  default_counts=
  if [ -n "$default_copy" ]; then
    default_counts=$(counts_of "$scratch/default-out" "$scratch/default-err") || default_counts=
    printf 'copy-of-the-default-branch %s exit=%s\n' "${default_counts:-commits=unread lines=unread}" "$default_code"
  else
    printf 'copy-of-the-default-branch %s\n' "$default_note"
  fi
  sh scripts/check-home-paths.sh --stale >"$scratch/stale" 2>&1 </dev/null
  stale_code=$?
  stale=$(sed -n 's/.*entries read \([0-9]*\), stale \([0-9]*\)\.$/entries=\1 stale=\2/p' "$scratch/stale") || stale=
  printf 'list %s exit=%s\n' "${stale:-entries=unread stale=unread}" "$stale_code"
  case $code in
    0)
      [ -n "$counts" ] || fail "sh scripts/check-home-paths.sh '$range' exited 0 and printed no count of the commits and lines that it read, so nothing shows what it read; run it on a clone of this head and read its last line"
      case $counts in
        commits=0\ *) [ "$listed" = 0 ] || fail "sh scripts/check-home-paths.sh '$range' read 0 commits, and git lists $listed for that range, so the scan did not read the head; run it on a clone of this head and compare its count with git rev-list --count '$range'" ;;
      esac
      ;;
    1)
      case $tag in
        fetched-and-read) fail "an added line or a message holds a home path that scripts/home-path-allow.txt does not allow; on a clone of this head, fetch the tag whole and run the scan: git fetch origin +$GITHUB_REF:$GITHUB_REF && sh scripts/check-home-paths.sh '$range' $GITHUB_REF" ;;
        read) fail "an added line or a message holds a home path that scripts/home-path-allow.txt does not allow; on a clone of this head that holds the tag, run the scan: sh scripts/check-home-paths.sh '$range' $GITHUB_REF" ;;
        *) fail "an added line or a message holds a home path that scripts/home-path-allow.txt does not allow; run sh scripts/check-home-paths.sh '$range' on a clone of this head" ;;
      esac
      ;;
    *) fail "sh scripts/check-home-paths.sh could not check the range (exit $code); run sh scripts/check-home-paths.sh '$range' on a clone of this head and read its error" ;;
  esac
  if [ -n "$default_copy" ]; then
    way_on="on a clone of this head, run the scanner of a checkout of the default branch: sh <checkout of the default branch>/scripts/check-home-paths.sh '$range'${tag_to_read:+ $GITHUB_REF}"
    case $default_code in
      0)
        [ -n "$default_counts" ] || fail "the copy of scripts/check-home-paths.sh that $default_ref holds exited 0 and printed no count of the commits and lines that it read, so nothing shows what it read; $way_on, and read its last line"
        case $default_counts in
          commits=0\ *) [ "$listed" = 0 ] || fail "the copy of scripts/check-home-paths.sh that $default_ref holds read 0 commits, and git lists $listed for that range, so it did not read the head; $way_on" ;;
        esac
        ;;
      1) fail "the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that $default_ref holds finds an added line or a message with a home path that scripts/home-path-allow.txt does not allow, and the copy of this branch finds none: this branch changed the scanner or its range file, or the default branch holds an older scanner that refuses a line which the newer one accepts; the commits must pass both copies, so take each line that it lists out of the commit that added it, and undo a change that weakens the scan; $way_on" ;;
      *) fail "the copy of scripts/check-home-paths.sh that $default_ref holds could not check the range (exit $default_code); $way_on, and read its error" ;;
    esac
  fi
  [ -z "$tag_cause" ] || fail "tag=$tag: $tag_cause"
  case $stale_code in
    0)
      [ -n "$stale" ] || fail 'sh scripts/check-home-paths.sh --stale exited 0 and printed no count of the entries that it read, so nothing shows what it read; run it on a clone of this head and read its last line'
      printf 'PASS %s\n' "$name"
      ;;
    1) fail "an entry of scripts/home-path-allow.txt allows nothing in HEAD; run sh scripts/check-home-paths.sh --stale" ;;
    *) fail "sh scripts/check-home-paths.sh --stale could not check the list (exit $stale_code); run it on a clone of this head and read its error" ;;
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
  (scan_head "$scratch/$2") >"$scratch/$2.out" 2>&1
  code=$?
  judge "$@"
}

judge() {
  want=$1
  case_name=$2
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

scanner_that_waits() {
  cat <<'EOF'
#!/bin/sh
: >"$SCAN_STARTED"
while [ ! -f "$SCAN_GO" ]; do sleep 1; done
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

scanner_without_a_stale_count() {
  cat <<'EOF'
#!/bin/sh
case $1 in
  --stale) exit 0 ;;
esac
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

clone_with_the_tag_as_its_commit() {
  git clone -q --no-local "$scratch/$1" "$scratch/$2" || exit 2
  git -C "$scratch/$2" update-ref refs/tags/v1 'refs/tags/v1^{commit}' || exit 2
}

state_of_the_clone() {
  git -C "$scratch/$1" for-each-ref || exit 2
  git -C "$scratch/$1" config --list --local || exit 2
  git -C "$scratch/$1" status --porcelain || exit 2
}

expect_the_clone_unchanged() {
  state_of_the_clone "$2" >"$scratch/$2.before"
  expect "$@"
  state_of_the_clone "$2" >"$scratch/$2.after"
  if cmp -s "$scratch/$2.before" "$scratch/$2.after"; then
    printf 'case %s/%s-leaves-the-clone-as-it-was want=same got=same\n' "$name" "$2"
  else
    printf 'case %s/%s-leaves-the-clone-as-it-was want=same got=changed\n' "$name" "$2"
    wrong="$wrong $2-leaves-the-clone-as-it-was"
  fi
}

weak_scanner() {
  cat <<'EOF'
#!/bin/sh
case ${1:-} in
  --stale)
    printf 'check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n'
    exit 0
    ;;
esac
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

event_of_a_run() {
  printf '{"ref": "%s", "before": "%s", "forced": %s, "repository": {"default_branch": "%s"}}\n' "$3" "$4" "$5" "$6" >"$scratch/$1.event.json" || exit 2
  GITHUB_EVENT_NAME=$2
  GITHUB_REF=$3
  GITHUB_EVENT_PATH=$scratch/$1.event.json
}

no_event() {
  unset GITHUB_EVENT_NAME GITHUB_REF GITHUB_EVENT_PATH
}

case_branch_scanner_that_prints_counts_and_finds_nothing() {
  c=branch-scanner-that-prints-counts-and-finds-nothing
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/trunk HEAD || exit 2
  weak_scanner >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'weaken the scanner'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false trunk
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that refs/remotes/origin/trunk holds finds'
  no_event
}

case_branch_range_file_that_leaves_out_the_commit_with_the_path() {
  c=branch-range-file-that-leaves-out-the-commit-with-the-path
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  add_commit "$c" "see /$u/zoe/x" 'second'
  printf 'commit_range() { git rev-list -n 1 HEAD; }\n' >>"$scratch/$c/scripts/commit-range.sh" || exit 2
  add_commit "$c" 'plain again' 'list the head commit alone'
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that refs/remotes/origin/main holds finds'
}

case_list_entry_of_the_branch_holds_for_both_copies() {
  c=list-entry-of-the-branch-holds-for-both-copies
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  printf 'notes.txt\t/%s/zoe\tmade up\n' "$u" >>"$scratch/$c/scripts/home-path-allow.txt" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'second'
  expect PASS "$c" 'copy-of-the-default-branch commits=1 lines=0 exit=0'
}

case_default_branch_missing_from_a_full_clone_of_a_workflow_run() {
  c=default-branch-missing-from-a-full-clone-of-a-workflow-run
  fixture "$c"
  add_commit "$c" 'more plain text' 'second'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'refs/remotes/origin/main is not in this clone'
  no_event
}

case_default_branch_without_the_range_file() {
  c=default-branch-without-the-range-file
  fixture "$c"
  mv "$scratch/$c/scripts/commit-range.sh" "$scratch/$c.commit-range.sh" || exit 2
  add_commit "$c" 'more plain text' 'take the range file out'
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  mv "$scratch/$c.commit-range.sh" "$scratch/$c/scripts/commit-range.sh" || exit 2
  add_commit "$c" 'plain again' 'put the range file back'
  expect FAIL "$c" 'refs/remotes/origin/main holds no scripts/commit-range.sh'
}

case_shallow_clone_without_the_default_branch() {
  c=shallow-clone-without-the-default-branch
  fixture "$c-source"
  add_commit "$c-source" 'more plain text' 'second'
  add_commit "$c-source" 'plain again' 'third'
  git clone -q --no-local --depth 2 "$scratch/$c-source" "$scratch/$c" || exit 2
  git -C "$scratch/$c" remote set-head origin --delete >/dev/null 2>&1
  for branch in main master; do
    git -C "$scratch/$c" update-ref -d "refs/remotes/origin/$branch" >/dev/null 2>&1
  done
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect PASS "$c" 'copy-of-the-default-branch not-run (no refs/remotes/origin/main in this shallow clone)'
  no_event
}

push_of_three_commits() {
  fixture "$1"
  below=$(git -C "$scratch/$1" rev-parse HEAD) || exit 2
  add_commit "$1" "$2" 'second'
  add_commit "$1" 'plain again' 'third'
  add_commit "$1" 'plain once more' 'fourth'
  git -C "$scratch/$1" update-ref refs/remotes/origin/main HEAD || exit 2
}

case_push_of_three_commits_to_the_default_branch() {
  c=push-of-three-commits-to-the-default-branch
  push_of_three_commits "$c" "see /$u/zoe/x"
  event_of_a_run "$c" push refs/heads/main "$below" false main
  expect FAIL "$c" 'range=the-push tag=not-a-tag-run commits=3 lines=1 '
  no_event
}

case_clean_push_of_three_commits_to_the_default_branch() {
  c=clean-push-of-three-commits-to-the-default-branch
  push_of_three_commits "$c" 'more plain text'
  event_of_a_run "$c" push refs/heads/main "$below" false main
  expect PASS "$c" 'range=the-push tag=not-a-tag-run commits=3 lines=0 '
  no_event
}

case_push_that_creates_the_default_branch() {
  c=push-that-creates-the-default-branch
  push_of_three_commits "$c" "see /$u/zoe/x"
  event_of_a_run "$c" push refs/heads/main 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'range=every-commit-of-the-head tag=not-a-tag-run commits=4 lines=1 '
  no_event
}

case_forced_push_whose_old_tip_is_not_in_the_clone() {
  c=forced-push-whose-old-tip-is-not-in-the-clone
  push_of_three_commits "$c" "see /$u/zoe/x"
  event_of_a_run "$c" push refs/heads/main abababababababababababababababababababab true main
  expect FAIL "$c" 'range=every-commit-of-the-head tag=not-a-tag-run commits=4 lines=1 '
  no_event
}

case_push_to_the_default_branch_with_an_event_that_is_no_json() {
  c=push-to-the-default-branch-with-an-event-that-is-no-json
  push_of_three_commits "$c" 'more plain text'
  event_of_a_run "$c" push refs/heads/main "$below" false main
  printf '{"before": "made-up-marker-of-the-event-file\n' >"$GITHUB_EVENT_PATH" || exit 2
  expect FAIL "$c" 'cannot be read as JSON'
  no_event
  if grep -q 'made-up-marker-of-the-event-file' "$scratch/$c.out"; then
    got=printed-the-file
  else
    got=printed-nothing-of-the-file
  fi
  printf 'case %s/%s-prints-nothing-of-the-file want=printed-nothing-of-the-file got=%s\n' "$name" "$c" "$got"
  [ "$got" = printed-nothing-of-the-file ] || wrong="$wrong $c-prints-nothing-of-the-file"
}

case_push_whose_before_holds_upper_case_letters() {
  c=push-whose-before-holds-upper-case-letters
  push_of_three_commits "$c" 'more plain text'
  event_of_a_run "$c" push refs/heads/main ABABABABABABABABABABABABABABABABABABABAB false main
  expect FAIL "$c" 'gives no before commit of 40 or 64 lower-case hexadecimal characters'
  no_event
}

case_push_whose_before_is_39_characters_long() {
  c=push-whose-before-is-39-characters-long
  push_of_three_commits "$c" 'more plain text'
  short_before=$(printf '%.39s' "$below") || exit 2
  event_of_a_run "$c" push refs/heads/main "$short_before" false main
  expect FAIL "$c" 'gives no before commit of 40 or 64 lower-case hexadecimal characters'
  no_event
}

case_push_to_the_default_branch_without_an_event_file() {
  c=push-to-the-default-branch-without-an-event-file
  push_of_three_commits "$c" 'more plain text'
  GITHUB_EVENT_NAME=push
  GITHUB_REF=refs/heads/main
  expect FAIL "$c" 'GITHUB_EVENT_PATH names no event file'
  no_event
}

new_cases='
branch-scanner-that-prints-counts-and-finds-nothing
branch-range-file-that-leaves-out-the-commit-with-the-path
list-entry-of-the-branch-holds-for-both-copies
default-branch-missing-from-a-full-clone-of-a-workflow-run
default-branch-without-the-range-file
shallow-clone-without-the-default-branch
push-of-three-commits-to-the-default-branch
clean-push-of-three-commits-to-the-default-branch
push-that-creates-the-default-branch
forced-push-whose-old-tip-is-not-in-the-clone
push-to-the-default-branch-with-an-event-that-is-no-json
push-to-the-default-branch-without-an-event-file
push-whose-before-holds-upper-case-letters
push-whose-before-is-39-characters-long
'

run_new_case() {
  case " $(printf '%s' "$new_cases" | tr '\n' ' ') " in
    *" $1 "*) ;;
    *)
      printf 'FAIL self-test:%s (no case %s)\n' "$name" "$1"
      exit 1
      ;;
  esac
  case_function=case_$(printf '%s' "$1" | sed 's/-/_/g') || exit 2
  "$case_function"
}

self_test_start() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  make_scratch
  no_event
  wrong=
  rejected=0
  u=Users
}

self_test_end() {
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$1"
}

self_test_cases() {
  [ $# -gt 0 ] || {
    printf 'FAIL %s (usage: --self-test-case <name>...)\n' "$name"
    exit 1
  }
  self_test_start
  for one_case in "$@"; do
    run_new_case "$one_case"
  done
  self_test_end "$rejected"
}

self_test() {
  self_test_start

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
  clone_with_the_tag_as_its_commit home-path-in-the-message-of-the-pushed-tag tag-fetched-as-its-commit
  expect_the_clone_unchanged FAIL tag-fetched-as-its-commit ' tag=fetched-and-read '
  way_on=$(sed -n 's/^FAIL 19-home-paths (.*fetch the tag whole and run the scan: \(.*\))$/\1/p' "$scratch/tag-fetched-as-its-commit.out") || way_on=
  git clone -q --no-local "$scratch/home-path-in-the-message-of-the-pushed-tag" "$scratch/fresh-clone-for-the-way-on" || exit 2
  (cd "$scratch/fresh-clone-for-the-way-on" && eval "$way_on") >"$scratch/way-on.out" 2>&1
  way_on_code=$?
  if [ -z "$way_on" ]; then
    got=no-command-in-the-line
  elif grep -q 'refs/ci-d/' "$scratch/tag-fetched-as-its-commit.out"; then
    got=line-names-the-own-ref
  elif grep -q 'the message of the tag, line 1' "$scratch/way-on.out"; then
    got=exit-$way_on_code-with-the-hit
  else
    got=exit-$way_on_code-without-the-hit
  fi
  printf 'case %s/the-way-on-of-a-fetched-tag-finds-the-hit-on-a-fresh-clone want=exit-1-with-the-hit got=%s\n' "$name" "$got"
  [ "$got" = exit-1-with-the-hit ] || wrong="$wrong the-way-on-of-a-fetched-tag-finds-the-hit-on-a-fresh-clone"
  clone_with_the_tag_as_its_commit home-path-in-the-message-of-the-pushed-tag remote-of-the-tag-gone
  git -C "$scratch/remote-of-the-tag-gone" remote set-url origin "$scratch/no-such-remote" || exit 2
  expect_the_clone_unchanged FAIL remote-of-the-tag-gone 'tag=remote-could-not-be-asked: git ls-remote could not ask origin for refs/tags/v1, so the tag'"'"'s message was not read; run the job again'
  fixture plain-message-of-the-pushed-tag
  add_commit plain-message-of-the-pushed-tag 'more plain text' 'second'
  in_fixture plain-message-of-the-pushed-tag tag -a -m 'built in /Users/<name>' v1 || exit 2
  expect PASS plain-message-of-the-pushed-tag ' tag=read '
  clone_with_the_tag_as_its_commit plain-message-of-the-pushed-tag plain-tag-fetched-as-its-commit
  expect_the_clone_unchanged PASS plain-tag-fetched-as-its-commit ' tag=fetched-and-read '
  fixture lightweight-tag-at-the-remote
  add_commit lightweight-tag-at-the-remote "see /$u/zoe/x" 'second'
  add_commit lightweight-tag-at-the-remote 'plain again' 'third'
  in_fixture lightweight-tag-at-the-remote tag v1 || exit 2
  git clone -q --no-local "$scratch/lightweight-tag-at-the-remote" "$scratch/lightweight-tag-in-the-checkout" || exit 2
  expect_the_clone_unchanged PASS lightweight-tag-in-the-checkout ' tag=remote-holds-a-commit '
  clone_with_the_tag_as_its_commit plain-message-of-the-pushed-tag tag-fetched-in-a-clone-with-a-tags-refspec
  git -C "$scratch/tag-fetched-in-a-clone-with-a-tags-refspec" config --add remote.origin.fetch '+refs/tags/*:refs/tags/*' || exit 2
  expect_the_clone_unchanged PASS tag-fetched-in-a-clone-with-a-tags-refspec ' tag=fetched-and-read '

  fixture remote-whose-tags-move
  add_commit remote-whose-tags-move 'more plain text' 'second'
  in_fixture remote-whose-tags-move tag -a -m 'built in /Users/<name>' v1 || exit 2
  in_fixture remote-whose-tags-move tag v2 || exit 2
  clone_with_the_tag_as_its_commit remote-whose-tags-move tag-object-moved-at-the-remote
  git clone -q --no-local "$scratch/remote-whose-tags-move" "$scratch/commit-tag-moved-at-the-remote" || exit 2
  add_commit remote-whose-tags-move 'later text' 'third'
  in_fixture remote-whose-tags-move tag -f -a -m 'built again in /Users/<name>' v1 >/dev/null || exit 2
  in_fixture remote-whose-tags-move tag -f v2 >/dev/null || exit 2
  expect_the_clone_unchanged FAIL tag-object-moved-at-the-remote 'tag=remote-holds-another-commit: origin'
  GITHUB_REF=refs/tags/v2
  expect_the_clone_unchanged FAIL commit-tag-moved-at-the-remote 'tag=remote-holds-another-commit: origin'
  GITHUB_REF=refs/tags/v1

  fixture remote-whose-tag-moves-over-a-home-path
  add_commit remote-whose-tag-moves-over-a-home-path "see /$u/zoe/x" 'second'
  in_fixture remote-whose-tag-moves-over-a-home-path tag -a -m 'built in /Users/<name>' v1 || exit 2
  clone_with_the_tag_as_its_commit remote-whose-tag-moves-over-a-home-path tag-moved-over-a-home-path-in-the-head
  add_commit remote-whose-tag-moves-over-a-home-path 'plain again' 'third'
  in_fixture remote-whose-tag-moves-over-a-home-path tag -f -a -m 'built again in /Users/<name>' v1 >/dev/null || exit 2
  expect_the_clone_unchanged FAIL tag-moved-over-a-home-path-in-the-head ' tag=remote-holds-another-commit commits=1 lines=1 '

  fixture remote-without-the-tag
  add_commit remote-without-the-tag 'more plain text' 'second'
  in_fixture remote-without-the-tag tag -a -m 'built in /Users/<name>' v1 || exit 2
  clone_with_the_tag_as_its_commit remote-without-the-tag tag-gone-from-the-remote
  in_fixture remote-without-the-tag tag -d v1 >/dev/null || exit 2
  expect_the_clone_unchanged FAIL tag-gone-from-the-remote 'tag=remote-holds-no-such-ref: origin holds no refs/tags/v1 any more'

  fixture remote-that-cannot-serve-its-tag
  add_commit remote-that-cannot-serve-its-tag 'more plain text' 'second'
  git clone -q --no-local "$scratch/remote-that-cannot-serve-its-tag" "$scratch/tag-that-the-remote-cannot-serve" || exit 2
  git -C "$scratch/tag-that-the-remote-cannot-serve" update-ref refs/tags/v1 HEAD || exit 2
  printf '1111111111111111111111111111111111111111\n' >"$scratch/remote-that-cannot-serve-its-tag/.git/refs/tags/v1" || exit 2
  expect_the_clone_unchanged FAIL tag-that-the-remote-cannot-serve 'tag=remote-could-not-be-asked: origin lists refs/tags/v1, and git fetch could not fetch it'

  clone_with_the_tag_as_its_commit plain-message-of-the-pushed-tag term-between-the-fetch-and-the-scan
  term_case=$scratch/term-between-the-fetch-and-the-scan
  scanner_that_waits >"$term_case/scripts/check-home-paths.sh" || exit 2
  SCAN_STARTED=$scratch/term-scan-started
  SCAN_GO=$scratch/term-scan-go
  export SCAN_STARTED SCAN_GO
  (scan_head "$term_case") >"$scratch/term-between-the-fetch-and-the-scan.out" 2>&1 &
  term_pid=$!
  waited=0
  while [ ! -f "$SCAN_STARTED" ] && [ "$waited" -lt 120 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  kill -TERM "$term_pid"
  : >"$SCAN_GO"
  wait "$term_pid"
  term_code=$?
  if [ ! -f "$SCAN_STARTED" ]; then
    got=scan-never-started
  elif git -C "$term_case" rev-parse -q --verify "$own_tag_ref" >/dev/null 2>&1; then
    got=own-ref-left
  elif [ "$term_code" = 0 ]; then
    got=exit-0
  else
    got=own-ref-gone
  fi
  printf 'case %s/term-between-the-fetch-and-the-scan want=own-ref-gone got=%s\n' "$name" "$got"
  [ "$got" = own-ref-gone ] || wrong="$wrong term-between-the-fetch-and-the-scan"
  unset GITHUB_REF
  clone_with_the_tag_as_its_commit plain-message-of-the-pushed-tag run-that-no-tag-started
  git -C "$scratch/run-that-no-tag-started" remote set-url origin "$scratch/no-such-remote" || exit 2
  expect_the_clone_unchanged PASS run-that-no-tag-started ' tag=not-a-tag-run '

  fixture stale-entry-in-the-list
  printf 'gone.txt\t/%s/zoe\tmade up, for a file that is gone\n' "$u" >>"$scratch/stale-entry-in-the-list/scripts/home-path-allow.txt"
  add_commit stale-entry-in-the-list 'more plain text' 'second'
  expect FAIL stale-entry-in-the-list

  fixture listed-line-and-its-entry
  printf 'notes.txt\t/%s/zoe\tmade up\n' "$u" >>"$scratch/listed-line-and-its-entry/scripts/home-path-allow.txt"
  add_commit listed-line-and-its-entry "see /$u/zoe/x" 'second'
  expect PASS listed-line-and-its-entry

  fixture exempt-commit-named-by-a-later-commit
  add_commit exempt-commit-named-by-a-later-commit "see /$u/zoe/x" 'second'
  earlier=$(git -C "$scratch/exempt-commit-named-by-a-later-commit" rev-parse HEAD) || exit 2
  first=$(git -C "$scratch/exempt-commit-named-by-a-later-commit" rev-parse HEAD^) || exit 2
  sed "s/^NEWEST_EXEMPT_COMMIT=.*/NEWEST_EXEMPT_COMMIT=$earlier/" "$repo/scripts/check-commit-messages.sh" >"$scratch/exempt-commit-named-by-a-later-commit/scripts/check-commit-messages.sh" || exit 2
  add_commit exempt-commit-named-by-a-later-commit 'plain again' 'name the commit below as the newest exempt one'
  git -C "$scratch/exempt-commit-named-by-a-later-commit" update-ref refs/remotes/origin/main "$first" || exit 2
  expect FAIL exempt-commit-named-by-a-later-commit ' commits=2 lines=1 '

  fixture scan-that-prints-no-count
  add_commit scan-that-prints-no-count 'more plain text' 'second'
  printf '#!/bin/sh\nexit 0\n' >"$scratch/scan-that-prints-no-count/scripts/check-home-paths.sh" || exit 2
  expect FAIL scan-that-prints-no-count 'printed no count of the commits and lines that it read'
  fixture stale-check-that-prints-no-count
  add_commit stale-check-that-prints-no-count 'more plain text' 'second'
  scanner_without_a_stale_count >"$scratch/stale-check-that-prints-no-count/scripts/check-home-paths.sh" || exit 2
  expect FAIL stale-check-that-prints-no-count 'printed no count of the entries that it read'
  fixture range-function-that-lists-nothing
  add_commit range-function-that-lists-nothing 'more plain text' 'second'
  printf 'commit_range() { return 0; }\n' >>"$scratch/range-function-that-lists-nothing/scripts/commit-range.sh" || exit 2
  expect FAIL range-function-that-lists-nothing 'read 0 commits, and git lists 1 for that range'

  fixture list-malformed-in-the-commit
  printf 'x\tnot a path\treason\n' >>"$scratch/list-malformed-in-the-commit/scripts/home-path-allow.txt"
  add_commit list-malformed-in-the-commit 'more plain text' 'second'
  expect FAIL list-malformed-in-the-commit

  for one_case in $new_cases; do
    run_new_case "$one_case"
  done
  self_test_end "$((rejected + predicate))"
}

case ${1:-} in
  --self-test) self_test ;;
  --self-test-case)
    shift
    self_test_cases "$@"
    ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
