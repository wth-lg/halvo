#!/bin/sh
set -eu

NEWEST_EXEMPT_COMMIT=685d84637c302976956d56e6bbb5c18a4503e7c8

RULE='Commits in this repository are authored by the configured git user only. See AGENTS.md.'

SCISSORS='------------------------ >8 ------------------------'

synopsis() {
  cat <<'EOF'
  sh scripts/check-commit-messages.sh [--no-exempt] [<range>]
      Check every commit that `git rev-list <range>` lists (default: HEAD).
      Commits reachable from NEWEST_EXEMPT_COMMIT are exempt; --no-exempt checks them too.
  sh scripts/check-commit-messages.sh --message-file <file>
      Check one message. .githooks/commit-msg calls this.
  sh scripts/check-commit-messages.sh --self-test
      Prove the predicate on fixtures. Needs no repository.
EOF
}

help_text() {
  cat <<'EOF'
Rejects commit messages that carry an attribution line. Commits in this repository
are authored by the configured git user only, so a message must not contain:
  - a Co-Authored-By trailer: a line that starts with "Co-Authored-By:"
  - a "Generated with" attribution: a line whose first words are "Generated with"
Letter case and leading whitespace do not matter. A line that mentions either
phrase after other words passes.

EOF
  synopsis
  cat <<'EOF'

Exit codes: 0 no attribution line, 1 at least one, 2 the check could not run,
64 bad usage. The script only reads, so it is safe to run twice.
EOF
}

die() {
  printf 'check-commit-messages: error: %s\n' "$1" >&2
  exit 2
}

usage() {
  printf 'check-commit-messages: %s\n' "$1" >&2
  synopsis >&2
  exit 64
}

awk_failed() {
  die "awk exited $1 while reading a message"
}

drop_commit_verbose_diff() {
  printf '%s\n' "$1" | LC_ALL=C awk -v scissors_end=" $SCISSORS" '
    dropping_diff { next }
    maybe_scissors_block != "" {
      if (index($0, "diff --git ") == 1) { dropping_diff = 1; next }
      if (index($0, comment_string) == 1) { maybe_scissors_block = maybe_scissors_block $0 "\n"; next }
      printf "%s", maybe_scissors_block
      maybe_scissors_block = ""
    }
    {
      comment_length = length($0) - length(scissors_end)
      if (comment_length > 0 && substr($0, comment_length + 1) == scissors_end) {
        comment_string = substr($0, 1, comment_length)
        maybe_scissors_block = $0 "\n"
        next
      }
      print
    }
    END { if (!dropping_diff) printf "%s", maybe_scissors_block }
  '
}

attribution_lines() {
  printf '%s\n' "$1" | LC_ALL=C awk '
    { line = tolower($0) }
    line ~ /^[ \t]*co-authored-by[ \t]*:/ || line ~ /^[^a-z]*generated with([^a-z]|$)/ {
      printf "  line %d: %s\n", NR, $0
    }
  '
}

check_file() {
  [ -f "$1" ] || die "no such message file: $1"
  hook_file=$(cat "$1") || die "cannot read the message file: $1"
  message=$(drop_commit_verbose_diff "$hook_file") || awk_failed $?
  found=$(attribution_lines "$message") || awk_failed $?
  [ -n "$found" ] || return 0
  {
    printf 'check-commit-messages: the commit message carries an attribution line:\n%s\n' "$found"
    printf 'Fix: remove the line and commit again. If the line is prose, reword it so that it does not start with these words. %s\n' "$RULE"
  } >&2
  return 1
}

check_range() {
  git rev-parse --git-dir >/dev/null 2>&1 || die 'not inside a git repository'
  if [ "$exempt" = 1 ]; then
    git cat-file -e "$NEWEST_EXEMPT_COMMIT^{commit}" 2>/dev/null ||
      die "the baseline commit $NEWEST_EXEMPT_COMMIT is not in this clone, so exempt commits cannot be told from new ones. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
    scope="commits reachable from the baseline $(git rev-parse --short "$NEWEST_EXEMPT_COMMIT") are exempt"
    commits=$(git rev-list --reverse --end-of-options "$1" "^$NEWEST_EXEMPT_COMMIT") ||
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
    found=$(attribution_lines "$message") || awk_failed $?
    if [ -n "$found" ]; then
      bad=$((bad + 1))
      printf 'check-commit-messages: %s\n%s\n' "$(git log -1 --format='%h %s' "$commit")" "$found" >&2
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
  scissors_block="# $SCISSORS${nl}# Do not modify or remove the line above.${nl}# Everything below it will be ignored."
  diff_with_attribution_text="diff --git a/bindings.rs b/bindings.rs${nl}--- a/bindings.rs${nl}+++ b/bindings.rs${nl}@@ -1 +1,2 @@${nl} $trailer${nl}+// Generated with bindgen"
  commit_verbose_file="Fix the gate${nl}${nl}# Please enter the commit message for your changes.${nl}#${nl}$scissors_block${nl}$diff_with_attribution_text"
  semicolon_commit_verbose_file="Fix the gate${nl}${nl}; $SCISSORS${nl}; Do not modify or remove the line above.${nl}; Everything below it will be ignored.${nl}$diff_with_attribution_text"

  expect() {
    name=$1 want_count=$2 message=$3
    found=$(attribution_lines "$message") || awk_failed $?
    if [ -z "$found" ]; then count=0; else count=$(printf '%s\n' "$found" | wc -l | tr -d ' '); fi
    if [ "$count" = "$want_count" ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected %s attribution lines, found %s)\n' "$name" "$want_count" "$count"
      failures=$((failures + 1))
    fi
  }

  expect_in_hook_file() {
    name=$1 want_count=$2 hook_file=$3
    message=$(drop_commit_verbose_diff "$hook_file") || awk_failed $?
    expect "$name" "$want_count" "$message"
  }

  expect 'a clean message passes' 0 "Add the store${nl}${nl}Stream segment files through bounded chunks."
  expect 'a Co-Authored-By trailer fails' 1 "Fix the gate${nl}${nl}$trailer"
  expect 'a lower-case co-authored-by trailer fails' 1 "Fix the gate${nl}${nl}co-authored-by: Claude <noreply@anthropic.com>"
  expect 'a mixed-case, indented trailer with a space before the colon fails' 1 "Fix the gate${nl}${nl}  cO-aUtHoReD-bY : Claude <noreply@anthropic.com>"
  expect 'a Generated with line after an emoji fails' 1 "Fix the gate${nl}${nl}$generated"
  expect 'a mixed-case gEnErAtEd WiTh line fails' 1 "Fix the gate${nl}${nl}gEnErAtEd WiTh Claude Code"
  expect 'both lines of the baseline commit fail, one finding each' 2 "Fix the gate${nl}${nl}$generated${nl}${nl}$trailer"
  expect 'prose that mentions both phrases passes' 0 "Reject attribution lines${nl}${nl}The check fails on a Co-Authored-By: trailer and on a line that says a${nl}tool was Generated with. A message that only mentions them passes."
  expect 'a line that starts with Generated without passes' 0 "Fix the gate${nl}${nl}Generated without a tool."
  expect 'the diff of git commit -v fails when it is a stored commit message' 2 "$commit_verbose_file"
  expect_in_hook_file 'the diff below the scissors block of git commit -v is dropped from the hook file' 0 "$commit_verbose_file"
  expect_in_hook_file 'the same diff is dropped under another comment character' 0 "$semicolon_commit_verbose_file"
  expect_in_hook_file 'a trailer above the scissors block fails in the hook file' 1 "Fix the gate${nl}${nl}$trailer${nl}$scissors_block${nl}$diff_with_attribution_text"
  expect_in_hook_file 'a trailer below a scissors line that no diff follows fails in the hook file' 1 "Fix the gate${nl}${nl}# $SCISSORS${nl}$trailer"
  expect_in_hook_file 'a trailer below a scissors block that no diff follows fails in the hook file' 1 "Fix the gate${nl}${nl}$scissors_block${nl}$trailer"
  expect_in_hook_file 'a trailer between a scissors line and a diff header fails in the hook file' 1 "Fix the gate${nl}${nl}# $SCISSORS${nl}$trailer${nl}diff --git a/bindings.rs b/bindings.rs"
  expect_in_hook_file 'a trailer below prose that holds the scissors text fails in the hook file' 1 "Fix the gate${nl}${nl}The hook stopped at $SCISSORS before.${nl}$trailer"

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
set_exclusive_mode() {
  [ -z "$mode" ] || [ "$mode" = "$1" ] || usage 'a range, --message-file and --self-test exclude each other'
  mode=$1
}
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) set_exclusive_mode self-test ;;
    --message-file)
      [ $# -ge 2 ] || usage '--message-file needs a file'
      [ -z "$file" ] || usage "more than one message file: $file and $2"
      set_exclusive_mode message
      file=$2
      shift
      ;;
    --no-exempt) set_exclusive_mode range; exempt=0 ;;
    -h|--help) help_text; exit 0 ;;
    -*) usage "unknown option: $1" ;;
    '') usage 'the range is empty' ;;
    *)
      set_exclusive_mode range
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
