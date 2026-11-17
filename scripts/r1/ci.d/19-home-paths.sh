#!/bin/sh
# jobs: macos windows
# rule: scripts/check-home-paths.sh passes its self-test and finds no added line with a home path that scripts/home-path-allow.txt does not allow, and no message with a home path, in the commits of the head that the default branch does not hold, or, on a push to the default branch in a clone that is not shallow, in the commits of the push as the event file that GITHUB_EVENT_PATH names gives its before commit (range=the-push, before..HEAD; range=every-commit-of-the-head when that commit is all zeros or not in the clone; FAIL when the variable names no file or the before commit is no object id of 40 or 64 lower-case hexadecimal characters), or else in the head commit when the default branch holds it or the checkout does not, and in the message of the tag of a run that a pushed tag started, beside the head's commits (tag=read from the checkout; when the checkout's ref names a commit, tag=fetched-and-read from origin under refs/ci-d/19-home-paths/tag-of-the-run, that ref deleted again, or tag=remote-holds-a-commit; FAIL with tag=remote-holds-another-commit when the tag moved, tag=remote-holds-no-such-ref when origin no longer holds it, and tag=remote-could-not-be-asked when the ask or the fetch fails or has not answered within 120 s, when the check ends the git process that it started for it, by its pid; tag=not-a-tag-run otherwise); beside the copy of this branch, the copies of scripts/check-home-paths.sh and scripts/commit-range.sh, with the exempt commit of scripts/check-commit-messages.sh, that the default branch holds, taken from the commit that refs/remotes/origin/<default branch> names by its object id (the default branch of the event file that GITHUB_EVENT_PATH names, or, when the event holds no repository, the branch of GITHUB_REF, else main), and on a push to the default branch from the before commit of the push, which a shallow clone fetches by its id within 120 s and FAILs without (copy-of-the-default-branch not-run when the push made the branch, or when a full clone does not hold that commit), scan the same range and tag with the list of the scanned commit, and the run FAILs when that copy finds a path, prints no count or reads 0 commits where git lists one, when the commit holds no such scanner or range file, and in a full clone of a workflow run without that ref (copy-of-the-default-branch not-run in a shallow clone without it, as the windows job's pull request runs are, and outside a workflow run); when the head commit's scripts/r1/ci.d/19-home-paths.sh differs from the default commit's by object id, that copy of the whole check runs from a new directory, after the default copies of the scanner and before this branch's scanner, with an own tag ref of another name, and its FAIL fails the run; the ranges are commit ids, git runs with replace objects off, the default copies are read and run before this branch's scanner, and the default branch's copy of the whole check runs this branch's scanner itself, as its own, after its own default copies have run, and the run FAILs when the clone holds a replace ref or a grafts file, before this branch's scanner ran or after it, and when the default ref or HEAD moved while that scanner ran; named limits: 19-home-paths, the driver and the workflow run as the pull request holds them (so a pull request that changes the lines of 19 that start the default copies passes, and so does a run in which the workflow or the driver sets HOME_PATHS_CHECK_OF_THE_DEFAULT_BRANCH), this branch's code of the step before (its self-tests) runs in the same clone first and can leave a process behind or write the repository before the default copy runs, and this branch's 19 runs as the work tree holds it, which this branch's scanner can write while 19 runs (a copy of the default branch's in a clone of its own, or in a step before any code of the branch, is a unit of its own), a default commit that holds no scripts/r1/ci.d/19-home-paths.sh skips the default branch's copy of the check with a line that says why, and a change that weakens the scanner or the range file without adding a home path passes both copies and then is the default branch's copy, so the review of the diffs of those files is the guard; and FAIL when the scan or --stale prints no count or the scan reads 0 commits where git lists one, and the list holds no entry that allows nothing in HEAD (sh scripts/check-home-paths.sh --stale); it does not refuse an entry of the list for the account that runs it, as the pre-push hook does, because the account of a hosted runner is no person's.
# lane: free
set -u
GIT_NO_REPLACE_OBJECTS=1
export GIT_NO_REPLACE_OBJECTS
name=19-home-paths
own_tag_ref=refs/ci-d/19-home-paths/tag-of-the-run
[ -z "${HOME_PATHS_CHECK_OF_THE_DEFAULT_BRANCH:-}" ] || own_tag_ref=refs/ci-d/19-home-paths/tag-of-the-run-of-the-default-check
remote_limit_s=120

fail() {
  drop_own_tag_ref
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

refuse_replaced_objects() {
  replace_refs=$(git for-each-ref --count=1 --format='%(refname)' refs/replace/ 2>/dev/null) || replace_refs=
  grafts=$(git rev-parse --git-path info/grafts 2>/dev/null) || grafts=
  if [ -n "$replace_refs" ] || { [ -n "$grafts" ] && [ -e "$grafts" ]; }; then
    fail "this clone holds a replace ref (refs/replace/) or a grafts file (info/grafts), which can show git another commit or file in place of the scanned one or the default branch's; $1: delete each replace ref (git replace -d <object>, for each that git replace -l lists) and the file that git rev-parse --git-path info/grafts names, and run the check again"
  fi
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

stop_the_remote_call() {
  [ -z "${remote_pid:-}" ] || kill -TERM "$remote_pid" 2>/dev/null
  return 0
}

call_the_remote() {
  remote_call_timed_out=
  "$@" &
  remote_pid=$!
  remote_waited=0
  while kill -0 "$remote_pid" 2>/dev/null; do
    if [ "$remote_waited" -ge "$remote_limit_s" ]; then
      remote_call_timed_out=yes
      stop_the_remote_call
      break
    fi
    sleep 1
    remote_waited=$((remote_waited + 1))
  done
  wait "$remote_pid"
  remote_status=$?
  remote_pid=
  return "$remote_status"
}

no_answer_within_the_limit() {
  printf "git %s did not answer within %s s, so the check ended it, the one process that it had started for it, by its pid, and the tag's message was not read; run the job again, or on a clone of this head fetch the tag whole and scan it: git fetch origin +%s:%s && sh scripts/check-home-paths.sh 'HEAD^!' %s" "$1" "$remote_limit_s" "$GITHUB_REF" "$GITHUB_REF" "$GITHUB_REF"
}

tag_from_the_remote() {
  trap 'stop_the_remote_call; drop_own_tag_ref; exit 2' INT TERM HUP
  if ! call_the_remote git ls-remote origin "$GITHUB_REF" >"$scratch/ls-remote" 2>"$scratch/ls-remote-error" </dev/null; then
    tag=remote-could-not-be-asked
    if [ -n "$remote_call_timed_out" ]; then
      tag_cause=$(no_answer_within_the_limit ls-remote) || tag_cause='git ls-remote did not answer'
    else
      tag_cause="git ls-remote could not ask origin for $GITHUB_REF, so the tag's message was not read; run the job again"
    fi
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
  fetch_done=0
  call_the_remote git -c maintenance.auto=false -c gc.auto=0 fetch -q --no-tags --no-write-fetch-head --refmap= origin "+$GITHUB_REF:$own_tag_ref" >"$scratch/fetch" 2>&1 </dev/null && fetch_done=1
  type=$(git cat-file -t "$own_tag_ref" 2>/dev/null) || type=
  fetched_commit=$(git rev-parse -q --verify "$own_tag_ref^{commit}" 2>/dev/null) || fetched_commit=
  case $fetch_done:$type in
    1:tag | 1:commit) ;;
    *)
      drop_own_tag_ref
      tag=remote-could-not-be-asked
      if [ -n "$remote_call_timed_out" ]; then
        tag_cause=$(no_answer_within_the_limit fetch) || tag_cause='git fetch did not answer'
      else
        tag_cause="origin lists $GITHUB_REF, and git fetch could not fetch it, so the tag's message was not read; run the job again"
      fi
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
  event = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8").replace(/^\uFEFF/, ""));
} catch (error) {
  process.exit(3);
}
const field = (object, key) => (object !== null && typeof object === "object" && typeof object[key] === "string" && !/[\r\n]/.test(object[key]) ? object[key] : "");
const repository = event !== null && typeof event === "object" ? event.repository : null;
const held = repository !== null && typeof repository === "object" ? "repository" : "no-repository";
process.stdout.write(field(repository, "default_branch") + "\n" + field(event, "before") + "\n" + held + "\n");
'

read_the_event() {
  default_branch=main
  event_before=
  [ -n "${GITHUB_EVENT_PATH:-}" ] || return 0
  command -v node >/dev/null 2>&1 || fail 'node is not on PATH, and the check reads the event file that GITHUB_EVENT_PATH names with it, to learn the default branch and the commits of a push; set Node.js up before this step'
  node -e "$event_program" "$GITHUB_EVENT_PATH" >"$scratch/event" 2>/dev/null </dev/null || fail 'the event file that GITHUB_EVENT_PATH names cannot be read as JSON, so the check cannot tell the default branch or the commits of a push; run the job again'
  default_branch=$(sed -n 1p "$scratch/event") || default_branch=
  event_before=$(sed -n 2p "$scratch/event") || event_before=
  repository_held=$(sed -n 3p "$scratch/event") || repository_held=
  if [ "$repository_held" = no-repository ]; then
    default_branch=main
    case ${GITHUB_REF:-} in
      refs/heads/?*)
        if git check-ref-format "$GITHUB_REF"; then
          default_branch=${GITHUB_REF#refs/heads/}
        fi
        ;;
    esac
  fi
  if [ -z "$default_branch" ] || ! git check-ref-format "refs/heads/$default_branch"; then
    fail 'the event file that GITHUB_EVENT_PATH names gives no default branch that git reads as a branch name (repository.default_branch); a rerun reads the same file, so mend the reading in scripts/r1/ci.d/19-home-paths.sh for this event: it takes the name from repository.default_branch, from the branch of GITHUB_REF when the event holds no repository, and else main'
  fi
}

before_is_an_object_id() {
  case $event_before in
    '' | *[!0123456789abcdef]*) return 1 ;;
  esac
  case ${#event_before} in
    40 | 64) return 0 ;;
  esac
  return 1
}

fetch_the_before_commit() {
  trap 'stop_the_remote_call; exit 2' INT TERM HUP
  if call_the_remote git -c maintenance.auto=false -c gc.auto=0 fetch -q --depth 1 --no-tags --no-write-fetch-head --refmap= origin "$event_before" >"$scratch/before-fetch" 2>&1 </dev/null && has_commit "$event_before"; then
    trap 'exit 2' INT TERM HUP
    return 0
  fi
  trap 'exit 2' INT TERM HUP
  if [ -n "$remote_call_timed_out" ]; then
    fail "the before commit of this push to $default_branch is not in this shallow clone, and git fetch did not answer within $remote_limit_s s when it fetched it by its id, so the check ended it, the one process that it had started for it, by its pid; run the job again"
  fi
  fail "the before commit of this push to $default_branch is not in this shallow clone, and git fetch could not fetch it by its id, so the copy of the default branch cannot scan beside the copy of this branch; run the job again, and when the push rewrote the default branch so that origin no longer holds that commit, the macos job's full clone is the one that reads the push"
}

the_commit_of_the_default_branch() {
  default_commit=
  default_note=
  default_label=$default_ref
  if [ -n "$push_to_the_default_branch" ]; then
    default_label='the commit before the push'
    before_is_an_object_id || fail "the event file that GITHUB_EVENT_PATH names gives no before commit of 40 or 64 lower-case hexadecimal characters for this push to $default_branch, so the check cannot take the copy of the commit before the push; run the job again"
    case $event_before in
      *[!0]*) ;;
      *)
        default_note='not-run (the push made the branch, so no commit was before it)'
        return 0
        ;;
    esac
    if ! has_commit "$event_before"; then
      if [ "$shallow" != true ]; then
        default_note='not-run (the before commit of the push is not in this clone)'
        return 0
      fi
      fetch_the_before_commit
      printf 'before-commit=fetched-by-its-id\n'
    fi
    default_commit=$event_before
  elif ! default_commit=$(git rev-parse -q --verify "$default_ref^{commit}" 2>/dev/null); then
    default_commit=
    if [ "$shallow" = true ]; then
      default_note="not-run (no $default_ref in this shallow clone)"
      return 0
    fi
    [ -z "${GITHUB_EVENT_PATH:-}" ] || fail "$default_ref is not in this clone, so the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that the default branch holds cannot scan beside the copy of this branch; the checkout step needs fetch-depth: 0"
    default_note="not-run (no $default_ref in this clone)"
  fi
}

run_the_check_of_the_default_branch() {
  if [ -n "${HOME_PATHS_CHECK_OF_THE_DEFAULT_BRANCH:-}" ]; then
    printf 'check-of-the-default-branch not-run (this run is that check)\n'
    return 0
  fi
  [ -n "$default_commit" ] || return 0
  default_check=$(git rev-parse -q --verify "$default_commit:scripts/r1/ci.d/19-home-paths.sh" 2>/dev/null) || default_check=
  if [ -z "$default_check" ]; then
    printf 'check-of-the-default-branch not-run (the copy of the check that the default branch holds did not run: %s holds no scripts/r1/ci.d/19-home-paths.sh, after a rename there or in a commit older than the check)\n' "$default_label"
    return 0
  fi
  head_check=$(git rev-parse -q --verify "$head_commit:scripts/r1/ci.d/19-home-paths.sh" 2>/dev/null) || head_check=
  if [ "$default_check" = "$head_check" ]; then
    printf 'check-of-the-default-branch same\n'
    return 0
  fi
  check_place=$(mktemp -d "$scratch/check-of-the-default-branch.XXXXXX") || fail 'mktemp failed'
  git cat-file blob "$default_check" >"$check_place/19-home-paths.sh" 2>/dev/null </dev/null || fail "git cannot read the copy of scripts/r1/ci.d/19-home-paths.sh that $default_label holds"
  mkdir "$check_place/tmp" || fail 'mkdir failed'
  HOME_PATHS_CHECK_OF_THE_DEFAULT_BRANCH=running CI_D_REPO=$repo TMPDIR=$check_place/tmp sh "$check_place/19-home-paths.sh" >"$check_place/out" 2>&1 </dev/null
  check_code=$?
  check_last=$(sed -n '$p' "$check_place/out") || check_last=
  case $check_code:$check_last in
    "0:PASS $name")
      printf 'check-of-the-default-branch PASS\n'
      ;;
    *)
      printf 'check-of-the-default-branch exit=%s\n' "$check_code"
      fail "the copy of scripts/r1/ci.d/19-home-paths.sh that $default_label holds refuses this head, and this branch changed that check: its last line was: $check_last"
      ;;
  esac
}

take_the_copy_of_the_default_branch() {
  default_copy=
  [ -n "$default_commit" ] || return 0
  copy_place=$(mktemp -d "$scratch/default.XXXXXX") || fail 'mktemp failed'
  mkdir "$copy_place/scripts" || fail 'mkdir failed'
  for file in check-home-paths.sh commit-range.sh; do
    if ! git cat-file blob "$default_commit:scripts/$file" >"$copy_place/scripts/$file" 2>/dev/null </dev/null; then
      if [ -z "${GITHUB_EVENT_PATH:-}" ]; then
        fail "$default_label holds no scripts/$file; outside a workflow run $default_ref is what this clone last fetched, and it may be older than the file: fetch the default branch (git fetch origin) and run the check again"
      fi
      fail "$default_label holds no scripts/$file; 19-home-paths reads the scanner and scripts/commit-range.sh of the default branch at these names, so after a rename or a removal there, change the names that scripts/r1/ci.d/19-home-paths.sh reads to the default branch's in this pull request, or restore the file on the default branch"
    fi
  done
  git cat-file blob "$default_commit:scripts/check-commit-messages.sh" >"$copy_place/scripts/check-commit-messages.sh" 2>/dev/null </dev/null ||
    : >"$copy_place/scripts/check-commit-messages.sh"
  default_copy=$copy_place/scripts/check-home-paths.sh
}

range_of_the_push() {
  [ -n "${GITHUB_EVENT_PATH:-}" ] || fail "this run is a push to $default_branch, and GITHUB_EVENT_PATH names no event file, so the check cannot read which commits the push added; run it in the workflow, which sets the variable"
  before_is_an_object_id || fail "the event file that GITHUB_EVENT_PATH names gives no before commit of 40 or 64 lower-case hexadecimal characters for this push to $default_branch, so the check cannot read which commits the push added; run the job again"
  if has_commit "$event_before"; then
    kind=the-push
    range=$event_before..$head_commit
  else
    kind=every-commit-of-the-head
    range=$head_commit
  fi
}

counts_of() {
  sed -n 's/.*commits read \([0-9]*\), lines with a home path \([0-9]*\)\.$/commits=\1 lines=\2/p' "$@"
}

scan_head() {
  repo=$1
  cd "$repo" || fail "cannot enter $repo"
  [ -f scripts/check-home-paths.sh ] || fail 'scripts/check-home-paths.sh is missing'
  head_commit=$(git rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null) || fail 'HEAD names no commit'
  refuse_replaced_objects 'a clone of a workflow run holds none'
  read_the_event
  default_ref=refs/remotes/origin/$default_branch
  shallow=$(git rev-parse --is-shallow-repository 2>/dev/null) || shallow=unknown
  push_to_the_default_branch=
  if [ "${GITHUB_EVENT_NAME:-}" = push ] && [ "${GITHUB_REF:-}" = "refs/heads/$default_branch" ]; then
    push_to_the_default_branch=yes
  fi
  default_ref_commit=$(git rev-parse -q --verify "$default_ref^{commit}" 2>/dev/null) || default_ref_commit=
  if [ -n "$push_to_the_default_branch" ] && [ "$shallow" != true ]; then
    range_of_the_push
  elif [ -n "$default_ref_commit" ] && ! git merge-base --is-ancestor "$head_commit" "$default_ref_commit"; then
    kind=since-the-default-branch
    range=$default_ref_commit..$head_commit
  elif has_commit "$head_commit^1"; then
    kind=head-commit
    range=$head_commit^!
  else
    kind=root-commit
    range=$head_commit
  fi
  listed=$(git rev-list --count "$range" 2>/dev/null) || listed=0
  the_commit_of_the_default_branch
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
  take_the_copy_of_the_default_branch
  default_code=
  if [ -n "$default_copy" ]; then
    sh "$default_copy" "$range" ${tag_to_read:+"$tag_to_read"} >"$scratch/default-out" 2>"$scratch/default-err" </dev/null
    default_code=$?
  fi
  run_the_check_of_the_default_branch
  sh scripts/check-home-paths.sh "$range" ${tag_to_read:+"$tag_to_read"} >"$scratch/out" 2>"$scratch/err" </dev/null
  code=$?
  ref_after=$(git rev-parse -q --verify "$default_ref^{commit}" 2>/dev/null) || ref_after=
  head_after=$(git rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null) || head_after=
  if [ "$ref_after" != "$default_ref_commit" ] || [ "$head_after" != "$head_commit" ]; then
    fail "$default_ref or HEAD moved while the scanner of this branch ran, so nothing that it printed counts; a scanner that writes refs is no scanner: read the diff of scripts/check-home-paths.sh"
  fi
  refuse_replaced_objects 'the scanner of this branch wrote it, and a scanner that writes the repository is no scanner: read the diff of scripts/check-home-paths.sh'
  drop_own_tag_ref
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
    way_on="on a clone of this head, run the scanner of a checkout of $default_label: sh <that checkout>/scripts/check-home-paths.sh '$range'${tag_to_read:+ $GITHUB_REF}"
    case $default_code in
      0)
        [ -n "$default_counts" ] || fail "the copy of scripts/check-home-paths.sh that $default_label holds exited 0 and printed no count of the commits and lines that it read, so nothing shows what it read; $way_on, and read its last line"
        case $default_counts in
          commits=0\ *) [ "$listed" = 0 ] || fail "the copy of scripts/check-home-paths.sh that $default_label holds read 0 commits, and git lists $listed for that range, so it did not read the head; $way_on" ;;
        esac
        ;;
      1) fail "the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that $default_label holds finds an added line or a message with a home path that scripts/home-path-allow.txt does not allow, and the copy of this branch finds none: this branch changed the scanner or its range file, or the default branch holds an older scanner that refuses a line which the newer one accepts; the commits must pass both copies, so take each line that it lists out of the commit that added it, and undo a change that weakens the scan; when this branch moves scripts/home-path-allow.txt or changes the form of its lines, the older copy reads the list at its old name and in its old form: land first a scanner that reads both, in a pull request that the older copy passes, and the move after it; $way_on" ;;
      *) fail "the copy of scripts/check-home-paths.sh that $default_label holds could not check the range (exit $default_code); $way_on, and read its error" ;;
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
  git init -q -b main "$dir" || exit 2
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
  clone_unchanged_by expect "$@"
}

expect_within_the_case_limit() {
  (scan_head "$scratch/$2") >"$scratch/$2.out" 2>&1 &
  case_pid=$!
  case_waited=0
  while kill -0 "$case_pid" 2>/dev/null && [ "$case_waited" -lt "$case_limit_s" ]; do
    sleep 1
    case_waited=$((case_waited + 1))
  done
  if kill -0 "$case_pid" 2>/dev/null; then
    : >"$STAND_IN_DIR/go" || exit 2
    kill -TERM "$case_pid" 2>/dev/null
    wait "$case_pid"
    code=$?
    printf 'the case gave the check %s s, and with no verdict by then it let the remote that never answers end and sent TERM to the one process that it had started itself, by its pid\n' "$case_limit_s" >>"$scratch/$2.out"
  else
    wait "$case_pid"
    code=$?
  fi
  judge "$@"
}

clone_unchanged_by() {
  runner=$1
  shift
  state_of_the_clone "$2" >"$scratch/$2.before"
  "$runner" "$@"
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
  c='branch-scanner-that-prints-counts-and-finds-nothing'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/trunk HEAD || exit 2
  add_commit "$c" "see /$u/zoe/x" 'second'
  weak_scanner >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" 'plain again' 'weaken the scanner'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false trunk
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that refs/remotes/origin/trunk holds finds'
  no_event
}

case_branch_range_file_that_leaves_out_the_commit_with_the_path() {
  c='branch-range-file-that-leaves-out-the-commit-with-the-path'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  add_commit "$c" "see /$u/zoe/x" 'second'
  printf 'commit_range() { git rev-list -n 1 HEAD; }\n' >>"$scratch/$c/scripts/commit-range.sh" || exit 2
  add_commit "$c" 'plain again' 'list the head commit alone'
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that refs/remotes/origin/main holds finds'
}

case_list_entry_of_the_branch_holds_for_both_copies() {
  c='list-entry-of-the-branch-holds-for-both-copies'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  printf 'notes.txt\t/%s/zoe\tmade up\n' "$u" >>"$scratch/$c/scripts/home-path-allow.txt" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'second'
  expect PASS "$c" 'copy-of-the-default-branch commits=1 lines=0 exit=0'
}

case_default_branch_missing_from_a_full_clone_of_a_workflow_run() {
  c='default-branch-missing-from-a-full-clone-of-a-workflow-run'
  fixture "$c"
  add_commit "$c" 'more plain text' 'second'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'refs/remotes/origin/main is not in this clone'
  no_event
}

case_default_branch_without_the_range_file() {
  c='default-branch-without-the-range-file'
  fixture "$c"
  mv "$scratch/$c/scripts/commit-range.sh" "$scratch/$c.commit-range.sh" || exit 2
  add_commit "$c" 'more plain text' 'take the range file out'
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  mv "$scratch/$c.commit-range.sh" "$scratch/$c/scripts/commit-range.sh" || exit 2
  add_commit "$c" 'plain again' 'put the range file back'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'refs/remotes/origin/main holds no scripts/commit-range.sh; 19-home-paths reads the scanner and scripts/commit-range.sh of the default branch at these names, so after a rename or a removal there, change the names that scripts/r1/ci.d/19-home-paths.sh reads'
  no_event
}

case_clone_outside_ci_whose_default_branch_has_no_scanner() {
  c='clone-outside-ci-whose-default-branch-has-no-scanner'
  fixture "$c"
  mv "$scratch/$c/scripts/check-home-paths.sh" "$scratch/$c.check-home-paths.sh" || exit 2
  add_commit "$c" 'more plain text' 'a commit from before the scanner'
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  mv "$scratch/$c.check-home-paths.sh" "$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" 'plain again' 'the scanner'
  expect FAIL "$c" 'outside a workflow run refs/remotes/origin/main is what this clone last fetched, and it may be older than the file: fetch the default branch (git fetch origin) and run the check again'
}

case_pull_request_that_moves_the_allow_list() {
  c='pull-request-that-moves-the-allow-list'
  fixture "$c"
  printf 'notes.txt\t/%s/zoe\tmade up\n' "$u" >>"$scratch/$c/scripts/home-path-allow.txt" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'allow the made-up path'
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  in_fixture "$c" mv scripts/home-path-allow.txt scripts/home-paths-allowed.txt || exit 2
  sed 's#^LIST_IN_THE_REPOSITORY=scripts/home-path-allow.txt$#LIST_IN_THE_REPOSITORY=scripts/home-paths-allowed.txt#' "$scratch/$c/scripts/check-home-paths.sh" >"$scratch/$c.scanner" || exit 2
  grep -q '^LIST_IN_THE_REPOSITORY=scripts/home-paths-allowed.txt$' "$scratch/$c.scanner" || exit 2
  cat "$scratch/$c.scanner" >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" 'plain again' 'move the list'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'when this branch moves scripts/home-path-allow.txt or changes the form of its lines, the older copy reads the list at its old name and in its old form: land first a scanner that reads both, in a pull request that the older copy passes, and the move after it'
  no_event
}

case_shallow_clone_without_the_default_branch() {
  c='shallow-clone-without-the-default-branch'
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

case_event_without_a_default_branch() {
  c='event-without-a-default-branch'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  add_commit "$c" 'more plain text' 'second'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false ''
  expect FAIL "$c" 'gives no default branch that git reads as a branch name (repository.default_branch); a rerun reads the same file'
  no_event
}

case_scheduled_run_whose_event_holds_no_repository() {
  c='scheduled-run-whose-event-holds-no-repository'
  fixture "$c"
  add_commit "$c" 'more plain text' 'second'
  git -C "$scratch/$c" update-ref refs/remotes/origin/trunk HEAD || exit 2
  event_of_a_run "$c" schedule refs/heads/trunk '' false trunk
  printf '{"schedule": "17 3 * * 0"}\n' >"$GITHUB_EVENT_PATH" || exit 2
  expect PASS "$c" 'copy-of-the-default-branch commits=1 lines=0 exit=0'
  no_event
}

case_event_without_a_repository_on_a_ref_that_is_no_branch() {
  c='event-without-a-repository-on-a-ref-that-is-no-branch'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  add_commit "$c" 'more plain text' 'second'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  printf '{"number": 1}\n' >"$GITHUB_EVENT_PATH" || exit 2
  expect PASS "$c" 'copy-of-the-default-branch commits=1 lines=0 exit=0'
  no_event
}

default_branch_whose_copy_differs() {
  fixture "$1"
  mv "$scratch/$1/scripts/$2" "$scratch/$1.kept" || exit 2
  printf '%s\n' "$3" >"$scratch/$1/scripts/$2" || exit 2
  add_commit "$1" 'more plain text' 'change the copy of the default branch'
  git -C "$scratch/$1" update-ref refs/remotes/origin/main HEAD || exit 2
  mv "$scratch/$1.kept" "$scratch/$1/scripts/$2" || exit 2
  add_commit "$1" 'plain again' 'take the copy of the repository again'
}

case_default_branch_scanner_that_prints_no_count() {
  c='default-branch-scanner-that-prints-no-count'
  default_branch_whose_copy_differs "$c" check-home-paths.sh 'exit 0'
  expect FAIL "$c" 'that refs/remotes/origin/main holds exited 0 and printed no count'
}

case_default_branch_range_file_that_lists_nothing() {
  c='default-branch-range-file-that-lists-nothing'
  default_branch_whose_copy_differs "$c" commit-range.sh 'commit_range_default_ref() { :; }
commit_range() { return 0; }'
  expect FAIL "$c" 'that refs/remotes/origin/main holds read 0 commits, and git lists 1 for that range'
}

case_default_branch_scanner_that_cannot_check() {
  c='default-branch-scanner-that-cannot-check'
  default_branch_whose_copy_differs "$c" check-home-paths.sh 'exit 2'
  expect FAIL "$c" 'that refs/remotes/origin/main holds could not check the range (exit 2)'
}

case_push_to_the_default_branch_in_a_shallow_clone() {
  c='push-to-the-default-branch-in-a-shallow-clone'
  push_of_three_commits "$c-source" 'more plain text'
  git clone -q --no-local --depth 2 "$scratch/$c-source" "$scratch/$c" || exit 2
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  event_of_a_run "$c" push refs/heads/main "$below" false main
  expect PASS "$c" 'before-commit=fetched-by-its-id'
  no_event
}

case_shallow_push_whose_before_cannot_be_fetched() {
  c='shallow-push-whose-before-cannot-be-fetched'
  push_of_three_commits "$c-source" 'more plain text'
  git clone -q --no-local --depth 2 "$scratch/$c-source" "$scratch/$c" || exit 2
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  event_of_a_run "$c" push refs/heads/main abababababababababababababababababababab false main
  expect FAIL "$c" 'is not in this shallow clone, and git fetch could not fetch it by its id'
  no_event
}

scanner_that_moves_the_default_ref() {
  cat <<'EOF'
#!/bin/sh
case ${1:-} in
  --stale)
    printf 'check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n'
    exit 0
    ;;
esac
git update-ref refs/remotes/origin/main HEAD
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

scanner_that_writes_over_the_default_copy() {
  cat <<'EOF'
#!/bin/sh
case ${1:-} in
  --stale)
    printf 'check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n'
    exit 0
    ;;
esac
for target in "$TMPDIR"/19-home-paths.*/default/scripts/check-home-paths.sh "$TMPDIR"/19-home-paths.*/default.*/scripts/check-home-paths.sh; do
  if [ -f "$target" ]; then cp "$0" "$target"; fi
done
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

case_branch_scanner_that_moves_the_default_ref() {
  c='branch-scanner-that-moves-the-default-ref'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  scanner_that_moves_the_default_ref >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'a scanner that moves the default ref'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'refs/remotes/origin/main or HEAD moved while the scanner of this branch ran'
  no_event
}

case_branch_scanner_that_writes_over_the_default_copy() {
  c='branch-scanner-that-writes-over-the-default-copy'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  scanner_that_writes_over_the_default_copy >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'a scanner that writes over the default copy'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that refs/remotes/origin/main holds finds'
  no_event
}

scanner_that_swaps_the_check_for_a_while() {
  cat <<'EOF'
#!/bin/sh
case ${1:-} in
  --stale)
    printf 'check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n'
    exit 0
    ;;
esac
git show 'refs/remotes/origin/main:scripts/r1/ci.d/19-home-paths.sh' >../swap.main 2>/dev/null &&
  git show 'HEAD:scripts/r1/ci.d/19-home-paths.sh' >../swap.branch 2>/dev/null &&
  cat ../swap.main >scripts/r1/ci.d/19-home-paths.sh
( sleep 3; cat ../swap.branch >scripts/r1/ci.d/19-home-paths.sh; : >../swap.restored ) </dev/null >/dev/null 2>&1 &
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

pull_request_that_edits_a_verdict_of_the_check() {
  fixture "$1"
  mkdir -p "$scratch/$1/scripts/r1/ci.d" || exit 2
  sed 's/kill -TERM /: /' "$repo/scripts/r1/ci.d/19-home-paths.sh" >"$scratch/$1/scripts/r1/ci.d/19-home-paths.sh" || exit 2
  add_commit "$1" 'more plain text' 'the check'
  git -C "$scratch/$1" update-ref refs/remotes/origin/main HEAD || exit 2
  sed 's/^      1) fail "the copy of scripts\/check-home-paths.sh and scripts\/commit-range.sh that/      1) : "the copy of scripts\/check-home-paths.sh and scripts\/commit-range.sh that/' "$scratch/$1/scripts/r1/ci.d/19-home-paths.sh" >"$scratch/$1.check" || exit 2
  if cmp -s "$scratch/$1.check" "$scratch/$1/scripts/r1/ci.d/19-home-paths.sh"; then
    printf 'case %s/%s: the line of the check that the case changes is not in it\n' "$name" "$1"
    exit 2
  fi
  cat "$scratch/$1.check" >"$scratch/$1/scripts/r1/ci.d/19-home-paths.sh" || exit 2
  "$2" >"$scratch/$1/scripts/check-home-paths.sh" || exit 2
  add_commit "$1" "see /$u/zoe/x" 'change a verdict of the check and the scanner'
  mkdir "$scratch/$1.tmp" || exit 2
  event_of_a_run "$1" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  (cd "$scratch/$1" && env GITHUB_EVENT_NAME="$GITHUB_EVENT_NAME" GITHUB_REF="$GITHUB_REF" GITHUB_EVENT_PATH="$GITHUB_EVENT_PATH" CI_D_REPO="$scratch/$1" TMPDIR="$scratch/$1.tmp" sh scripts/r1/ci.d/19-home-paths.sh) >"$scratch/$1.out" 2>&1 </dev/null
  code=$?
  no_event
}

case_pull_request_that_edits_a_verdict_of_the_check() {
  c='pull-request-that-edits-a-verdict-of-the-check'
  pull_request_that_edits_a_verdict_of_the_check "$c" weak_scanner
  judge FAIL "$c" 'the copy of scripts/r1/ci.d/19-home-paths.sh that refs/remotes/origin/main holds refuses this head'
}

case_scanner_that_swaps_the_check_for_a_while() {
  c='scanner-that-swaps-the-check-for-a-while'
  pull_request_that_edits_a_verdict_of_the_check "$c" scanner_that_swaps_the_check_for_a_while
  swap_wait=0
  while [ -f "$scratch/swap.main" ] && [ ! -f "$scratch/swap.restored" ] && [ "$swap_wait" -lt 20 ]; do
    sleep 1
    swap_wait=$((swap_wait + 1))
  done
  judge FAIL "$c" 'the copy of scripts/r1/ci.d/19-home-paths.sh that refs/remotes/origin/main holds refuses this head'
}

case_pull_request_that_edits_the_check_itself() {
  c='pull-request-that-edits-the-check-itself'
  fixture "$c"
  mkdir -p "$scratch/$c/scripts/r1/ci.d" || exit 2
  cp "$repo/scripts/r1/ci.d/19-home-paths.sh" "$scratch/$c/scripts/r1/ci.d/19-home-paths.sh" || exit 2
  add_commit "$c" 'more plain text' 'the check'
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  weak_scanner >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  sed 's/^  take_the_copy_of_the_default_branch$/  default_copy= default_note=not-run/' "$repo/scripts/r1/ci.d/19-home-paths.sh" >"$scratch/$c/scripts/r1/ci.d/19-home-paths.sh" || exit 2
  if cmp -s "$repo/scripts/r1/ci.d/19-home-paths.sh" "$scratch/$c/scripts/r1/ci.d/19-home-paths.sh"; then
    printf 'case %s/%s: the line of the check that the case changes is not in it\n' "$name" "$c"
    exit 2
  fi
  add_commit "$c" "see /$u/zoe/x" 'weaken the scanner and the check'
  mkdir "$scratch/$c.tmp" || exit 2
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  (cd "$scratch/$c" && env GITHUB_EVENT_NAME="$GITHUB_EVENT_NAME" GITHUB_REF="$GITHUB_REF" GITHUB_EVENT_PATH="$GITHUB_EVENT_PATH" CI_D_REPO="$scratch/$c" TMPDIR="$scratch/$c.tmp" sh scripts/r1/ci.d/19-home-paths.sh) >"$scratch/$c.out" 2>&1 </dev/null
  code=$?
  judge FAIL "$c" 'the copy of scripts/r1/ci.d/19-home-paths.sh that refs/remotes/origin/main holds refuses this head'
  no_event
}

scanner_that_plants_a_replace_ref() {
  cat <<'EOF'
#!/bin/sh
blob=$(git rev-parse -q --verify 'refs/remotes/origin/main:scripts/check-home-paths.sh' 2>/dev/null)
if [ -n "$blob" ] && ! git cat-file -e "refs/replace/$blob" 2>/dev/null; then
  weak=$(printf '%s\n' '#!/bin/sh' 'case ${1:-} in --stale) printf "check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n"; exit 0 ;; esac' 'printf "check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n"' | git hash-object -w --stdin)
  git replace -f "$blob" "$weak" 2>/dev/null
fi
case ${1:-} in
  --stale)
    printf 'check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n'
    exit 0
    ;;
esac
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

scanner_that_leaves_a_writer_behind() {
  cat <<'EOF'
#!/bin/sh
case ${1:-} in
  --stale)
    printf 'check-home-paths: PASS: --stale: stand-in: entries read 0, stale 0.\n'
    exit 0
    ;;
esac
parent=$(cd .. && pwd)
(
  n=0
  while [ "$n" -lt 4000 ]; do
    for f in "$parent"/default.*/scripts/check-home-paths.sh; do
      if [ -s "$f" ] && ! grep -q 'written-over' "$f"; then
        printf '%s\n' '#!/bin/sh' '# written-over' 'case ${1:-} in --stale) exit 0 ;; esac' 'printf "check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n"' >"$f"
        exit 0
      fi
    done
    sleep 0.01
    n=$((n + 1))
  done
) </dev/null >/dev/null 2>&1 &
printf 'check-home-paths: PASS: stand-in: commits read 1, lines with a home path 0.\n'
EOF
}

case_branch_scanner_that_plants_a_replace_ref() {
  c='branch-scanner-that-plants-a-replace-ref'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  scanner_that_plants_a_replace_ref >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'a scanner that plants a replace ref, and a path'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'this clone holds a replace ref (refs/replace/) or a grafts file (info/grafts)'
  no_event
}

case_clone_that_holds_a_replace_ref_of_the_scanner() {
  c='clone-that-holds-a-replace-ref-of-the-scanner'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  weak_scanner >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'weaken the scanner and add a path'
  blob=$(git -C "$scratch/$c" rev-parse 'refs/remotes/origin/main:scripts/check-home-paths.sh') || exit 2
  weak=$(git -C "$scratch/$c" hash-object -w scripts/check-home-paths.sh) || exit 2
  git -C "$scratch/$c" replace -f "$blob" "$weak" || exit 2
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'this clone holds a replace ref (refs/replace/) or a grafts file (info/grafts)'
  no_event
}

case_clone_that_holds_a_grafts_file() {
  c='clone-that-holds-a-grafts-file'
  fixture "$c"
  base_of_the_branch=$(git -C "$scratch/$c" rev-parse HEAD) || exit 2
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  add_commit "$c" "see /$u/zoe/x" 'second'
  add_commit "$c" 'plain again' 'third'
  newest=$(git -C "$scratch/$c" rev-parse HEAD) || exit 2
  grafts=$(git -C "$scratch/$c" rev-parse --git-path info/grafts) || exit 2
  printf '%s %s\n' "$newest" "$base_of_the_branch" >"$scratch/$c/$grafts" || exit 2
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'this clone holds a replace ref (refs/replace/) or a grafts file (info/grafts)'
  no_event
}

case_branch_scanner_that_leaves_a_writer_behind() {
  c='branch-scanner-that-leaves-a-writer-behind'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  scanner_that_leaves_a_writer_behind >"$scratch/$c/scripts/check-home-paths.sh" || exit 2
  add_commit "$c" "see /$u/zoe/x" 'a scanner that leaves a writer behind, and a path'
  event_of_a_run "$c" pull_request refs/pull/1/merge 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that refs/remotes/origin/main holds finds'
  no_event
}

record_the_stop() {
  stopped_started=$(ps -o lstart= -p "${remote_pid:-0}" 2>/dev/null) || stopped_started=
  printf '%s %s\n' "${remote_pid:-none}" "${stopped_started:-start-time-unread}" >>"$stop_record"
}

release_the_stand_in() {
  : >"$STAND_IN_DIR/go" || exit 2
  if [ -f "$STAND_IN_DIR/asked" ]; then
    released_wait=0
    while [ ! -f "$STAND_IN_DIR/ended" ] && [ "$released_wait" -lt 20 ]; do
      sleep 1
      released_wait=$((released_wait + 1))
    done
  fi
}

case_term_during_the_fetch_of_the_before_commit() {
  c='term-during-the-fetch-of-the-before-commit'
  push_of_three_commits "$c-source" 'more plain text'
  git clone -q --no-local --depth 2 "$scratch/$c-source" "$scratch/$c" || exit 2
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  mkdir "$scratch/$c.stand-in" "$scratch/$c.tmp" || exit 2
  upload_pack_that_waits >"$scratch/$c.upload-pack.sh" || exit 2
  git -C "$scratch/$c" config remote.origin.uploadpack "sh $scratch/$c.upload-pack.sh" || exit 2
  STAND_IN_DIR=$scratch/$c.stand-in
  STAND_IN_ANSWERS_FIRST=no
  export STAND_IN_DIR STAND_IN_ANSWERS_FIRST
  stop_record=$scratch/$c.stops
  : >"$stop_record" || exit 2
  event_of_a_run "$c" push refs/heads/main "$below" false main
  fixture_path=$scratch/$c
  (
    # shellcheck disable=SC2329
    stop_the_remote_call() {
      record_the_stop
    }
    TMPDIR=$scratch/$c.tmp
    make_scratch
    scan_head "$fixture_path"
  ) >"$scratch/$c.out" 2>&1 &
  fetch_case_pid=$!
  waited=0
  while [ ! -f "$STAND_IN_DIR/asked" ] && [ "$waited" -lt 60 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  kill -TERM "$fetch_case_pid" 2>/dev/null
  wait "$fetch_case_pid"
  fetch_case_code=$?
  release_the_stand_in
  no_event
  left=$(find "$scratch/$c.tmp" -mindepth 1 | wc -l | tr -d ' ') || left=unread
  stops=$(grep -c '^[0-9][0-9]* ' "$stop_record") || stops=0
  if [ ! -f "$STAND_IN_DIR/asked" ]; then
    got=fetch-never-started
  elif [ "$stops" != 1 ]; then
    got=stops-recorded-$stops
  elif [ "$fetch_case_code" != 2 ]; then
    got=exit-$fetch_case_code
  elif [ "$left" != 0 ]; then
    got=temporary-entries-left-$left
  else
    got=the-fetch-stopped-by-its-pid
  fi
  printf 'case %s/%s want=the-fetch-stopped-by-its-pid got=%s (the case sent TERM to the one process that it started, by its pid; the check'"'"'s stop of git fetch was recorded, not sent)\n' "$name" "$c" "$got"
  if [ "$got" = the-fetch-stopped-by-its-pid ]; then
    rejected=$((rejected + 1))
  else
    wrong="$wrong $c"
  fi
}

case_shallow_push_whose_commit_weakens_the_scanner() {
  c='shallow-push-whose-commit-weakens-the-scanner'
  fixture "$c-source"
  add_commit "$c-source" 'more plain text' 'second'
  below=$(git -C "$scratch/$c-source" rev-parse HEAD) || exit 2
  weak_scanner >"$scratch/$c-source/scripts/check-home-paths.sh" || exit 2
  add_commit "$c-source" "see /$u/zoe/x" 'weaken the scanner and add a path'
  git clone -q --no-local --depth 2 "$scratch/$c-source" "$scratch/$c" || exit 2
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  event_of_a_run "$c" push refs/heads/main "$below" false main
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that the commit before the push holds finds'
  no_event
}

case_scheduled_run_on_the_default_branch() {
  c='scheduled-run-on-the-default-branch'
  fixture "$c"
  add_commit "$c" 'more plain text' 'second'
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  event_of_a_run "$c" schedule refs/heads/main '' false main
  expect PASS "$c" 'copy-of-the-default-branch commits=1 lines=0 exit=0'
  no_event
}

push_whose_last_commit_changes() {
  fixture "$1"
  below=$(git -C "$scratch/$1" rev-parse HEAD) || exit 2
  add_commit "$1" "see /$u/zoe/x" 'second'
  first_of_the_push=$(git -C "$scratch/$1" rev-parse HEAD) || exit 2
  "$2" "$1"
  add_commit "$1" 'plain again' 'change a script of the check'
  git -C "$scratch/$1" update-ref refs/remotes/origin/main HEAD || exit 2
  event_of_a_run "$1" push refs/heads/main "$below" false main
}

weaken_the_scanner() {
  weak_scanner >"$scratch/$1/scripts/check-home-paths.sh" || exit 2
}

name_the_first_commit_exempt() {
  printf 'NEWEST_EXEMPT_COMMIT=%s\n' "$first_of_the_push" >"$scratch/$1/scripts/check-commit-messages.sh" || exit 2
}

case_push_whose_last_commit_weakens_the_scanner() {
  c='push-whose-last-commit-weakens-the-scanner'
  push_whose_last_commit_changes "$c" weaken_the_scanner
  expect FAIL "$c" 'the copy of scripts/check-home-paths.sh and scripts/commit-range.sh that the commit before the push holds finds'
  no_event
}

case_push_whose_last_commit_names_the_first_exempt() {
  c='push-whose-last-commit-names-the-first-exempt'
  push_whose_last_commit_changes "$c" name_the_first_commit_exempt
  expect FAIL "$c" 'copy-of-the-default-branch commits=2 lines=1 exit=1'
  no_event
}

case_event_whose_default_branch_holds_a_line_break() {
  c='event-whose-default-branch-holds-a-line-break'
  push_of_three_commits "$c" "see /$u/zoe/x"
  head_of_the_push=$(git -C "$scratch/$c" rev-parse HEAD) || exit 2
  printf '{"ref": "refs/heads/main", "repository": {"default_branch": "main\\n%s"}}\n' "$head_of_the_push" >"$scratch/$c.event.json" || exit 2
  GITHUB_EVENT_NAME=push
  GITHUB_REF=refs/heads/main
  GITHUB_EVENT_PATH=$scratch/$c.event.json
  expect FAIL "$c" 'gives no default branch that git reads as a branch name'
  no_event
}

case_event_that_starts_with_a_byte_order_mark() {
  c='event-that-starts-with-a-byte-order-mark'
  push_of_three_commits "$c" 'more plain text'
  event_of_a_run "$c" push refs/heads/main "$below" false main
  mv "$GITHUB_EVENT_PATH" "$scratch/$c.event-without-the-mark.json" || exit 2
  printf '\357\273\277' >"$GITHUB_EVENT_PATH" || exit 2
  cat "$scratch/$c.event-without-the-mark.json" >>"$GITHUB_EVENT_PATH" || exit 2
  expect PASS "$c" 'range=the-push tag=not-a-tag-run commits=3 lines=0 '
  no_event
}

case_push_to_another_branch() {
  c='push-to-another-branch'
  fixture "$c"
  git -C "$scratch/$c" update-ref refs/remotes/origin/main HEAD || exit 2
  add_commit "$c" 'more plain text' 'second'
  below=$(git -C "$scratch/$c" rev-parse HEAD) || exit 2
  add_commit "$c" 'plain again' 'third'
  event_of_a_run "$c" push refs/heads/feature "$below" false main
  expect PASS "$c" 'copy-of-the-default-branch commits=2 lines=0 exit=0'
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
  c='push-of-three-commits-to-the-default-branch'
  push_of_three_commits "$c" "see /$u/zoe/x"
  event_of_a_run "$c" push refs/heads/main "$below" false main
  expect FAIL "$c" 'range=the-push tag=not-a-tag-run commits=3 lines=1 '
  no_event
}

case_clean_push_of_three_commits_to_the_default_branch() {
  c='clean-push-of-three-commits-to-the-default-branch'
  push_of_three_commits "$c" 'more plain text'
  event_of_a_run "$c" push refs/heads/main "$below" false main
  expect PASS "$c" 'range=the-push tag=not-a-tag-run commits=3 lines=0 '
  no_event
}

case_push_that_creates_the_default_branch() {
  c='push-that-creates-the-default-branch'
  push_of_three_commits "$c" "see /$u/zoe/x"
  event_of_a_run "$c" push refs/heads/main 0000000000000000000000000000000000000000 false main
  expect FAIL "$c" 'range=every-commit-of-the-head tag=not-a-tag-run commits=4 lines=1 '
  no_event
}

case_forced_push_whose_old_tip_is_not_in_the_clone() {
  c='forced-push-whose-old-tip-is-not-in-the-clone'
  push_of_three_commits "$c" "see /$u/zoe/x"
  event_of_a_run "$c" push refs/heads/main abababababababababababababababababababab true main
  expect FAIL "$c" 'range=every-commit-of-the-head tag=not-a-tag-run commits=4 lines=1 '
  no_event
}

case_push_to_the_default_branch_with_an_event_that_is_no_json() {
  c='push-to-the-default-branch-with-an-event-that-is-no-json'
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
  c='push-whose-before-holds-upper-case-letters'
  push_of_three_commits "$c" 'more plain text'
  event_of_a_run "$c" push refs/heads/main ABABABABABABABABABABABABABABABABABABABAB false main
  expect FAIL "$c" 'gives no before commit of 40 or 64 lower-case hexadecimal characters'
  no_event
}

case_push_whose_before_is_39_characters_long() {
  c='push-whose-before-is-39-characters-long'
  push_of_three_commits "$c" 'more plain text'
  short_before=$(printf '%.39s' "$below") || exit 2
  event_of_a_run "$c" push refs/heads/main "$short_before" false main
  expect FAIL "$c" 'gives no before commit of 40 or 64 lower-case hexadecimal characters'
  no_event
}

case_push_to_the_default_branch_without_an_event_file() {
  c='push-to-the-default-branch-without-an-event-file'
  push_of_three_commits "$c" 'more plain text'
  GITHUB_EVENT_NAME=push
  GITHUB_REF=refs/heads/main
  expect FAIL "$c" 'GITHUB_EVENT_PATH names no event file'
  no_event
}

upload_pack_that_waits() {
  cat <<'EOF'
#!/bin/sh
if [ "$STAND_IN_ANSWERS_FIRST" = yes ] && mkdir "$STAND_IN_DIR/answered" 2>/dev/null; then
  exec git upload-pack "$@"
fi
: >"$STAND_IN_DIR/asked"
while [ ! -f "$STAND_IN_DIR/go" ]; do sleep 1; done
: >"$STAND_IN_DIR/ended"
EOF
}

remote_that_never_answers() {
  fixture "$1-source"
  add_commit "$1-source" 'more plain text' 'second'
  in_fixture "$1-source" tag -a -m 'built in /Users/<name>' v1 || exit 2
  clone_with_the_tag_as_its_commit "$1-source" "$1"
  mkdir "$scratch/$1.stand-in" || exit 2
  upload_pack_that_waits >"$scratch/$1.upload-pack.sh" || exit 2
  git -C "$scratch/$1" config remote.origin.uploadpack "sh $scratch/$1.upload-pack.sh" || exit 2
  STAND_IN_DIR=$scratch/$1.stand-in
  STAND_IN_ANSWERS_FIRST=$2
  export STAND_IN_DIR STAND_IN_ANSWERS_FIRST
  GITHUB_REF=refs/tags/v1
  limit_of_the_run=$remote_limit_s
  remote_limit_s=2
  case_limit_s=60
}

after_the_remote_that_never_answers() {
  release_the_stand_in
  remote_limit_s=$limit_of_the_run
  no_event
}

case_tag_ask_that_never_answers() {
  c='tag-ask-that-never-answers'
  remote_that_never_answers "$c" no
  clone_unchanged_by expect_within_the_case_limit FAIL "$c" 'tag=remote-could-not-be-asked: git ls-remote did not answer within 2 s'
  after_the_remote_that_never_answers
}

case_tag_fetch_that_never_answers() {
  c='tag-fetch-that-never-answers'
  remote_that_never_answers "$c" yes
  clone_unchanged_by expect_within_the_case_limit FAIL "$c" 'tag=remote-could-not-be-asked: git fetch did not answer within 2 s'
  after_the_remote_that_never_answers
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
tag-ask-that-never-answers
tag-fetch-that-never-answers
event-without-a-default-branch
default-branch-scanner-that-prints-no-count
default-branch-range-file-that-lists-nothing
default-branch-scanner-that-cannot-check
push-to-the-default-branch-in-a-shallow-clone
scheduled-run-on-the-default-branch
push-to-another-branch
push-whose-last-commit-weakens-the-scanner
push-whose-last-commit-names-the-first-exempt
event-whose-default-branch-holds-a-line-break
event-that-starts-with-a-byte-order-mark
shallow-push-whose-commit-weakens-the-scanner
scheduled-run-whose-event-holds-no-repository
event-without-a-repository-on-a-ref-that-is-no-branch
pull-request-that-moves-the-allow-list
clone-outside-ci-whose-default-branch-has-no-scanner
shallow-push-whose-before-cannot-be-fetched
branch-scanner-that-moves-the-default-ref
branch-scanner-that-writes-over-the-default-copy
pull-request-that-edits-the-check-itself
branch-scanner-that-plants-a-replace-ref
clone-that-holds-a-replace-ref-of-the-scanner
clone-that-holds-a-grafts-file
branch-scanner-that-leaves-a-writer-behind
term-during-the-fetch-of-the-before-commit
pull-request-that-edits-a-verdict-of-the-check
scanner-that-swaps-the-check-for-a-while
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

  git init -q -b main "$scratch/home-path-in-the-root-commit" || exit 2
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
