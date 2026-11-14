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

case $0 in
  */*) own_directory=$(cd "$(dirname "$0")" 2>/dev/null && pwd) || own_directory=$(dirname "$0") ;;
  *) own_directory= ;;
esac

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
      the diff that git commit -v appends, a line that starts with a space, a
      plus, a minus or an at sign is skipped: it is a line of a file, or a hunk
      header that copies one. Every other line is read. The mode takes as
      that diff what stands below a scissors line of git and a diff header,
      with nothing but comment lines and empty lines between the two. A diff
      header is a line that starts with "diff --git " or with "Submodule ". So
      when someone writes both by hand, an attribution line below them passes
      this mode when a space indents it, or when it follows a carriage return
      in a skipped line. Below a diff header with no scissors line above it,
      every line is read.
  sh scripts/check-commit-messages.sh --stored-message
      Check one message from standard input exactly as given: a squash message,
      or the title and description of a pull request.
  sh scripts/check-commit-messages.sh --self-test
      Prove the predicate on fixtures. Needs git, and no repository. One
      fixture gives git a comment string that is not valid UTF-8. It does so in
      a config file, a new file under the temporary directory that is left in
      place: Git for Windows converts such a byte in a variable or an argument
      on its way to git, and reads a file as it is. Two fixtures hand the hook
      path a message file, also a new file there that is left in place. Three
      fixtures run a copy of .githooks/commit-msg, in a new directory there that
      is left in place, without the scanner of home paths, without its list,
      and with both; they need the .githooks directory beside scripts.
  sh scripts/check-commit-messages.sh --self-test-two-paths
      Send every fixture message of --self-test down both paths in a throwaway
      repository: through git commit -F with a copy of the commit-msg hook on,
      beside this script, the scanner of home paths and its list, and, as the
      commit that git stored, through the range check. The two verdicts must
      agree. The exceptions are the hand-made shapes that --message-file names
      above: git commit -F stores a file with a scissors block and a diff whole,
      so that diff counts as written by hand. The test lists each such fixture
      with its shape. The repository is under a new temporary directory that is
      left in place. Needs the .githooks directory beside scripts, and a name
      and an email for commits, as below. The test hands git the comment string
      with -c on the command line, where the shell of Git for Windows turns a
      byte that is not valid UTF-8 into the code point of the same number, so
      the test is right only where the workflow runs it, in the macos job.
  sh scripts/check-commit-messages.sh --self-test-pre-push
      Prove the pre-push mode with real pushes between throwaway repositories,
      under a new temporary directory that is left in place. The hook runs
      from a copy of .githooks/pre-push with this script, the scanner of home
      paths, its list and scripts/commit-range.sh beside it. The pushed commits
      sit on the newest exempt commit and hold its tree, so they add no line,
      and the result does not depend on the other commits of this repository
      or on its files. Needs a clone of
      this repository, and a name and an email for commits: a configured git
      user, or the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL,
      GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.
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
skipped, and so is every character from U+2000 to U+2FFF, from U+FE00 to
U+FE0F or above U+FFFF. Those ranges hold the emoji that tools put before the
words and the variation selector that can follow an emoji, and also symbols
and a few scripts. A carriage return ends a line as a newline does.
A line passes when any other character comes before those words: an ASCII
character such as a digit, a bracket or the "> " of a quotation, or a letter
from below U+2000 or from U+3000 to U+FFFF outside U+FE00 to U+FE0F, such as
a Cyrillic or a Chinese one. A trailer that names a person, such as
Signed-off-by, passes. A line also passes when an ASCII letter or digit
follows the words Co-Authored-By or Generated with directly, as in Generated
without: that makes another word.

Three layers apply this predicate.
  1. .githooks/commit-msg gives early notice: it reads a message before git
     stores it. It refuses the commit when scripts/check-home-paths.sh or
     scripts/home-path-allow.txt is missing beside it, because the pre-push
     hook cannot scan a push there. Git does not run it for git commit
     --no-verify, for a revert
     or a cherry-pick that makes its commit without a stop, for am and
     commit-tree, for a commit that a rebase picks without a new message,
     for the commit that a squash, a fixup -c or a fixup -C of git rebase -i
     makes, although that commit gets a new message, for the commit that git
     rebase --continue makes after an edit or after a conflict, with -i or
     without, when it opens an editor for the message, or in a checkout that
     has no .githooks directory. git rebase -i runs it for a commit that it
     rewords, unless the pick of that commit stops for a conflict.
  2. .githooks/pre-push reads the stored messages of the commits that a push
     would add to the remote, and refuses the push. It also refuses a push
     when scripts/check-home-paths.sh or scripts/home-path-allow.txt is
     missing beside it. For each pushed ref these
     are the commits that are on neither the remote's copy of that ref nor the
     default branch as this clone last fetched it: the default branch of that
     remote, or the one of origin when the push names a URL or a remote whose
     default branch this clone does not know. The hook reads more commits in
     two cases. When this clone does not hold the commit that the remote's
     copy of the ref names, as in a force push over a commit that was never
     fetched, it also reads the commits of the pushed ref that are already
     on that copy. When this clone knows no default branch, of that remote
     or of origin, it reads every commit of the pushed ref that is not
     exempt and not on the remote's copy of that ref. It cannot see a push made
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
The commits of a range and of a push come from scripts/commit-range.sh, the
one function that scripts/check-home-paths.sh calls too, so both checks of the
hook read the same commits. The script starts in one of two forms, and each
has one source of that file. Started from its file (the name under which it
runs holds a slash), it reads scripts/commit-range.sh beside that file, and
when that file is missing it exits 2 and reads no commit. Run as text (the
name holds no slash, as the CI step runs the copy of the default branch with
sh -c), it never reads a file of the work tree: when the name ends in
@<full object id>, as in check-commit-messages.sh@<id>, it reads the range
file of that commit; with a bare name it reads the one of the default branch
as this clone last fetched it (--default-ref, or refs/remotes/origin/HEAD, or
refs/remotes/origin/main, the first that names a commit). When that commit
holds no scripts/commit-range.sh, it exits 2. The CI step resolves the default
branch to one commit and runs that commit's copy under the name
check-commit-messages.sh@<that commit>, so a branch cannot change the function
with which the copy of the default branch lists its commits.
No layer reads the author or the committer of a commit, or words that are
spelled in another way. Before the layers, the tracked file .claude/settings.json
tells Claude Code to add no attribution line, and the CI step checks it.

EOF
  synopsis
  cat <<'EOF'

Exit codes: 0 no attribution line, 1 at least one, 2 the check could not run,
64 bad usage. A check only reads, and a self-test writes only under the
temporary directory, so the script is safe to run twice.
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
      if ($0 ~ /^[ +@-]/) print ""; else print
      next
    }
    scissors_block_lines > 0 {
      if (index($0, "diff --git ") == 1 || index($0, "Submodule ") == 1) {
        dropping_diff = 1
        print_scissors_block()
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

load_commit_range() {
  if [ -n "$own_directory" ]; then
    [ -f "$own_directory/commit-range.sh" ] ||
      die "$own_directory/commit-range.sh, which lists the commits of a range or a push, is missing beside this script, so no commit was read. Restore it (git checkout HEAD -- scripts/commit-range.sh; in a sparse checkout without scripts/, git sparse-checkout add scripts) and run the check again."
    # shellcheck source=scripts/commit-range.sh
    . "$own_directory/commit-range.sh"
    return 0
  fi
  case $0 in
    *@*)
      range_commit=${0##*@}
      case $range_commit in
        '' | *[!0123456789abcdef]*) range_commit_is_an_id= ;;
        *) range_commit_is_an_id=1 ;;
      esac
      { [ -n "$range_commit_is_an_id" ] && { [ ${#range_commit} = 40 ] || [ ${#range_commit} = 64 ]; }; } ||
        die "this text runs under the name $0, and the part after @ is no full object id, so it cannot tell which commit's scripts/commit-range.sh lists the commits. Run it as sh -c \"<text>\" check-commit-messages.sh@<full id of a commit>, or run the file itself."
      range_source=$range_commit
      ;;
    *)
      range_source=
      for range_ref in ${default_ref:+"$default_ref"} refs/remotes/origin/HEAD refs/remotes/origin/main; do
        if range_source=$(git rev-parse -q --verify "$range_ref^{commit}" 2>/dev/null); then
          break
        fi
        range_source=
      done
      [ -n "$range_source" ] ||
        die 'this text runs without a file and without a commit in its name, and this clone names no default branch (--default-ref, refs/remotes/origin/HEAD or refs/remotes/origin/main), so no scripts/commit-range.sh can be read and no commit was checked. Run it as sh -c "<text>" check-commit-messages.sh@<full id of a commit>, fetch the default branch, or run the file itself.'
      ;;
  esac
  range_text=$(git show "$range_source:scripts/commit-range.sh" 2>/dev/null) ||
    die "the commit $range_source, from which this text takes its range function, holds no scripts/commit-range.sh, so no commit was checked. Name a commit that holds it (sh -c \"<text>\" check-commit-messages.sh@<full id>), or run the file itself."
  eval "$range_text"
}

resolve_default_ref() {
  if [ -n "$default_ref" ]; then
    git rev-parse -q --verify "$default_ref^{commit}" >/dev/null ||
      die "--default-ref $default_ref does not name a commit in this clone"
  else
    default_ref=$(commit_range_default_ref)
  fi
}

check_commits() {
  label=$1
  commits=$2

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

check_push() {
  remote=$1
  require_repository_with_exempt_commit
  load_commit_range
  resolve_default_ref
  default_ref_of_remote=$(commit_range_default_of_remote "$remote" "$default_ref")
  while IFS= read -r pushed_ref; do
    commit_range_read_push_line "$pushed_ref" ||
      die "cannot read this line that git wrote to the pre-push hook, so the push was not checked: $pushed_ref. Each line holds a local ref, a local object id, a remote ref and a remote object id of 40 or 64 lower-case hexadecimal characters; check the commits by hand with sh scripts/check-commit-messages.sh <remote>/<branch>..<local branch>, and push again with a git that writes such lines."
    range_status=0
    commits=$(commit_range_of_push "$NEWEST_EXEMPT_COMMIT" "$default_ref_of_remote") || range_status=$?
    case $range_status in
      0) ;;
      3) die "git cannot resolve the object $push_local_object that the push names for $push_local_ref, so the push was not checked" ;;
      *) die "git rev-list could not list the commits of the push of $push_local_ref" ;;
    esac
    check_commits "$(commit_range_push_label)" "$commits"
  done
  [ "$any_range_failed" = 0 ]
}

check_range() {
  require_repository_with_exempt_commit
  load_commit_range
  resolve_default_ref
  exempt_commit=
  [ "$exempt" = 0 ] || exempt_commit=$NEWEST_EXEMPT_COMMIT
  commits=$(commit_range "$exempt_commit" "$default_ref" "$1") || die "git rev-list could not list the commits of '$1'"
  check_commits "$1" "$commits"
  [ "$any_range_failed" = 0 ]
}

fixture_messages() {
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
  variation_selector=$(printf '\357\270\217')
  heavy_black_heart=$(printf '\342\235\244')
  ogham_space_mark=$(printf '\341\232\200')
  scissors_sign=$(printf '\342\234\202')
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
  expect 'a trailer that follows a variation selector fails' 1 "Fix the gate${nl}${nl}${variation_selector}$trailer"
  expect 'a Generated with line after an emoji and its variation selector fails' 1 "Fix the gate${nl}${nl}${heavy_black_heart}${variation_selector} Generated with a tool"
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
  expect 'a line that starts with Generated without passes, because a letter follows the words' 0 "Fix the gate${nl}${nl}Generated without a tool, by hand."
  expect 'a line that starts with Co-Authored-Byte passes, because a letter follows the words' 0 "Fix the gate${nl}${nl}Co-Authored-Byte: a made-up key"
  expect 'Generated with fails when a full stop follows the words' 1 "Fix the gate${nl}${nl}Generated with."
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
  expect_in_hook_file 'a trailer that a space indents passes below a scissors block and a diff header written by hand' 0 '#' "Fix the gate${nl}${nl}$scissors_block${nl}diff --git a/x b/x${nl} $trailer"
  expect_in_hook_file 'a trailer after a carriage return on an added line passes below a scissors block and a diff header written by hand' 0 '#' "Fix the gate${nl}${nl}$scissors_block${nl}diff --git a/x b/x${nl}+x${carriage_return}$trailer"
  expect_in_hook_file 'a comment line of a scissors block that names a diff header does not start the diff' 1 '#' "Fix the gate${nl}${nl}$scissors_block${nl}# see diff --git a/x b/x${nl} $trailer"
  expect_in_hook_file 'a comment line of a scissors block that names a submodule does not start the diff' 1 '#' "Fix the gate${nl}${nl}$scissors_block${nl}# see Submodule notes${nl} $trailer"
  expect_in_hook_file 'a trailer that a space indents passes below a scissors line and a line that starts with Submodule, written by hand' 0 '#' "Fix the gate${nl}${nl}# $SCISSORS${nl}Submodule notes follow${nl} $trailer"
  expect_in_hook_file 'a trailer that a space indents fails below a scissors line and a line that starts no diff' 1 '#' "Fix the gate${nl}${nl}# $SCISSORS${nl}Notes follow${nl} $trailer"
  expect_in_hook_file 'a trailer that a space indents fails below a scissors line and a line that starts with Submodules' 1 '#' "Fix the gate${nl}${nl}# $SCISSORS${nl}Submodules follow${nl} $trailer"
  expect_in_hook_file 'a trailer that a space indents fails below a diff header with no scissors line above it' 1 '#' "Fix the gate${nl}${nl}diff --git a/x b/x${nl} $trailer"
  expect_in_hook_file 'an added line of the diff that holds a carriage return before a trailer is dropped' 0 '#' "Add a fixture${nl}${nl}$scissors_block${nl}diff --git a/f b/f${nl}--- /dev/null${nl}+++ b/f${nl}@@ -0,0 +1 @@${nl}+Fix the gate${carriage_return}${carriage_return}$trailer${carriage_return}"
  expect_in_hook_file 'a removed line of the diff that holds a carriage return before the footer words is dropped' 0 '#' "Drop a log${nl}${nl}$scissors_block${nl}diff --git a/f b/f${nl}--- a/f${nl}+++ /dev/null${nl}@@ -1 +0,0 @@${nl}-Building the bindings${carriage_return}Generated with wasm-bindgen 0.2"
  expect_in_hook_file 'a hunk header of the diff that copies a line with a carriage return before the footer words is dropped' 0 '#' "Extend a log${nl}${nl}$scissors_block${nl}diff --git a/f b/f${nl}--- a/f${nl}+++ b/f${nl}@@ -2,3 +2,4 @@ Building the bindings${carriage_return}Generated with wasm-bindgen 0.2${nl} unchanged${nl}+added"
  expect_in_hook_file 'a trailer after a carriage return on a comment line of a hand-made scissors block fails' 1 '#' "Fix the gate${nl}${nl}# $SCISSORS${nl}#${carriage_return}$trailer${nl}diff --git a/x b/x"
  expect_in_hook_file 'a trailer on a comment line of a hand-made scissors block fails when the predicate skips the comment string' 1 "$scissors_sign" "Fix the gate${nl}${nl}$scissors_sign $SCISSORS${nl}$scissors_sign $trailer${nl}diff --git a/x b/x"
  expect_in_hook_file 'the diff of git commit -v is dropped when it starts with the summary of a submodule' 0 '#' "Bump the submodule${nl}${nl}$scissors_block${nl}Submodule aaa-sub 4792642..08b513d:${nl}  > Generated with love in the submodule${nl}diff --git a/notes.txt b/notes.txt${nl}--- a/notes.txt${nl}+++ b/notes.txt${nl}@@ -1,4 +1,4 @@${nl} Generated with bindgen 0.71${nl} $trailer${nl}-old line${nl}+new line"

  expect_hook_path 'the hook path exits 1 on a file with a trailer and says how to reword prose' 1 'reword it so that it does not start with these words' "Fix the gate${nl}${nl}$trailer"
  expect_hook_path 'the hook path exits 0 on a clean file' 0 '' "Add the store${nl}${nl}Stream segment files through bounded chunks."
  expect_hook_path 'the hook path exits 2 when the file does not exist' 2 'no such message file' '' /dev/null/absent

  expect_commit_msg_hook 'the commit-msg hook refuses a tree without the scanner of home paths' 1 check-home-paths.sh
  expect_commit_msg_hook 'the commit-msg hook refuses a tree without the list of the scanner' 1 home-path-allow.txt
  expect_commit_msg_hook 'the commit-msg hook accepts a clean message in a tree with both' 0 ''
}

self_test() {
  failures=0
  fixtures=0

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
    name=$1 want_status=$2 want_text=$3 hook_file_text=$4
    if [ $# -ge 5 ]; then
      hook_file=$5
    else
      hook_file=$(mktemp "${TMPDIR:-/tmp}/check-commit-messages-self-test.XXXXXX") || die 'mktemp could not make a file'
      printf '%s\n' "$hook_file_text" >"$hook_file" || die "cannot write $hook_file"
    fi
    status=0
    (check_hook_file "$hook_file") >/dev/null 2>&1 || status=$?
    text=present
    if [ -n "$want_text" ]; then
      (check_hook_file "$hook_file") 2>&1 | grep -qF -- "$want_text" || text=absent
    fi
    fixtures=$((fixtures + 1))
    if [ "$status" = "$want_status" ] && [ "$text" = present ]; then
      printf 'PASS %s (the message file is %s)\n' "$name" "$hook_file"
    else
      printf 'FAIL %s (expected exit %s and the text "%s" for the message file %s; got exit %s, text %s)\n' "$name" "$want_status" "$want_text" "$hook_file" "$status" "$text"
      failures=$((failures + 1))
    fi
  }

  expect_commit_msg_hook() {
    name=$1 want_status=$2 left_out=$3
    hook_tree=$(mktemp -d "${TMPDIR:-/tmp}/check-commit-messages-hook-tree.XXXXXX") || die 'mktemp could not make a directory'
    mkdir -p "$hook_tree/.githooks" "$hook_tree/scripts" || die "cannot make the tree $hook_tree"
    cp "$own_directory/../.githooks/commit-msg" "$hook_tree/.githooks/commit-msg" || die "this self-test runs a copy of .githooks/commit-msg, and $own_directory/../.githooks/commit-msg cannot be copied"
    cp "$own_directory/$(basename "$0")" "$hook_tree/scripts/check-commit-messages.sh" || die 'cannot copy this script'
    for beside in check-home-paths.sh home-path-allow.txt; do
      [ "$beside" = "$left_out" ] || cp "$own_directory/$beside" "$hook_tree/scripts/$beside" || die "cannot copy scripts/$beside"
    done
    printf 'Add the store\n' >"$hook_tree/message" || die "cannot write $hook_tree/message"
    status=0
    sh "$hook_tree/.githooks/commit-msg" "$hook_tree/message" >"$hook_tree/out" 2>&1 || status=$?
    text=present
    [ -z "$left_out" ] || grep -qF -- "scripts/$left_out is missing" "$hook_tree/out" || text=absent
    [ -z "$left_out" ] || grep -qF -- "git checkout HEAD -- scripts/$left_out; in a sparse checkout without scripts/, git sparse-checkout add scripts" "$hook_tree/out" || text=absent
    fixtures=$((fixtures + 1))
    if [ "$status" = "$want_status" ] && [ "$text" = present ]; then
      printf 'PASS %s (the tree is %s)\n' "$name" "$hook_tree"
    else
      printf 'FAIL %s (expected exit %s and a line that names scripts/%s; got exit %s, text %s; the tree is %s)\n' "$name" "$want_status" "$left_out" "$status" "$text" "$hook_tree"
      failures=$((failures + 1))
    fi
  }

  bytes_in_hex() {
    printf '%s' "$1" | od -An -tx1 | tr -d ' \n'
  }

  expect_comment_string_read_from_git() {
    name=$1 want=$2
    config_file=$(mktemp "${TMPDIR:-/tmp}/check-commit-messages-self-test.XXXXXX") || die 'mktemp could not make a file'
    printf '[core]\n\tcommentString = "%s"\n' "$want" >"$config_file" || die "cannot write $config_file"
    got=$(
      export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0="$config_file" LC_ALL=en_US.UTF-8
      git_comment_string 2>/dev/null
    ) || got=
    fixtures=$((fixtures + 1))
    if [ "$got" = "$want" ]; then
      printf 'PASS %s (from the git config file %s, which is left in place)\n' "$name" "$config_file"
    else
      printf 'FAIL %s (the git config file %s holds a comment string with the bytes [%s], and the script read the bytes [%s])\n' "$name" "$config_file" "$(bytes_in_hex "$want")" "$(bytes_in_hex "$got")"
      failures=$((failures + 1))
    fi
  }

  fixture_messages

  if [ "$failures" -gt 0 ]; then
    printf 'check-commit-messages: self-test FAIL: %d of %d fixtures misclassified\n' "$failures" "$fixtures"
    return 1
  fi
  printf 'check-commit-messages: self-test PASS: %d fixtures\n' "$fixtures"
}

self_test_two_paths() {
  script=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
  root=$(cd "$(dirname "$0")/.." && pwd)
  [ -f "$root/.githooks/commit-msg" ] || die "this self-test runs the commit-msg hook, and $root/.githooks/commit-msg does not exist"
  for repository_variable in $(git rev-parse --local-env-vars); do
    unset "$repository_variable"
  done
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 ||
    die 'the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.'
  work=$(mktemp -d "${TMPDIR:-/tmp}/check-commit-messages-two-paths-self-test.XXXXXX") || die 'mktemp could not make a directory'
  printf 'check-commit-messages: two-path self-test in %s, which is left in place\n' "$work"
  mkdir -p "$work/tree/.githooks" "$work/tree/scripts" "$work/messages" "$work/out"
  cp "$root/.githooks/commit-msg" "$work/tree/.githooks/commit-msg"
  cp "$script" "$work/tree/scripts/check-commit-messages.sh"
  cp "$root/scripts/check-home-paths.sh" "$root/scripts/commit-range.sh" "$root/scripts/home-path-allow.txt" "$work/tree/scripts/"
  chmod +x "$work/tree/.githooks/commit-msg"
  git init -q "$work/repository"
  git -C "$work/repository" config core.hooksPath "$work/tree/.githooks"
  failures=0
  fixtures=0
  expected_differences_seen=0
  newline='
'
  space_indented_line_below_a_diff_header='the diff of git commit -v fails when it is a stored commit message
the diff below the scissors block of git commit -v is dropped from the hook file
the same diff is dropped when the comment string of git is a semicolon
the same diff is dropped when the comment string is auto and git picked a semicolon
the diff below a scissors block with empty lines, as git writes when it concludes a merge, is dropped from the hook file
the diff of git commit -v is dropped when the comment string is two backslashes
the diff of git commit -v is dropped when the comment string is a backslash and the letter t
the diff of git commit -v is dropped when the comment string is that byte
the diff of git commit -v is dropped when it starts with the summary of a submodule
a trailer that a space indents passes below a scissors block and a diff header written by hand
a trailer that a space indents passes below a scissors line and a line that starts with Submodule, written by hand'
  line_after_a_carriage_return_in_a_skipped_line='an added line of the diff that holds a carriage return before a trailer is dropped
a removed line of the diff that holds a carriage return before the footer words is dropped
a hunk header of the diff that copies a line with a carriage return before the footer words is dropped
a trailer after a carriage return on an added line passes below a scissors block and a diff header written by hand'
  hook_accepts_and_range_check_refuses=$space_indented_line_below_a_diff_header$newline$line_after_a_carriage_return_in_a_skipped_line

  in_repository() {
    git -C "$work/repository" -c commit.gpgsign=false -c core.commentString="$comment_string" "$@"
  }

  two_paths() {
    name=$1 comment_string=$2 text=$3
    fixtures=$((fixtures + 1))
    printf '%s\n' "$text" >"$work/messages/$fixtures.txt"
    before=$(in_repository rev-parse HEAD)
    hook=accepts
    if ! in_repository commit -q --allow-empty --cleanup=whitespace -F "$work/messages/$fixtures.txt" >"$work/out/$fixtures.hook.txt" 2>&1; then
      hook=refuses
      if ! grep -qF 'check-commit-messages: the message carries an attribution line' "$work/out/$fixtures.hook.txt" ||
        ! in_repository commit -q --allow-empty --no-verify --cleanup=whitespace -F "$work/messages/$fixtures.txt" >"$work/out/$fixtures.no-verify.txt" 2>&1; then
        hook='fails for another reason than the hook'
      fi
    fi
    after=$(in_repository rev-parse HEAD)
    status=0
    (cd "$work/repository" && exempt=0 && check_range "$before..$after") >"$work/out/$fixtures.range.txt" 2>&1 || status=$?
    case $status in
      0) range=accepts ;;
      1) range=refuses ;;
      *) range="exits $status" ;;
    esac
    case $newline$hook_accepts_and_range_check_refuses$newline in
      *"$newline$name$newline"*) listed=1 ;;
      *) listed=0 ;;
    esac
    case "$listed $hook/$range" in
      '0 accepts/accepts' | '0 refuses/refuses')
        printf 'PASS %s (both paths: %s)\n' "$name" "$hook"
        ;;
      '1 accepts/refuses')
        expected_differences_seen=$((expected_differences_seen + 1))
        printf 'PASS %s (a listed hand-made shape: the hook accepts, the range check refuses)\n' "$name"
        ;;
      1*)
        printf 'FAIL %s (listed as a shape that the hook accepts and the range check refuses, and now git commit with the hook %s and the range check %s. Take it off the list in self_test_two_paths.)\n' "$name" "$hook" "$range"
        failures=$((failures + 1))
        ;;
      *)
        printf 'FAIL %s (git commit with the hook %s, and the range check on the stored commit %s. The two paths must agree, unless the shape is one that --help names for --message-file and self_test_two_paths lists it. Outputs: %s/out/%s.*.txt)\n' "$name" "$hook" "$range" "$work" "$fixtures"
        failures=$((failures + 1))
        ;;
    esac
  }

  expect() { two_paths "$1" '#' "$3"; }
  expect_at_lines() { two_paths "$1" '#' "$3"; }
  expect_in_hook_file() { two_paths "$1" "$3" "$4"; }
  expect_in_hook_file_at_lines() { two_paths "$1" "$3" "$4"; }
  expect_hook_path() { [ $# -ge 5 ] || two_paths "$1" '#' "$4"; }
  expect_comment_string_read_from_git() { :; }
  expect_commit_msg_hook() { :; }

  comment_string='#'
  in_repository commit -q --allow-empty --no-verify -m 'Start the repository'
  fixture_messages

  listed_count=$(printf '%s\n' "$hook_accepts_and_range_check_refuses" | grep -c . || true)
  if [ "$expected_differences_seen" != "$listed_count" ]; then
    printf 'FAIL the list of hand-made shapes in self_test_two_paths names %s fixtures, and %s fixtures of that name ran and differed (a name on the list that no fixture has?)\n' "$listed_count" "$expected_differences_seen"
    failures=$((failures + 1))
  fi
  if [ "$failures" -gt 0 ]; then
    printf 'check-commit-messages: two-path self-test FAIL: %d problems in %d fixtures. Outputs are in %s/out.\n' "$failures" "$fixtures" "$work"
    return 1
  fi
  printf 'check-commit-messages: two-path self-test PASS: %d fixtures went through git commit with the hook and through the range check on the stored commit. The verdicts agree for %d, and %d are the listed hand-made shapes. Outputs are in %s/out.\n' "$fixtures" "$((fixtures - expected_differences_seen))" "$expected_differences_seen" "$work"
}

self_test_pre_push() {
  script=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
  root=$(cd "$(dirname "$0")/.." && pwd)
  git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || die 'this self-test clones the repository, and the script is not inside one'
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 ||
    die 'the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.'
  work=$(mktemp -d "${TMPDIR:-/tmp}/check-commit-messages-pre-push-self-test.XXXXXX") || die 'mktemp could not make a directory'
  printf 'check-commit-messages: pre-push self-test in %s, which is left in place\n' "$work"
  mkdir -p "$work/tree/.githooks" "$work/tree/scripts" "$work/out"
  cp "$root/.githooks/pre-push" "$work/tree/.githooks/pre-push"
  cp "$script" "$work/tree/scripts/check-commit-messages.sh"
  cp "$root/scripts/check-home-paths.sh" "$root/scripts/commit-range.sh" "$root/scripts/home-path-allow.txt" "$work/tree/scripts/"
  chmod +x "$work/tree/.githooks/pre-push"
  git clone -q --no-local "$root" "$work/clone"
  git -C "$work/clone" config core.hooksPath "$work/tree/.githooks"
  git init -q --bare "$work/remote.git"
  git init -q --bare "$work/empty-remote.git"
  git -C "$work/clone" remote add proof "$work/remote.git"
  git -C "$work/clone" remote add proof-empty "$work/empty-remote.git"
  tree=$(git -C "$work/clone" rev-parse "$NEWEST_EXEMPT_COMMIT^{tree}") ||
    die "the self-test builds its commits on the newest exempt commit $NEWEST_EXEMPT_COMMIT, and the clone of this repository does not hold it. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
  base=$(git -C "$work/clone" commit-tree "$tree" -p "$NEWEST_EXEMPT_COMMIT" -m 'Hold the tree of the newest exempt commit above it') ||
    die "the self-test builds its commits on the newest exempt commit $NEWEST_EXEMPT_COMMIT, and git could not make a commit above it in the clone of this repository. Fetch the full history: git fetch --unshallow (in CI, fetch-depth: 0 on actions/checkout)."
  failures=0
  pushes=0
  nl='
'
  trailer='Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>'
  no_commit=0000000000000000000000000000000000000000

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

  expect_push a-commit-above-the-exempt-commits 0 'commits checked 1, with an attribution line 0' proof "$base:refs/heads/base"
  expect_push a-clean-commit 0 'commits checked ' proof "$clean:refs/heads/clean"
  expect_push a-commit-with-a-trailer 1 'with an attribution line 1' proof "$with_trailer:refs/heads/trailer"
  expect_push the-same-commit-to-a-path 1 'with an attribution line 1' "$work/remote.git" "$with_trailer:refs/heads/trailer-by-path"
  expect_push a-clean-ref-and-a-ref-with-a-trailer-in-one-push 1 'with an attribution line 1' proof "$clean:refs/heads/clean-again" "$with_trailer:refs/heads/trailer-again"
  expect_push a-deleted-ref 0 'deleted' proof :refs/heads/clean
  expect_push the-exempt-commits-alone 0 'commits checked 0,' proof-empty "$NEWEST_EXEMPT_COMMIT:refs/heads/exempt"

  git -C "$work/clone" checkout -q --detach "$NEWEST_EXEMPT_COMMIT"
  git -C "$work/clone" cherry-pick --keep-redundant-commits "$NEWEST_EXEMPT_COMMIT" >"$work/out/cherry-pick.txt" 2>&1 ||
    die "the self-test could not cherry-pick the newest exempt commit onto itself. git printed: $(cat "$work/out/cherry-pick.txt")"
  expect_push a-cherry-pick-of-the-newest-exempt-commit 1 'Generated with [Claude Code]' proof HEAD:refs/heads/cherry-picked

  git -C "$work/clone" tag -a -m 'A tag on a commit with a trailer' tag-on-a-trailer "$with_trailer"
  expect_push an-annotated-tag-on-a-commit-with-a-trailer 1 'with an attribution line 1' proof refs/tags/tag-on-a-trailer

  git -C "$work/clone" branch -f spaced "$with_trailer_and_a_subject_of_its_own"
  expect_push a-source-expression-that-holds-a-space 1 'with an attribution line 1' proof 'spaced^{/gate two}:refs/heads/spaced'

  git -C "$work/clone" replace "$hidden_by_a_replace_ref" "$shown_by_the_replace_ref"
  expect_push a-commit-under-a-replace-ref 1 'with an attribution line 1' proof "$hidden_by_a_replace_ref:refs/heads/replaced"

  git clone -q --no-local "$work/remote.git" "$work/other-clone" 2>/dev/null
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

  git -C "$work/clone" update-ref refs/remotes/origin/base "$base"
  git -C "$work/clone" symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/base
  expect_push a-clean-commit-above-the-default-branch-of-origin-to-a-path 0 'commits checked 1,' "$work/remote.git" "$clean:refs/heads/clean-by-path"

  git -C "$work/clone" symbolic-ref --delete refs/remotes/origin/HEAD
  git -C "$work/clone" update-ref refs/remotes/origin/main "$base"
  expect_push a-clean-commit-to-a-path-above-origin-main-in-a-clone-without-origin-head 0 'commits checked 1,' "$work/remote.git" "$clean:refs/heads/clean-by-path-over-origin-main"
  git -C "$work/clone" update-ref -d refs/remotes/origin/main
  expect_push a-commit-with-a-trailer-to-a-path-in-a-clone-without-a-default-branch 1 'This clone names no default branch' "$work/remote.git" "$with_trailer:refs/heads/trailer-without-a-default-branch"

  expect_push a-clean-commit-to-a-remote-without-fetched-refs-in-a-clone-without-a-default-branch 0 'commits checked 2,' proof-empty "$clean:refs/heads/clean-to-an-unfetched-remote"

  in_other_clone checkout -q -b unseen origin/base
  in_other_clone commit -q --allow-empty --no-verify -m 'Add a change that the first clone never fetches'
  in_other_clone push -q origin unseen:refs/heads/unseen
  expect_push a-forced-push-over-a-commit-that-this-clone-lacks 0 'commits checked 1,' proof "+$clean:refs/heads/unseen"
  git -C "$work/clone" -c tag.gpgsign=false tag -a -m 'A tag of a tree' tag-of-a-tree "$tree"
  expect_push an-annotated-tag-of-a-tree 0 'commits checked 0,' proof refs/tags/tag-of-a-tree

  status=0
  (cd "$work/clone" && printf 'not a line that git writes\n' | sh "$work/tree/scripts/check-commit-messages.sh" --pre-push proof) >"$work/out/a-line-that-git-does-not-write.txt" 2>&1 || status=$?
  judge a-line-that-git-does-not-write "$status" 2 'cannot read this line'
  utf8_locale=$(locale -a 2>/dev/null | grep -m 1 -E '^(en_US|C)\.(UTF-8|utf8)$') || utf8_locale=
  [ -n "$utf8_locale" ] || printf 'case a-line-with-one-field-that-is-no-object-id under a UTF-8 locale not run: this system names no UTF-8 locale\n'
  for case_locale in C ${utf8_locale:+"$utf8_locale"}; do
    one_field_got=2
    line_number=0
    for bad_line in "refs/heads/x $clean refs/heads/x A${clean#?}" "refs/heads/x A${clean#?} refs/heads/x $no_commit" \
      "refs/heads/x $clean refs/heads/x F${clean#?}" "refs/heads/x F${clean#?} refs/heads/x $no_commit"; do
      line_number=$((line_number + 1))
      status=0
      (cd "$work/clone" && printf '%s\n' "$bad_line" | LC_ALL=$case_locale sh "$work/tree/scripts/check-commit-messages.sh" --pre-push proof) >"$work/out/one-field-$case_locale-$line_number.txt" 2>&1 || status=$?
      if [ "$status" != 2 ] || ! grep -qF 'cannot read this line' "$work/out/one-field-$case_locale-$line_number.txt"; then
        one_field_got="line-$line_number-exit-$status"
      fi
      cat "$work/out/one-field-$case_locale-$line_number.txt" >>"$work/out/a-line-with-one-field-that-is-no-object-id-$case_locale.txt"
    done
    judge "a-line-with-one-field-that-is-no-object-id-$case_locale" "$one_field_got" 2 'cannot read this line'
    status=0
    (cd "$work/clone" && printf 'refs/heads/x %s refs/heads/x %s\n' "${clean%?}" "$no_commit" | LC_ALL=$case_locale sh "$work/tree/scripts/check-commit-messages.sh" --pre-push proof) >"$work/out/a-line-with-an-object-id-of-39-characters-$case_locale.txt" 2>&1 || status=$?
    judge "a-line-with-an-object-id-of-39-characters-$case_locale" "$status" 2 'cannot read this line'
  done

  status=0
  (cd "$work/clone" && printf 'refs/heads/x %s refs/heads/x %s\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 0000000000000000000000000000000000000000000000000000000000000000 | sh "$work/tree/scripts/check-commit-messages.sh" --pre-push proof) >"$work/out/a-line-with-two-object-ids-of-64-characters.txt" 2>&1 || status=$?
  judge a-line-with-two-object-ids-of-64-characters "$status" 2 'git cannot resolve the object aaaaaaaa'
  status=0
  (cd "$work/clone" && printf 'refs/heads/x %s refs/heads/x %s\n' 1234567890123456789012345678901234567890 "$no_commit" | sh "$work/tree/scripts/check-commit-messages.sh" --pre-push proof) >"$work/out/a-pushed-object-that-the-clone-lacks.txt" 2>&1 || status=$?
  judge a-pushed-object-that-the-clone-lacks "$status" 2 'git cannot resolve the object 1234567890123456789012345678901234567890'

  exempt_tree=$work/tree-with-an-exempt-commit-named-by-a-later-one
  mkdir -p "$exempt_tree/.githooks" "$exempt_tree/scripts"
  cp "$work/tree/.githooks/pre-push" "$exempt_tree/.githooks/pre-push"
  cp "$work/tree/scripts/check-home-paths.sh" "$work/tree/scripts/commit-range.sh" "$work/tree/scripts/home-path-allow.txt" "$exempt_tree/scripts/"
  push_with_an_exempt_commit_named_by_a_later_one() {
    name=$1 earlier=$2 later_tree=$3 want_text=$4
    sed "s/^NEWEST_EXEMPT_COMMIT=.*/NEWEST_EXEMPT_COMMIT=$earlier/" "$work/tree/scripts/check-commit-messages.sh" >"$exempt_tree/scripts/check-commit-messages.sh"
    later=$(git -C "$work/clone" commit-tree "$later_tree" -p "$earlier" -m 'Name the commit below as the newest exempt one')
    status=0
    git -C "$work/clone" -c core.hooksPath="$exempt_tree/.githooks" push proof "$later:refs/heads/$name" >"$work/out/$name.txt" 2>&1 || status=$?
    judge "$name" "$status" 1 "$want_text"
  }
  earlier_with_a_trailer=$(commit_on_base "Fix the gate three$nl$nl$trailer")
  push_with_an_exempt_commit_named_by_a_later_one a-later-commit-names-an-earlier-one-with-a-trailer-as-exempt "$earlier_with_a_trailer" "$tree" 'commits checked 2, with an attribution line 1'
  note_blob=$(printf 'see /%s/zed/notes\n' Users | git -C "$work/clone" hash-object -w --stdin)
  tree_with_a_note=$({ git -C "$work/clone" ls-tree "$tree"; printf '100644 blob %s\tzed-notes.txt\n' "$note_blob"; } | git -C "$work/clone" mktree)
  earlier_with_a_home_path=$(git -C "$work/clone" commit-tree "$tree_with_a_note" -p "$base" -m 'Add a note')
  push_with_an_exempt_commit_named_by_a_later_one a-later-commit-names-an-earlier-one-with-a-home-path-as-exempt "$earlier_with_a_home_path" "$tree_with_a_note" 'commits read 2, lines with a home path 1.'

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
  [ -z "$mode" ] || [ "$mode" = "$1" ] || usage 'a range, --pre-push, --message-file, --stored-message and each self-test exclude each other'
  mode=$1
}
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) set_exclusive_mode self-test ;;
    --self-test-pre-push) set_exclusive_mode self-test-pre-push ;;
    --self-test-two-paths) set_exclusive_mode self-test-two-paths ;;
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
  self-test-two-paths) self_test_two_paths ;;
  message) check_hook_file "$file" ;;
  stored-message) check_stored_message ;;
  pre-push) check_push "$remote" ;;
  range) check_range "${range:-HEAD}" ;;
esac
