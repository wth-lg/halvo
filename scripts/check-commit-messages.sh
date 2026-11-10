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

git_comment_string() {
  configured=$(git config --get-regexp '^core\.comment(char|string)$' 2>/dev/null | sed -n '$s/^[^ ]* //p')
  printf '%s\n' "${configured:-#}"
}

drop_commit_verbose_diff() {
  printf '%s\n' "$2" | LC_ALL=C awk -v git_comment_string="$1" -v scissors_end=" $SCISSORS" '
    function scissors_line_comment_string(line,    comment_length, candidate) {
      comment_length = length(line) - length(scissors_end)
      if (comment_length < 1 || substr(line, comment_length + 1) != scissors_end) return ""
      candidate = substr(line, 1, comment_length)
      if (candidate == git_comment_string) return candidate
      if (tolower(git_comment_string) == "auto" && comment_length == 1 && index("#;@!$%^&|:", candidate)) return candidate
      return ""
    }
    function print_scissors_block(    i) {
      for (i = 1; i <= scissors_block_lines; i++) print scissors_block[i]
      scissors_block_lines = 0
    }
    dropping_diff { next }
    scissors_block_lines > 0 {
      if (index($0, "diff --git ") == 1) { dropping_diff = 1; next }
      if ($0 == "" || index($0, comment_string) == 1) { scissors_block[++scissors_block_lines] = $0; next }
      print_scissors_block()
    }
    {
      comment_string = scissors_line_comment_string($0)
      if (comment_string != "") { scissors_block[++scissors_block_lines] = $0; next }
      print
    }
    END { if (!dropping_diff) print_scissors_block() }
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
  hook_file_text=$(cat "$1") || die "cannot read the message file: $1"
  message=$(drop_commit_verbose_diff "$(git_comment_string)" "$hook_file_text") || awk_failed $?
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
      die "the newest exempt commit $NEWEST_EXEMPT_COMMIT is not in this clone, so exempt commits cannot be told from new ones. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
    scope="$(git rev-parse --short "$NEWEST_EXEMPT_COMMIT") and the commits reachable from it are exempt"
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
  fixtures=0
  nl='
'
  generated='🤖 Generated with [Claude Code](https://claude.com/claude-code)'
  trailer='Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>'
  scissors_block="# $SCISSORS${nl}# Do not modify or remove the line above.${nl}# Everything below it will be ignored."
  diff_with_attribution_text="diff --git a/bindings.rs b/bindings.rs${nl}--- a/bindings.rs${nl}+++ b/bindings.rs${nl}@@ -1 +1,2 @@${nl} $trailer${nl}+// Generated with bindgen"
  commit_verbose_file="Fix the gate${nl}${nl}# Please enter the commit message for your changes.${nl}#${nl}$scissors_block${nl}$diff_with_attribution_text"
  semicolon_commit_verbose_file="Fix the gate${nl}${nl}; $SCISSORS${nl}; Do not modify or remove the line above.${nl}; Everything below it will be ignored.${nl}$diff_with_attribution_text"
  tab=$(printf '\t')
  carriage_return=$(printf '\r')
  no_break_space=$(printf '\302\240')
  byte_that_is_not_utf8=$(printf '\351')
  backslash=$(printf '\134')
  claude_session='Claude-Session: https://claude.ai/code/session_0000000000000000000000'
  agent_logs_url='Agent-Logs-Url: https://github.com/example/example/sessions/00000000-0000-0000-0000-000000000000'
  amp_thread_id='Amp-Thread-ID: https://ampcode.com/threads/T-00000000-0000-0000-0000-000000000000'
  assisted_by='Assisted-by: GPT-5 via Crush <crush@charm.land>'

  commit_verbose_file_with_comment_string() {
    printf 'Fix the gate\n\n%s Please enter the commit message for your changes.\n%s %s\n%s Do not modify or remove the line above.\n%s Everything below it will be ignored.\n%s\n' \
      "$1" "$1" "$SCISSORS" "$1" "$1" "$diff_with_attribution_text"
  }

  expect() {
    name=$1 want_count=$2 message=$3
    found=$(attribution_lines "$message") || awk_failed $?
    if [ -z "$found" ]; then count=0; else count=$(printf '%s\n' "$found" | wc -l | tr -d ' '); fi
    fixtures=$((fixtures + 1))
    if [ "$count" = "$want_count" ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected %s attribution lines, found %s)\n' "$name" "$want_count" "$count"
      failures=$((failures + 1))
    fi
  }

  expect_in_hook_file() {
    name=$1 want_count=$2 comment_string=$3 hook_file_text=$4
    message=$(drop_commit_verbose_diff "$comment_string" "$hook_file_text") || awk_failed $?
    expect "$name" "$want_count" "$message"
  }

  expect_comment_string_read_from_git() {
    name=$1 want=$2
    got=$(
      export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.commentString GIT_CONFIG_VALUE_0="$want" LC_ALL=en_US.UTF-8
      git_comment_string 2>/dev/null
    ) || got=
    fixtures=$((fixtures + 1))
    if [ "$got" = "$want" ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (git holds a comment string of %s bytes, the script read one of %s bytes)\n' "$name" "${#want}" "${#got}"
      failures=$((failures + 1))
    fi
  }

  expect 'a clean message passes' 0 "Add the store${nl}${nl}Stream segment files through bounded chunks."
  expect 'a Co-Authored-By trailer fails' 1 "Fix the gate${nl}${nl}$trailer"
  expect 'a lower-case co-authored-by trailer fails' 1 "Fix the gate${nl}${nl}co-authored-by: Claude <noreply@anthropic.com>"
  expect 'a mixed-case, indented trailer with a space before the colon fails' 1 "Fix the gate${nl}${nl}  cO-aUtHoReD-bY : Claude <noreply@anthropic.com>"
  expect 'a Generated with line after an emoji fails' 1 "Fix the gate${nl}${nl}$generated"
  expect 'a mixed-case gEnErAtEd WiTh line fails' 1 "Fix the gate${nl}${nl}gEnErAtEd WiTh Claude Code"
  expect 'both lines of the newest exempt commit fail, one finding each' 2 "Fix the gate${nl}${nl}$generated${nl}${nl}$trailer"
  expect 'prose that mentions both phrases passes' 0 "Reject attribution lines${nl}${nl}The check fails on a Co-Authored-By: trailer and on a line that says a${nl}tool was Generated with. A message that only mentions them passes."
  expect 'a line that starts with Generated without passes' 0 "Fix the gate${nl}${nl}Generated without a tool."
  expect 'the diff of git commit -v fails when it is a stored commit message' 2 "$commit_verbose_file"
  expect_in_hook_file 'the diff below the scissors block of git commit -v is dropped from the hook file' 0 '#' "$commit_verbose_file"
  expect_in_hook_file 'the same diff is dropped when the comment string of git is a semicolon' 0 ';' "$semicolon_commit_verbose_file"
  expect_in_hook_file 'the same diff is dropped when the comment string is auto and git picked a semicolon' 0 auto "$semicolon_commit_verbose_file"
  expect_in_hook_file 'a scissors block under a comment string that git does not use keeps its diff' 2 '#' "$semicolon_commit_verbose_file"
  expect_in_hook_file 'a trailer above the scissors block fails in the hook file' 1 '#' "Fix the gate${nl}${nl}$trailer${nl}$scissors_block${nl}$diff_with_attribution_text"
  expect_in_hook_file 'a trailer below a scissors line that no diff follows fails in the hook file' 1 '#' "Fix the gate${nl}${nl}# $SCISSORS${nl}$trailer"
  expect_in_hook_file 'a trailer below a scissors block that no diff follows fails in the hook file' 1 '#' "Fix the gate${nl}${nl}$scissors_block${nl}$trailer"
  expect_in_hook_file 'a trailer between a scissors line and a diff header fails in the hook file' 1 '#' "Fix the gate${nl}${nl}# $SCISSORS${nl}$trailer${nl}diff --git a/bindings.rs b/bindings.rs"
  expect_in_hook_file 'a trailer below prose that holds the scissors text fails in the hook file' 1 '#' "Fix the gate${nl}${nl}The hook stopped at $SCISSORS before.${nl}$trailer"
  expect_in_hook_file 'a trailer below prose that ends with the scissors text and a diff header fails in the hook file' 1 '#' "Fix the gate${nl}${nl}The hook stopped at $SCISSORS${nl}diff --git a/bindings.rs b/bindings.rs${nl}$trailer"
  expect_in_hook_file 'trailers between a scissors line that starts like them and a diff header fail in the hook file' 2 '#' "Fix the gate${nl}${nl}Co-Authored-By: C $SCISSORS${nl}$trailer${nl}diff --git a/bindings.rs b/bindings.rs"
  expect_in_hook_file 'the diff below a scissors block with empty lines, as git writes when it concludes a merge, is dropped from the hook file' 0 '#' "Merge branch 'side'${nl}${nl}$scissors_block${nl}#${nl}# It looks like you may be committing a merge.${nl}${nl}${nl}# On branch main${nl}#${nl}$diff_with_attribution_text"


  expect 'Generated with fails with two spaces between the words' 1 "Fix the gate${nl}${nl}Generated  with Tool"
  expect 'Generated with fails with a tab between the words' 1 "Fix the gate${nl}${nl}Generated${tab}with Tool"
  expect 'Generated with fails with a no-break space between the words' 1 "Fix the gate${nl}${nl}Generated${no_break_space}with Tool"
  expect 'a line that is exactly Generated with fails' 1 "Fix the gate${nl}${nl}Generated with"
  expect 'a Generated with line after an emoji and a no-break space fails' 1 "Fix the gate${nl}${nl}💘${no_break_space}Generated with Crush"
  expect 'a co-author trailer without its colon fails' 1 "Fix the gate${nl}${nl}Co-Authored-By Claude <noreply@anthropic.com>"
  expect 'the spelling Co-Authored By: fails' 1 "Fix the gate${nl}${nl}Co-Authored By: Claude <noreply@anthropic.com>"
  expect 'a trailer with a space before the colon fails' 1 "Fix the gate${nl}${nl}Co-Authored-By : Claude <noreply@anthropic.com>"
  expect 'a trailer indented by a tab fails' 1 "Fix the gate${nl}${nl}${tab}$trailer"
  expect 'a trailer that follows a no-break space fails' 1 "Fix the gate${nl}${nl}${no_break_space}$trailer"
  expect 'a trailer fails in a message whose line breaks are carriage returns alone' 1 "Fix the gate${carriage_return}${carriage_return}$trailer${carriage_return}"
  expect 'a trailer fails once in a message whose line breaks are a carriage return and a newline' 1 "Fix the gate${carriage_return}${nl}${carriage_return}${nl}$trailer${carriage_return}"
  expect 'the Claude-Session trailer fails' 1 "Fix the gate${nl}${nl}$claude_session"
  expect 'the Agent-Logs-Url trailer fails' 1 "Fix the gate${nl}${nl}$agent_logs_url"
  expect 'the Amp-Thread-ID trailer fails' 1 "Fix the gate${nl}${nl}$amp_thread_id"
  expect 'the Assisted-by trailer fails' 1 "Fix the gate${nl}${nl}$assisted_by"
  expect 'three tool trailers in one message fail, one finding each' 3 "Fix the gate${nl}${nl}$claude_session${nl}$agent_logs_url${nl}$amp_thread_id"
  expect 'a Signed-off-by trailer, which names a person, passes' 0 "Fix the gate${nl}${nl}Signed-off-by: Some Person <person@example.invalid>"
  expect 'a Reviewed-by trailer, which names a person, passes' 0 "Fix the gate${nl}${nl}Reviewed-by: Some Person <person@example.invalid>"
  expect 'a word that only starts like the trailer passes' 0 "Fix the gate${nl}${nl}Co-Authored-Bystander effects are out of scope."
  expect 'a date before generated with passes' 0 "Bump the release tool${nl}${nl}2026-11-09: generated with cargo-release 1.1.7"
  expect 'a number before generated with passes' 0 "Regenerate the bindings${nl}${nl}100% generated with bindgen, no hand edits"
  expect 'a parenthesis before generated with passes' 0 "Add the app icons${nl}${nl}The icons come from docs/icon.png${nl}(generated with \`npm run tauri icon\`) and unsigned."
  expect 'a Cyrillic word before generated with passes' 0 "Add the app icons${nl}${nl}Иконки generated with tauri icon"
  expect 'a quoted Generated with line passes' 0 "Reject attribution lines${nl}${nl}The footer was:${nl}> $generated"
  expect 'a quoted trailer passes' 0 "Reject attribution lines${nl}${nl}The trailer was:${nl}> $trailer"
  expect_in_hook_file 'the diff of git commit -v is dropped when the comment string is two backslashes' 0 "$backslash$backslash" "$(commit_verbose_file_with_comment_string "$backslash$backslash")"
  expect_in_hook_file 'the diff of git commit -v is dropped when the comment string is a backslash and the letter t' 0 "${backslash}t" "$(commit_verbose_file_with_comment_string "${backslash}t")"
  expect_comment_string_read_from_git 'a comment string that is not valid UTF-8 is read from git under a UTF-8 locale' "$byte_that_is_not_utf8"
  expect_in_hook_file 'the diff of git commit -v is dropped when the comment string is that byte' 0 "$byte_that_is_not_utf8" "$(commit_verbose_file_with_comment_string "$byte_that_is_not_utf8")"
  expect_in_hook_file 'under the auto setting, a trailer below a line that spells auto before the scissors text fails' 1 auto "Fix the gate${nl}${nl}auto $SCISSORS${nl}diff --git a/x b/x${nl}$trailer"
  expect_in_hook_file 'a comment line of git that lists an untracked file named Generated with passes' 0 '#' "Fix the gate${nl}${nl}$scissors_block${nl}#${nl}# Untracked files:${nl}#${tab}Generated with notes.txt${nl}#"

  if [ "$failures" -gt 0 ]; then
    printf 'check-commit-messages: self-test FAIL: %d of %d fixtures misclassified\n' "$failures" "$fixtures"
    return 1
  fi
  printf 'check-commit-messages: self-test PASS: %d fixtures\n' "$fixtures"
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
