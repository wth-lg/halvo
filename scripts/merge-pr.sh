#!/bin/sh
set -eu

script_directory=$(cd "$(dirname "$0")" && pwd)
CHECK=$script_directory/check-commit-messages.sh

THREADS_QUERY="query(\$owner: String!, \$repo: String!, \$number: Int!, \$endCursor: String) { repository(owner: \$owner, name: \$repo) { pullRequest(number: \$number) { reviewThreads(first: 100, after: \$endCursor) { pageInfo { hasNextPage endCursor } nodes { isResolved } } } } }"

nl='
'
tab=$(printf '\t')

help_text() {
  cat <<'EOF'
The one supported way to merge a pull request in this repository. It squashes
the pull request into one commit whose message it checks first, so that no
attribution line reaches the default branch.

  sh scripts/merge-pr.sh <number> --head <full sha> --subject <text> --body-file <file> [--dry-run]
      Read pull request <number> with gh and refuse unless all of these hold:
        - it is OPEN, it is not a draft, and GitHub reports it MERGEABLE
        - its head is the commit given with --head, the one that was reviewed
        - check runs named macos and windows exist for that head, and every
          check run on it completed with success
        - it has 0 unresolved review threads
        - the squash message (the subject, an empty line, the body file) passes
          sh scripts/check-commit-messages.sh --stored-message
        - the title and the description of the pull request pass it too
      Then merge with
        gh pr merge <number> --squash --match-head-commit <sha> --subject <text> --body-file -
      which hands gh, on standard input, the body that was checked. Then fetch,
      print the merge commit with its author and committer, and run the range
      check on that commit.
      --dry-run prints every fact and stops before the merge. It exits 0 only
      when every gate holds.
  sh scripts/merge-pr.sh --self-test
      Prove each refusal and the accepted case with a stub gh first on PATH, in
      a throwaway clone under a new temporary directory that is left in place.
      Needs a clone of this repository, and a name and an email for commits:
      a configured git user, or the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL,
      GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.

The script never passes --admin, never pushes and never edits a pull request.
It cannot see a merge that someone makes on the website or with gh pr merge
directly. The push run of CI reports such a merge after the fact, and only a
ruleset on the default branch can stop it.

Exit codes: 0 merged and the merge commit passed the range check, or with
--dry-run every gate holds; 1 a gate refuses and nothing was merged; 2 the
script could not run; 3 the pull request is merged, and its merge commit failed
the range check or could not be checked; 64 bad usage.
EOF
}

say() {
  printf 'merge-pr: %s\n' "$1"
}

die() {
  printf 'merge-pr: error: %s\n' "$1" >&2
  exit 2
}

usage() {
  printf 'merge-pr: %s\n' "$1" >&2
  printf 'Usage: sh scripts/merge-pr.sh <number> --head <full sha> --subject <text> --body-file <file> [--dry-run]\n       sh scripts/merge-pr.sh --help\n' >&2
  exit 64
}

refuse() {
  printf 'merge-pr: REFUSED: %s\n' "$1" >&2
  refusals=$((refusals + 1))
}

count_lines() {
  printf '%s\n' "$1" | grep -c "$2" || true
}

read_facts() {
  fields=$(gh pr view "$number" --json state,isDraft,mergeable,headRefOid,baseRefName --jq '.state, .isDraft, .mergeable, .headRefOid, .baseRefName') ||
    die "gh could not read pull request #$number. Its message is above."
  {
    read -r state || :
    read -r is_draft || :
    read -r mergeable || :
    read -r head_on_github || :
    read -r base || :
  } <<EOF
$fields
EOF
  [ -n "$base" ] || die "gh printed fewer than 5 fields for pull request #$number: $fields"
  title=$(gh pr view "$number" --json title --jq .title) || die "gh could not read the title of pull request #$number"
  description=$(gh pr view "$number" --json body --jq .body) || die "gh could not read the description of pull request #$number"
  check_runs=$(gh api "repos/{owner}/{repo}/commits/$head/check-runs?per_page=100" --paginate --jq '.check_runs[] | "\(.name)\t\(.status)\t\(.conclusion // "none")"') ||
    die "gh could not read the check runs of $head"
  thread_states=$(gh api graphql --paginate -F owner='{owner}' -F repo='{repo}' -F number="$number" -f query="$THREADS_QUERY" --jq '.data.repository.pullRequest.reviewThreads.nodes[].isResolved') ||
    die "gh could not read the review threads of pull request #$number"
  if [ -n "$thread_states" ]; then
    unread_states=$(printf '%s\n' "$thread_states" | awk '$0 != "true" && $0 != "false" { printf "%sline %d: %s", separator, NR, $0; separator = ", " }') ||
      die 'awk could not read the states of the review threads'
    [ -z "$unread_states" ] ||
      die "gh printed a review thread state that is neither true nor false for pull request #$number ($unread_states). This script counts a thread as resolved only for true and as unresolved only for false, so it stops here."
  fi
  thread_total=$(count_lines "$thread_states" .)
  thread_unresolved=$(count_lines "$thread_states" '^false$')
}

print_facts() {
  say "pull request #$number: state $state, draft $is_draft, mergeable $mergeable, base $base"
  say "head on GitHub: $head_on_github"
  say "head reviewed:  $head"
  say "title: $title"
  say "description: $(count_lines "$description" '') lines"
  say "check runs on $head (name, status, conclusion): $(count_lines "$check_runs" .)"
  printf '%s\n' "$check_runs" | sed -n '/./s/^/merge-pr:   /p'
  say "review threads: $thread_total, unresolved: $thread_unresolved"
  say "squash subject: $subject"
  say "squash body: $body_file, $(count_lines "$body" '') lines"
}

apply_gates() {
  refusals=0
  if [ "$state" != OPEN ]; then
    refuse "pull request #$number is $state, and only an OPEN pull request can be merged. If the work is not on $base yet, open a new pull request for it."
  else
    case $mergeable in
      MERGEABLE) ;;
      CONFLICTING) refuse "GitHub reports that pull request #$number conflicts with $base. Merge origin/$base into the branch in a clone, push, and wait for the checks of the new head." ;;
      *) refuse "GitHub reports mergeable=$mergeable for pull request #$number: it has not computed the merge yet. Run this script again in a minute." ;;
    esac
  fi
  [ "$is_draft" = false ] ||
    refuse "pull request #$number is a draft. Mark it ready when its review is done: gh pr ready $number"
  [ "$head_on_github" = "$head" ] ||
    refuse "the head of pull request #$number is $head_on_github, and --head names $head. A push arrived after the review. Review and verify the new head, then pass it as --head."

  for required in macos windows; do
    printf '%s\n' "$check_runs" | grep -q "^$required$tab" ||
      refuse "no check run named $required exists for $head. CI has not started for this commit, or a job was renamed. Wait for the run: gh pr checks $number --watch"
  done
  not_green=$(printf '%s\n' "$check_runs" | awk -F '\t' 'NF && !($2 == "completed" && $3 == "success")')
  [ -z "$not_green" ] ||
    refuse "these check runs on $head did not complete with success (name, status, conclusion):$nl$not_green${nl}Wait for a run that is in progress: gh pr checks $number --watch. Fix a run that failed and push; the new head needs a new review."

  [ "$thread_unresolved" = 0 ] ||
    refuse "pull request #$number has unresolved review threads: $thread_unresolved of $thread_total. Answer each one and resolve it on GitHub, then run this script again."

  if ! printf '%s\n\n%s\n' "$subject" "$body" | sh "$CHECK" --stored-message; then
    refuse "the squash message carries an attribution line, listed above. Remove it from --subject or from $body_file."
  fi
  if ! printf '%s\n\n%s\n' "$title" "$description" | sh "$CHECK" --stored-message; then
    refuse "the title or the description of pull request #$number carries an attribution line, listed above. This script does not edit a pull request. Remove the line on GitHub (gh pr edit $number --body-file <file>), then run this script again."
  fi
}

merged_but() {
  printf "merge-pr: error: pull request #%s is merged, but %s, so its merge commit was not checked. Run: git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/%s 'refs/remotes/origin/%s^!'\\n" "$number" "$1" "$base" "$base" >&2 || :
  exit 3
}

merge() {
  printf '%s\n' "$body" | gh pr merge "$number" --squash --match-head-commit "$head" --subject "$subject" --body-file - ||
    die "gh pr merge failed. Its message is above. Read it, and do not retry with --admin. gh pr view $number --json state shows whether GitHub merged it."
  trap '' PIPE
  say "pull request #$number is merged" || :
  merge_commit=$(gh pr view "$number" --json mergeCommit --jq .mergeCommit.oid) ||
    merged_but 'gh could not read its merge commit'
  [ -n "$merge_commit" ] || merged_but 'GitHub reports no merge commit for it yet'
  git fetch -q origin "$base" || merged_but "git fetch origin $base failed"
  git cat-file -e "$merge_commit^{commit}" 2>/dev/null ||
    merged_but "the merge commit $merge_commit is not in this clone after git fetch origin $base"
  git log -1 --format='merge-pr: merged as %H%nmerge-pr: author:    %an <%ae>%nmerge-pr: committer: %cn <%ce>%nmerge-pr: subject:   %s' "$merge_commit" || :
  range_check_status=0
  range_check_output=$(sh "$CHECK" --default-ref "refs/remotes/origin/$base" "$merge_commit^!") || range_check_status=$?
  printf '%s\n' "$range_check_output" || :
  [ "$range_check_status" = 0 ] || exit 3
}

run() {
  case $number in
    ''|*[!0-9]*) usage "the pull request number must be digits, and got: $number" ;;
  esac
  head=$(printf '%s' "$head" | LC_ALL=C tr 'ABCDEF' 'abcdef')
  case $head in
    *[!0-9a-f]*) usage "--head needs the full sha of the reviewed commit, 40 hex digits, and got: $head" ;;
  esac
  [ ${#head} = 40 ] || usage "--head needs the full sha of the reviewed commit, 40 hex digits, and got: $head"
  case $subject in
    *[![:space:]]*) ;;
    *) usage '--subject needs the subject line of the squash commit' ;;
  esac
  case $subject in
    *"$nl"*) usage '--subject must be one line; the rest of the message goes into --body-file' ;;
  esac
  [ -n "$body_file" ] || usage '--body-file needs the file that holds the body of the squash message'
  [ -f "$body_file" ] || usage "--body-file: no such file: $body_file"
  body=$(cat <"$body_file") || die "cannot read the file of --body-file: $body_file"
  command -v gh >/dev/null 2>&1 || die 'gh is not on PATH. Install the GitHub CLI and run: gh auth login'
  git rev-parse --git-dir >/dev/null 2>&1 || die 'not inside a git repository'

  read_facts
  print_facts
  apply_gates
  if [ "$refusals" -gt 0 ]; then
    printf 'merge-pr: gates that refuse the merge of pull request #%s: %d. Nothing was merged.\n' "$number" "$refusals" >&2
    exit 1
  fi
  if [ "$dry_run" = 1 ]; then
    say "dry run: every gate holds. Without --dry-run this script runs: gh pr merge $number --squash --match-head-commit $head --subject <subject> --body-file -, with the text of $body_file on standard input"
    exit 0
  fi
  merge
}

self_test() {
  root=$(cd "$script_directory/.." && pwd)
  git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || die 'the self-test clones this repository, and this script is not inside one'
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 ||
    die 'the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.'
  work=$(mktemp -d "${TMPDIR:-/tmp}/merge-pr-self-test.XXXXXX") || die 'mktemp could not make a directory'
  say "self-test in $work, which is left in place"
  mkdir "$work/bin" "$work/state" "$work/out" "$work/check-that-cannot-run"

  cat >"$work/bin/gh" <<'EOF'
#!/bin/sh
state=$MERGE_PR_STUB_STATE
case "$1 $2" in
  'pr view')
    case " $* " in
      *' --json state,isDraft,mergeable,headRefOid,baseRefName'*) cat "$state/fields" ;;
      *' --json title '*) cat "$state/title" ;;
      *' --json body '*) cat "$state/description" ;;
      *' --json mergeCommit '*) cat "$state/merge-commit" ;;
      *' --json state '*)
        if [ -e "$state/state-cannot-be-read" ]; then
          echo 'stub gh: GitHub did not answer' >&2
          exit 1
        fi
        cat "$state/state-after-the-merge-call"
        ;;
      *) echo "stub gh: unexpected pr view: $*" >&2; exit 97 ;;
    esac
    ;;
  'api graphql') cat "$state/threads" ;;
  'api repos/{owner}/{repo}/commits/'*) cat "$state"/jobs-* ;;
  'api repos/{owner}/{repo}/actions/runs?head_sha='*) cat "$state/runs" ;;
  'api repos/{owner}/{repo}/actions/runs/'*)
    run=${2#*/actions/runs/}
    cat "$state/jobs-${run%%/*}"
    ;;
  'pr merge')
    printf '%s\n' "$@" >"$state/merge-arguments"
    case " $* " in
      *' --body-file - '*) cat >"$state/merge-body" ;;
    esac
    if [ -e "$state/github-refuses-the-merge" ]; then
      echo 'stub gh: GitHub refused the merge' >&2
      exit 1
    fi
    if [ -s "$state/merge-commit" ]; then
      git --git-dir="$MERGE_PR_STUB_GITHUB" update-ref refs/heads/main "$(cat "$state/merge-commit")"
    fi
    if [ -e "$state/call-fails-after-github-merged" ]; then
      echo 'stub gh: the connection dropped after GitHub merged' >&2
      exit 1
    fi
    ;;
  *) echo "stub gh: unexpected call: $*" >&2; exit 97 ;;
esac
EOF
  chmod +x "$work/bin/gh"

  git init -q --bare "$work/github.git"
  git -C "$work/github.git" fetch -q "$root" HEAD:refs/heads/main
  git clone -q --no-local "$work/github.git" "$work/clone"
  commit() {
    git -C "$work/clone" -c commit.gpgsign=false commit -q --allow-empty --no-verify --cleanup=verbatim -m "$1"
  }
  commit 'Add the feature'
  reviewed_head=$(git -C "$work/clone" rev-parse HEAD)
  git -C "$work/clone" checkout -q --detach origin/main
  commit "Add the feature (#7)${nl}${nl}What changed and why."
  clean_squash=$(git -C "$work/clone" rev-parse HEAD)
  git -C "$work/clone" checkout -q --detach origin/main
  commit "Add the feature (#7)${nl}${nl}What changed and why.${nl}${nl}Co-authored-by: Someone Else <someone@example.invalid>"
  squash_with_a_trailer=$(git -C "$work/clone" rev-parse HEAD)
  git -C "$work/clone" push -q --no-verify origin "$clean_squash:refs/prepared/clean" "$squash_with_a_trailer:refs/prepared/trailer"
  git -C "$work/clone" checkout -q main

  subject='Add the feature (#7)'
  printf 'What changed and why.\n' >"$work/body.txt"
  printf 'What changed and why.\n\nCo-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>\n' >"$work/body-with-a-trailer.txt"
  printf 'What changed and why.\n\n# ------------------------ >8 ------------------------\ndiff --git a/x b/x\nCo-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>\n' >"$work/body-with-a-trailer-below-a-scissors-line.txt"
  other_head=1111111111111111111111111111111111111111
  green_jobs="macos${tab}completed${tab}success${nl}windows${tab}completed${tab}success"
  run_line() {
    printf '%s\t%s\t%s\t%s\t%s\t1\t.github/workflows/ci.yml\n' "$1" "$2" "$3" "$4" "$5"
  }
  green_push_run=$(run_line push 2026-11-10T09:00:00Z 101 completed success)
  green_pull_request_run=$(run_line pull_request 2026-11-10T09:00:04Z 102 completed success)

  failures=0
  cases=0
  state_of() {
    state_directory=$work/state/$1
    mkdir "$state_directory"
    printf 'OPEN\nfalse\nMERGEABLE\n%s\nmain\nfalse\nnone\n' "$reviewed_head" >"$state_directory/fields"
    printf 'Add the feature\n' >"$state_directory/title"
    printf 'A plain description.\n' >"$state_directory/description"
    printf '%s\n%s\n' "$green_push_run" "$green_pull_request_run" >"$state_directory/runs"
    printf '%s\n' "$green_jobs" >"$state_directory/jobs-101"
    printf '%s\n' "$green_jobs" >"$state_directory/jobs-102"
    printf 'true\ntrue\n' >"$state_directory/threads"
    printf '%s\n' "$clean_squash" >"$state_directory/merge-commit"
    printf 'OPEN\n' >"$state_directory/state-after-the-merge-call"
  }
  fields() {
    printf '%s\n%s\n%s\n%s\nmain\n%s\n%s\n' "$2" "$3" "$4" "$5" "${6-false}" "${7-none}" >"$work/state/$1/fields"
  }
  runs() {
    name=$1
    shift
    printf '%s\n' "$@" >"$work/state/$name/runs"
  }
  jobs() {
    printf '%s\n' "$3" >"$work/state/$1/jobs-$2"
  }
  judge() {
    name=$1 status=$2 want_status=$3 want_merge=$4 want_text=$5
    if [ -e "$work/state/$name/merge-arguments" ]; then merge=called; else merge=not-called; fi
    cases=$((cases + 1))
    if [ "$status" = "$want_status" ] && [ "$merge" = "$want_merge" ] && grep -qF -- "$want_text" "$work/out/$name.txt"; then
      printf 'PASS %s (exit %s, gh pr merge %s)\n' "$name" "$status" "$merge"
    else
      printf 'FAIL %s (exit %s, gh pr merge %s; expected exit %s, %s and the text "%s"; output in %s)\n' "$name" "$status" "$merge" "$want_status" "$want_merge" "$want_text" "$work/out/$name.txt"
      failures=$((failures + 1))
    fi
  }
  expect() {
    name=$1 want_status=$2 want_merge=$3 want_text=$4
    shift 4
    status=0
    (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/$name" MERGE_PR_STUB_GITHUB="$work/github.git" \
      sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --subject "$subject" "$@") >"$work/out/$name.txt" 2>&1 || status=$?
    judge "$name" "$status" "$want_status" "$want_merge" "$want_text"
  }
  refused() {
    name=$1 want_text=$2
    shift 2
    [ $# -gt 0 ] || set -- --body-file "$work/body.txt"
    expect "$name" 1 not-called "$want_text" "$@"
  }

  state_of dry-run-with-every-gate-open
  expect dry-run-with-every-gate-open 0 not-called 'dry run: every gate holds' --body-file "$work/body.txt" --dry-run

  state_of merged-pull-request
  fields merged-pull-request MERGED false UNKNOWN "$reviewed_head" false "$clean_squash"
  refused merged-pull-request "holds its merge commit, $clean_squash"
  cases=$((cases + 1))
  if grep -qF 'This run merged nothing' "$work/out/merged-pull-request.txt" && ! grep -qF 'Nothing was merged' "$work/out/merged-pull-request.txt"; then
    printf 'PASS merged-pull-request: a line says that this run merged nothing, and no line says that nothing was merged\n'
  else
    printf 'FAIL merged-pull-request: a line says "Nothing was merged" about a pull request that GitHub merged, or no line says "This run merged nothing"; see %s\n' "$work/out/merged-pull-request.txt"
    failures=$((failures + 1))
  fi

  state_of closed-pull-request
  fields closed-pull-request CLOSED false UNKNOWN "$reviewed_head"
  refused closed-pull-request 'is CLOSED, and only an OPEN pull request can be merged'

  state_of draft
  fields draft OPEN true MERGEABLE "$reviewed_head"
  refused draft 'is a draft'

  state_of conflicting
  fields conflicting OPEN false CONFLICTING "$reviewed_head"
  refused conflicting 'conflicts with main'

  state_of mergeable-not-computed
  fields mergeable-not-computed OPEN false UNKNOWN "$reviewed_head"
  refused mergeable-not-computed 'mergeable=UNKNOWN'

  state_of head-moved-after-the-review
  fields head-moved-after-the-review OPEN false MERGEABLE "$other_head"
  refused head-moved-after-the-review 'A push arrived after the review'

  state_of no-workflow-run-at-all
  runs no-workflow-run-at-all
  refused no-workflow-run-at-all 'gates that refuse the merge of pull request #7: 2'

  state_of only-a-push-run-exists
  runs only-a-push-run-exists "$green_push_run"
  refused only-a-push-run-exists 'When none has started by then, close and reopen the pull request'

  state_of only-a-pull-request-run-exists
  runs only-a-pull-request-run-exists "$green_pull_request_run"
  refused only-a-pull-request-run-exists "no push run exists for $reviewed_head"

  state_of failed-pull-request-run-below-a-newer-green-one
  runs failed-pull-request-run-below-a-newer-green-one "$(run_line pull_request 2026-11-10T09:00:04Z 102 completed failure)" "$green_push_run" "$(run_line pull_request 2026-11-10T09:10:00Z 103 completed success)"
  jobs failed-pull-request-run-below-a-newer-green-one 102 "macos${tab}completed${tab}failure${nl}windows${tab}completed${tab}success"
  jobs failed-pull-request-run-below-a-newer-green-one 103 "$green_jobs"
  expect failed-pull-request-run-below-a-newer-green-one 0 not-called 'dry run: every gate holds' --body-file "$work/body.txt" --dry-run

  state_of newest-pull-request-run-failed-above-an-older-green-one
  runs newest-pull-request-run-failed-above-an-older-green-one "$green_pull_request_run" "$green_push_run" "$(run_line pull_request 2026-11-10T09:20:00Z 104 completed failure)"
  jobs newest-pull-request-run-failed-above-an-older-green-one 104 "macos${tab}completed${tab}failure${nl}windows${tab}completed${tab}success"
  refused newest-pull-request-run-failed-above-an-older-green-one 'a re-run reads the old text'

  state_of newest-push-run-failed
  runs newest-push-run-failed "$(run_line push 2026-11-10T09:00:00Z 101 completed failure)" "$green_pull_request_run"
  jobs newest-push-run-failed 101 "macos${tab}completed${tab}success${nl}windows${tab}completed${tab}failure"
  refused newest-push-run-failed 'run 101, completed with failure'

  state_of newest-push-run-in-progress
  runs newest-push-run-in-progress "$(run_line push 2026-11-10T09:00:00Z 101 in_progress none)" "$green_pull_request_run"
  jobs newest-push-run-in-progress 101 "macos${tab}completed${tab}success${nl}windows${tab}in_progress${tab}none"
  refused newest-push-run-in-progress 'gh run watch 101'

  state_of newest-pull-request-run-in-progress
  runs newest-pull-request-run-in-progress "$green_push_run" "$(run_line pull_request 2026-11-10T09:00:04Z 102 queued none)"
  jobs newest-pull-request-run-in-progress 102 "macos${tab}queued${tab}none${nl}windows${tab}queued${tab}none"
  refused newest-pull-request-run-in-progress 'gh run watch 102'

  state_of push-run-without-the-job-windows
  jobs push-run-without-the-job-windows 101 "macos${tab}completed${tab}success"
  refused push-run-without-the-job-windows 'no job named windows succeeded in push run 101'

  state_of pull-request-run-without-the-job-macos
  jobs pull-request-run-without-the-job-macos 102 "windows${tab}completed${tab}success"
  refused pull-request-run-without-the-job-macos 'no job named macos succeeded in pull_request run 102'

  state_of job-windows-skipped-in-a-green-run
  jobs job-windows-skipped-in-a-green-run 101 "macos${tab}completed${tab}success${nl}windows${tab}completed${tab}skipped"
  refused job-windows-skipped-in-a-green-run 'no job named windows succeeded in push run 101'

  state_of run-failed-in-a-job-of-another-name
  runs run-failed-in-a-job-of-another-name "$green_push_run" "$(run_line pull_request 2026-11-10T09:00:04Z 102 completed failure)"
  jobs run-failed-in-a-job-of-another-name 102 "$green_jobs${nl}lint${tab}completed${tab}failure"
  refused run-failed-in-a-job-of-another-name 'run 102, completed with failure'

  state_of pull-request-from-another-repository
  fields pull-request-from-another-repository OPEN false MERGEABLE "$reviewed_head" true
  runs pull-request-from-another-repository "$green_pull_request_run"
  expect pull-request-from-another-repository 0 not-called 'Here the pull_request run alone is required' --body-file "$work/body.txt" --dry-run

  state_of pull-request-from-another-repository-with-only-a-push-run
  fields pull-request-from-another-repository-with-only-a-push-run OPEN false MERGEABLE "$reviewed_head" true
  runs pull-request-from-another-repository-with-only-a-push-run "$green_push_run"
  refused pull-request-from-another-repository-with-only-a-push-run "no pull_request run exists for $reviewed_head"

  state_of unresolved-review-thread
  printf 'true\nfalse\ntrue\n' >"$work/state/unresolved-review-thread/threads"
  refused unresolved-review-thread 'has unresolved review threads: 1 of 3'

  state_of review-thread-state-that-is-neither-true-nor-false
  printf 'true\nFalse\n' >"$work/state/review-thread-state-that-is-neither-true-nor-false/threads"
  expect review-thread-state-that-is-neither-true-nor-false 2 not-called 'line 2: False' --body-file "$work/body.txt"

  state_of squash-message-with-a-trailer
  refused squash-message-with-a-trailer 'the squash message carries an attribution line' --body-file "$work/body-with-a-trailer.txt"

  state_of squash-message-with-a-trailer-below-a-scissors-line
  refused squash-message-with-a-trailer-below-a-scissors-line 'the squash message carries an attribution line' --body-file "$work/body-with-a-trailer-below-a-scissors-line.txt"

  state_of description-with-a-tool-footer
  printf 'A plain description.\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\n' >"$work/state/description-with-a-tool-footer/description"
  refused description-with-a-tool-footer 'the title or the description of pull request #7 carries an attribution line'

  state_of title-with-a-tool-trailer
  printf 'Claude-Session: https://claude.ai/code/session_0000000000000000000000\n' >"$work/state/title-with-a-tool-trailer/title"
  refused title-with-a-tool-trailer 'the title or the description of pull request #7 carries an attribution line'

  state_of draft-with-an-unresolved-thread
  fields draft-with-an-unresolved-thread OPEN true MERGEABLE "$reviewed_head"
  printf 'false\n' >"$work/state/draft-with-an-unresolved-thread/threads"
  refused draft-with-an-unresolved-thread 'gates that refuse the merge of pull request #7: 2'

  state_of dry-run-of-a-draft
  fields dry-run-of-a-draft OPEN true MERGEABLE "$reviewed_head"
  refused dry-run-of-a-draft 'is a draft' --body-file "$work/body.txt" --dry-run

  state_of merge-that-github-refuses
  : >"$work/state/merge-that-github-refuses/github-refuses-the-merge"
  expect merge-that-github-refuses 2 called 'GitHub reports pull request #7 as OPEN: it is not merged' --body-file "$work/body.txt"

  printf 'What changed and why.\n\nCo-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>\n' >"$work/clone/-body-with-a-trailer.txt"
  state_of body-file-whose-name-starts-with-a-hyphen
  expect body-file-whose-name-starts-with-a-hyphen 64 not-called 'starts with a hyphen' --body-file -body-with-a-trailer.txt

  state_of body-file-is-standard-input
  expect body-file-is-standard-input 64 not-called 'starts with a hyphen' --body-file -

  state_of subject-value-missing-before-dry-run
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/subject-value-missing-before-dry-run" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --body-file "$work/body.txt" --subject --dry-run) >"$work/out/subject-value-missing-before-dry-run.txt" 2>&1 || status=$?
  judge subject-value-missing-before-dry-run "$status" 64 not-called 'starts with a hyphen'

  state_of head-in-upper-case
  upper_case_head=$(printf '%s' "$reviewed_head" | LC_ALL=C tr 'abcdef' 'ABCDEF')
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/head-in-upper-case" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$script_directory/merge-pr.sh" 7 --head "$upper_case_head" --subject "$subject" --body-file "$work/body.txt" --dry-run) >"$work/out/head-in-upper-case.txt" 2>&1 || status=$?
  judge head-in-upper-case "$status" 0 not-called 'dry run: every gate holds'

  state_of accepted
  expect accepted 0 called "merge-pr: merged as $clean_squash" --body-file "$work/body.txt"
  expected_arguments="pr${nl}merge${nl}7${nl}--squash${nl}--match-head-commit${nl}$reviewed_head${nl}--subject${nl}$subject${nl}--body-file${nl}-"
  cases=$((cases + 1))
  if [ "$(cat "$work/state/accepted/merge-arguments")" = "$expected_arguments" ] &&
    cmp -s "$work/state/accepted/merge-body" "$work/body.txt" &&
    grep -q '^merge-pr: committer: ' "$work/out/accepted.txt" &&
    grep -qF "check-commit-messages: PASS: $clean_squash^!: commits checked 1," "$work/out/accepted.txt"; then
    printf 'PASS accepted: gh pr merge got exactly --squash, --match-head-commit, --subject and --body-file -, with the checked body on standard input; the merge commit was printed and range-checked\n'
  else
    printf 'FAIL accepted: the arguments of gh pr merge, the body it received or the report of the merge commit differ; see %s and %s\n' "$work/state/accepted" "$work/out/accepted.txt"
    failures=$((failures + 1))
  fi

  state_of merged-commit-to-which-github-added-a-trailer
  printf '%s\n' "$squash_with_a_trailer" >"$work/state/merged-commit-to-which-github-added-a-trailer/merge-commit"
  expect merged-commit-to-which-github-added-a-trailer 3 called 'an agent does not rewrite the default branch' --body-file "$work/body.txt"

  state_of merged-and-github-reports-no-merge-commit
  : >"$work/state/merged-and-github-reports-no-merge-commit/merge-commit"
  expect merged-and-github-reports-no-merge-commit 3 called 'pull request #7 is merged, but' --body-file "$work/body.txt"
  cases=$((cases + 1))
  if grep -qF "'refs/remotes/origin/main^!'" "$work/out/merged-and-github-reports-no-merge-commit.txt"; then
    printf 'PASS merged-and-github-reports-no-merge-commit: the remedy names the default branch by its full ref\n'
  else
    printf 'FAIL merged-and-github-reports-no-merge-commit: the remedy does not name refs/remotes/origin/main; see %s\n' "$work/out/merged-and-github-reports-no-merge-commit.txt"
    failures=$((failures + 1))
  fi

  state_of merge-call-fails-after-github-merged
  : >"$work/state/merge-call-fails-after-github-merged/call-fails-after-github-merged"
  printf 'MERGED\n' >"$work/state/merge-call-fails-after-github-merged/state-after-the-merge-call"
  expect merge-call-fails-after-github-merged 0 called "merge-pr: merged as $clean_squash" --body-file "$work/body.txt"

  state_of merge-call-fails-after-github-merged-a-commit-with-a-trailer
  : >"$work/state/merge-call-fails-after-github-merged-a-commit-with-a-trailer/call-fails-after-github-merged"
  printf 'MERGED\n' >"$work/state/merge-call-fails-after-github-merged-a-commit-with-a-trailer/state-after-the-merge-call"
  printf '%s\n' "$squash_with_a_trailer" >"$work/state/merge-call-fails-after-github-merged-a-commit-with-a-trailer/merge-commit"
  expect merge-call-fails-after-github-merged-a-commit-with-a-trailer 3 called 'an agent does not rewrite the default branch' --body-file "$work/body.txt"

  state_of merge-call-fails-and-the-state-cannot-be-read
  : >"$work/state/merge-call-fails-and-the-state-cannot-be-read/github-refuses-the-merge"
  : >"$work/state/merge-call-fails-and-the-state-cannot-be-read/state-cannot-be-read"
  expect merge-call-fails-and-the-state-cannot-be-read 2 called 'does not know whether pull request #7 was merged. Run: gh pr view 7 --json state,mergeCommit' --body-file "$work/body.txt"

  state_of subject-of-spaces
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/subject-of-spaces" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --subject '   ' --body-file "$work/body.txt") >"$work/out/subject-of-spaces.txt" 2>&1 || status=$?
  judge subject-of-spaces "$status" 64 not-called '--subject needs the subject line'

  state_of standard-output-closed-before-the-merge
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/standard-output-closed-before-the-merge" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --subject "$subject" --body-file "$work/body.txt" >&- 2>"$work/out/standard-output-closed-before-the-merge.txt") || status=$?
  judge standard-output-closed-before-the-merge "$status" 2 not-called 'cannot write to standard output'

  state_of reader-of-the-output-left-before-the-facts
  {
    status=0
    (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/reader-of-the-output-left-before-the-facts" MERGE_PR_STUB_GITHUB="$work/github.git" \
      sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --subject "$subject" --body-file "$work/body.txt" 2>"$work/out/reader-of-the-output-left-before-the-facts.txt") || status=$?
    printf '%s\n' "$status" >"$work/out/reader-of-the-output-left-before-the-facts.status"
  } | true
  judge reader-of-the-output-left-before-the-facts "$(cat "$work/out/reader-of-the-output-left-before-the-facts.status")" 2 not-called 'cannot write to standard output'

  cp "$script_directory/merge-pr.sh" "$work/check-that-cannot-run/merge-pr.sh"
  printf '#!/bin/sh\necho "check-commit-messages: error: this copy cannot run" >&2\nexit 2\n' >"$work/check-that-cannot-run/check-commit-messages.sh"
  state_of check-script-that-cannot-run
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/check-script-that-cannot-run" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$work/check-that-cannot-run/merge-pr.sh" 7 --head "$reviewed_head" --subject "$subject" --body-file "$work/body.txt") >"$work/out/check-script-that-cannot-run.txt" 2>&1 || status=$?
  judge check-script-that-cannot-run "$status" 2 not-called 'exited 2 for the squash message'

  git --git-dir="$work/github.git" update-ref refs/heads/main "$(git -C "$work/clone" rev-parse "$clean_squash^")"
  state_of reader-of-the-output-leaves-before-the-merge
  fact_lines=$(grep -c '^merge-pr: ' "$work/out/dry-run-with-every-gate-open.txt")
  fact_lines=$((fact_lines - 1))
  {
    status=0
    (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/reader-of-the-output-leaves-before-the-merge" MERGE_PR_STUB_GITHUB="$work/github.git" \
      sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --subject "$subject" --body-file "$work/body.txt" 2>"$work/out/reader-of-the-output-leaves-before-the-merge.txt") || status=$?
    printf '%s\n' "$status" >"$work/out/reader-of-the-output-leaves-before-the-merge.status"
  } | head -n "$fact_lines" >/dev/null
  status=$(cat "$work/out/reader-of-the-output-leaves-before-the-merge.status")
  cases=$((cases + 1))
  if [ "$status" = 0 ] && [ -e "$work/state/reader-of-the-output-leaves-before-the-merge/merge-arguments" ]; then
    printf 'PASS reader-of-the-output-leaves-before-the-merge (exit 0 from the range check, gh pr merge called)\n'
  else
    printf 'FAIL reader-of-the-output-leaves-before-the-merge (exit %s; expected exit 0 after a merge; output in %s)\n' "$status" "$work/out/reader-of-the-output-leaves-before-the-merge.txt"
    failures=$((failures + 1))
  fi

  if [ "$failures" -gt 0 ]; then
    say "self-test FAIL: $failures of $cases cases behaved otherwise. Outputs are in $work/out."
    return 1
  fi
  say "self-test PASS: $cases cases. Outputs are in $work/out."
}

number=
head=
subject=
body_file=
dry_run=0
mode=run
while [ $# -gt 0 ]; do
  case $1 in
    --head)
      [ $# -ge 2 ] || usage '--head needs the full sha of the reviewed commit'
      head=$2
      shift
      ;;
    --subject)
      [ $# -ge 2 ] || usage '--subject needs the subject line of the squash commit'
      case $2 in
        -*) usage "--subject needs the subject line of the squash commit, and the next argument starts with a hyphen like an option: $2" ;;
      esac
      subject=$2
      shift
      ;;
    --body-file)
      [ $# -ge 2 ] || usage '--body-file needs the file that holds the body of the squash message'
      case $2 in
        -*) usage "--body-file needs a file, and the next argument starts with a hyphen: $2. gh reads - as standard input, and cat reads such a name as an option. Give the file another name, or a path such as ./$2" ;;
      esac
      body_file=$2
      shift
      ;;
    --dry-run) dry_run=1 ;;
    --self-test) mode=self-test ;;
    -h|--help) help_text; exit 0 ;;
    -*) usage "unknown option: $1" ;;
    *)
      [ -z "$number" ] || usage "more than one pull request number: $number and $1"
      number=$1
      ;;
  esac
  shift
done

case $mode in
  self-test) self_test ;;
  run) run ;;
esac
