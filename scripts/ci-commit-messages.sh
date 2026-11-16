#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

CHECK=scripts/check-commit-messages.sh
AGENT_SETTINGS=.claude/settings.json

SETTINGS_PROBLEM='
const text = require("fs").readFileSync(process.argv[1], "utf8");
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
let settings;
try {
  settings = JSON.parse(text);
} catch (error) {
  console.log("the file is not valid JSON (" + error.message + ")");
  process.exit(0);
}
if (!isObject(settings)) console.log("the top level is not an object");
else if (!isObject(settings.attribution)) console.log("the key attribution is not an object");
else if (settings.attribution.commit !== "") console.log("attribution.commit is not the empty string");
else if (settings.attribution.pr !== "") console.log("attribution.pr is not the empty string");
else if (settings.attribution.sessionUrl !== false) console.log("attribution.sessionUrl is not false");
else if ("includeCoAuthoredBy" in settings && settings.includeCoAuthoredBy !== false) console.log("includeCoAuthoredBy is not false");
'

help_text() {
  cat <<'EOF'
The CI step "Commit messages". It runs scripts/check-commit-messages.sh on the
commits of the event that started the workflow. No flag of git skips it.

  sh scripts/ci-commit-messages.sh
      Read the event from these variables. GitHub Actions sets three of them
      for every step:
        GITHUB_EVENT_NAME  push or pull_request
        GITHUB_REF         the ref of the event, for example refs/heads/main
        GITHUB_BASE_REF    the base branch of a pull request. When it is empty,
                           the base counts as the default branch.
      The env block of the workflow step sets the other five from the event:
        DEFAULT_BRANCH     the default branch of the repository, for example main
        BEFORE             the tip of the ref before a push
        AFTER              the newest commit of the event: the head of the pull
                           request, or the pushed commit
        PR_TITLE, PR_BODY  the title and the description of the pull request
      Each step below can fail the job.
        1. The agent settings. The tracked file .claude/settings.json must hold
           "attribution": {"commit": "", "pr": "", "sessionUrl": false}, which
           stops Claude Code from adding its attribution lines, and no
           includeCoAuthoredBy other than false. Needs node, which parses the
           file as strictly as JSON asks.
        2. The self-test of the predicate.
        3. Controls. The range check must fail on the exempt commits when no
           commit is exempt, and the check of a stored message must fail on a
           trailer, on a tool footer and on a tool trailer. A check that passes
           there is blind.
        4. The range check on the commits of the event. On the default branch
           these are the pushed commits. On every other ref they are the commits
           that refs/remotes/origin/<DEFAULT_BRANCH> does not have, and a pull
           request into the default branch with 0 such commits fails.
        5. On every other ref, the same range with the copy of the check script
           that the default branch holds. A branch cannot move the exempt commit
           or weaken the predicate for its own commits.
        6. On a pull request, its title and description as one message, with
           the copy of this branch, and with the copy of the default branch
           once that copy has the mode --stored-message.
  sh scripts/ci-commit-messages.sh --self-test
      Run those steps on event shapes and on settings files, in throwaway
      repositories under a new temporary directory that is left in place. The
      commits of the event shapes sit on a commit that holds the tree of HEAD
      above the newest exempt commit, so the result does not depend on the
      messages of this repository's commits above the newest exempt commit.
      Needs a clone of this repository with its full history, and a name and
      an email for commits: a configured git user, or the variables
      GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and
      GIT_COMMITTER_EMAIL.

It cannot see a description that is edited after the last push, a push whose
newest commit says [skip ci], a settings file of a user that overrides the
tracked one, or a change to this script or to the workflow on the branch under
test. That change is in the diff of the pull request.

Exit codes: 0 every step passed, 1 a step found a problem, 2 the step could not
run, 64 bad usage.
EOF
}

say() {
  printf 'ci-commit-messages: %s\n' "$1"
}

fail() {
  printf 'ci-commit-messages: FAIL: %s\n' "$1" >&2
  exit 1
}

die() {
  printf 'ci-commit-messages: error: %s\n' "$1" >&2
  exit 2
}

commits_checked_on_pass_line() {
  sed -n 's/^check-commit-messages: PASS: .*: commits checked \([0-9][0-9]*\), with an attribution line 0 .*$/\1/p'
}

fail_pull_request_without_commits() {
  fail "the pull request run checked 0 commits in $range with $1. A pull request into $DEFAULT_BRANCH has at least one commit that $default_ref does not have, so the check read the wrong range, or the script exempts the commits of this branch. Compare NEWEST_EXEMPT_COMMIT in $CHECK with the default branch."
}

pull_request_into_the_default_branch() {
  [ "$GITHUB_EVENT_NAME" = pull_request ] && [ "${GITHUB_BASE_REF:-$DEFAULT_BRANCH}" = "$DEFAULT_BRANCH" ]
}

check_agent_settings() {
  settings_rule="$1 must hold \"attribution\": {\"commit\": \"\", \"pr\": \"\", \"sessionUrl\": false}. The empty values of commit and pr stop Claude Code from adding its co-author trailer and its pull request line, and sessionUrl false stops its session trailer."
  [ -f "$1" ] || fail "$1 is missing. $settings_rule Restore the tracked file."
  [ "$(od -An -tx1 -N 3 "$1" | tr -d ' \n')" != efbbbf ] ||
    fail "$1 starts with a byte order mark, and a JSON parser may reject the whole file for it. Save the file as UTF-8 without one. $settings_rule"
  command -v node >/dev/null 2>&1 || die "node is not on PATH, and this step reads $1 with it. Install Node.js, which the build of this repository needs as well."
  settings_problem=$(node -e "$SETTINGS_PROBLEM" "$1" 2>&1) || die "node could not read $1: $settings_problem"
  [ -z "$settings_problem" ] ||
    fail "in $1, $settings_problem. $settings_rule Do not write \"attribution\": false: older versions of Claude Code reject it and skip the whole file."
  say "$1 turns the attribution lines of Claude Code off"
}

run_event() {
  [ -n "${GITHUB_EVENT_NAME-}" ] || die 'GITHUB_EVENT_NAME is empty. GitHub Actions sets it; see --help.'
  [ -n "${GITHUB_REF-}" ] || die 'GITHUB_REF is empty. GitHub Actions sets it; see --help.'
  [ -n "${DEFAULT_BRANCH-}" ] || die 'DEFAULT_BRANCH is empty. The workflow step sets it; see --help.'
  [ -n "${AFTER-}" ] || die 'AFTER is empty. The workflow step sets it; see --help.'

  default_ref=refs/remotes/origin/$DEFAULT_BRANCH
  git rev-parse -q --verify "$default_ref^{commit}" >/dev/null ||
    die "$default_ref is not in this clone. The checkout step needs fetch-depth: 0."
  git rev-parse -q --verify "$AFTER^{commit}" >/dev/null ||
    die "the commit of the event, $AFTER, is not in this clone"

  check_agent_settings "$AGENT_SETTINGS"

  if ! self_test_output=$(sh "$CHECK" --self-test 2>&1); then
    printf '%s\n' "$self_test_output" >&2
    fail "the self-test of $CHECK failed. Its FAIL lines above name the fixtures."
  fi
  say "$(printf '%s\n' "$self_test_output" | sed -n '$p')"

  exempt_commit=$(sed -n 's/^NEWEST_EXEMPT_COMMIT=//p' "$CHECK")
  [ -n "$exempt_commit" ] || die "cannot read NEWEST_EXEMPT_COMMIT from $CHECK"
  control_status=0
  control_output=$(sh "$CHECK" --no-exempt "$exempt_commit" 2>&1) || control_status=$?
  if [ "$control_status" != 1 ]; then
    printf '%s\n' "$control_output" >&2
    fail "the control did not fail. sh $CHECK --no-exempt $exempt_commit exited $control_status, and it must exit 1, because those commits carry attribution lines. The range check is blind, so a PASS from it means nothing. Repair check_commits in $CHECK."
  fi
  say "control, which must fail: $(printf '%s\n' "$control_output" | sed -n 's/^check-commit-messages: FAIL: //p')"
  for control_line in 'Co-Authored-By: Control <control@example.invalid>' 'Generated with a control' 'Claude-Session: https://example.invalid/control'; do
    stored_message_control_status=0
    printf 'Control\n\n%s\n' "$control_line" | sh "$CHECK" --stored-message >/dev/null 2>&1 || stored_message_control_status=$?
    [ "$stored_message_control_status" = 1 ] ||
      fail "the control for --stored-message did not fail. A message with the line '$control_line' got exit $stored_message_control_status from sh $CHECK --stored-message, and it must get exit 1. The check of a title and a description is blind for such a line. Repair check_stored_message and report_message in $CHECK."
  done

  if [ "$GITHUB_REF" != "refs/heads/$DEFAULT_BRANCH" ]; then
    range="$default_ref..$AFTER"
  elif git rev-parse -q --verify "${BEFORE-}^{commit}" >/dev/null 2>&1; then
    range="$BEFORE..$AFTER"
  else
    say "the before value '${BEFORE-}' is not a commit in this clone: the push created the branch, or a force push dropped its old tip. Checking every commit reachable from $AFTER that is not exempt."
    range=$AFTER
  fi

  head_output=$(sh "$CHECK" --default-ref "$default_ref" "$range") || exit $?
  printf '%s\n' "$head_output"
  head_count=$(printf '%s\n' "$head_output" | commits_checked_on_pass_line)
  [ -n "$head_count" ] || fail "sh $CHECK $range exited 0 and printed no PASS line with a commit count, so nothing shows that it read the range."
  if pull_request_into_the_default_branch && [ "$head_count" = 0 ]; then
    fail_pull_request_without_commits "$CHECK"
  fi

  if [ "$GITHUB_REF" != "refs/heads/$DEFAULT_BRANCH" ] || [ "$GITHUB_EVENT_NAME" = pull_request ]; then
    default_commit=$(git rev-parse --verify "$default_ref^{commit}") ||
      die "$default_ref names no commit in this clone"
    default_script=$(git show "$default_commit:$CHECK") ||
      die "$default_ref holds no $CHECK, so nothing can be checked with the copy of the default branch"
  fi
  if [ "$GITHUB_REF" != "refs/heads/$DEFAULT_BRANCH" ]; then
    say "the same range, with the copy of $CHECK that $default_ref holds:"
    default_status=0
    default_output=$(sh -c "$default_script" "check-commit-messages.sh@$default_commit" "$range") || default_status=$?
    if [ "$default_status" != 0 ]; then
      fail "the copy of $CHECK that $default_ref holds refuses this range (exit $default_status, its findings are above), and the copy of this branch accepts it. Either this branch moved the exempt commit or changed the predicate, or $default_ref holds an older predicate that refuses a line which the newer one accepts. In both cases the commits must pass the copy of the default branch: reword the listed lines, and undo a change to the exempt commit."
    fi
    printf '%s\n' "$default_output"
    default_count=$(printf '%s\n' "$default_output" | commits_checked_on_pass_line)
    [ -n "$default_count" ] || fail "the copy of $CHECK that $default_ref holds exited 0 and printed no PASS line with a commit count, so nothing shows that it read the range."
    if pull_request_into_the_default_branch && [ "$default_count" = 0 ]; then
      fail_pull_request_without_commits "the copy of $CHECK that $default_ref holds"
    fi
    say "the copy of this branch read $head_count commits, and the copy of the default branch read $default_count"
  fi

  if [ "$GITHUB_EVENT_NAME" = pull_request ]; then
    printf '%s\n\n%s\n' "${PR_TITLE-}" "${PR_BODY-}" | sh "$CHECK" --stored-message ||
      fail "the title or the description of the pull request carries an attribution line, listed above. GitHub offers that text as the squash message. Remove the line on GitHub, then close and reopen the pull request. That starts a new pull_request run, which reads the new text; a re-run reads the old text, because this job reads the text from the event that started it."
    if sh -c "$default_script" "check-commit-messages.sh@$default_commit" --stored-message </dev/null >/dev/null 2>&1; then
      printf '%s\n\n%s\n' "${PR_TITLE-}" "${PR_BODY-}" | sh -c "$default_script" "check-commit-messages.sh@$default_commit" --stored-message ||
        fail "the copy of $CHECK that $default_ref holds refuses the title or the description of the pull request (its findings are above), and the copy of this branch accepts them. The branch weakened the predicate for this text. Undo that change, and remove the line on GitHub."
      say 'the title and the description of the pull request carry no attribution line, for the copy of this branch and for the copy of the default branch'
    else
      say "the title and the description of the pull request carry no attribution line for the copy of this branch. The copy that $default_ref holds has no --stored-message mode yet, so it did not read them."
    fi
  fi
  say 'PASS'
}

self_test() {
  root=$(pwd)
  git rev-parse --git-dir >/dev/null 2>&1 || die 'the self-test clones this repository, and this directory is not inside one'
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 ||
    die 'the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.'
  work=$(mktemp -d "${TMPDIR:-/tmp}/ci-commit-messages-self-test.XXXXXX") || die 'mktemp could not make a directory'
  say "self-test in $work, which is left in place"
  mkdir "$work/out" "$work/settings"

  failures=0
  shapes=0
  settings_files=0
  nl='
'
  trailer='Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>'
  footer='🤖 Generated with [Claude Code](https://claude.com/claude-code)'
  no_commit=0000000000000000000000000000000000000000
  commit_with_the_predicate_before_this_change=d7b245a0a29f4c212173e46bb8f0d3fa881c3ed5
  default_ref_in_runner=refs/remotes/origin/main
  commit_that_no_clone_has=1111111111111111111111111111111111111111
  commit_that_added_the_range_file=$(git log -1 --format=%H --diff-filter=A HEAD -- scripts/commit-range.sh) ||
    die 'git log could not find the commit that added scripts/commit-range.sh'
  [ -n "$commit_that_added_the_range_file" ] || die 'HEAD has no commit that added scripts/commit-range.sh'
  copy_before_the_range_file=$(git rev-parse --verify "$commit_that_added_the_range_file^") ||
    die 'git rev-parse could not name the parent of the commit that added scripts/commit-range.sh'

  exempt_commit=$(sed -n 's/^NEWEST_EXEMPT_COMMIT=//p' "$root/$CHECK")
  [ -n "$exempt_commit" ] || die "cannot read NEWEST_EXEMPT_COMMIT from $CHECK"
  dev=$work/dev
  git init -q "$dev"
  git -C "$dev" fetch -q "$root" HEAD
  base=$(git -C "$dev" commit-tree 'FETCH_HEAD^{tree}' -p "$exempt_commit" -m 'Hold the tree of HEAD above the newest exempt commit') ||
    die "the self-test builds its fixtures on the newest exempt commit $exempt_commit, and git could not make a commit above it from the history of HEAD. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
  git -C "$dev" checkout -q -b main "$base"

  commit() {
    git -C "$dev" -c commit.gpgsign=false commit -q --allow-empty --no-verify --cleanup=verbatim -m "$1"
  }
  start_at() {
    git -C "$dev" checkout -q --detach "$1"
  }
  tip() {
    git -C "$dev" rev-parse HEAD
  }
  commit_edited_check_script() {
    cmp -s "$root/$CHECK" "$dev/$CHECK" && die "the self-test could not edit $CHECK for '$1': the line that it replaces has changed"
    git -C "$dev" add "$CHECK"
    commit "$1"
  }

  cp "$root/$CHECK" "$root/scripts/ci-commit-messages.sh" "$dev/scripts/"
  if [ -f "$root/$AGENT_SETTINGS" ]; then
    mkdir -p "$dev/.claude"
    cp "$root/$AGENT_SETTINGS" "$dev/$AGENT_SETTINGS"
  fi
  if [ -n "$(git -C "$dev" status --porcelain)" ]; then
    git -C "$dev" add -A
    commit 'Take the scripts and the agent settings of the working tree'
  fi
  main=$(tip)

  commit 'Add a clean change'
  main_with_clean_commit=$(tip)
  start_at "$main"
  commit "Squash a pull request$nl$nl$trailer"
  main_with_bad_squash=$(tip)
  start_at "$main"
  commit 'Add a clean change'
  commit 'Add another clean change'
  clean_branch=$(tip)
  start_at "$main"
  commit 'Add a clean change'
  commit "Fix the gate$nl$nl$trailer"
  bad_branch=$(tip)
  start_at "$main"
  commit "Fix the gate$nl$nl$trailer"
  sed "s/^NEWEST_EXEMPT_COMMIT=.*/NEWEST_EXEMPT_COMMIT=$(tip)/" "$root/$CHECK" >"$dev/$CHECK"
  commit_edited_check_script 'Move the exempt commit onto the commit above'
  branch_that_moved_the_exempt_commit=$(tip)
  start_at "$main"
  commit "Fix the gate$nl$nl$trailer"
  sed 's/^    \[ -n "[^"]*" \] || continue$/    continue/' "$root/$CHECK" >"$dev/$CHECK"
  commit_edited_check_script 'Blind the range check'
  branch_that_blinded_the_range_check=$(tip)

  runner() {
    name=$1 main_commit=$2 checked_out=$3
    shift 3
    git init -q --bare "$work/$name.github.git"
    git -C "$dev" push -q --no-verify "$work/$name.github.git" "$main_commit:refs/heads/main" "$@"
    git init -q "$work/$name"
    git -C "$work/$name" remote add origin "$work/$name.github.git"
    git -C "$work/$name" fetch -q origin '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'
    git -C "$work/$name" checkout -q --detach "$checked_out"
  }

  expect_event() {
    name=$1 want_status=$2 want_text=$3
    shift 3
    status=0
    (cd "$work/$name" && env DEFAULT_BRANCH=main "$@" sh scripts/ci-commit-messages.sh) >"$work/out/$name.txt" 2>&1 || status=$?
    shapes=$((shapes + 1))
    if [ "$status" = "$want_status" ] && grep -qF -- "$want_text" "$work/out/$name.txt"; then
      printf 'PASS %s (exit %s)\n' "$name" "$status"
    else
      printf 'FAIL %s (exit %s, expected exit %s and the text "%s"; output in %s)\n' "$name" "$status" "$want_status" "$want_text" "$work/out/$name.txt"
      failures=$((failures + 1))
    fi
  }

  expect_settings() {
    name=$1 want_status=$2 want_text=$3
    [ $# -lt 4 ] || printf '%s\n' "$4" >"$work/settings/$name.json"
    status=0
    (check_agent_settings "$work/settings/$name.json") >"$work/out/settings-$name.txt" 2>&1 || status=$?
    settings_files=$((settings_files + 1))
    if [ "$status" = "$want_status" ] && grep -qF -- "$want_text" "$work/out/settings-$name.txt"; then
      printf 'PASS settings file %s (exit %s)\n' "$name" "$status"
    else
      printf 'FAIL settings file %s (exit %s, expected exit %s and the text "%s"; output in %s)\n' "$name" "$status" "$want_status" "$want_text" "$work/out/settings-$name.txt"
      failures=$((failures + 1))
    fi
  }

  push_to_main() {
    expect_event "$1" "$2" "$3" GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main BEFORE="$4" AFTER="$5" PR_TITLE= PR_BODY=
  }
  push_to_branch() {
    expect_event "$1" "$2" "$3" GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/feature BEFORE="$4" AFTER="$5" PR_TITLE= PR_BODY=
  }
  pull_request() {
    expect_event "$1" "$2" "$3" GITHUB_EVENT_NAME=pull_request GITHUB_REF=refs/pull/7/merge GITHUB_BASE_REF=main BEFORE= AFTER="$4" PR_TITLE="$5" PR_BODY="$6"
  }

  runner push-to-main-with-a-valid-before-value "$main_with_clean_commit" "$main_with_clean_commit"
  push_to_main push-to-main-with-a-valid-before-value 0 'commits checked 1,' "$main" "$main_with_clean_commit"

  runner push-to-main-of-a-squash-with-a-trailer "$main_with_bad_squash" "$main_with_bad_squash"
  push_to_main push-to-main-of-a-squash-with-a-trailer 1 'an agent does not rewrite the default branch' "$main" "$main_with_bad_squash"

  runner push-that-creates-a-branch "$main" "$clean_branch" "$clean_branch:refs/heads/feature"
  push_to_branch push-that-creates-a-branch 0 'the copy of this branch read 2 commits, and the copy of the default branch read 2' "$no_commit" "$clean_branch"

  runner push-that-creates-a-branch-with-a-trailer "$main" "$bad_branch" "$bad_branch:refs/heads/feature"
  push_to_branch push-that-creates-a-branch-with-a-trailer 1 'commits checked 2, with an attribution line 1' "$no_commit" "$bad_branch"

  runner force-push-to-a-branch "$main" "$clean_branch" "$clean_branch:refs/heads/feature"
  push_to_branch force-push-to-a-branch 0 'commits checked 2,' "$commit_that_no_clone_has" "$clean_branch"

  runner force-push-to-main "$main_with_clean_commit" "$main_with_clean_commit"
  push_to_main force-push-to-main 0 'is not a commit in this clone' "$commit_that_no_clone_has" "$main_with_clean_commit"

  runner force-push-to-main-of-a-squash-with-a-trailer "$main_with_bad_squash" "$main_with_bad_squash"
  push_to_main force-push-to-main-of-a-squash-with-a-trailer 1 'an agent does not rewrite the default branch' "$commit_that_no_clone_has" "$main_with_bad_squash"

  runner pull-request "$main" "$clean_branch" "$clean_branch:refs/heads/feature"
  pull_request pull-request 0 'for the copy of this branch and for the copy of the default branch' "$clean_branch" 'Add two clean changes' 'A plain description.'
  expect_event pull-request 0 'the copy of this branch read 2 commits, and the copy of the default branch read 2' GITHUB_EVENT_NAME=pull_request GITHUB_REF=refs/pull/7/merge BEFORE= AFTER="$clean_branch" PR_TITLE='Add two clean changes' PR_BODY=

  runner pull-request-with-a-trailer-commit "$main" "$bad_branch" "$bad_branch:refs/heads/feature"
  pull_request pull-request-with-a-trailer-commit 1 'commits checked 2, with an attribution line 1' "$bad_branch" 'Fix the gate' 'A plain description.'

  runner pull-request-that-checks-0-commits "$main" "$main"
  pull_request pull-request-that-checks-0-commits 1 'the pull request run checked 0 commits' "$main" 'Nothing' 'A plain description.'

  runner pull-request-whose-description-ends-with-a-tool-footer "$main" "$clean_branch" "$clean_branch:refs/heads/feature"
  pull_request pull-request-whose-description-ends-with-a-tool-footer 1 'then close and reopen the pull request. That starts a new pull_request run, which reads the new text; a re-run reads the old text' "$clean_branch" 'Add two clean changes' "A plain description.$nl$nl$footer"

  runner tag-named-origin-main-on-a-trailer-commit "$main" "$bad_branch" "$bad_branch:refs/heads/feature" "$bad_branch:refs/tags/origin/main"
  pull_request tag-named-origin-main-on-a-trailer-commit 1 'commits checked 2, with an attribution line 1' "$bad_branch" 'Fix the gate' 'A plain description.'

  runner tag-named-origin-main-on-a-clean-commit "$main" "$clean_branch" "$clean_branch:refs/heads/feature" "$clean_branch:refs/tags/origin/main"
  push_to_branch tag-named-origin-main-on-a-clean-commit 0 'commits checked 2,' "$no_commit" "$clean_branch"

  runner branch-that-moved-the-exempt-commit "$main" "$branch_that_moved_the_exempt_commit" "$branch_that_moved_the_exempt_commit:refs/heads/feature"
  pull_request branch-that-moved-the-exempt-commit 1 'commits checked 2, with an attribution line 1' "$branch_that_moved_the_exempt_commit" 'Fix the gate' 'A plain description.'

  start_at "$main"
  awk '{ print } $0 == "check_stored_message() {" { print "  exit 0" }' "$root/$CHECK" >"$dev/$CHECK"
  commit_edited_check_script 'Blind the check of a stored message'
  branch_that_blinded_the_stored_message_check=$(tip)
  runner branch-that-blinded-the-check-of-the-pull-request-text "$main" "$branch_that_blinded_the_stored_message_check" "$branch_that_blinded_the_stored_message_check:refs/heads/feature"
  pull_request branch-that-blinded-the-check-of-the-pull-request-text 1 'the control for --stored-message did not fail' "$branch_that_blinded_the_stored_message_check" 'Fix the gate' "A plain description.$nl$nl$footer"

  start_at "$main"
  awk -v added="  case \$1 in *\"[Claude Code]\"*) return 0 ;; esac" '{ print } $0 == "report_message() {" { print added }' "$root/$CHECK" >"$dev/$CHECK"
  commit_edited_check_script 'Accept the footer of one tool in a stored message'
  branch_that_accepts_the_footer_of_one_tool=$(tip)
  runner branch-that-accepts-the-footer-of-one-tool-in-the-pull-request-text "$main" "$branch_that_accepts_the_footer_of_one_tool" "$branch_that_accepts_the_footer_of_one_tool:refs/heads/feature"
  pull_request branch-that-accepts-the-footer-of-one-tool-in-the-pull-request-text 1 'refuses the title or the description of the pull request' "$branch_that_accepts_the_footer_of_one_tool" 'Fix the gate' "A plain description.$nl$nl$footer"

  start_at "$main"
  awk -v added="  case \$1 in *Generated*) return 0 ;; esac" '{ print } $0 == "report_message() {" { print added }' "$root/$CHECK" >"$dev/$CHECK"
  commit_edited_check_script 'Accept every tool footer in a stored message'
  branch_that_accepts_every_tool_footer=$(tip)

  runner branch-that-accepts-every-tool-footer-while-main-holds-the-older-script "$commit_with_the_predicate_before_this_change" "$branch_that_accepts_every_tool_footer" "$branch_that_accepts_every_tool_footer:refs/heads/feature"
  pull_request branch-that-accepts-every-tool-footer-while-main-holds-the-older-script 1 'the control for --stored-message did not fail' "$branch_that_accepts_every_tool_footer" 'Fix the gate' "A plain description.$nl$nl$footer"

  runner pull-request-into-another-base-with-no-commit-beyond-main "$main" "$main"
  expect_event pull-request-into-another-base-with-no-commit-beyond-main 0 'commits checked 0,' GITHUB_EVENT_NAME=pull_request GITHUB_REF=refs/pull/8/merge GITHUB_BASE_REF=release BEFORE= AFTER="$main" PR_TITLE='Merge main into release' PR_BODY='A plain description.'

  start_at "$main"
  commit "Bump the release tool${nl}${nl}2026-11-09: generated with cargo-release 1.1.7"
  branch_with_a_line_that_only_the_older_predicate_refuses=$(tip)
  runner default-branch-with-the-older-predicate "$commit_with_the_predicate_before_this_change" "$branch_with_a_line_that_only_the_older_predicate_refuses" "$branch_with_a_line_that_only_the_older_predicate_refuses:refs/heads/feature"
  push_to_branch default-branch-with-the-older-predicate 1 "$default_ref_in_runner holds an older predicate" "$no_commit" "$branch_with_a_line_that_only_the_older_predicate_refuses"

  runner branch-that-blinded-the-range-check "$main" "$branch_that_blinded_the_range_check" "$branch_that_blinded_the_range_check:refs/heads/feature"
  push_to_branch branch-that-blinded-the-range-check 1 'the control did not fail' "$no_commit" "$branch_that_blinded_the_range_check"

  start_at "$main"
  awk '{ print } $0 == "commit_range() {" { print "  case \"$*\" in *..*) return 0 ;; esac" }' "$root/scripts/commit-range.sh" >"$dev/scripts/commit-range.sh"
  cmp -s "$root/scripts/commit-range.sh" "$dev/scripts/commit-range.sh" && die "the self-test could not edit scripts/commit-range.sh for 'Give no commit to a range with two dots': the line that it follows has changed"
  git -C "$dev" add scripts/commit-range.sh
  commit 'Give no commit to a range with two dots'
  commit "Fix the gate$nl$nl$trailer"
  branch_that_emptied_its_ranges=$(tip)
  runner branch-whose-range-function-gives-its-range-no-commit "$main" "$branch_that_emptied_its_ranges" "$branch_that_emptied_its_ranges:refs/heads/feature"
  push_to_branch branch-whose-range-function-gives-its-range-no-commit 1 'holds refuses this range' "$no_commit" "$branch_that_emptied_its_ranges"

  expect_text_run() {
    name=$1 want_status=$2 want_text=$3 run_name=$4 text_commit=$5
    shift 5
    status=0
    script_text=$(git -C "$work/$name" show "$text_commit:$CHECK") || die "the runner clone of $name holds no $CHECK at $text_commit"
    (cd "$work/$name" && sh -c "$script_text" "$run_name" "$@") >"$work/out/$name.txt" 2>&1 || status=$?
    shapes=$((shapes + 1))
    if [ "$status" = "$want_status" ] && grep -qF -- "$want_text" "$work/out/$name.txt"; then
      printf 'PASS %s (exit %s)\n' "$name" "$status"
    else
      printf 'FAIL %s (exit %s, expected exit %s and the text "%s"; output in %s)\n' "$name" "$status" "$want_status" "$want_text" "$work/out/$name.txt"
      failures=$((failures + 1))
    fi
  }
  plant_range_files() {
    start_at "$main"
    cp "$root/scripts/commit-range.sh" "$dev/commit-range.sh"
    printf '%s\n' "$1" >>"$dev/commit-range.sh"
    cp "$root/scripts/commit-range.sh" "$dev/scripts/commit-range.sh"
    printf '%s\n' "$2" >>"$dev/scripts/commit-range.sh"
    printf '#!/bin/sh\nprintf "check-commit-messages: PASS: planted: commits checked 1, with an attribution line 0 (planted).\\n"\n' >"$dev/check-commit-messages.sh"
    git -C "$dev" add commit-range.sh check-commit-messages.sh scripts/commit-range.sh
    commit "Plant range files at the root$nl${nl}$3"
    commit "Fix the gate$nl$nl$trailer"
    tip
  }
  d='$'
  every_range_empty="commit_range() { case \"${d}*\" in *..*) return 0 ;; esac; for commit_range_last do :; done; git rev-list --reverse --end-of-options \"${d}commit_range_last\" -- </dev/null || return 2; }"
  without_the_newest="commit_range() { for commit_range_last do :; done; git rev-list --reverse --end-of-options \"${d}commit_range_last\" -- </dev/null | sed '${d}d'; }"
  branch_with_empty_ranges_at_the_root=$(plant_range_files "$every_range_empty" "$every_range_empty" 'Every range with two dots is empty.')
  runner branch-that-plants-range-files-with-every-range-empty "$main" "$branch_with_empty_ranges_at_the_root" "$branch_with_empty_ranges_at_the_root:refs/heads/feature"
  push_to_branch branch-that-plants-range-files-with-every-range-empty 1 'holds refuses this range' "$no_commit" "$branch_with_empty_ranges_at_the_root"
  branch_without_its_newest_commit=$(plant_range_files "$without_the_newest" "$without_the_newest" 'Each range leaves out its newest commit.')
  runner branch-that-plants-range-files-without-the-newest-commit "$main" "$branch_without_its_newest_commit" "$branch_without_its_newest_commit:refs/heads/feature"
  push_to_branch branch-that-plants-range-files-without-the-newest-commit 1 'holds refuses this range' "$no_commit" "$branch_without_its_newest_commit"

  runner default-branch-with-the-copy-before-the-range-file "$copy_before_the_range_file" "$clean_branch" "$clean_branch:refs/heads/feature"
  push_to_branch default-branch-with-the-copy-before-the-range-file 0 'and the copy of the default branch read' "$no_commit" "$clean_branch"

  runner text-with-a-bare-name-reads-the-default-branch "$main" "$branch_with_empty_ranges_at_the_root" "$branch_with_empty_ranges_at_the_root:refs/heads/feature"
  expect_text_run text-with-a-bare-name-reads-the-default-branch 1 'with an attribution line 1' check-commit-messages.sh refs/remotes/origin/main refs/remotes/origin/main..HEAD

  start_at "$main"
  printf '%s\n' "$every_range_empty" >>"$dev/scripts/commit-range.sh"
  git -C "$dev" add scripts/commit-range.sh
  commit 'Give no commit to a range with two dots on the default branch'
  main_with_empty_ranges=$(tip)
  commit "Fix the gate$nl$nl$trailer"
  branch_on_a_main_with_empty_ranges=$(tip)
  runner text-named-for-a-commit-reads-that-commit "$main_with_empty_ranges" "$branch_on_a_main_with_empty_ranges" "$branch_on_a_main_with_empty_ranges:refs/heads/feature"
  git -C "$work/text-named-for-a-commit-reads-that-commit" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  expect_text_run text-named-for-a-commit-reads-that-commit 1 'with an attribution line 1' "check-commit-messages.sh@$main" "$main" refs/remotes/origin/main..HEAD

  start_at "$main"
  git -C "$dev" update-index --force-remove "$AGENT_SETTINGS"
  commit 'Delete the agent settings'
  branch_without_the_agent_settings=$(tip)
  runner branch-without-the-agent-settings "$main" "$branch_without_the_agent_settings" "$branch_without_the_agent_settings:refs/heads/feature"
  push_to_branch branch-without-the-agent-settings 1 '.claude/settings.json is missing' "$no_commit" "$branch_without_the_agent_settings"

  as_decided='{"attribution": {"commit": "", "pr": "", "sessionUrl": false}}'
  expect_settings as-decided 0 '' "$as_decided"
  expect_settings with-other-keys 0 '' '{"permissions": {"allow": ["Bash(git status)"]}, "attribution": {"commit": "", "pr": "", "sessionUrl": false}, "includeCoAuthoredBy": false}'
  expect_settings missing 1 'is missing'
  expect_settings commit-not-empty 1 'attribution.commit is not the empty string' '{"attribution": {"commit": "Co-Authored-By: Claude <noreply@anthropic.com>", "pr": "", "sessionUrl": false}}'
  expect_settings pr-not-empty 1 'attribution.pr is not the empty string' '{"attribution": {"commit": "", "pr": "Generated with Claude Code", "sessionUrl": false}}'
  expect_settings pr-absent 1 'attribution.pr is not the empty string' '{"attribution": {"commit": "", "sessionUrl": false}}'
  expect_settings session-url-absent 1 'attribution.sessionUrl is not false' '{"attribution": {"commit": "", "pr": ""}}'
  expect_settings session-url-true 1 'attribution.sessionUrl is not false' '{"attribution": {"commit": "", "pr": "", "sessionUrl": true}}'
  expect_settings boolean-form 1 'attribution is not an object' '{"attribution": false}'
  expect_settings no-attribution-key 1 'attribution is not an object' '{"permissions": {}}'
  expect_settings older-key-true 1 'includeCoAuthoredBy is not false' '{"attribution": {"commit": "", "pr": "", "sessionUrl": false}, "includeCoAuthoredBy": true}'
  expect_settings not-json 1 'is not valid JSON' '{"attribution": {"commit": "", "pr": ""'
  expect_settings empty 1 'is not valid JSON' ''
  expect_settings two-values 1 'is not valid JSON' "$as_decided$nl$as_decided"
  expect_settings number-with-a-leading-zero 1 'is not valid JSON' '{"attribution": {"commit": "", "pr": "", "sessionUrl": false}, "cleanupPeriodDays": 030}'
  expect_settings number-that-is-not-a-number 1 'is not valid JSON' '{"attribution": {"commit": "", "pr": "", "sessionUrl": false}, "cleanupPeriodDays": NaN}'
  expect_settings not-an-object 1 'is not an object' '[]'
  expect_settings byte-order-mark 1 'starts with a byte order mark' "$(printf '\357\273\277')$as_decided"
  expect_settings duplicate-key-whose-last-value-is-wrong 1 'attribution.commit is not the empty string' '{"attribution": {"commit": "", "pr": "", "sessionUrl": false}, "attribution": {"commit": "x", "pr": "", "sessionUrl": false}}'

  if [ "$failures" -gt 0 ]; then
    say "self-test FAIL: $failures of $shapes event shapes and $settings_files settings files behaved otherwise. Outputs are in $work/out."
    return 1
  fi
  say "self-test PASS: $shapes event shapes and $settings_files settings files. Outputs are in $work/out."
}

case ${1-} in
  '') run_event ;;
  --self-test) self_test ;;
  -h|--help) help_text ;;
  *)
    printf 'ci-commit-messages: unknown argument: %s\n' "$1" >&2
    help_text >&2
    exit 64
    ;;
esac
