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
        gh pr merge <number> --squash --match-head-commit <sha> --subject <text> --body-file <file>
      fetch, print the merge commit with its author and committer, and run the
      range check on that commit.
      --dry-run prints every fact and stops before the merge. It exits 0 only
      when every gate holds.
  sh scripts/merge-pr.sh --self-test
      Prove each refusal and the accepted case with a stub gh first on PATH, in
      a throwaway clone under a new temporary directory that is left in place.
      Needs a clone of this repository and a configured git user.

The script never passes --admin, never pushes and never edits a pull request.
It cannot see a merge that someone makes on the website or with gh pr merge
directly. The push run of CI reports such a merge after the fact, and only a
ruleset on the default branch can stop it.

Exit codes: 0 merged, or with --dry-run every gate holds; 1 a gate refuses, or
the merged commit fails the range check; 2 the script could not run; 64 bad usage.
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
  say "squash body: $body_file, $(grep -c '' "$body_file" || true) lines"
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

  if ! { printf '%s\n\n' "$subject"; cat "$body_file"; } | sh "$CHECK" --stored-message; then
    refuse "the squash message carries an attribution line, listed above. Remove it from --subject or from $body_file."
  fi
  if ! printf '%s\n\n%s\n' "$title" "$description" | sh "$CHECK" --stored-message; then
    refuse "the title or the description of pull request #$number carries an attribution line, listed above. This script does not edit a pull request. Remove the line on GitHub (gh pr edit $number --body-file <file>), then run this script again."
  fi
}

merge() {
  gh pr merge "$number" --squash --match-head-commit "$head" --subject "$subject" --body-file "$body_file" ||
    die "gh pr merge failed. Its message is above. Read it, and do not retry with --admin."
  merge_commit=$(gh pr view "$number" --json mergeCommit --jq .mergeCommit.oid) ||
    die "gh could not read the merge commit of pull request #$number after the merge"
  [ -n "$merge_commit" ] || die "GitHub reports no merge commit for pull request #$number after the merge"
  git fetch -q origin "$base" ||
    die "git fetch origin $base failed, so the merge commit was not checked. Run: git fetch origin && sh scripts/check-commit-messages.sh '$merge_commit^!'"
  git log -1 --format='merge-pr: merged as %H%nmerge-pr: author:    %an <%ae>%nmerge-pr: committer: %cn <%ce>%nmerge-pr: subject:   %s' "$merge_commit" ||
    die "the merge commit $merge_commit is not in this clone after git fetch origin $base"
  sh "$CHECK" --default-ref "refs/remotes/origin/$base" "$merge_commit^!"
}

run() {
  case $number in
    ''|*[!0-9]*) usage "the pull request number must be digits, and got: $number" ;;
  esac
  case $head in
    *[!0-9a-f]*) usage "--head needs the full sha of the reviewed commit, 40 hex digits, and got: $head" ;;
  esac
  [ ${#head} = 40 ] || usage "--head needs the full sha of the reviewed commit, 40 hex digits, and got: $head"
  [ -n "$subject" ] || usage '--subject needs the subject line of the squash commit'
  case $subject in
    *"$nl"*) usage '--subject must be one line; the rest of the message goes into --body-file' ;;
  esac
  [ -n "$body_file" ] || usage '--body-file needs the file that holds the body of the squash message'
  [ -f "$body_file" ] || usage "--body-file: no such file: $body_file"
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
    say "dry run: every gate holds. Without --dry-run this script runs: gh pr merge $number --squash --match-head-commit $head --subject <subject> --body-file $body_file"
    exit 0
  fi
  merge
}

self_test() {
  root=$(cd "$script_directory/.." && pwd)
  git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || die 'the self-test clones this repository, and this script is not inside one'
  [ -n "$(git config --get user.name || true)" ] || die 'the self-test commits in a throwaway clone and needs a configured git user'
  work=$(mktemp -d "${TMPDIR:-/tmp}/merge-pr-self-test.XXXXXX") || die 'mktemp could not make a directory'
  say "self-test in $work, which is left in place"
  mkdir "$work/bin" "$work/state" "$work/out"

  cat >"$work/bin/gh" <<'EOF'
#!/bin/sh
state=$MERGE_PR_STUB_STATE
case "$1 $2" in
  'pr view')
    case " $* " in
      *' --json state,isDraft,mergeable,headRefOid,baseRefName '*) cat "$state/fields" ;;
      *' --json title '*) cat "$state/title" ;;
      *' --json body '*) cat "$state/description" ;;
      *' --json mergeCommit '*) cat "$state/merge-commit" ;;
      *) echo "stub gh: unexpected pr view: $*" >&2; exit 97 ;;
    esac
    ;;
  'api graphql') cat "$state/threads" ;;
  'api repos/'*) cat "$state/check-runs" ;;
  'pr merge')
    printf '%s\n' "$@" >"$state/merge-arguments"
    case " $* " in
      *' --body-file - '*) cat >"$state/merge-body" ;;
    esac
    if [ -e "$state/merge-fails" ]; then
      echo 'stub gh: GitHub refused the merge' >&2
      exit 1
    fi
    git --git-dir="$MERGE_PR_STUB_GITHUB" update-ref refs/heads/main "$(cat "$state/merge-commit")"
    ;;
  *) echo "stub gh: unexpected call: $*" >&2; exit 97 ;;
esac
EOF
  chmod +x "$work/bin/gh"

  git init -q --bare "$work/github.git"
  git -C "$work/github.git" fetch -q "$root" HEAD:refs/heads/main
  git clone -q "$work/github.git" "$work/clone"
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
  green_runs="macos${tab}completed${tab}success${nl}windows${tab}completed${tab}success${nl}macos${tab}completed${tab}success${nl}windows${tab}completed${tab}success"

  failures=0
  cases=0
  state_of() {
    state_directory=$work/state/$1
    mkdir "$state_directory"
    printf 'OPEN\nfalse\nMERGEABLE\n%s\nmain\n' "$reviewed_head" >"$state_directory/fields"
    printf 'Add the feature\n' >"$state_directory/title"
    printf 'A plain description.\n' >"$state_directory/description"
    printf '%s\n' "$green_runs" >"$state_directory/check-runs"
    printf 'true\ntrue\n' >"$state_directory/threads"
    printf '%s\n' "$clean_squash" >"$state_directory/merge-commit"
  }
  fields() {
    printf '%s\n%s\n%s\n%s\nmain\n' "$2" "$3" "$4" "$5" >"$work/state/$1/fields"
  }
  expect() {
    name=$1 want_status=$2 want_merge=$3 want_text=$4
    shift 4
    status=0
    (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/$name" MERGE_PR_STUB_GITHUB="$work/github.git" \
      sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --subject "$subject" "$@") >"$work/out/$name.txt" 2>&1 || status=$?
    if [ -e "$work/state/$name/merge-arguments" ]; then merge=called; else merge=not-called; fi
    cases=$((cases + 1))
    if [ "$status" = "$want_status" ] && [ "$merge" = "$want_merge" ] && grep -qF -- "$want_text" "$work/out/$name.txt"; then
      printf 'PASS %s (exit %s, gh pr merge %s)\n' "$name" "$status" "$merge"
    else
      printf 'FAIL %s (exit %s, gh pr merge %s; expected exit %s, %s and the text "%s"; output in %s)\n' "$name" "$status" "$merge" "$want_status" "$want_merge" "$want_text" "$work/out/$name.txt"
      failures=$((failures + 1))
    fi
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
  fields merged-pull-request MERGED false UNKNOWN "$reviewed_head"
  refused merged-pull-request 'is MERGED, and only an OPEN pull request can be merged'

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

  state_of no-check-run-named-windows
  printf 'macos\tcompleted\tsuccess\n' >"$work/state/no-check-run-named-windows/check-runs"
  refused no-check-run-named-windows 'no check run named windows exists'

  state_of no-check-run-named-macos
  printf 'windows\tcompleted\tsuccess\n' >"$work/state/no-check-run-named-macos/check-runs"
  refused no-check-run-named-macos 'no check run named macos exists'

  state_of no-check-run-at-all
  : >"$work/state/no-check-run-at-all/check-runs"
  refused no-check-run-at-all 'no check run named macos exists'

  state_of check-run-in-progress
  printf '%s\nwindows\tin_progress\tnone\n' "$green_runs" >"$work/state/check-run-in-progress/check-runs"
  refused check-run-in-progress "windows${tab}in_progress${tab}none"

  state_of check-run-failed
  printf '%s\nmacos\tcompleted\tfailure\n' "$green_runs" >"$work/state/check-run-failed/check-runs"
  refused check-run-failed "macos${tab}completed${tab}failure"

  state_of check-run-of-another-name-failed
  printf '%s\nlint\tcompleted\tfailure\n' "$green_runs" >"$work/state/check-run-of-another-name-failed/check-runs"
  refused check-run-of-another-name-failed "lint${tab}completed${tab}failure"

  state_of check-run-skipped
  printf '%s\nwindows\tcompleted\tskipped\n' "$green_runs" >"$work/state/check-run-skipped/check-runs"
  refused check-run-skipped 'did not complete with success'

  state_of unresolved-review-thread
  printf 'true\nfalse\ntrue\n' >"$work/state/unresolved-review-thread/threads"
  refused unresolved-review-thread 'has unresolved review threads: 1 of 3'

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
  : >"$work/state/merge-that-github-refuses/merge-fails"
  expect merge-that-github-refuses 2 called 'do not retry with --admin' --body-file "$work/body.txt"

  printf 'What changed and why.\n\nCo-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>\n' >"$work/clone/-body-with-a-trailer.txt"
  state_of body-file-whose-name-starts-with-a-hyphen
  expect body-file-whose-name-starts-with-a-hyphen 64 not-called 'starts with a hyphen' --body-file -body-with-a-trailer.txt

  state_of body-file-is-standard-input
  expect body-file-is-standard-input 64 not-called 'starts with a hyphen' --body-file -

  state_of subject-value-missing-before-dry-run
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/subject-value-missing-before-dry-run" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$script_directory/merge-pr.sh" 7 --head "$reviewed_head" --body-file "$work/body.txt" --subject --dry-run) >"$work/out/subject-value-missing-before-dry-run.txt" 2>&1 || status=$?
  cases=$((cases + 1))
  if [ "$status" = 64 ] && [ ! -e "$work/state/subject-value-missing-before-dry-run/merge-arguments" ] && grep -qF 'starts with a hyphen' "$work/out/subject-value-missing-before-dry-run.txt"; then
    printf 'PASS subject-value-missing-before-dry-run (exit 64, gh pr merge not-called)\n'
  else
    printf 'FAIL subject-value-missing-before-dry-run (exit %s; expected exit 64, no merge and the text "starts with a hyphen"; output in %s)\n' "$status" "$work/out/subject-value-missing-before-dry-run.txt"
    failures=$((failures + 1))
  fi

  state_of head-in-upper-case
  upper_case_head=$(printf '%s' "$reviewed_head" | LC_ALL=C tr 'abcdef' 'ABCDEF')
  status=0
  (cd "$work/clone" && PATH="$work/bin:$PATH" MERGE_PR_STUB_STATE="$work/state/head-in-upper-case" MERGE_PR_STUB_GITHUB="$work/github.git" \
    sh "$script_directory/merge-pr.sh" 7 --head "$upper_case_head" --subject "$subject" --body-file "$work/body.txt" --dry-run) >"$work/out/head-in-upper-case.txt" 2>&1 || status=$?
  cases=$((cases + 1))
  if [ "$status" = 0 ] && grep -qF 'dry run: every gate holds' "$work/out/head-in-upper-case.txt"; then
    printf 'PASS head-in-upper-case (exit 0)\n'
  else
    printf 'FAIL head-in-upper-case (exit %s; expected exit 0 and "dry run: every gate holds"; output in %s)\n' "$status" "$work/out/head-in-upper-case.txt"
    failures=$((failures + 1))
  fi

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
      subject=$2
      shift
      ;;
    --body-file)
      [ $# -ge 2 ] || usage '--body-file needs the file that holds the body of the squash message'
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
