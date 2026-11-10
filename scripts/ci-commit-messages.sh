#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

CHECK=scripts/check-commit-messages.sh
AGENT_SETTINGS=.claude/settings.json

SETTINGS_PROBLEM='
  if length != 1 then "the file holds \(length) JSON values"
  else .[0] |
    if type != "object" then "the top level is not an object"
    elif (.attribution | type) != "object" then "the key attribution is not an object"
    elif .attribution.commit != "" then "attribution.commit is not the empty string"
    elif .attribution.pr != "" then "attribution.pr is not the empty string"
    elif .attribution.sessionUrl != false then "attribution.sessionUrl is not false"
    elif has("includeCoAuthoredBy") and .includeCoAuthoredBy != false then "includeCoAuthoredBy is not false"
    else empty end
  end'

help_text() {
  cat <<'EOF'
The CI step "Commit messages". It runs scripts/check-commit-messages.sh on the
commits of the event that started the workflow. No flag of git skips it.

  sh scripts/ci-commit-messages.sh
      Read the event from these variables, which the workflow step sets:
        GITHUB_EVENT_NAME  push or pull_request
        GITHUB_REF         the ref of the event, for example refs/heads/main
        DEFAULT_BRANCH     the default branch of the repository, for example main
        BEFORE             the tip of the ref before a push
        AFTER              the newest commit of the event: the head of the pull
                           request, or the pushed commit
        PR_TITLE, PR_BODY  the title and the description of the pull request
      Each step below can fail the job.
        1. The agent settings. The tracked file .claude/settings.json must hold
           "attribution": {"commit": "", "pr": "", "sessionUrl": false}, which
           stops the agent harness from adding its attribution lines, and no
           includeCoAuthoredBy other than false. Needs jq.
        2. The self-test of the predicate.
        3. A control. The range check must fail on the exempt commits when no
           commit is exempt. If it passes there, it is blind.
        4. The range check on the commits of the event. On the default branch
           these are the pushed commits. On every other ref they are the commits
           that refs/remotes/origin/<DEFAULT_BRANCH> does not have, and a pull
           request with 0 such commits fails.
        5. On every other ref, the same range with the copy of the check script
           that the default branch holds. A branch cannot move the exempt commit
           or weaken the predicate for its own commits.
        6. On a pull request, its title and description as one message.
  sh scripts/ci-commit-messages.sh --self-test
      Run those steps on event shapes and on settings files, in throwaway
      repositories under a new temporary directory that is left in place. Needs
      a clone of this repository and a configured git user.

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
  fail "the pull request run checked 0 commits in $range with $1. A pull request has at least one commit that $default_ref does not have, so the check read the wrong range, or the script exempts the commits of this branch. Compare NEWEST_EXEMPT_COMMIT in $CHECK with the default branch."
}

check_agent_settings() {
  settings_rule="$1 must hold \"attribution\": {\"commit\": \"\", \"pr\": \"\", \"sessionUrl\": false}. The empty values of commit and pr stop the agent harness from adding its co-author trailer and its pull request line, and sessionUrl false stops its session trailer."
  [ -f "$1" ] || fail "$1 is missing. $settings_rule Restore the tracked file."
  command -v jq >/dev/null 2>&1 || die "jq is not on PATH, and this step reads $1 with it. Install jq."
  if ! settings_problem=$(jq -r -s "$SETTINGS_PROBLEM" "$1" 2>&1); then
    fail "$1 is not valid JSON: $settings_problem. $settings_rule"
  fi
  [ -z "$settings_problem" ] ||
    fail "in $1, $settings_problem. $settings_rule Do not write \"attribution\": false: older versions of the harness reject it and skip the whole file."
  say "$1 turns the attribution lines of the agent harness off"
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
  if [ "$GITHUB_EVENT_NAME" = pull_request ] && [ "$head_count" = 0 ]; then
    fail_pull_request_without_commits "$CHECK"
  fi

  if [ "$GITHUB_REF" != "refs/heads/$DEFAULT_BRANCH" ]; then
    default_script=$(git show "$default_ref:$CHECK") ||
      die "$default_ref holds no $CHECK, so the range cannot be checked with the copy of the default branch"
    say "the same range, with the copy of $CHECK that $default_ref holds:"
    default_status=0
    default_output=$(sh -c "$default_script" check-commit-messages.sh "$range") || default_status=$?
    if [ "$default_status" != 0 ]; then
      fail "the copy of $CHECK that $default_ref holds refuses this range (exit $default_status, its findings are above), and the copy of this branch accepts it. The branch moved the exempt commit or weakened the predicate. Undo that change, then fix the listed commits."
    fi
    printf '%s\n' "$default_output"
    default_count=$(printf '%s\n' "$default_output" | commits_checked_on_pass_line)
    [ -n "$default_count" ] || fail "the copy of $CHECK that $default_ref holds exited 0 and printed no PASS line with a commit count, so nothing shows that it read the range."
    if [ "$GITHUB_EVENT_NAME" = pull_request ] && [ "$default_count" = 0 ]; then
      fail_pull_request_without_commits "the copy of $CHECK that $default_ref holds"
    fi
    say "the copy of this branch read $head_count commits, and the copy of the default branch read $default_count"
  fi

  if [ "$GITHUB_EVENT_NAME" = pull_request ]; then
    printf '%s\n\n%s\n' "${PR_TITLE-}" "${PR_BODY-}" | sh "$CHECK" --stored-message ||
      fail "the title or the description of the pull request carries an attribution line, listed above. GitHub offers that text as the squash message. Remove the line on GitHub. This job reads the text from the event that started it, so a re-run reads the old text: push a commit, or close and reopen the pull request."
    say 'the title and the description of the pull request carry no attribution line'
  fi
  say 'PASS'
}

self_test() {
  root=$(pwd)
  git rev-parse --git-dir >/dev/null 2>&1 || die 'the self-test clones this repository, and this directory is not inside one'
  [ -n "$(git config --get user.name || true)" ] || die 'the self-test commits in throwaway repositories and needs a configured git user'
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
  commit_that_no_clone_has=1111111111111111111111111111111111111111

  dev=$work/dev
  git init -q "$dev"
  git -C "$dev" fetch -q "$root" HEAD
  git -C "$dev" checkout -q -b main FETCH_HEAD

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
    expect_event "$1" "$2" "$3" GITHUB_EVENT_NAME=pull_request GITHUB_REF=refs/pull/7/merge BEFORE= AFTER="$4" PR_TITLE="$5" PR_BODY="$6"
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
  pull_request pull-request 0 'the title and the description of the pull request carry no attribution line' "$clean_branch" 'Add two clean changes' 'A plain description.'
  expect_event pull-request 0 'the copy of this branch read 2 commits, and the copy of the default branch read 2' GITHUB_EVENT_NAME=pull_request GITHUB_REF=refs/pull/7/merge BEFORE= AFTER="$clean_branch" PR_TITLE='Add two clean changes' PR_BODY=

  runner pull-request-with-a-trailer-commit "$main" "$bad_branch" "$bad_branch:refs/heads/feature"
  pull_request pull-request-with-a-trailer-commit 1 'commits checked 2, with an attribution line 1' "$bad_branch" 'Fix the gate' 'A plain description.'

  runner pull-request-that-checks-0-commits "$main" "$main"
  pull_request pull-request-that-checks-0-commits 1 'the pull request run checked 0 commits' "$main" 'Nothing' 'A plain description.'

  runner pull-request-whose-description-ends-with-a-tool-footer "$main" "$clean_branch" "$clean_branch:refs/heads/feature"
  pull_request pull-request-whose-description-ends-with-a-tool-footer 1 'the title or the description of the pull request carries an attribution line' "$clean_branch" 'Add two clean changes' "A plain description.$nl$nl$footer"

  runner tag-named-origin-main-on-a-trailer-commit "$main" "$bad_branch" "$bad_branch:refs/heads/feature" "$bad_branch:refs/tags/origin/main"
  pull_request tag-named-origin-main-on-a-trailer-commit 1 'commits checked 2, with an attribution line 1' "$bad_branch" 'Fix the gate' 'A plain description.'

  runner tag-named-origin-main-on-a-clean-commit "$main" "$clean_branch" "$clean_branch:refs/heads/feature" "$clean_branch:refs/tags/origin/main"
  push_to_branch tag-named-origin-main-on-a-clean-commit 0 'commits checked 2,' "$no_commit" "$clean_branch"

  runner branch-that-moved-the-exempt-commit "$main" "$branch_that_moved_the_exempt_commit" "$branch_that_moved_the_exempt_commit:refs/heads/feature"
  pull_request branch-that-moved-the-exempt-commit 1 'The branch moved the exempt commit or weakened the predicate' "$branch_that_moved_the_exempt_commit" 'Fix the gate' 'A plain description.'

  runner branch-that-blinded-the-range-check "$main" "$branch_that_blinded_the_range_check" "$branch_that_blinded_the_range_check:refs/heads/feature"
  push_to_branch branch-that-blinded-the-range-check 1 'the control did not fail' "$no_commit" "$branch_that_blinded_the_range_check"

  start_at "$main"
  git -C "$dev" rm -q --ignore-unmatch .claude/settings.json
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
  expect_settings empty 1 'holds 0 JSON values' ''
  expect_settings two-values 1 'holds 2 JSON values' "$as_decided$nl$as_decided"
  expect_settings not-an-object 1 'is not an object' '[]'

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
