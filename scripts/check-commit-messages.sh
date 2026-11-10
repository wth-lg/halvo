#!/bin/sh
set -eu

NEWEST_EXEMPT_COMMIT=685d84637c302976956d56e6bbb5c18a4503e7c8
TOOL_TRAILER_KEYS='Claude-Session Agent-Logs-Url Amp-Thread-ID Assisted-by'

RULE='Commits in this repository are authored by the configured git user only. See AGENTS.md.'

SCISSORS='------------------------ >8 ------------------------'

DEFAULT_BRANCH_REMEDY='The owner decides between rewriting that branch and moving the exempt commit (NEWEST_EXEMPT_COMMIT in scripts/check-commit-messages.sh), with the reason recorded in AGENTS.md. scripts/merge-pr.sh exists so that this does not happen: it checks the squash message before it merges.'

exempt_commit_short=${NEWEST_EXEMPT_COMMIT%"${NEWEST_EXEMPT_COMMIT#???????}"}

synopsis() {
  cat <<EOF
  sh scripts/check-commit-messages.sh [--no-exempt] [--default-ref <ref>] [<range>]
      Check the stored message of every commit that git rev-list <range> lists
      (default: HEAD). These commits are exempt, because the owner kept them on
      2026-11-09: $NEWEST_EXEMPT_COMMIT and the commits
      reachable from it. --no-exempt checks them too. --default-ref names the
      branch that is never rewritten, by its full ref (default: the branch that
      refs/remotes/origin/HEAD points to).
  sh scripts/check-commit-messages.sh --pre-push <remote>
      Check the commits that a push would add to <remote>. .githooks/pre-push calls
      this and passes on the lines that git writes to standard input.
  sh scripts/check-commit-messages.sh --message-file <file>
      Check a message before git stores it. .githooks/commit-msg calls this. The
      lines of the diff that git commit -v appends are dropped, as git drops
      them. A line below that diff that starts with other text is still read.
  sh scripts/check-commit-messages.sh --stored-message
      Check one message from standard input exactly as given: a squash message,
      or the title and description of a pull request.
  sh scripts/check-commit-messages.sh --self-test
      Prove the predicate on fixtures. Needs git, and no repository.
EOF
}

help_text() {
  cat <<EOF
Rejects messages that carry an attribution line. Commits in this repository are
authored by the configured git user only, so no line of a message may start with:
  - Co-Authored-By, with or without its colon, its three words joined by hyphens
    or by whitespace
  - Generated with
  - one of these trailer keys, which name a tool or its session, and a colon:
    $TOOL_TRAILER_KEYS
Letter case does not matter. Whitespace is a space, a tab, a no-break space or
another Unicode space, and any run of it separates the words. Whitespace and
emoji (a character from U+2000 to U+2FFF or above U+FFFF) before the first word
are skipped. A carriage return ends a line as a newline does.
A line passes when anything else comes before those words: a letter of any
script, a digit, a bracket or the "> " of a quotation. A trailer that names a
person, such as Signed-off-by, passes.

Three layers apply this predicate.
  1. .githooks/commit-msg gives early notice: it reads a message before git
     stores it. Git does not run it under --no-verify, for revert, cherry-pick,
     rebase, am and commit-tree, or in a checkout that has no .githooks directory.
  2. .githooks/pre-push reads the stored messages of the commits that a push
     would add to the remote, and refuses the push. It cannot see a push made
     with --no-verify, a push from a clone where sh scripts/prerequisites.sh has
     not set core.hooksPath, or a commit that GitHub itself makes.
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
      if ($0 ~ /^[ +-]/) print ""; else print
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
      gsub(/\t|\302\240|\342\200[\200-\212\257]|\342\201\237|\343\200\200/, " ", text)
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
  [ -f "$1" ] || die "no such message file: $1"
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
    if [ "$bad" -gt "$bad_on_default_branch" ]; then
      printf 'Fix: remove each listed line from its commit message: git commit --amend for the newest commit, git rebase -i with "reword" for an older one. If a listed line is prose, reword it so that it does not start with these words. Then run this check again. %s\n' "$RULE"
      printf 'sh scripts/prerequisites.sh turns on the commit-msg and pre-push hooks, which report such a line before it leaves this clone.\n'
    fi
    if [ "$bad_on_default_branch" -gt 0 ]; then
      printf '%d of the listed commits are already on %s. Do not amend or rebase them: an agent does not rewrite the default branch. %s\n' "$bad_on_default_branch" "$default_ref" "$DEFAULT_BRANCH_REMEDY"
    elif [ -z "$default_ref" ]; then
      printf 'If a listed commit is already on the default branch, do not amend or rebase it: an agent does not rewrite the default branch. %s\n' "$DEFAULT_BRANCH_REMEDY"
    fi
  } >&2
}

check_push() {
  remote=$1
  require_repository_with_exempt_commit
  resolve_default_ref
  while read -r local_ref local_object remote_ref remote_object; do
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
    if git config --get "remote.$remote.url" >/dev/null 2>&1; then set -- "$@" --not "--remotes=$remote"; fi
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
    name=$1 want_status=$2 want_text=$3 hook_file=$4
    status=0
    output=$(check_hook_file "$hook_file" 2>&1) || status=$?
    fixtures=$((fixtures + 1))
    case $output in
      *"$want_text"*) text=present ;;
      *) text=absent ;;
    esac
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
  expect_in_hook_file 'a comment line of git that lists an untracked file named Generated with passes' 0 '#' "Fix the gate${nl}${nl}$scissors_block${nl}#${nl}# Untracked files:${nl}#${tab}Generated with notes.txt${nl}#"

  expect_at_lines 'a trailer on the first line of a message fails at line 1' '1' "$trailer"
  expect_at_lines 'the two lines of the newest exempt commit fail at lines 3 and 5' '3 5' "Fix the gate${nl}${nl}$generated${nl}${nl}$trailer"
  expect_at_lines 'a trailer after two carriage returns fails at line 3' '3' "Fix the gate${carriage_return}${carriage_return}$trailer${carriage_return}"
  expect_at_lines 'a trailer after two pairs of carriage return and newline fails at line 3' '3' "Fix the gate${carriage_return}${nl}${carriage_return}${nl}$trailer${carriage_return}"
  expect_at_lines 'prose that starts with the footer words fails, as the rule says' '1' "Generated with bindgen 0.71: refresh the Windows bindings"
  expect_in_hook_file_at_lines 'a trailer above the diff of git commit -v fails at its line in the hook file' '3' '#' "Fix the gate${nl}${nl}$trailer${nl}$scissors_block${nl}$diff_with_attribution_text"
  expect_in_hook_file_at_lines 'a trailer at the start of a line below a scissors block and a diff header fails at its line in the hook file' '7' '#' "Fix the gate${nl}${nl}$scissors_block${nl}diff --git a/x b/x${nl}$trailer"
  expect_in_hook_file_at_lines 'a tool footer at the start of a line below a scissors block and a diff header fails in the hook file' '8' '#' "Fix the gate${nl}${nl}$scissors_block${nl}${nl}diff --git a/x b/x${nl}$generated"
  expect_in_hook_file_at_lines 'a tool trailer at the start of a line below a scissors block and a diff header fails in the hook file' '8' '#' "Fix the gate${nl}${nl}$scissors_block${nl}#${nl}diff --git a/x b/x${nl}$claude_session"
  expect_in_hook_file_at_lines 'a trailer at the start of a line fails below a whole diff, and the context line of that diff does not' '12' '#' "Fix the gate${nl}${nl}$scissors_block${nl}$diff_with_attribution_text${nl}$trailer"

  hook_files=$(mktemp -d "${TMPDIR:-/tmp}/check-commit-messages-self-test.XXXXXX") || die 'mktemp could not make a directory for the hook files of the self-test'
  printf 'Fix the gate\n\n%s\n' "$trailer" >"$hook_files/with-a-trailer.txt"
  printf 'Add the store\n\nStream segment files through bounded chunks.\n' >"$hook_files/clean.txt"
  expect_hook_path 'the hook path exits 1 on a file with a trailer and says how to reword prose' 1 'reword it so that it does not start with these words' "$hook_files/with-a-trailer.txt"
  expect_hook_path 'the hook path exits 0 on a clean file' 0 '' "$hook_files/clean.txt"
  expect_hook_path 'the hook path exits 2 when the file does not exist' 2 'no such message file' "$hook_files/absent.txt"

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
remote=
default_ref=
any_range_failed=0
set_exclusive_mode() {
  [ -z "$mode" ] || [ "$mode" = "$1" ] || usage 'a range, --pre-push, --message-file, --stored-message and --self-test exclude each other'
  mode=$1
}
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) set_exclusive_mode self-test ;;
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
  message) check_hook_file "$file" ;;
  stored-message) check_stored_message ;;
  pre-push) check_push "$remote" ;;
  range) check_range "${range:-HEAD}" ;;
esac
