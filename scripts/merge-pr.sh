#!/bin/sh
set -eu

script_directory=$(cd "$(dirname "$0")" && pwd)
CHECK=$script_directory/check-commit-messages.sh

THREADS_QUERY="query(\$owner: String!, \$repo: String!, \$number: Int!, \$endCursor: String) { repository(owner: \$owner, name: \$repo) { pullRequest(number: \$number) { reviewThreads(first: 100, after: \$endCursor) { pageInfo { hasNextPage endCursor } nodes { isResolved } } } } }"

RUNS_FILTER='.workflow_runs[] | "\(.event)\t\(.created_at)\t\(.id)\t\(.status)\t\(.conclusion // "none")\t\(.run_attempt)\t\(.head_branch // "no branch")\t\(.path)"'
JOBS_FILTER='.jobs[] | "\(.name)\t\(.status)\t\(.conclusion // "none")"'
CI_WORKFLOW=.github/workflows/ci.yml

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
        - for each of the events push and pull_request, the newest workflow
          run on that head, the one created last, completed with success, and
          the jobs macos and windows succeeded in it. For the event push only
          a run counts whose head branch, as GitHub names it, is the head
          branch of the pull request. An older run of the same event, and a
          push run that another branch started on the same commit, is printed
          and is not a gate. When the head branch is in another repository, a
          push to it starts no run in this repository, and the pull_request
          run alone is required.
        - it has 0 unresolved review threads
        - the squash message (the subject, an empty line, the body file) passes
          sh scripts/check-commit-messages.sh --stored-message
        - the title and the description of the pull request pass it too
      Then merge with
        gh pr merge <number> --squash --match-head-commit <sha> --subject <text> --body-file -
      which hands gh, on standard input, the body that was checked. Then read
      the state, the head and the merge commit of the pull request, whatever
      the exit code of gh was. Only when GitHub reports it MERGED with the
      head given with --head, fetch, print the merge commit with its author
      and committer, and run the range check on that commit.
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
It finds the workflow runs by the head commit, whatever workflow file started
them. So it cannot tell which pull request a pull_request run belongs to when
two pull requests share a head, and it refuses when the newest run of an event
comes from a workflow file without the jobs macos and windows. It reads no
check that an app other than GitHub Actions reports on the head.
A refusal over a run that failed names the full re-run, gh run rerun <run>,
and not gh run rerun <run> --failed: the gate reads the job list of the run,
and nobody has seen what that list holds after only the failed jobs ran again.

Exit codes: 0 merged and the merge commit passed the range check, or with
--dry-run every gate holds; 1 a gate refuses, and this run merged nothing; 2
the script could not finish: either before the merge call, and then this run
merged nothing, or after it, and then the last line says which of these
holds: GitHub reports the pull request as not merged; the script could not
read its state; or a signal stopped the script. In the last two cases the
pull request may be merged, and the line names the command that shows its
state and the command that checks its merge commit; 3 the pull request is
merged, and its merge commit failed the range check or was not checked:
GitHub reports no merge commit for it yet, the script could not fetch the
commit, or GitHub merged another head than the one given with --head; 64 bad
usage.
The signals INT, TERM and HUP stop the script with exit 2. Its last line then
says whether the signal came before the merge call or after it.
EOF
}

merge_called=0

output_failed() {
  [ "$merge_called" = 1 ] || die 'cannot write to standard output, so this run stops before the merge call. It merged nothing.'
}

say() {
  printf 'merge-pr: %s\n' "$1" || output_failed
}

say_lines() {
  printf '%s\n' "$1" | sed -n '/./s/^/merge-pr:   /p' || output_failed
}

die() {
  printf 'merge-pr: error: %s\n' "$1" >&2 || :
  exit 2
}

usage() {
  {
    printf 'merge-pr: %s\n' "$1"
    printf 'Usage: sh scripts/merge-pr.sh <number> --head <full sha> --subject <text> --body-file <file> [--dry-run]\n       sh scripts/merge-pr.sh --help\n'
  } >&2 || :
  exit 64
}

refuse() {
  printf 'merge-pr: REFUSED: %s\n' "$1" >&2
  refusals=$((refusals + 1))
}

count_lines() {
  printf '%s\n' "$1" | grep -c "$2" || true
}

runs_of() {
  printf '%s\n' "$workflow_runs" | awk -F '\t' -v event="$1" '$1 == event'
}

newest_run_of() {
  case $1 in
    push) runs_of push | awk -F '\t' -v branch="$head_branch" '$7 == branch' ;;
    *) runs_of "$1" ;;
  esac | awk 'NR == 1'
}

newest_of() {
  case $1 in
    push) printf 'the newest push run of the head branch %s on %s' "$head_branch" "$head" ;;
    *) printf 'the newest %s run on %s' "$1" "$head" ;;
  esac
}

jobs_of() {
  printf '%s\n' "$jobs" | awk -F '\t' -v run="$1" '$1 == run { print $2 "\t" $3 "\t" $4 }'
}

read_facts() {
  fields=$(gh pr view "$number" --json state,isDraft,mergeable,headRefOid,headRefName,baseRefName,isCrossRepository,mergeCommit --jq '.state, .isDraft, .mergeable, .headRefOid, .headRefName, .baseRefName, .isCrossRepository, (.mergeCommit.oid // "none")') ||
    die "gh could not read pull request #$number. Its message is above."
  {
    read -r state || :
    read -r is_draft || :
    read -r mergeable || :
    read -r head_on_github || :
    read -r head_branch || :
    read -r base || :
    read -r is_cross_repository || :
    read -r merge_commit_on_github || :
  } <<EOF
$fields
EOF
  [ -n "$merge_commit_on_github" ] || die "gh printed fewer than 8 fields for pull request #$number: $fields"
  case $is_cross_repository in
    false) required_events='push pull_request' ;;
    true) required_events=pull_request ;;
    *) die "gh printed a value of isCrossRepository that is neither true nor false for pull request #$number: $fields" ;;
  esac
  title=$(gh pr view "$number" --json title --jq .title) || die "gh could not read the title of pull request #$number"
  description=$(gh pr view "$number" --json body --jq .body) || die "gh could not read the description of pull request #$number"
  workflow_runs=$(gh api "repos/{owner}/{repo}/actions/runs?head_sha=$head&per_page=100" --paginate --jq "$RUNS_FILTER") ||
    die "gh could not read the workflow runs of $head"
  unread_runs=$(printf '%s\n' "$workflow_runs" | awk -F '\t' 'NF && (NF != 8 || $3 !~ /^[0-9]+$/)') ||
    die 'awk could not read the workflow runs'
  [ -z "$unread_runs" ] ||
    die "gh printed a workflow run that this script cannot read. It reads 8 fields divided by tabs, the third a run id of digits, and got:$nl$unread_runs"
  workflow_runs=$(printf '%s\n' "$workflow_runs" | LC_ALL=C sort -t "$tab" -k 2,2r -k 3,3nr) ||
    die 'sort could not order the workflow runs'
  jobs=
  for event in $required_events; do
    run_id=$(newest_run_of "$event" | cut -f 3)
    [ -n "$run_id" ] || continue
    jobs_of_run=$(gh api "repos/{owner}/{repo}/actions/runs/$run_id/jobs?per_page=100" --paginate --jq "$JOBS_FILTER") ||
      die "gh could not read the jobs of workflow run $run_id"
    unread_jobs=$(printf '%s\n' "$jobs_of_run" | awk -F '\t' 'NF && NF != 3') ||
      die 'awk could not read the jobs'
    [ -z "$unread_jobs" ] ||
      die "gh printed a job of workflow run $run_id that this script cannot read. It reads 3 fields divided by tabs, and got:$nl$unread_jobs"
    jobs=$jobs$(printf '%s\n' "$jobs_of_run" | sed -n "/./s/^/$run_id$tab/p")$nl
  done
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
  say "pull request #$number: state $state, draft $is_draft, mergeable $mergeable, base $base, head branch $head_branch"
  say "head on GitHub: $head_on_github"
  say "head reviewed:  $head"
  say "title: $title"
  say "description: $(count_lines "$description" '') lines"
  [ "$is_cross_repository" = false ] ||
    say "the head branch of pull request #$number is in another repository, so a push to it starts no run in this repository. The pull_request run alone is required."
  for event in push pull_request; do
    runs_of_event=$(runs_of "$event")
    say "$event runs on $head, newest first (event, created, run, status, conclusion, attempt, head branch, workflow file): $(count_lines "$runs_of_event" .)"
    say_lines "$runs_of_event"
    case " $required_events " in
      *" $event "*) ;;
      *) continue ;;
    esac
    run_id=$(newest_run_of "$event" | cut -f 3)
    [ -n "$run_id" ] || continue
    jobs_of_run=$(jobs_of "$run_id")
    say "jobs of run $run_id, $(newest_of "$event") (name, status, conclusion): $(count_lines "$jobs_of_run" .)"
    say_lines "$jobs_of_run"
  done
  say "review threads: $thread_total, unresolved: $thread_unresolved"
  say "squash subject: $subject"
  say "squash body: $body_file, $(count_lines "$body" '') lines"
}

refuse_a_missing_run() {
  case $event in
    push)
      refuse "no push run of the head branch $head_branch exists for $head. GitHub starts one when the commit is pushed to that branch: wait a minute and run this script again. A push run that another branch started on the same commit is not a gate. A push whose newest commit says [skip ci] starts none. When none has started by then, push a new commit: the new head needs a new review."
      ;;
    pull_request)
      refuse "no pull_request run exists for $head. GitHub starts one when the pull request is opened or its head moves: wait a minute and run this script again. When none has started by then, close and reopen the pull request, which starts one: gh pr close $number && gh pr reopen $number. A head commit whose message says [skip ci] starts none that way either: push a new commit then, and the new head needs a new review."
      ;;
  esac
}

refuse_a_run_without_success() {
  run_fact="$(newest_of "$event"), run $run_id,"
  case $run_conclusion in
    action_required)
      refuse "$run_fact waits for an approval: its conclusion is action_required. GitHub can hold the run of a pull request from another repository until someone with write access approves it. Approve it on its page (gh run view $run_id --web), wait for it (gh run watch $run_id), then run this script again."
      ;;
    skipped)
      refuse "$run_fact completed with skipped: it ran no job, so it tested nothing. The workflow has to run the jobs macos and windows for every push and every pull request: restore that in $CI_WORKFLOW and push. The new head needs a new review."
      ;;
    failure|timed_out)
      failed_run="$run_fact completed with $run_conclusion. Read why: gh run view $run_id --log-failed."
      case $event in
        push)
          refuse "$failed_run If the cause is in the commits, fix it and push: the new head needs a new review. If the cause is outside them, such as a runner that failed, run it again and wait: gh run rerun $run_id && gh run watch $run_id. Then run this script again."
          ;;
        pull_request)
          refuse "$failed_run If its step Commit messages names a line in the title or the description, remove the line on GitHub (gh pr edit $number), then close and reopen the pull request: gh pr close $number && gh pr reopen $number. That starts a new pull_request run, which reads the new text; a re-run reads the old text. If the cause is in the commits or in their merge with $base, fix it and push: the new head needs a new review. If the cause is outside both, such as a runner that failed, run it again and wait: gh run rerun $run_id && gh run watch $run_id. Then run this script again."
          ;;
      esac
      ;;
    *)
      refuse "$run_fact completed with $run_conclusion. Run it again and wait: gh run rerun $run_id && gh run watch $run_id. Then run this script again."
      ;;
  esac
}

refuse_a_job_without_success() {
  job_fact="the job $required did not succeed in $event run $run_id, $(newest_of "$event"), although the run completed with success: no job of the run has that name, or a job of that name did not complete with success. Its jobs (name, status, conclusion):$nl$(jobs_of "$run_id")$nl"
  if [ "$run_path" = "$CI_WORKFLOW" ]; then
    refuse "${job_fact}A job was renamed, removed or skipped, or it failed where the workflow lets a job fail. The workflow has to run the jobs macos and windows for every push and every pull request, and both have to succeed: restore that in $CI_WORKFLOW and push. The new head needs a new review."
  else
    refuse "${job_fact}Another workflow file than $CI_WORKFLOW started this run: $run_path. This gate reads the newest run of each event, whatever file started it, so it cannot hold while two workflow files run for the event $event: keep the jobs of that event in one workflow file, or change this gate in the pull request that adds the second file."
  fi
}

read_stored_message() {
  stored_message_status=0
  printf '%s\n\n%s\n' "$2" "$3" | sh "$CHECK" --stored-message || stored_message_status=$?
  case $stored_message_status in
    0|1) ;;
    *) die "sh $CHECK --stored-message exited $stored_message_status for $1, and only 0 and 1 are verdicts. Its message is above. This script cannot tell whether that text carries an attribution line, so it stops here." ;;
  esac
}

apply_gates() {
  refusals=0
  case $state in
    OPEN)
      case $mergeable in
        MERGEABLE) ;;
        CONFLICTING) refuse "GitHub reports that pull request #$number conflicts with $base. Merge origin/$base into the branch in a clone, push, and wait for the checks of the new head." ;;
        *) refuse "GitHub reports mergeable=$mergeable for pull request #$number: it has not computed the merge yet. Run this script again in a minute." ;;
      esac
      ;;
    MERGED)
      if [ "$merge_commit_on_github" = none ]; then
        refuse "pull request #$number is MERGED, and only an OPEN pull request can be merged. GitHub merged it before this run started and reports no merge commit for it yet. gh pr view $number --json mergeCommit shows the commit when GitHub has it."
      else
        refuse "pull request #$number is MERGED, and only an OPEN pull request can be merged. GitHub merged it before this run started and holds its merge commit, $merge_commit_on_github. This run did not check that commit. To check it: $(range_check_command "$merge_commit_on_github")"
      fi
      ;;
    *)
      refuse "pull request #$number is $state, and only an OPEN pull request can be merged. If the work is not on $base yet, open a new pull request for it."
      ;;
  esac
  [ "$is_draft" = false ] ||
    refuse "pull request #$number is a draft. Mark it ready when its review is done: gh pr ready $number"
  [ "$head_on_github" = "$head" ] ||
    refuse "the head of pull request #$number is $head_on_github, and --head names $head. A push arrived after the review. Review and verify the new head, then pass it as --head."

  for event in $required_events; do
    run=$(newest_run_of "$event")
    if [ -z "$run" ]; then
      refuse_a_missing_run
      continue
    fi
    IFS=$tab read -r _ _ run_id run_status run_conclusion _ _ run_path <<EOF
$run
EOF
    if [ "$run_status" != completed ]; then
      refuse "$(newest_of "$event"), run $run_id, has not completed: its status is $run_status. Wait for it: gh run watch $run_id. Then run this script again."
    elif [ "$run_conclusion" != success ]; then
      refuse_a_run_without_success
    else
      for required in macos windows; do
        jobs_of "$run_id" | awk -F '\t' -v job="$required" '$1 == job { seen = 1; if ($2 != "completed" || $3 != "success") failed = 1 } END { exit !(seen && !failed) }' && continue
        refuse_a_job_without_success
        [ "$run_path" = "$CI_WORKFLOW" ] || break
      done
    fi
  done

  [ "$thread_unresolved" = 0 ] ||
    refuse "pull request #$number has unresolved review threads: $thread_unresolved of $thread_total. Answer each one and resolve it on GitHub, then run this script again."

  read_stored_message 'the squash message' "$subject" "$body"
  [ "$stored_message_status" = 0 ] ||
    refuse "the squash message carries an attribution line, listed above. Remove it from --subject or from $body_file."
  read_stored_message "the title and the description of pull request #$number" "$title" "$description"
  [ "$stored_message_status" = 0 ] ||
    refuse "the title or the description of pull request #$number carries an attribution line, listed above. This script does not edit a pull request. Remove the line on GitHub (gh pr edit $number --body-file <file>), then run this script again."
}

range_check_command() {
  printf "git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/%s '%s^!'" "$base" "$1"
}

merged_but() {
  if [ -n "$merge_commit" ]; then
    how_to_check="Run: $(range_check_command "$merge_commit")"
  else
    how_to_check="gh pr view $number --json mergeCommit shows that commit when GitHub has it, and this command checks it: $(range_check_command '<merge commit>')"
  fi
  printf 'merge-pr: error: pull request #%s is merged, but %s, so its merge commit was not checked. %s\n' "$number" "$1" "$how_to_check" >&2 || :
  exit 3
}

stopped_by() {
  [ "$merge_called" = 1 ] ||
    die "the signal $1 stopped this script before its merge call. This run merged nothing."
  printf 'merge-pr: error: the signal %s stopped this script after it called gh pr merge, so pull request #%s may be merged, and this run did not finish the check of its merge commit. Run: gh pr view %s --json state,mergeCommit. If the state is MERGED, check the merge commit that it names: %s\n' "$1" "$number" "$number" "$(range_check_command '<merge commit>')" >&2 || :
  exit 2
}

merge() {
  merge_called=1
  merge_status=0
  printf '%s\n' "$body" | gh pr merge "$number" --squash --match-head-commit "$head" --subject "$subject" --body-file - || merge_status=$?
  facts_after_the_call=$(gh pr view "$number" --json state,headRefOid,mergeCommit --jq '.state, .headRefOid, (.mergeCommit.oid // "none")') || facts_after_the_call=
  {
    read -r state_after_the_call || :
    read -r head_after_the_call || :
    read -r merge_commit || :
  } <<EOF
$facts_after_the_call
EOF
  [ "$merge_commit" != none ] || merge_commit=
  case $state_after_the_call in
    MERGED)
      [ "$merge_status" = 0 ] ||
        say "gh pr merge exited $merge_status, and its message is above. GitHub reports pull request #$number as MERGED all the same, so this script goes on as after a merge call that succeeded."
      say "pull request #$number is merged"
      ;;
    OPEN|CLOSED)
      [ "$merge_status" = 0 ] ||
        die "gh pr merge exited $merge_status, and its message is above. GitHub reports pull request #$number as $state_after_the_call: it is not merged. Read the message, and do not retry with --admin."
      die "gh pr merge exited 0, and GitHub reports pull request #$number as $state_after_the_call: it is not merged. GitHub may hold the merge, as a merge queue does, and merge later without this script. gh pr view $number --json state,mergeCommit shows it then, and this command checks the merge commit: $(range_check_command '<merge commit>')"
      ;;
    *)
      die "gh pr merge exited $merge_status, and its message is above. Then this script could not read the state of the pull request (gh printed '$state_after_the_call'), so it does not know whether pull request #$number was merged. Run: gh pr view $number --json state,mergeCommit. If the state is MERGED, check the merge commit that it names: $(range_check_command '<merge commit>'). Do not retry with --admin."
      ;;
  esac
  [ "$head_after_the_call" = "$head" ] ||
    merged_but "with the head $head_after_the_call, and --head names $head, the commit that was reviewed. Someone pushed to the pull request and merged it outside this script after this script read its head, so what that push added was not reviewed. This script stops at such a merge"
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
  trap '' PIPE
  trap 'stopped_by INT' INT
  trap 'stopped_by TERM' TERM
  trap 'stopped_by HUP' HUP
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
    printf 'merge-pr: gates that refuse the merge of pull request #%s: %d. This run merged nothing.\n' "$number" "$refusals" >&2
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
  mkdir "$work/bin" "$work/state" "$work/out" "$work/check-that-cannot-run" "$work/git-whose-fetch-fails" "$work/git-that-signals-the-script"

  cat >"$work/bin/gh" <<'EOF'
#!/bin/sh
state=$MERGE_PR_STUB_STATE
case "$1 $2" in
  'pr view')
    case " $* " in
      *' --json state,isDraft,mergeable,headRefOid,headRefName,baseRefName'*) cat "$state/fields" ;;
      *' --json title '*) cat "$state/title" ;;
      *' --json body '*) cat "$state/description" ;;
      *' --json state,headRefOid,mergeCommit '*)
        if [ -e "$state/state-cannot-be-read" ]; then
          echo 'stub gh: GitHub did not answer' >&2
          exit 1
        fi
        cat "$state/state-after-the-merge-call" "$state/head-after-the-merge-call"
        if [ -s "$state/merge-commit" ]; then cat "$state/merge-commit"; else echo none; fi
        ;;
      *) echo "stub gh: unexpected pr view: $*" >&2; exit 97 ;;
    esac
    ;;
  'api graphql') cat "$state/threads" ;;
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
    if [ -e "$state/call-exits-0-and-github-did-not-merge" ]; then
      echo 'stub gh: the pull request will be added to the merge queue'
      exit 0
    fi
    if [ -s "$state/merge-commit" ]; then
      git --git-dir="$MERGE_PR_STUB_GITHUB" update-ref refs/heads/main "$(cat "$state/merge-commit")"
    fi
    printf 'MERGED\n' >"$state/state-after-the-merge-call"
    if [ -e "$state/signal-during-the-merge-call" ]; then
      kill -s "$(cat "$state/signal-during-the-merge-call")" "$PPID"
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

  cat >"$work/git-whose-fetch-fails/git" <<'EOF'
#!/bin/sh
case " $* " in
  *' fetch '*)
    echo 'stub git: the fetch failed' >&2
    exit 1
    ;;
esac
exec "$MERGE_PR_STUB_REAL_GIT" "$@"
EOF
  chmod +x "$work/git-whose-fetch-fails/git"
  cat >"$work/git-that-signals-the-script/git" <<'EOF'
#!/bin/sh
case " $* " in
  ' rev-parse --git-dir ') kill -s "$MERGE_PR_STUB_SIGNAL" "$PPID" ;;
esac
exec "$MERGE_PR_STUB_REAL_GIT" "$@"
EOF
  chmod +x "$work/git-that-signals-the-script/git"
  MERGE_PR_STUB_REAL_GIT=$(command -v git) || die 'git is not on PATH'
  export MERGE_PR_STUB_REAL_GIT
  stub_path=$work/bin

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
    printf '%s\t%s\t%s\t%s\t%s\t1\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "${7-feature}" "${6-.github/workflows/ci.yml}"
  }
  green_push_run=$(run_line push 2026-11-10T09:00:00Z 101 completed success)
  green_pull_request_run=$(run_line pull_request 2026-11-10T09:00:04Z 102 completed success)

  failures=0
  cases=0
  state_of() {
    state_directory=$work/state/$1
    mkdir "$state_directory"
    printf 'OPEN\nfalse\nMERGEABLE\n%s\nfeature\nmain\nfalse\nnone\n' "$reviewed_head" >"$state_directory/fields"
    printf 'Add the feature\n' >"$state_directory/title"
    printf 'A plain description.\n' >"$state_directory/description"
    printf '%s\n%s\n' "$green_push_run" "$green_pull_request_run" >"$state_directory/runs"
    printf '%s\n' "$green_jobs" >"$state_directory/jobs-101"
    printf '%s\n' "$green_jobs" >"$state_directory/jobs-102"
    printf 'true\ntrue\n' >"$state_directory/threads"
    printf '%s\n' "$clean_squash" >"$state_directory/merge-commit"
    printf 'OPEN\n' >"$state_directory/state-after-the-merge-call"
    printf '%s\n' "$reviewed_head" >"$state_directory/head-after-the-merge-call"
  }
  fields() {
    printf '%s\n%s\n%s\n%s\nfeature\nmain\n%s\n%s\n' "$2" "$3" "$4" "$5" "${6-false}" "${7-none}" >"$work/state/$1/fields"
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
  run_script() {
    name=$1
    shift
    status=0
    (cd "$work/clone" && PATH="$stub_path:$PATH" MERGE_PR_STUB_STATE="$work/state/$name" MERGE_PR_STUB_GITHUB="$work/github.git" \
      sh "$script_directory/merge-pr.sh" "$@") >"$work/out/$name.txt" 2>&1 || status=$?
  }
  expect() {
    name=$1 want_status=$2 want_merge=$3 want_text=$4
    shift 4
    run_script "$name" 7 --head "$reviewed_head" --subject "$subject" "$@"
    judge "$name" "$status" "$want_status" "$want_merge" "$want_text"
  }
  refused() {
    name=$1 want_text=$2
    shift 2
    [ $# -gt 0 ] || set -- --body-file "$work/body.txt"
    expect "$name" 1 not-called "$want_text" "$@"
  }
  says() {
    name=$1 text=$2
    cases=$((cases + 1))
    if grep -qF -- "$text" "$work/out/$name.txt"; then
      printf 'PASS %s: the output holds the text "%s"\n' "$name" "$text"
    else
      printf 'FAIL %s: the output does not hold the text "%s"; see %s\n' "$name" "$text" "$work/out/$name.txt"
      failures=$((failures + 1))
    fi
  }
  does_not_say() {
    name=$1 text=$2
    cases=$((cases + 1))
    if grep -qF -- "$text" "$work/out/$name.txt"; then
      printf 'FAIL %s: the output holds the text "%s"; see %s\n' "$name" "$text" "$work/out/$name.txt"
      failures=$((failures + 1))
    else
      printf 'PASS %s: the output does not hold the text "%s"\n' "$name" "$text"
    fi
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

  state_of merged-pull-request-without-a-merge-commit
  fields merged-pull-request-without-a-merge-commit MERGED false UNKNOWN "$reviewed_head"
  refused merged-pull-request-without-a-merge-commit 'reports no merge commit for it yet. gh pr view 7 --json mergeCommit shows the commit when GitHub has it'

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
  refused only-a-pull-request-run-exists "no push run of the head branch feature exists for $reviewed_head"

  state_of failed-pull-request-run-below-a-newer-green-one
  runs failed-pull-request-run-below-a-newer-green-one "$(run_line pull_request 2026-11-10T09:00:04Z 102 completed failure)" "$green_push_run" "$(run_line pull_request 2026-11-10T09:10:00Z 103 completed success)"
  jobs failed-pull-request-run-below-a-newer-green-one 102 "macos${tab}completed${tab}failure${nl}windows${tab}completed${tab}success"
  jobs failed-pull-request-run-below-a-newer-green-one 103 "$green_jobs"
  expect failed-pull-request-run-below-a-newer-green-one 0 not-called 'dry run: every gate holds' --body-file "$work/body.txt" --dry-run

  state_of newest-pull-request-run-failed-above-an-older-green-one
  runs newest-pull-request-run-failed-above-an-older-green-one "$green_pull_request_run" "$green_push_run" "$(run_line pull_request 2026-11-10T09:20:00Z 104 completed failure)"
  jobs newest-pull-request-run-failed-above-an-older-green-one 104 "macos${tab}completed${tab}failure${nl}windows${tab}completed${tab}success"
  refused newest-pull-request-run-failed-above-an-older-green-one 'a re-run reads the old text'
  says newest-pull-request-run-failed-above-an-older-green-one 'such as a runner that failed, run it again and wait: gh run rerun 104 && gh run watch 104. Then run this script again.'
  does_not_say newest-pull-request-run-failed-above-an-older-green-one '--failed'

  state_of newest-push-run-failed
  runs newest-push-run-failed "$(run_line push 2026-11-10T09:00:00Z 101 completed failure)" "$green_pull_request_run"
  jobs newest-push-run-failed 101 "macos${tab}completed${tab}success${nl}windows${tab}completed${tab}failure"
  refused newest-push-run-failed 'run 101, completed with failure'
  says newest-push-run-failed 'such as a runner that failed, run it again and wait: gh run rerun 101 && gh run watch 101. Then run this script again.'
  does_not_say newest-push-run-failed '--failed'

  state_of newest-push-run-timed-out
  runs newest-push-run-timed-out "$(run_line push 2026-11-10T09:00:00Z 101 completed timed_out)" "$green_pull_request_run"
  refused newest-push-run-timed-out 'run 101, completed with timed_out. Read why: gh run view 101 --log-failed'

  state_of newest-push-run-in-progress
  runs newest-push-run-in-progress "$(run_line push 2026-11-10T09:00:00Z 101 in_progress none)" "$green_pull_request_run"
  jobs newest-push-run-in-progress 101 "macos${tab}completed${tab}success${nl}windows${tab}in_progress${tab}none"
  refused newest-push-run-in-progress 'run 101, has not completed: its status is in_progress. Wait for it: gh run watch 101'

  state_of newest-pull-request-run-in-progress
  runs newest-pull-request-run-in-progress "$green_push_run" "$(run_line pull_request 2026-11-10T09:00:04Z 102 queued none)"
  jobs newest-pull-request-run-in-progress 102 "macos${tab}queued${tab}none${nl}windows${tab}queued${tab}none"
  refused newest-pull-request-run-in-progress 'run 102, has not completed: its status is queued. Wait for it: gh run watch 102'

  state_of push-run-without-the-job-windows
  jobs push-run-without-the-job-windows 101 "macos${tab}completed${tab}success"
  refused push-run-without-the-job-windows 'the job windows did not succeed in push run 101'

  state_of pull-request-run-without-the-job-macos
  jobs pull-request-run-without-the-job-macos 102 "windows${tab}completed${tab}success"
  refused pull-request-run-without-the-job-macos 'the job macos did not succeed in pull_request run 102'

  state_of job-windows-skipped-in-a-green-run
  jobs job-windows-skipped-in-a-green-run 101 "macos${tab}completed${tab}success${nl}windows${tab}completed${tab}skipped"
  refused job-windows-skipped-in-a-green-run 'the job windows did not succeed in push run 101'

  state_of job-macos-twice-and-one-failed
  jobs job-macos-twice-and-one-failed 101 "macos${tab}completed${tab}failure${nl}macos${tab}completed${tab}success${nl}windows${tab}completed${tab}success"
  refused job-macos-twice-and-one-failed 'the job macos did not succeed in push run 101'

  state_of newest-push-run-of-another-workflow-file
  runs newest-push-run-of-another-workflow-file "$green_push_run" "$green_pull_request_run" "$(run_line push 2026-11-10T09:30:00Z 105 completed success .github/workflows/docs.yml)"
  jobs newest-push-run-of-another-workflow-file 105 "build${tab}completed${tab}success"
  refused newest-push-run-of-another-workflow-file 'Another workflow file than .github/workflows/ci.yml started this run'
  says newest-push-run-of-another-workflow-file 'gates that refuse the merge of pull request #7: 1.'

  state_of newest-push-run-of-another-workflow-file-with-jobs-of-the-same-names
  runs newest-push-run-of-another-workflow-file-with-jobs-of-the-same-names "$green_push_run" "$green_pull_request_run" "$(run_line push 2026-11-10T09:30:00Z 105 completed success .github/workflows/other.yml)"
  jobs newest-push-run-of-another-workflow-file-with-jobs-of-the-same-names 105 "$green_jobs"
  expect newest-push-run-of-another-workflow-file-with-jobs-of-the-same-names 0 not-called 'dry run: every gate holds' --body-file "$work/body.txt" --dry-run

  state_of newer-failed-push-run-of-another-branch
  runs newer-failed-push-run-of-another-branch "$green_push_run" "$green_pull_request_run" "$(run_line push 2026-11-10T09:40:00Z 106 completed failure .github/workflows/ci.yml scratch)"
  jobs newer-failed-push-run-of-another-branch 106 "macos${tab}completed${tab}failure${nl}windows${tab}completed${tab}success"
  expect newer-failed-push-run-of-another-branch 0 not-called 'dry run: every gate holds' --body-file "$work/body.txt" --dry-run

  state_of green-push-run-of-another-branch-above-a-failed-one-of-the-head-branch
  runs green-push-run-of-another-branch-above-a-failed-one-of-the-head-branch "$(run_line push 2026-11-10T09:00:00Z 101 completed failure)" "$green_pull_request_run" "$(run_line push 2026-11-10T09:40:00Z 106 completed success .github/workflows/ci.yml scratch)"
  jobs green-push-run-of-another-branch-above-a-failed-one-of-the-head-branch 101 "macos${tab}completed${tab}failure${nl}windows${tab}completed${tab}success"
  jobs green-push-run-of-another-branch-above-a-failed-one-of-the-head-branch 106 "$green_jobs"
  refused green-push-run-of-another-branch-above-a-failed-one-of-the-head-branch "the newest push run of the head branch feature on $reviewed_head, run 101, completed with failure"

  state_of only-a-push-run-of-another-branch-exists
  runs only-a-push-run-of-another-branch-exists "$(run_line push 2026-11-10T09:00:00Z 101 completed success .github/workflows/ci.yml scratch)" "$green_pull_request_run"
  refused only-a-push-run-of-another-branch-exists "no push run of the head branch feature exists for $reviewed_head"

  state_of workflow-run-line-of-seven-fields
  printf 'push\t2026-11-10T09:00:00Z\t101\tcompleted\tsuccess\t1\t.github/workflows/ci.yml\n' >"$work/state/workflow-run-line-of-seven-fields/runs"
  expect workflow-run-line-of-seven-fields 2 not-called 'It reads 8 fields divided by tabs' --body-file "$work/body.txt"

  state_of pull-request-facts-of-seven-lines
  printf 'OPEN\nfalse\nMERGEABLE\n%s\nmain\nfalse\nnone\n' "$reviewed_head" >"$work/state/pull-request-facts-of-seven-lines/fields"
  expect pull-request-facts-of-seven-lines 2 not-called 'gh printed fewer than 8 fields' --body-file "$work/body.txt"

  state_of job-line-of-two-fields
  printf 'macos\tcompleted\n' >"$work/state/job-line-of-two-fields/jobs-101"
  expect job-line-of-two-fields 2 not-called 'It reads 3 fields divided by tabs' --body-file "$work/body.txt"

  state_of newest-pull-request-run-waits-for-approval
  fields newest-pull-request-run-waits-for-approval OPEN false MERGEABLE "$reviewed_head" true
  runs newest-pull-request-run-waits-for-approval "$(run_line pull_request 2026-11-10T09:00:04Z 102 completed action_required)"
  refused newest-pull-request-run-waits-for-approval 'run 102, waits for an approval'

  state_of newest-push-run-skipped
  runs newest-push-run-skipped "$(run_line push 2026-11-10T09:00:00Z 101 completed skipped)" "$green_pull_request_run"
  refused newest-push-run-skipped 'run 101, completed with skipped: it ran no job'

  state_of newest-push-run-cancelled
  runs newest-push-run-cancelled "$(run_line push 2026-11-10T09:00:00Z 101 completed cancelled)" "$green_pull_request_run"
  refused newest-push-run-cancelled 'run 101, completed with cancelled. Run it again and wait: gh run rerun 101 && gh run watch 101'

  state_of run-failed-in-a-job-of-another-name
  runs run-failed-in-a-job-of-another-name "$green_push_run" "$(run_line pull_request 2026-11-10T09:00:04Z 102 completed failure)"
  jobs run-failed-in-a-job-of-another-name 102 "$green_jobs${nl}lint${tab}completed${tab}failure"
  refused run-failed-in-a-job-of-another-name 'run 102, completed with failure'

  state_of pull-request-from-another-repository
  fields pull-request-from-another-repository OPEN false MERGEABLE "$reviewed_head" true
  runs pull-request-from-another-repository "$green_pull_request_run"
  expect pull-request-from-another-repository 0 not-called 'so a push to it starts no run in this repository. The pull_request run alone is required' --body-file "$work/body.txt" --dry-run

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
  expect merge-that-github-refuses 2 called 'gh pr merge exited 1, and its message is above. GitHub reports pull request #7 as OPEN: it is not merged. Read the message, and do not retry with --admin.' --body-file "$work/body.txt"

  state_of merge-that-github-refuses-on-a-closed-pull-request
  : >"$work/state/merge-that-github-refuses-on-a-closed-pull-request/github-refuses-the-merge"
  printf 'CLOSED\n' >"$work/state/merge-that-github-refuses-on-a-closed-pull-request/state-after-the-merge-call"
  expect merge-that-github-refuses-on-a-closed-pull-request 2 called 'gh pr merge exited 1, and its message is above. GitHub reports pull request #7 as CLOSED: it is not merged. Read the message, and do not retry with --admin.' --body-file "$work/body.txt"

  printf 'What changed and why.\n\nCo-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>\n' >"$work/clone/-body-with-a-trailer.txt"
  state_of body-file-whose-name-starts-with-a-hyphen
  expect body-file-whose-name-starts-with-a-hyphen 64 not-called 'starts with a hyphen' --body-file -body-with-a-trailer.txt

  state_of body-file-is-standard-input
  expect body-file-is-standard-input 64 not-called 'starts with a hyphen' --body-file -

  state_of subject-value-missing-before-dry-run
  run_script subject-value-missing-before-dry-run 7 --head "$reviewed_head" --body-file "$work/body.txt" --subject --dry-run
  judge subject-value-missing-before-dry-run "$status" 64 not-called 'starts with a hyphen'

  state_of head-in-upper-case
  upper_case_head=$(printf '%s' "$reviewed_head" | LC_ALL=C tr 'abcdef' 'ABCDEF')
  run_script head-in-upper-case 7 --head "$upper_case_head" --subject "$subject" --body-file "$work/body.txt" --dry-run
  judge head-in-upper-case "$status" 0 not-called 'dry run: every gate holds'

  state_of number-that-is-not-digits
  run_script number-that-is-not-digits 7a --head "$reviewed_head" --subject "$subject" --body-file "$work/body.txt"
  judge number-that-is-not-digits "$status" 64 not-called 'the pull request number must be digits, and got: 7a'

  state_of head-that-is-not-hex
  run_script head-that-is-not-hex 7 --head "${reviewed_head%?}g" --subject "$subject" --body-file "$work/body.txt"
  judge head-that-is-not-hex "$status" 64 not-called '--head needs the full sha of the reviewed commit, 40 hex digits, and got:'

  state_of head-that-is-short
  run_script head-that-is-short 7 --head "${reviewed_head%?}" --subject "$subject" --body-file "$work/body.txt"
  judge head-that-is-short "$status" 64 not-called '--head needs the full sha of the reviewed commit, 40 hex digits, and got:'

  state_of subject-of-two-lines
  run_script subject-of-two-lines 7 --head "$reviewed_head" --subject "Add the feature${nl}and a second line" --body-file "$work/body.txt"
  judge subject-of-two-lines "$status" 64 not-called '--subject must be one line'

  state_of body-file-missing
  run_script body-file-missing 7 --head "$reviewed_head" --subject "$subject"
  judge body-file-missing "$status" 64 not-called '--body-file needs the file that holds the body of the squash message'

  state_of body-file-that-does-not-exist
  run_script body-file-that-does-not-exist 7 --head "$reviewed_head" --subject "$subject" --body-file "$work/no-such-body.txt"
  judge body-file-that-does-not-exist "$status" 64 not-called '--body-file: no such file'

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
  does_not_say accepted 'MERGED all the same'

  state_of merged-commit-to-which-github-added-a-trailer
  printf '%s\n' "$squash_with_a_trailer" >"$work/state/merged-commit-to-which-github-added-a-trailer/merge-commit"
  expect merged-commit-to-which-github-added-a-trailer 3 called 'an agent does not rewrite the default branch' --body-file "$work/body.txt"

  state_of merged-and-github-reports-no-merge-commit
  : >"$work/state/merged-and-github-reports-no-merge-commit/merge-commit"
  expect merged-and-github-reports-no-merge-commit 3 called 'pull request #7 is merged, but GitHub reports no merge commit for it yet' --body-file "$work/body.txt"
  says merged-and-github-reports-no-merge-commit "gh pr view 7 --json mergeCommit shows that commit when GitHub has it, and this command checks it: git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/main '<merge commit>^!'"
  does_not_say merged-and-github-reports-no-merge-commit "'refs/remotes/origin/main^!'"

  state_of merged-and-the-fetch-fails
  stub_path=$work/git-whose-fetch-fails:$work/bin
  expect merged-and-the-fetch-fails 3 called "pull request #7 is merged, but git fetch origin main failed, so its merge commit was not checked. Run: git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/main '$clean_squash^!'" --body-file "$work/body.txt"
  stub_path=$work/bin

  absent_commit=2222222222222222222222222222222222222222
  state_of merged-and-the-merge-commit-is-not-in-the-clone
  printf '%s\n' "$absent_commit" >"$work/state/merged-and-the-merge-commit-is-not-in-the-clone/merge-commit"
  expect merged-and-the-merge-commit-is-not-in-the-clone 3 called "pull request #7 is merged, but the merge commit $absent_commit is not in this clone after git fetch origin main, so its merge commit was not checked. Run: git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/main '$absent_commit^!'" --body-file "$work/body.txt"

  state_of merge-call-fails-after-github-merged
  : >"$work/state/merge-call-fails-after-github-merged/call-fails-after-github-merged"
  expect merge-call-fails-after-github-merged 0 called "merge-pr: merged as $clean_squash" --body-file "$work/body.txt"
  says merge-call-fails-after-github-merged 'gh pr merge exited 1, and its message is above. GitHub reports pull request #7 as MERGED all the same'

  state_of merge-call-fails-after-github-merged-a-commit-with-a-trailer
  : >"$work/state/merge-call-fails-after-github-merged-a-commit-with-a-trailer/call-fails-after-github-merged"
  printf '%s\n' "$squash_with_a_trailer" >"$work/state/merge-call-fails-after-github-merged-a-commit-with-a-trailer/merge-commit"
  expect merge-call-fails-after-github-merged-a-commit-with-a-trailer 3 called 'an agent does not rewrite the default branch' --body-file "$work/body.txt"

  state_of merged-outside-the-script-with-another-head
  : >"$work/state/merged-outside-the-script-with-another-head/call-fails-after-github-merged"
  printf '%s\n' "$other_head" >"$work/state/merged-outside-the-script-with-another-head/head-after-the-merge-call"
  expect merged-outside-the-script-with-another-head 3 called "pull request #7 is merged, but with the head $other_head, and --head names $reviewed_head" --body-file "$work/body.txt"
  says merged-outside-the-script-with-another-head "This script stops at such a merge, so its merge commit was not checked. Run: git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/main '$clean_squash^!'"
  does_not_say merged-outside-the-script-with-another-head 'merge-pr: merged as'

  state_of merged-outside-the-script-with-another-head-and-no-merge-commit
  : >"$work/state/merged-outside-the-script-with-another-head-and-no-merge-commit/merge-commit"
  printf '%s\n' "$other_head" >"$work/state/merged-outside-the-script-with-another-head-and-no-merge-commit/head-after-the-merge-call"
  expect merged-outside-the-script-with-another-head-and-no-merge-commit 3 called "pull request #7 is merged, but with the head $other_head, and --head names $reviewed_head" --body-file "$work/body.txt"
  says merged-outside-the-script-with-another-head-and-no-merge-commit 'gh pr view 7 --json mergeCommit shows that commit when GitHub has it'

  for signal in INT TERM HUP; do
    signal_case=signal-$signal-during-the-merge-call
    trap_status=0
    sh -c 'trap "exit 7" "$1"; kill -s "$1" "$$"; exit 0' sh "$signal" || trap_status=$?
    if [ "$trap_status" != 7 ]; then
      printf 'SKIP %s: this process ignores the signal %s, so no script that it starts can set a trap for it. A background job of a shell without job control ignores INT, and nohup ignores HUP. Run the self-test in the foreground to prove this case.\n' "$signal_case" "$signal"
      continue
    fi
    state_of "$signal_case"
    printf '%s\n' "$signal" >"$work/state/$signal_case/signal-during-the-merge-call"
    expect "$signal_case" 2 called "the signal $signal stopped this script after it called gh pr merge, so pull request #7 may be merged, and this run did not finish the check of its merge commit. Run: gh pr view 7 --json state,mergeCommit. If the state is MERGED, check the merge commit that it names: git fetch origin && sh scripts/check-commit-messages.sh --default-ref refs/remotes/origin/main '<merge commit>^!'" --body-file "$work/body.txt"
    state_of "signal-$signal-before-the-merge-call"
    MERGE_PR_STUB_SIGNAL=$signal
    export MERGE_PR_STUB_SIGNAL
    stub_path=$work/git-that-signals-the-script:$work/bin
    expect "signal-$signal-before-the-merge-call" 2 not-called "the signal $signal stopped this script before its merge call. This run merged nothing." --body-file "$work/body.txt"
    stub_path=$work/bin
  done

  state_of merge-call-fails-and-the-state-cannot-be-read
  : >"$work/state/merge-call-fails-and-the-state-cannot-be-read/github-refuses-the-merge"
  : >"$work/state/merge-call-fails-and-the-state-cannot-be-read/state-cannot-be-read"
  expect merge-call-fails-and-the-state-cannot-be-read 2 called 'does not know whether pull request #7 was merged. Run: gh pr view 7 --json state,mergeCommit' --body-file "$work/body.txt"

  state_of merge-call-exits-0-and-github-did-not-merge
  : >"$work/state/merge-call-exits-0-and-github-did-not-merge/call-exits-0-and-github-did-not-merge"
  expect merge-call-exits-0-and-github-did-not-merge 2 called 'gh pr merge exited 0, and GitHub reports pull request #7 as OPEN: it is not merged. GitHub may hold the merge' --body-file "$work/body.txt"

  state_of subject-of-spaces
  run_script subject-of-spaces 7 --head "$reviewed_head" --subject '   ' --body-file "$work/body.txt"
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
    -h|--help)
      help_text || die 'cannot write the help text to standard output'
      exit 0
      ;;
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
