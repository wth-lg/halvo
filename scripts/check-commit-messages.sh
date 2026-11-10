#!/bin/sh
set -eu
export GIT_NO_REPLACE_OBJECTS=1

NEWEST_EXEMPT_COMMIT=685d84637c302976956d56e6bbb5c18a4503e7c8
TOOL_TRAILER_KEYS='Claude-Session Agent-Logs-Url Amp-Thread-ID Assisted-by'

RULE='Commits in this repository are authored by the configured git user only. See AGENTS.md.'

SCISSORS='------------------------ >8 ------------------------'

DEFAULT_BRANCH_REMEDY='The owner decides between rewriting that branch and moving the exempt commit (NEWEST_EXEMPT_COMMIT in scripts/check-commit-messages.sh), with the reason recorded in AGENTS.md. scripts/merge-pr.sh exists so that this does not happen: it checks the squash message before it merges.'

DEFAULT_REF_WITHOUT_ORIGIN_HEAD=refs/remotes/origin/main

FIX_FOR_A_COMMIT_OFF_THE_DEFAULT_BRANCH='remove each listed line from its commit message: git commit --amend for the newest commit, git rebase -i with "reword" for an older one. If a listed line is prose, reword it so that it does not start with these words. Then run this check again.'

exempt_commit_short=${NEWEST_EXEMPT_COMMIT%"${NEWEST_EXEMPT_COMMIT#???????}"}

synopsis() {
  cat <<EOF
  sh scripts/check-commit-messages.sh [--no-exempt] [--default-ref <ref>] [<range>]
      Check the stored message of every commit that git rev-list <range> lists
      (default: HEAD). These commits are exempt, because the owner kept them on
      2026-11-09: $NEWEST_EXEMPT_COMMIT and the commits
      reachable from it. --no-exempt checks them too. --default-ref names the
      branch that is never rewritten, by its full ref (default: the branch that
      refs/remotes/origin/HEAD points to, or refs/remotes/origin/main).
  sh scripts/check-commit-messages.sh --pre-push <remote>
      Check the commits that a push would add to <remote>. .githooks/pre-push calls
      this and passes on the lines that git writes to standard input.
  sh scripts/check-commit-messages.sh --message-file <file>
      Check a message before git stores it. .githooks/commit-msg calls this. In
      the diff that git commit -v appends, a line that starts with a space is
      skipped: it copies an unchanged line of a file. Every other line is read.
  sh scripts/check-commit-messages.sh --stored-message
      Check one message from standard input exactly as given: a squash message,
      or the title and description of a pull request.
  sh scripts/check-commit-messages.sh --self-test
      Prove the predicate on fixtures. Needs git, and no repository.
  sh scripts/check-commit-messages.sh --self-test-pre-push
      Prove the pre-push mode with real pushes between throwaway repositories,
      under a new temporary directory that is left in place. Needs a clone of
      this repository and a configured git user.
EOF
}

help_text() {
  cat <<EOF
Rejects messages that carry an attribution line. Commits in this repository are
authored by the configured git user only, so no line of a message may start with:
  - Co-Authored-By, with or without its colon, its three words joined by hyphens,
    by whitespace or by nothing
  - Generated with
  - one of these trailer keys, which name a tool or its session, and a colon:
    $TOOL_TRAILER_KEYS
Letter case does not matter. Whitespace is a space, a tab, a no-break space or
another space of Unicode (U+1680, U+2000 to U+200A, U+202F, U+205F, U+3000),
and any run of it separates the words. Before the first word, whitespace is
skipped, and so is every character from U+2000 to U+2FFF or above U+FFFF.
Those ranges hold the emoji that tools put before the words, and also symbols
and a few scripts. A carriage return ends a line as a newline does.
A line passes when any other character comes before those words: an ASCII
character such as a digit, a bracket or the "> " of a quotation, or a letter
from below U+2000 or from U+3000 to U+FFFF, such as a Cyrillic or a Chinese
one. A trailer that names a person, such as Signed-off-by, passes.

Three layers apply this predicate.
  1. .githooks/commit-msg gives early notice: it reads a message before git
     stores it. Git does not run it under --no-verify, for revert, cherry-pick,
     rebase, am and commit-tree, or in a checkout that has no .githooks directory.
  2. .githooks/pre-push reads the stored messages of the commits that a push
     would add to the remote, and refuses the push. For each pushed ref these
     are the commits that are on neither the remote's copy of that ref nor the
     default branch as this clone last fetched it. It cannot see a push made
     with --no-verify, a push from a checkout that has no .githooks/pre-push
     file, such as a branch cut before the file existed, a push from a clone
     where sh scripts/prerequisites.sh has not set core.hooksPath, or a commit
     that GitHub itself makes.
  3. The CI step "Commit messages" (scripts/ci-commit-messages.sh) cannot be
     skipped from a clone. It reads the stored messages of every push and pull
     request, and the title and description of a pull request. It reads them
     after the push, so it cannot stop a squash commit that GitHub writes on the
     default branch: scripts/merge-pr.sh checks that message before the merge.
     It does not run for a push whose newest commit says [skip ci], and it reads
     a description again only when a commit is pushed.
No layer reads the author or the committer of a commit, or words that are
spelled in another way. Before the layers, the tracked file .claude/settings.json
tells the agent harness to add no attribution line, and the CI step checks it.

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
  configured=$(git config --get-regexp '^core\.comment(char|string)$' 2>/dev/null | LC_ALL=C sed -n '$s/^[^ ]* //p')
  printf '%s\n' "${configured:-#}"
}

drop_commit_verbose_diff() {
  printf '%s\n' "$2" | COMMENT_STRING_OF_GIT=$1 SCISSORS_END=" $SCISSORS" LC_ALL=C awk '
    BEGIN {
      git_comment_string = ENVIRON["COMMENT_STRING_OF_GIT"]
      scissors_end = ENVIRON["SCISSORS_END"]
      git_picks_the_comment_character = (tolower(git_comment_string) == "auto")
    }
    function scissors_line_comment_string(line,    comment_length, candidate) {
      comment_length = length(line) - length(scissors_end)
      if (comment_length < 1 || substr(line, comment_length + 1) != scissors_end) return ""
      candidate = substr(line, 1, comment_length)
      if (git_picks_the_comment_character) return (comment_length == 1 && index("#;@!$%^&|:", candidate)) ? candidate : ""
      return (candidate == git_comment_string) ? candidate : ""
    }
    function print_scissors_block(    i) {
      for (i = 1; i <= scissors_block_lines; i++) print scissors_block[i]
      scissors_block_lines = 0
    }
    dropping_diff {
      if ($0 ~ /^ /) print ""; else print
      next
    }
    scissors_block_lines > 0 {
      if (index($0, "diff --git ") == 1) {
        dropping_diff = 1
        for (i = 1; i <= scissors_block_lines; i++) print ""
        print
        next
      }
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
  printf '%s\n' "$1" | TOOL_TRAILER_KEYS=$TOOL_TRAILER_KEYS LC_ALL=C awk '
    BEGIN { tool_key_count = split(tolower(ENVIRON["TOOL_TRAILER_KEYS"]), tool_key, " ") }
    function starts_with_tool_trailer(text,    k) {
      for (k = 1; k <= tool_key_count; k++)
        if (index(text, tool_key[k]) == 1 && substr(text, length(tool_key[k]) + 1) ~ /^ *:/) return 1
      return 0
    }
    function is_attribution(line,    text) {
      text = tolower(line)
      gsub(/\t|\302\240|\341\232\200|\342\200[\200-\212\257]|\342\201\237|\343\200\200/, " ", text)
      sub(/^( |\342[\200-\277][\200-\277]|\357\270[\200-\217]|[\360-\364][\200-\277][\200-\277][\200-\277])+/, "", text)
      return text ~ /^co[- ]*authored[- ]*by([^a-z0-9]|$)/ || text ~ /^generated +with([^a-z0-9]|$)/ || starts_with_tool_trailer(text)
    }
    {
      record = $0
      sub(/\r$/, "", record)
      part_count = split(record, part, "\r")
      if (part_count == 0) line_number++
      for (i = 1; i <= part_count; i++) {
        line_number++
        if (is_attribution(part[i])) printf "  line %d: %s\n", line_number, part[i]
      }
    }
  '
}

report_message() {
  found=$(attribution_lines "$1") || awk_failed $?
  [ -n "$found" ] || return 0
  {
    printf 'check-commit-messages: the message carries an attribution line:\n%s\n' "$found"
    printf 'Fix: remove the line and try again. If the line is prose, reword it so that it does not start with these words. %s\n' "$RULE"
  } >&2
  return 1
}

check_hook_file() {
  { [ -r "$1" ] && [ ! -d "$1" ]; } || die "no such message file: $1"
  hook_file_text=$(cat "$1") || die "cannot read the message file: $1"
  comment_string=$(git_comment_string) || die 'git config could not report the comment string of git'
  message=$(drop_commit_verbose_diff "$comment_string" "$hook_file_text") || awk_failed $?
  report_message "$message"
}

check_stored_message() {
  message=$(cat) || die 'cannot read the message from standard input'
  report_message "$message"
}

require_repository_with_exempt_commit() {
  git rev-parse --git-dir >/dev/null 2>&1 || die 'not inside a git repository'
  [ "$exempt" = 0 ] || git cat-file -e "$NEWEST_EXEMPT_COMMIT^{commit}" 2>/dev/null ||
    die "the newest exempt commit $NEWEST_EXEMPT_COMMIT is not in this clone, so exempt commits cannot be told from new ones. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
  if [ "$exempt" = 1 ]; then
    scope="$exempt_commit_short and the commits reachable from it are exempt"
  else
    scope='no commit is exempt'
  fi
}

resolve_default_ref() {
  if [ -n "$default_ref" ]; then
    git rev-parse -q --verify "$default_ref^{commit}" >/dev/null ||
      die "--default-ref $default_ref does not name a commit in this clone"
  else
    default_ref=$(git symbolic-ref -q refs/remotes/origin/HEAD) || default_ref=
    if [ -z "$default_ref" ] && git rev-parse -q --verify "$DEFAULT_REF_WITHOUT_ORIGIN_HEAD^{commit}" >/dev/null; then
      default_ref=$DEFAULT_REF_WITHOUT_ORIGIN_HEAD
    fi
  fi
}

check_commits() {
  label=$1
  shift
  commits=$(git rev-list --reverse "$@" --) || die "git rev-list could not list the commits of '$label'"

  total=0
  bad=0
  bad_on_default_branch=0
  for commit in $commits; do
    total=$((total + 1))
    stored_commit=$(git cat-file commit "$commit") || die "git cat-file could not read $commit"
    message=$(printf '%s\n' "$stored_commit" | LC_ALL=C sed '1,/^$/d') || die "sed could not read the message of $commit"
    found=$(attribution_lines "$message") || awk_failed $?
    [ -n "$found" ] || continue
    bad=$((bad + 1))
    place=
    if [ -n "$default_ref" ] && git merge-base --is-ancestor "$commit" "$default_ref" 2>/dev/null; then
      bad_on_default_branch=$((bad_on_default_branch + 1))
      place=" (already on $default_ref)"
    fi
    printf 'check-commit-messages: %s%s\n%s\n' "$(git log -1 --format='%h %s' "$commit")" "$place" "$found" >&2
  done

  if [ "$bad" = 0 ]; then
    printf 'check-commit-messages: PASS: %s: commits checked %d, with an attribution line 0 (%s).\n' "$label" "$total" "$scope"
    return 0
  fi
  any_range_failed=1
  {
    printf 'check-commit-messages: FAIL: %s: commits checked %d, with an attribution line %d (%s).\n' "$label" "$total" "$bad" "$scope"
    if [ -z "$default_ref" ]; then
      printf 'This clone names no default branch (it has no refs/remotes/origin/HEAD and no %s), so the check cannot tell which listed commits are already on it. Do not amend or rebase a commit that is: an agent does not rewrite the default branch. %s\n' "$DEFAULT_REF_WITHOUT_ORIGIN_HEAD" "$DEFAULT_BRANCH_REMEDY"
      printf 'Fix for a listed commit that is not on the default branch: %s %s\n' "$FIX_FOR_A_COMMIT_OFF_THE_DEFAULT_BRANCH" "$RULE"
    elif [ "$bad" -gt "$bad_on_default_branch" ]; then
      printf 'Fix: %s %s\n' "$FIX_FOR_A_COMMIT_OFF_THE_DEFAULT_BRANCH" "$RULE"
      printf 'sh scripts/prerequisites.sh turns on the commit-msg and pre-push hooks, which report such a line before it leaves this clone.\n'
    fi
    if [ "$bad_on_default_branch" -gt 0 ]; then
      printf 'Listed commits that are already on %s: %d. Do not amend or rebase them: an agent does not rewrite the default branch. %s\n' "$default_ref" "$bad_on_default_branch" "$DEFAULT_BRANCH_REMEDY"
    fi
  } >&2
}

is_object_id() {
  case $1 in
    ''|*[!0-9a-f]*) return 1 ;;
  esac
  [ ${#1} = 40 ] || [ ${#1} = 64 ]
}

check_push() {
  remote=$1
  require_repository_with_exempt_commit
  resolve_default_ref
  default_ref_of_remote=$(git symbolic-ref -q "refs/remotes/$remote/HEAD" 2>/dev/null) || default_ref_of_remote=refs/remotes/$remote/main
  git rev-parse -q --verify "$default_ref_of_remote^{commit}" >/dev/null 2>&1 || default_ref_of_remote=
  while IFS= read -r pushed_ref; do
    remote_object=${pushed_ref##* }
    rest=${pushed_ref% *}
    remote_ref=${rest##* }
    rest=${rest% *}
    local_object=${rest##* }
    local_ref=${rest% *}
    if ! { is_object_id "$local_object" && is_object_id "$remote_object"; }; then
      die "cannot read this line that git wrote to the pre-push hook, so the push was not checked: $pushed_ref"
    fi
    case $local_object in
      *[!0]*) ;;
      *) continue ;;
    esac
    git rev-parse -q --verify "$local_object^{commit}" >/dev/null || continue
    set -- "$local_object" "^$NEWEST_EXEMPT_COMMIT"
    case $remote_object in
      *[!0]*)
        if git cat-file -e "$remote_object^{commit}" 2>/dev/null; then set -- "$@" "^$remote_object"; fi
        ;;
    esac
    if [ -n "$default_ref_of_remote" ]; then set -- "$@" "^$default_ref_of_remote"; fi
    check_commits "push of $local_ref to $remote_ref" "$@"
  done
  [ "$any_range_failed" = 0 ]
}

check_range() {
  require_repository_with_exempt_commit
  resolve_default_ref
  if [ "$exempt" = 1 ]; then
    check_commits "$1" --end-of-options "$1" "^$NEWEST_EXEMPT_COMMIT"
  else
    check_commits "$1" --end-of-options "$1"
  fi
  [ "$any_range_failed" = 0 ]
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
  ogham_space_mark=$(printf '\341\232\200')
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

  expect_at_lines() {
    name=$1 want_lines=$2 message=$3
    found=$(attribution_lines "$message") || awk_failed $?
    lines=$(printf '%s\n' "$found" | sed -n 's/^  line \([0-9][0-9]*\): .*/\1/p' | paste -s -d ' ' -)
    fixtures=$((fixtures + 1))
    if [ "$lines" = "$want_lines" ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected attribution lines at [%s], found them at [%s])\n' "$name" "$want_lines" "$lines"
      failures=$((failures + 1))
    fi
  }

  expect_in_hook_file_at_lines() {
    name=$1 want_lines=$2 comment_string=$3 hook_file_text=$4
    message=$(drop_commit_verbose_diff "$comment_string" "$hook_file_text") || awk_failed $?
    expect_at_lines "$name" "$want_lines" "$message"
  }

  expect_hook_path() {
    name=$1 want_status=$2 want_text=$3 hook_file=$4 hook_file_text=${5-}
    status=0
    printf '%s\n' "$hook_file_text" | (check_hook_file "$hook_file") >/dev/null 2>&1 || status=$?
    text=present
    if [ -n "$want_text" ]; then
      printf '%s\n' "$hook_file_text" | (check_hook_file "$hook_file") 2>&1 | grep -qF -- "$want_text" || text=absent
    fi
    fixtures=$((fixtures + 1))
    if [ "$status" = "$want_status" ] && [ "$text" = present ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected exit %s and the text "%s"; got exit %s, text %s)\n' "$name" "$want_status" "$want_text" "$status" "$text"
      failures=$((failures + 1))
    fi
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
  expect 'the diff of git commit -v fails when it is a stored commit message' 1 "$commit_verbose_file"
  expect_in_hook_file 'the diff below the scissors block of git commit -v is dropped from the hook file' 0 '#' "$commit_verbose_file"
  expect_in_hook_file 'the same diff is dropped when the comment string of git is a semicolon' 0 ';' "$semicolon_commit_verbose_file"
  expect_in_hook_file 'the same diff is dropped when the comment string is auto and git picked a semicolon' 0 auto "$semicolon_commit_verbose_file"
  expect_in_hook_file 'a scissors block under a comment string that git does not use keeps its diff' 1 '#' "$semicolon_commit_verbose_file"
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
  expect_in_hook_file 'under the auto setting, an indented trailer below such a line fails too, because the line starts no scissors block' 1 auto "Fix the gate${nl}${nl}auto $SCISSORS${nl}diff --git a/x b/x${nl} $trailer"
  expect 'a line that starts with a tool trailer key and has no colon passes' 0 "Fix the gate${nl}${nl}Assisted-by design, the gate stays manual."
  expect 'Generated with fails with two no-break spaces between the words' 1 "Fix the gate${nl}${nl}Generated${no_break_space}${no_break_space}with Tool"
  expect 'Generated with fails with an Ogham space mark between the words' 1 "Fix the gate${nl}${nl}Generated${ogham_space_mark}with Tool"
  expect 'a line that is exactly Co-Authored-By fails' 1 "Fix the gate${nl}${nl}Co-Authored-By"
  expect 'the trailer fails when nothing joins its three words' 1 "Fix the gate${nl}${nl}CoAuthoredBy: Claude <noreply@anthropic.com>"
  expect 'a tool trailer with a space before its colon fails' 1 "Fix the gate${nl}${nl}Claude-Session : https://claude.ai/code/session_0000000000000000000000"
  expect_in_hook_file 'a comment line of git that lists an untracked file named Generated with passes' 0 '#' "Fix the gate${nl}${nl}$scissors_block${nl}#${nl}# Untracked files:${nl}#${tab}Generated with notes.txt${nl}#"

  expect_at_lines 'a trailer on the first line of a message fails at line 1' '1' "$trailer"
  expect_at_lines 'the two lines of the newest exempt commit fail at lines 3 and 5' '3 5' "Fix the gate${nl}${nl}$generated${nl}${nl}$trailer"
  expect_at_lines 'a trailer after two carriage returns fails at line 3' '3' "Fix the gate${carriage_return}${carriage_return}$trailer${carriage_return}"
  expect_at_lines 'a trailer after two pairs of carriage return and newline fails at line 3' '3' "Fix the gate${carriage_return}${nl}${carriage_return}${nl}$trailer${carriage_return}"
  expect_at_lines 'prose that starts with the footer words fails, as the rule says' '1' "Generated with bindgen 0.71: refresh the Windows bindings"
  expect_at_lines 'prose that starts with Co-authored by fails, as the rule says' '3' "Keep both gates${nl}${nl}Co-authored by the capture team, this change keeps both gates."
  expect_in_hook_file_at_lines 'a trailer above the diff of git commit -v fails at its line in the hook file' '3' '#' "Fix the gate${nl}${nl}$trailer${nl}$scissors_block${nl}$diff_with_attribution_text"
  expect_in_hook_file_at_lines 'a trailer at the start of a line below a scissors block and a diff header fails at its line in the hook file' '7' '#' "Fix the gate${nl}${nl}$scissors_block${nl}diff --git a/x b/x${nl}$trailer"
  expect_in_hook_file_at_lines 'a tool footer at the start of a line below a scissors block and a diff header fails in the hook file' '8' '#' "Fix the gate${nl}${nl}$scissors_block${nl}${nl}diff --git a/x b/x${nl}$generated"
  expect_in_hook_file_at_lines 'a tool trailer at the start of a line below a scissors block and a diff header fails in the hook file' '8' '#' "Fix the gate${nl}${nl}$scissors_block${nl}#${nl}diff --git a/x b/x${nl}$claude_session"
  expect_in_hook_file_at_lines 'a trailer at the start of a line fails below a whole diff, and the context line of that diff does not' '12' '#' "Fix the gate${nl}${nl}$scissors_block${nl}$diff_with_attribution_text${nl}$trailer"
  expect_in_hook_file_at_lines 'a trailer indented by a tab below a scissors block and a diff header fails, because no line of a diff starts with a tab' '7' '#' "Fix the gate${nl}${nl}$scissors_block${nl}diff --git a/x b/x${nl}${tab}$trailer"

  expect_hook_path 'the hook path exits 1 on a file with a trailer and says how to reword prose' 1 'reword it so that it does not start with these words' /dev/stdin "Fix the gate${nl}${nl}$trailer"
  expect_hook_path 'the hook path exits 0 on a clean file' 0 '' /dev/stdin "Add the store${nl}${nl}Stream segment files through bounded chunks."
  expect_hook_path 'the hook path exits 2 when the file does not exist' 2 'no such message file' /dev/null/absent

  if [ "$failures" -gt 0 ]; then
    printf 'check-commit-messages: self-test FAIL: %d of %d fixtures misclassified\n' "$failures" "$fixtures"
    return 1
  fi
  printf 'check-commit-messages: self-test PASS: %d fixtures\n' "$fixtures"
}

self_test_pre_push() {
  script=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
  root=$(cd "$(dirname "$0")/.." && pwd)
  git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || die 'this self-test clones the repository, and the script is not inside one'
  [ -n "$(git config --get user.name || true)" ] || die 'this self-test commits in a throwaway clone and needs a configured git user'
  work=$(mktemp -d "${TMPDIR:-/tmp}/check-commit-messages-pre-push-self-test.XXXXXX") || die 'mktemp could not make a directory'
  printf 'check-commit-messages: pre-push self-test in %s, which is left in place\n' "$work"
  mkdir -p "$work/tree/.githooks" "$work/tree/scripts" "$work/out"
  cp "$root/.githooks/pre-push" "$work/tree/.githooks/pre-push"
  cp "$script" "$work/tree/scripts/check-commit-messages.sh"
  chmod +x "$work/tree/.githooks/pre-push"
  git clone -q "$root" "$work/clone"
  git -C "$work/clone" config core.hooksPath "$work/tree/.githooks"
  git init -q --bare "$work/remote.git"
  git init -q --bare "$work/empty-remote.git"
  git -C "$work/clone" remote add proof "$work/remote.git"
  git -C "$work/clone" remote add proof-empty "$work/empty-remote.git"
  base=$(git -C "$work/clone" rev-parse HEAD)
  tree=$(git -C "$work/clone" rev-parse 'HEAD^{tree}')
  failures=0
  pushes=0
  nl='
'
  trailer='Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>'

  commit_on_base() {
    git -C "$work/clone" commit-tree "$tree" -p "$base" -m "$1"
  }
  in_other_clone() {
    git -C "$work/other-clone" -c commit.gpgsign=false "$@"
  }
  judge() {
    name=$1 status=$2 want_status=$3 want_text=$4
    pushes=$((pushes + 1))
    if [ "$status" = "$want_status" ] && grep -qF -- "$want_text" "$work/out/$name.txt"; then
      printf 'PASS %s (exit %s)\n' "$name" "$status"
    else
      printf 'FAIL %s (exit %s, expected exit %s and the text "%s"; output in %s)\n' "$name" "$status" "$want_status" "$want_text" "$work/out/$name.txt"
      failures=$((failures + 1))
    fi
  }
  expect_push() {
    name=$1 want_status=$2 want_text=$3 remote=$4
    shift 4
    status=0
    git -C "$work/clone" push "$remote" "$@" >"$work/out/$name.txt" 2>&1 || status=$?
    judge "$name" "$status" "$want_status" "$want_text"
  }

  clean=$(commit_on_base 'Add a clean change')
  with_trailer=$(commit_on_base "Fix the gate$nl$nl$trailer")
  with_trailer_and_a_subject_of_its_own=$(commit_on_base "Fix the gate two$nl$nl$trailer")
  hidden_by_a_replace_ref=$(commit_on_base "Tune the gate$nl$nl$trailer")
  shown_by_the_replace_ref=$(commit_on_base 'Tune the gate')

  expect_push the-history-above-the-exempt-commit 0 'with an attribution line 0' proof "$base:refs/heads/base"
  expect_push a-clean-commit 0 'commits checked ' proof "$clean:refs/heads/clean"
  expect_push a-commit-with-a-trailer 1 'with an attribution line 1' proof "$with_trailer:refs/heads/trailer"
  expect_push the-same-commit-to-a-path 1 'with an attribution line 1' "$work/remote.git" "$with_trailer:refs/heads/trailer-by-path"
  expect_push a-clean-ref-and-a-ref-with-a-trailer-in-one-push 1 'with an attribution line 1' proof "$clean:refs/heads/clean-again" "$with_trailer:refs/heads/trailer-again"
  expect_push a-deleted-ref 0 'deleted' proof :refs/heads/clean
  expect_push the-exempt-commits-alone 0 'commits checked 0,' proof-empty "$NEWEST_EXEMPT_COMMIT:refs/heads/exempt"

  git -C "$work/clone" checkout -q --detach "$base"
  git -C "$work/clone" cherry-pick --keep-redundant-commits "$NEWEST_EXEMPT_COMMIT" >"$work/out/cherry-pick.txt" 2>&1 ||
    die "the self-test could not cherry-pick the exempt commit; see $work/out/cherry-pick.txt"
  expect_push a-cherry-pick-of-the-newest-exempt-commit 1 'Generated with [Claude Code]' proof HEAD:refs/heads/cherry-picked

  git -C "$work/clone" tag -a -m 'A tag on a commit with a trailer' tag-on-a-trailer "$with_trailer"
  expect_push an-annotated-tag-on-a-commit-with-a-trailer 1 'with an attribution line 1' proof refs/tags/tag-on-a-trailer

  git -C "$work/clone" branch -f spaced "$with_trailer_and_a_subject_of_its_own"
  expect_push a-source-expression-that-holds-a-space 1 'with an attribution line 1' proof 'spaced^{/gate two}:refs/heads/spaced'

  git -C "$work/clone" replace "$hidden_by_a_replace_ref" "$shown_by_the_replace_ref"
  expect_push a-commit-under-a-replace-ref 1 'with an attribution line 1' proof "$hidden_by_a_replace_ref:refs/heads/replaced"

  git clone -q "$work/remote.git" "$work/other-clone" 2>/dev/null
  in_other_clone checkout -q -b shared origin/base
  in_other_clone commit -q --allow-empty --no-verify -m "Add the shared change$nl$nl$trailer"
  in_other_clone push -q origin shared:refs/heads/shared
  git -C "$work/clone" fetch -q proof
  in_other_clone commit -q --amend --allow-empty --no-verify -m 'Add the shared change'
  in_other_clone push -q -f origin shared:refs/heads/shared
  expect_push a-commit-that-the-remote-had-and-no-longer-has 1 'with an attribution line 1' proof refs/remotes/proof/shared:refs/heads/mine

  in_other_clone checkout -q -b held origin/base
  in_other_clone commit -q --allow-empty --no-verify -m "Add a change that the remote keeps$nl$nl$trailer"
  in_other_clone push -q origin held:refs/heads/held
  git -C "$work/clone" fetch -q proof
  above_the_held_commit=$(git -C "$work/clone" commit-tree "$tree" -p refs/remotes/proof/held -m 'Add a clean change above it')
  expect_push a-clean-commit-above-a-trailer-that-the-remote-already-holds 0 'commits checked 1,' proof "$above_the_held_commit:refs/heads/held"

  git -C "$work/clone" remote set-head proof base
  expect_push a-clean-commit-above-the-default-branch-of-the-remote 0 'commits checked 1,' proof "$clean:refs/heads/clean-above-the-default-branch"

  status=0
  (cd "$work/clone" && printf 'not a line that git writes\n' | sh "$work/tree/scripts/check-commit-messages.sh" --pre-push proof) >"$work/out/a-line-that-git-does-not-write.txt" 2>&1 || status=$?
  judge a-line-that-git-does-not-write "$status" 2 'cannot read this line'

  if [ "$failures" -gt 0 ]; then
    printf 'check-commit-messages: pre-push self-test FAIL: %d of %d pushes behaved otherwise. Outputs are in %s.\n' "$failures" "$pushes" "$work/out"
    return 1
  fi
  printf 'check-commit-messages: pre-push self-test PASS: %d pushes. Outputs are in %s.\n' "$pushes" "$work/out"
}

mode=
exempt=1
range=
file=
remote=
default_ref=
any_range_failed=0
set_exclusive_mode() {
  [ -z "$mode" ] || [ "$mode" = "$1" ] || usage 'a range, --pre-push, --message-file, --stored-message, --self-test and --self-test-pre-push exclude each other'
  mode=$1
}
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) set_exclusive_mode self-test ;;
    --self-test-pre-push) set_exclusive_mode self-test-pre-push ;;
    --stored-message) set_exclusive_mode stored-message ;;
    --message-file)
      [ $# -ge 2 ] || usage '--message-file needs a file'
      [ -z "$file" ] || usage "more than one message file: $file and $2"
      set_exclusive_mode message
      file=$2
      shift
      ;;
    --pre-push)
      [ $# -ge 2 ] || usage '--pre-push needs the name or the URL of the remote'
      set_exclusive_mode pre-push
      remote=$2
      shift
      ;;
    --default-ref)
      [ $# -ge 2 ] || usage '--default-ref needs a full ref name'
      case $2 in
        refs/?*) ;;
        *) usage "--default-ref needs a full ref name such as refs/remotes/origin/main, and got: $2" ;;
      esac
      default_ref=$2
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
  self-test-pre-push) self_test_pre_push ;;
  message) check_hook_file "$file" ;;
  stored-message) check_stored_message ;;
  pre-push) check_push "$remote" ;;
  range) check_range "${range:-HEAD}" ;;
esac
