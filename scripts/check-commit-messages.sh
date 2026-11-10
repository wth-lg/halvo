#!/bin/sh
# Rejects commit messages that carry an attribution line. Commits in this repository
# are authored by the configured git user only, so a message must not contain:
#   - a Co-Authored-By trailer: a line that starts with "Co-Authored-By:"
#   - a "Generated with" attribution: a line whose first words are "Generated with"
# Letter case and leading whitespace do not matter. A line that mentions either
# phrase after other words passes.
#
#   sh scripts/check-commit-messages.sh [--no-exempt] [<range>]
#       Check every commit that `git rev-list <range>` lists (default: HEAD).
#       Commits reachable from BASELINE are exempt; --no-exempt checks them too.
#   sh scripts/check-commit-messages.sh --message-file <file>
#       Check one message. .githooks/commit-msg calls this.
#   sh scripts/check-commit-messages.sh --self-test
#       Prove the predicate on fixtures. Needs no repository.
#
# Exit codes: 0 no attribution line, 1 at least one, 2 the check could not run,
# 64 bad usage. The script only reads, so it is safe to run twice.
set -eu

BASELINE=685d84637c302976956d56e6bbb5c18a4503e7c8

RULE='Commits in this repository are authored by the configured git user only. See AGENTS.md.'

die() {
  printf 'check-commit-messages: error: %s\n' "$1" >&2
  exit 2
}

usage() {
  printf 'check-commit-messages: %s\n' "$1" >&2
  sed -n '9,15p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 64
}

attribution_lines() {
  LC_ALL=C awk -v scissors="$1" '
    scissors && index($0, "------------------------ >8 ------------------------") { exit }
    { line = tolower($0) }
    line ~ /^[ \t]*co-authored-by[ \t]*:/ || line ~ /^[^a-z]*generated with([^a-z]|$)/ {
      printf "  line %d: %s\n", NR, $0
      found = 1
    }
    END { exit found }
  '
}

scan() {
  status=0
  lines=$(printf '%s\n' "$2" | attribution_lines "$1") || status=$?
  case $status in
    0|1) ;;
    *) die "awk exited $status while reading a message" ;;
  esac
}

check_file() {
  [ -f "$1" ] || die "no such message file: $1"
  message=$(cat "$1") || die "cannot read the message file: $1"
  scan 1 "$message"
  [ "$status" = 1 ] || return 0
  {
    printf 'check-commit-messages: the commit message carries an attribution line:\n%s\n' "$lines"
    printf 'Fix: remove the line and commit again. If the line is prose, reword it so that it does not start with these words. %s\n' "$RULE"
  } >&2
  return 1
}

check_range() {
  git rev-parse --git-dir >/dev/null 2>&1 || die 'not inside a git repository'
  if [ "$exempt" = 1 ]; then
    git cat-file -e "$BASELINE^{commit}" 2>/dev/null ||
      die "the baseline commit $BASELINE is not in this clone, so exempt commits cannot be told from new ones. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
    scope="commits reachable from the baseline $(git rev-parse --short "$BASELINE") are exempt"
    commits=$(git rev-list --reverse --end-of-options "$1" "^$BASELINE") ||
      die "git rev-list could not list the commits of '$1'"
  else
    scope='no commit is exempt'
    commits=$(git rev-list --reverse --end-of-options "$1") ||
      die "git rev-list could not list the commits of '$1'"
  fi

  total=0
  bad=0
  for commit in $commits; do
    total=$((total + 1))
    message=$(git log -1 --format=%B "$commit") || die "git log could not read $commit"
    scan 0 "$message"
    if [ "$status" = 1 ]; then
      bad=$((bad + 1))
      printf 'check-commit-messages: %s\n%s\n' "$(git log -1 --format='%h %s' "$commit")" "$lines" >&2
    fi
  done

  if [ "$bad" -gt 0 ]; then
    {
      printf 'check-commit-messages: FAIL: %s: commits checked %d, with an attribution line %d (%s).\n' "$1" "$total" "$bad" "$scope"
      printf 'Fix: remove each listed line from its commit message (git commit --amend for the newest commit, git rebase -i for an older one). %s\n' "$RULE"
      printf 'Run sh scripts/prerequisites.sh once: it turns on the commit-msg hook that stops such a commit before it is made.\n'
    } >&2
    return 1
  fi
  printf 'check-commit-messages: PASS: %s: commits checked %d, with an attribution line 0 (%s).\n' "$1" "$total" "$scope"
}

self_test() {
  failures=0
  nl='
'
  generated='🤖 Generated with [Claude Code](https://claude.com/claude-code)'
  trailer='Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>'
  diff_below_scissors="Fix the gate${nl}# ------------------------ >8 ------------------------${nl}+// Generated with bindgen${nl} $trailer"

  expect() {
    scan "$3" "$4"
    if [ -z "$lines" ]; then count=0; else count=$(printf '%s\n' "$lines" | wc -l | tr -d ' '); fi
    want_status=1
    [ "$2" != 0 ] || want_status=0
    if [ "$count" = "$2" ] && [ "$status" = "$want_status" ]; then
      printf 'PASS %s\n' "$1"
    else
      printf 'FAIL %s (expected %s attribution lines and status %s, found %s and status %s)\n' "$1" "$2" "$want_status" "$count" "$status"
      failures=$((failures + 1))
    fi
  }

  expect 'a clean message passes' 0 0 "Add the store${nl}${nl}Stream segment files through bounded chunks."
  expect 'a Co-Authored-By trailer fails' 1 0 "Fix the gate${nl}${nl}$trailer"
  expect 'a lower-case co-authored-by trailer fails' 1 0 "Fix the gate${nl}${nl}co-authored-by: Claude <noreply@anthropic.com>"
  expect 'a mixed-case, indented trailer with a space before the colon fails' 1 0 "Fix the gate${nl}${nl}  cO-aUtHoReD-bY : Claude <noreply@anthropic.com>"
  expect 'a Generated with line after an emoji fails' 1 0 "Fix the gate${nl}${nl}$generated"
  expect 'a mixed-case gEnErAtEd WiTh line fails' 1 0 "Fix the gate${nl}${nl}gEnErAtEd WiTh Claude Code"
  expect 'both lines of the baseline commit fail, one finding each' 2 0 "Fix the gate${nl}${nl}$generated${nl}${nl}$trailer"
  expect 'prose that mentions both phrases passes' 0 0 "Reject attribution lines${nl}${nl}The check fails on a Co-Authored-By: trailer and on a line that says a${nl}tool was Generated with. A message that only mentions them passes."
  expect 'a line that starts with Generated without passes' 0 0 "Fix the gate${nl}${nl}Generated without a tool."
  expect 'the diff below the scissors line passes in a message file' 0 1 "$diff_below_scissors"
  expect 'the same text fails when it is a stored commit message' 2 0 "$diff_below_scissors"
  expect 'a trailer above the scissors line fails in a message file' 1 1 "Fix the gate${nl}${nl}$trailer${nl}# ------------------------ >8 ------------------------${nl}+// Generated with bindgen"

  if [ "$failures" -gt 0 ]; then
    printf 'check-commit-messages: self-test FAIL: %d fixtures misclassified\n' "$failures"
    return 1
  fi
  printf 'check-commit-messages: self-test PASS\n'
}

mode=
exempt=1
range=
file=
pick() {
  [ -z "$mode" ] || [ "$mode" = "$1" ] || usage 'a range, --message-file and --self-test exclude each other'
  mode=$1
}
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) pick self-test ;;
    --message-file)
      [ $# -ge 2 ] || usage '--message-file needs a file'
      [ -z "$file" ] || usage "more than one message file: $file and $2"
      pick message
      file=$2
      shift
      ;;
    --no-exempt) pick range; exempt=0 ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) usage "unknown option: $1" ;;
    '') usage 'the range is empty' ;;
    *)
      pick range
      [ -z "$range" ] || usage "more than one range: $range and $1"
      range=$1
      ;;
  esac
  shift
done

case ${mode:-range} in
  self-test) self_test ;;
  message) check_file "$file" ;;
  range) check_range "${range:-HEAD}" ;;
esac
