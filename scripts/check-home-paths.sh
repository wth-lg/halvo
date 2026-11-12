#!/bin/sh
set -u
export GIT_NO_REPLACE_OBJECTS=1
LC_ALL=C
export LC_ALL

LIST_IN_THE_REPOSITORY=scripts/home-path-allow.txt
TAB=$(printf '\t')
NAME=check-home-paths

script_dir=$(dirname "$0")
root=$(cd "$script_dir/.." && pwd -P) || {
  printf '%s: cannot check: the directory above %s cannot be entered\n' "$NAME" "$script_dir" >&2
  exit 2
}

help_text() {
  cat <<'EOF'
check-home-paths: refuse a home path in the lines that commits add.

  sh scripts/check-home-paths.sh [--allow <file>] <revision>...
      Read every commit that git rev-list <revision>... lists, for example
      52a4687e..HEAD or origin/main, and the lines that each of them adds.
  sh scripts/check-home-paths.sh [--allow <file>] --pre-push <remote>
      Read the commits that a push would add to <remote>, from the lines that
      git writes to the standard input of a pre-push hook. .githooks/pre-push
      runs this after its check of the commit messages.
  sh scripts/check-home-paths.sh --self-test
      Prove the grammar on its fixtures, the reading of commits in throwaway
      repositories, and .githooks/pre-push with pushes to a local bare remote.
      Needs a name and an email for commits: a configured git user, or the
      variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and
      GIT_COMMITTER_EMAIL.
  sh scripts/check-home-paths.sh --help

Why. A home path names the account of a machine, and this repository is
public: a line that a push carries out cannot be taken back by a later
commit.

The grammar. A home path is a form below, then a name.

A separator is a slash, a backslash, or %2F or %5C in either case. A run of
separators counts as one, so a JSON string that doubles its backslashes or
writes \/ for a slash counts too.

A name starts with an ASCII letter, a digit, an underscore or a byte above
127, and goes on with those, a period and a hyphen. It ends before a
character of Unicode's General Punctuation (U+2000 to U+206F: quotation
marks, dashes, an ellipsis, blanks), before the guillemets and the no-break
space (U+00AB, U+00BB, U+00A0) and before CJK punctuation (U+3000 to
U+303F), so "/Users/<name>" in curly quotes ends at the name. A name of one
letter after a backslash and before +, *, ? or { is the escape of a regular
expression (/Users/\w+) and passes. Any other first character
makes a placeholder, which passes: /Users/<name>, /Users/$USER,
/home/${USER}, C:\Users\%USERNAME%, /home/*, /Users/{name}, /Users/... and
/home/.config. The directory alone passes, as in "under /Users/ or /home/".
The directories that the systems keep for themselves pass, in any case:
Shared, Public, Default (so Default User too) and All Users.

The forms. A form in this list counts where an earlier form of the list did
not already take the same characters.
  wsl-unc        separators, wsl$ or wsl.localhost, a separator, the name
                 of a distribution, a separator, home, a separator; or
                 separators, wsl+ or wsl%2B and the distribution, as an
                 editor's remote address writes it, a separator, home, a
                 separator
  macos-volume   /System/Volumes/Data/Users/ or /Volumes/<volume>/Users/
  file-url-host  file: and separators, a host, a separator, then Users or
                 home in any case, a separator
  url-file-host  :// and the host file (an editor's link), a separator, then
                 Users or home in any case, a separator
  cygwin         /cygdrive/<drive letter>/Users/, Users in any case
  wsl-mnt        /mnt/<drive letter>/Users/, Users in any case
  git-bash       /<drive letter>/Users/, Users in any case
  windows-old    a drive letter, a colon or %3A and a separator, or a
                 backslash, then Documents and Settings in any case, a
                 separator
  windows-drive  a drive letter in either case, a colon or %3A, a
                 separator, Users in any case, a separator
  windows-root   a backslash, Users in any case, a separator
  macos          a separator, Users in this case only, a separator
  linux          a separator, home in this case only, a separator
  dash           -Users- or -home- in this case only, or a drive letter,
                 two hyphens and Users- in any case: the form that a tool
                 writes into the name of a directory; the name may hold
                 hyphens, and its part before the first hyphen is the one
                 that is compared with the directories of the systems

Where a form may start. The forms but dash and url-file-host count at the
start of a line and after any character but an ASCII letter, a digit, an
underscore, a hyphen or a period: after a blank, a quote, an equals sign, a
colon, a bracket, a comma or a slash, so also in file:///Users/<name>. They
count after two periods (../../Users/<name>), after a percent-encoded byte
(%3D/Users/<name>), after a backslash and a letter (the escape \n or \t of a
JSON or a Python string), and after a hyphen and a letter at the start of a
word (a flag of a compiler or a linker: -I/Users/<name>). They do not count
inside a path below another directory
(src/home/<name>) or in the path of a web address (https://<host>/home/<name>).
url-file-host counts anywhere. dash counts at the start of a line and after
a blank, a tab, a quote, a backtick, a slash, a backslash, an equals sign, a
colon, a comma, or an opening bracket of any kind.

The self-test fixtures that hold each form (form: fixture...):
FIXTURES-BEGIN
wsl-unc: wsl-unc-dollar wsl-unc-localhost wsl-unc-forward-slashes wsl-unc-file-url wsl-unc-editor-remote
macos-volume: macos-volume-data macos-volume-named
file-url-host: file-url-host-localhost file-url-host-linux file-url-host-drive
url-file-host: url-file-host-editor-link
cygwin: cygwin-drive
wsl-mnt: wsl-mnt-drive
git-bash: git-bash-drive git-bash-upper-drive
windows-old: windows-old-drive windows-old-root
windows-drive: windows-drive-backslash windows-drive-forward-slash windows-drive-lower-case windows-drive-upper-case windows-drive-json-doubled windows-drive-percent-encoded windows-drive-long-path windows-drive-name-all
windows-root: windows-root-exact-case windows-root-backslash windows-root-unc-share
macos: macos-after-blank macos-in-double-quotes macos-in-single-quotes macos-after-equals macos-after-colon macos-json-escaped-slash macos-percent-encoded macos-percent-encoded-lower macos-after-encoded-byte macos-after-two-periods macos-file-url macos-file-url-one-slash macos-name-with-digit macos-name-with-underscore macos-name-non-ascii macos-in-curly-quotes macos-in-guillemets macos-at-line-start macos-carriage-return macos-after-tab macos-after-bracket macos-after-scp-host macos-after-json-newline-escape macos-after-one-letter-flag
linux: linux-home linux-home-after-colon linux-home-numeric linux-after-python-escape linux-after-one-letter-flag
dash: dash-macos dash-linux dash-after-slash dash-windows
FIXTURES-END

What is read. For each commit, the lines that it adds: against its parent, a
root commit against the empty tree, and a merge only the lines that stand in
none of its parents (git log --cc), since a line that a merge takes from a
parent was added by a commit of that parent, which is read itself or is
already on the remote. For a file that git calls binary, git log --cc prints
no lines of a merge, so the merge's own lines in it are found by comparing
the merge's copy of the file with the copies of its parents, line by line;
when git quotes the name of such a file, the commits count as refused.
Every file is read: a file that git calls binary, a
lock file and a generated file by their bytes (git log --text, with no
textconv and no external diff), split at each newline, with every NUL byte
taken out first, so that UTF-16 text reads as ASCII; the target of a symbolic
link; and the name of a file that a commit adds, as line 0. A renamed file
counts as added in full. A line that a later commit removes again still
counts, because the commit that holds it is pushed. A carriage return at the
end of a line changes no answer. The settings of a clone that change what git
log prints (colour, prefixes, a relative diff, renames, submodule logs,
ignored submodules, blank context lines, quoted names) are set for the call.

The commits of a push are the ones that the hook's check of commit messages
reads (sh scripts/check-commit-messages.sh --help, layer 2): for each pushed
ref the commits that are on neither the remote's copy of that ref nor the
default branch as this clone last fetched it, and not the newest exempt
commit of that script or a commit below it. A deleted ref adds none. A tag
is read through the commit that it names. So a commit that the default
branch held when this clone last fetched it is not read, also after the
remote's default branch was rewritten to drop it, and a push to a remote
that is named by its URL, or whose default branch this clone does not know,
leaves out the commits of origin's default branch: fetch before you push
after a rewrite, and run sh scripts/check-home-paths.sh <base>..<head>
yourself before a push to another remote.

The list. scripts/home-path-allow.txt in the checkout where this script
runs, or the file that --allow names, holds one entry per line: the path of
a file as git prints it with core.quotepath=off, a tab, the home path as this
script matches it (from the first character of the form to the last of the
name, as /Users/<name>), a tab, and the reason. A line that starts with # and
an empty line are skipped, and a carriage return at the end of a line is
dropped. A hit passes when its file and its path stand in one entry, so a new
name in a listed file is still refused. A line of the list itself passes when
its path is the path of an entry. A line with fewer than three fields, with
an empty file or reason, or with a path that is not one home path as a whole
makes the list malformed. An entry is only for a made-up name that a fixture
holds on purpose, never the home path of a person's machine: in the pre-push
mode the scan refuses an entry whose name is the name of the home directory
of the account that runs it (the last part of $HOME, in any case), and names
the entry by its line. The range mode does not apply that rule, because the
account of a hosted runner in CI is no person's.

Output and exit. A hit prints the commit, the file, the line number and the
form, and never the text of the line. Exit 0 when no added line holds a home
path that the list does not allow, 1 when one does, 2 when the check could
not run (git failed, a range or a line that git wrote cannot be read, the
list is missing or malformed), 64 on bad usage. In the pre-push mode 1 and 2
both make .githooks/pre-push refuse the push. A temporary directory of the
script goes by its own trap.

Limits. The scan does not find a path spelled in a way that the grammar does
not hold: /users/<name> in lower case, /var/home/<name>, a name whose first
character is percent-encoded, an HTML entity or a \u escape for a separator,
base64, a compressed file (zip, gzip, a PNG chunk), UTF-32, a path broken
over two lines. It does not find an account name outside a path, such as an
email address or a host name. It reads no commit message, no tag message, no
author or committer, and no blob or tree that a tag names without a commit.
The list is read from the work tree, not from the pushed commits. The hook
does not run for git push --no-verify, or in a clone where
sh scripts/prerequisites.sh has not set core.hooksPath, and when this script
is not beside .githooks/pre-push the hook says that the scan did not run
and lets the push go on.
EOF
}

usage() {
  printf '%s: %s\n' "$NAME" "$1" >&2
  printf 'Usage: sh scripts/check-home-paths.sh [--allow <file>] (<revision>... | --pre-push <remote>) | --self-test | --help\n' >&2
  exit 64
}

die() {
  printf '%s: cannot check, so the commits count as refused: %s\n' "$NAME" "$1" >&2
  exit 2
}

make_work() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$NAME.XXXXXX") || die 'mktemp could not make a directory'
  trap 'rm -rf "$work"' EXIT
  trap 'exit 2' INT TERM HUP
}

scan_program() {
  cat <<'EOF'
function add(name, lower, prefix, kind) {
  nforms++
  form[nforms] = name
  low[nforms] = lower
  pre[nforms] = prefix
  re[nforms] = prefix NAMED
  kind_of[nforms] = kind
}
function init(   S) {
  unicode_end[1] = "\342\200"
  unicode_end[2] = "\342\201"
  unicode_end[3] = "\302\253"
  unicode_end[4] = "\302\273"
  unicode_end[5] = "\302\240"
  unicode_end[6] = "\343\200"
  S = "([/\\\\]|%2[Ff]|%5[Cc])+"
  NAMED = "([^[:space:][:cntrl:][:punct:]]|_)([^[:space:][:cntrl:][:punct:]]|[._-])*"
  add("wsl-unc", 1, S "wsl(\\$|\\.localhost)" S "[^/\\\\%]+" S "home" S, "path")
  add("wsl-unc", 1, S "wsl(%2b|\\+)[^/\\\\%]+" S "home" S, "path")
  add("macos-volume", 0, S "System" S "Volumes" S "Data" S "Users" S, "path")
  add("macos-volume", 0, S "Volumes" S "[^/\\\\%]+" S "Users" S, "path")
  add("file-url-host", 1, "file:" S "[^/\\\\%[:space:]]+" S "(users|home)" S, "path")
  add("url-file-host", 1, "://file" S "(users|home)" S, "any")
  add("cygwin", 1, S "cygdrive" S "[a-z]" S "users" S, "path")
  add("wsl-mnt", 1, S "mnt" S "[a-z]" S "users" S, "path")
  add("git-bash", 1, S "[a-z]" S "users" S, "path")
  add("windows-old", 1, "([a-z](:|%3a)" S "|(\\\\|%5c)+)documents and settings" S, "path")
  add("windows-drive", 1, "[a-z](:|%3a)" S "users" S, "path")
  add("windows-root", 1, "(\\\\|%5c)+users" S, "path")
  add("macos", 0, S "Users" S, "path")
  add("linux", 0, S "home" S, "path")
  add("dash", 0, "-(Users|home)-", "word")
  add("dash", 1, "[a-z]--users-", "word")
}
function starts_here(text, p, k,   c) {
  if (k == "any" || p == 1) return 1
  c = substr(text, p - 1, 1)
  if (k == "word") return index(" \t\"'`/\\=:,([{<", c) > 0
  if (c ~ /[A-Za-z0-9_-]/) {
    if (p >= 4 && substr(text, p - 3, 1) == "%" && substr(text, p - 2, 2) ~ /^[0-9A-Fa-f][0-9A-Fa-f]$/) return 1
    if (c !~ /[A-Za-z]/ || p < 3) return 0
    if (substr(text, p - 2, 1) == "\\") return 1
    return substr(text, p - 2, 1) == "-" && (p == 3 || index(" \t\"'`=,([{<", substr(text, p - 3, 1)) > 0)
  }
  if (c == ".") return p >= 3 && substr(text, p - 2, 1) == "."
  return 1
}
function name_in(m, i,   t) {
  t = low[i] ? tolower(m) : m
  if (match(t, "^" pre[i])) return substr(m, RLENGTH + 1)
  return m
}
function name_length(name,   k, end, s) {
  end = length(name) + 1
  for (s = 1; s <= 6; s++) {
    k = index(name, unicode_end[s])
    if (k && k < end) end = k
  }
  return end - 1
}
function own_directory(name, after, i,   n) {
  n = tolower(name)
  if (kind_of[i] == "word") sub(/-.*/, "", n)
  if (n == "shared" || n == "public" || n == "default") return 1
  return n == "all" && tolower(after) ~ /^ users([^a-z0-9._-]|$)/
}
function covered(p,   k) {
  for (k = 1; k <= ncov; k++)
    if (p >= cs[k] && p <= ce[k]) return 1
  return 0
}
function scan_text(text, file, use_list,   lowered, i, t, off, rest, p, n, m, nm, k, adv, out) {
  lowered = tolower(text)
  nmatch = 0
  if (!index(lowered, "users") && !index(lowered, "home") && !index(lowered, "documents and settings")) return ""
  ncov = 0
  out = ""
  for (i = 1; i <= nforms; i++) {
    t = low[i] ? lowered : text
    off = 0
    rest = t
    while (match(rest, re[i])) {
      p = off + RSTART
      n = RLENGTH
      adv = RSTART + n - 1
      if (starts_here(text, p, kind_of[i]) && !covered(p)) {
        m = substr(text, p, n)
        nm = name_in(m, i)
        k = name_length(nm)
        m = substr(m, 1, length(m) - length(nm) + k)
        n = length(m)
        if (k == 1 && substr(text, p + n - 2, 1) == "\\" && index("+*?{", substr(text, p + n, 1)) > 0) k = 0
        if (k > 0 && !own_directory(substr(nm, 1, k), substr(text, p + n, 8), i)) {
          ncov++
          cs[ncov] = p
          ce[ncov] = p + n - 1
          nmatch++
          last_match = m
          last_name = substr(nm, 1, k)
          if (kind_of[i] == "word") sub(/-.*/, "", last_name)
          if (!use_list || !((file SUBSEP m) in allowed))
            if (index("," out ",", "," form[i] ",") == 0) out = out (out == "" ? "" : ",") form[i]
        }
      }
      off += adv
      rest = substr(rest, adv + 1)
    }
  }
  return out
}
function malformed(n, why) {
  print "ERROR " list ": line " n ": " why
  failed = 1
  exit 2
}
function load_list(   n, rc, rec, k, rest, file, path, reason) {
  n = 0
  while ((rc = (getline rec < list)) > 0) {
    n++
    sub(/\r$/, "", rec)
    if (rec == "" || substr(rec, 1, 1) == "#") continue
    k = index(rec, "\t")
    if (!k) malformed(n, "an entry is a file, a tab, a home path, a tab and a reason")
    file = substr(rec, 1, k - 1)
    rest = substr(rec, k + 1)
    k = index(rest, "\t")
    if (!k) malformed(n, "an entry is a file, a tab, a home path, a tab and a reason")
    path = substr(rest, 1, k - 1)
    reason = substr(rest, k + 1)
    if (file == "") malformed(n, "the file is empty")
    if (reason !~ /[^[:space:]]/) malformed(n, "the reason is empty")
    scan_text(path, "", 0)
    if (nmatch != 1 || last_match != path) malformed(n, "the path is not one home path of the grammar as a whole")
    if (own_name != "" && tolower(last_name) == tolower(own_name)) print "OWN " n " " file
    allowed[file SUBSEP path] = 1
    allowed[list_path SUBSEP path] = 1
  }
  if (rc < 0) malformed(0, "the list cannot be read")
  close(list)
}
function name_from(h,   rest, half) {
  if (substr(h, 1, 11) == "diff --git ") {
    rest = substr(h, 12)
    half = substr(rest, 1, int((length(rest) - 1) / 2))
    if (substr(half, 1, 3) == "\"a/") return "\"" substr(half, 4)
    if (substr(half, 1, 2) == "a/") return substr(half, 3)
    return half
  }
  if (substr(h, 1, 10) == "diff --cc ") return substr(h, 11)
  if (substr(h, 1, 16) == "diff --combined ") return substr(h, 17)
  return h
}
function check(n, text,   forms) {
  forms = scan_text(text, file, 1)
  if (forms == "") return
  hits++
  print "HIT " commit " " n " " forms " " file
}
function start_hunk(h,   n, t) {
  n = 0
  while (substr(h, n + 1, 1) == "@") n++
  width = n - 1
  all_added = ""
  for (t = 0; t < width; t++) all_added = all_added "+"
  t = substr(h, index(h, " +") + 2)
  sub(/[^0-9].*/, "", t)
  current = t + 0
  in_hunk = 1
}
BEGIN {
  init()
  if (list_forms) {
    for (i = 1; i <= nforms; i++)
      if (!(form[i] in printed)) {
        printed[form[i]] = 1
        print form[i]
      }
    exit 0
  }
  load_list()
}
{
  line = $0
  if (in_hunk) {
    c = substr(line, 1, 1)
    if (c == "+" || c == "-" || c == " ") {
      if (!index(substr(line, 1, width), "-")) {
        if (substr(line, 1, width) == all_added) check(current, substr(line, width + 1))
        current++
      }
      next
    }
    if (c == "\\") next
    in_hunk = 0
  }
  if (substr(line, 1, 1) == "\001") {
    if (substr(line, 2, 6) == "again ") {
      commit = substr(line, 8)
    } else {
      commit = substr(line, 9)
      commits++
    }
    combined = 0
    next
  }
  if (substr(line, 1, 5) == "diff ") {
    file = name_from(line)
    combined = substr(line, 1, 10) == "diff --cc " || substr(line, 1, 16) == "diff --combined "
    next
  }
  if (combined && substr(line, 1, 13) == "Binary files ") {
    print "BINARY-MERGE " commit "\t" file
    next
  }
  if (substr(line, 1, 2) == "@@") {
    start_hunk(line)
    next
  }
  if (substr(line, 1, 14) == "new file mode ") check(0, file)
}
END {
  if (failed) exit 2
  if (!list_forms) print "READ " commits + 0 " " hits + 0
}
EOF
}

run_scan() {
  awk -v list="$1" -v list_path="$LIST_IN_THE_REPOSITORY" -v list_forms="${3:-}" -v own_name="${4:-}" -f "$work/scan.awk" "$2"
}

scan_stream() {
  run_scan "$allow" "$1" >"$2" </dev/null
  scan_status=$?
  error=$(sed -n 's/^ERROR //p' "$2") || error=
  [ -z "$error" ] || die "cannot use the list: $error"
  [ "$scan_status" = 0 ] || die "awk could not scan the commits of $label (exit $scan_status)"
  read_line=$(sed -n 's/^READ //p' "$2") || read_line=
  [ -n "$read_line" ] || die "awk printed no count for $label"
  stream_commits=${read_line% *}
  stream_found=${read_line#* }
}

lines_of_a_merge_in_a_binary_file() {
  case $2 in
    '"'*) die "cannot read the lines that merge $1 adds to the binary file $2, whose name git quotes" ;;
  esac
  git cat-file blob "$1:$2" >"$work/merge-blob" 2>/dev/null </dev/null || die "cannot read $2 in merge $1"
  tr -d '\000' <"$work/merge-blob" >"$work/merge-blob.text" || die 'tr failed'
  git rev-parse "$1^@" >"$work/parents" </dev/null || die "cannot list the parents of merge $1"
  : >"$work/parent-lines"
  while read -r parent; do
    git cat-file -e "$parent:$2" 2>/dev/null </dev/null || continue
    git cat-file blob "$parent:$2" >"$work/parent-blob" </dev/null || die "cannot read $2 in commit $parent"
    tr -d '\000' <"$work/parent-blob" >>"$work/parent-lines" || die 'tr failed'
  done <"$work/parents"
  printf '\001again %s\ndiff --git a/%s b/%s\n' "$1" "$2" "$2"
  awk 'FILENAME == ARGV[1] { seen[$0] = 1; next } !($0 in seen) { printf "@@ -0,0 +%d @@\n+%s\n", FNR, $0 }' \
    "$work/parent-lines" "$work/merge-blob.text" || die 'awk could not compare the lines of a merge'
}

short_id() {
  printf '%s' "$1" | cut -c1-12
}

scan() {
  label=$1
  shift
  git -c core.quotepath=off -c diff.suppressBlankEmpty=false -c diff.noprefix=false \
    -c diff.mnemonicPrefix=false -c diff.relative=false -c diff.submodule=short -c diff.ignoreSubmodules=none \
    -c color.ui=never -c log.showSignature=false \
    log --no-color --format='%x01commit %H' -p --cc -U0 --root --no-renames --text \
    --no-ext-diff --no-textconv --no-relative --submodule=short --ignore-submodules=none --src-prefix=a/ --dst-prefix=b/ \
    --reverse --end-of-options "$@" -- >"$work/log" 2>"$work/log-error" </dev/null || {
    first_error=$(sed -n 1p "$work/log-error") || first_error=
    die "git log could not read the commits of $label: $first_error"
  }
  tr -d '\000' <"$work/log" >"$work/log.text" || die 'tr could not read the output of git log'
  scan_stream "$work/log.text" "$work/hits"
  commits=$stream_commits
  found=$stream_found
  sed -n 's/^BINARY-MERGE //p' "$work/hits" >"$work/binary-merges" || die 'sed could not read the output of awk'
  if [ -s "$work/binary-merges" ]; then
    : >"$work/supplement"
    while IFS="$TAB" read -r commit file; do
      lines_of_a_merge_in_a_binary_file "$commit" "$file" >>"$work/supplement"
    done <"$work/binary-merges"
    scan_stream "$work/supplement" "$work/hits-of-merges"
    found=$((found + stream_found))
    cat "$work/hits-of-merges" >>"$work/hits" || die 'cat failed'
  fi
  while read -r tag commit number forms file; do
    [ "$tag" = HIT ] || continue
    short=$(short_id "$commit")
    printf '%s: %s %s:%s: a home path (%s)\n' "$NAME" "$short" "$file" "$number" "$forms" >&2
  done <"$work/hits"
  if [ "$found" = 0 ]; then
    printf '%s: PASS: %s: commits read %s, lines with a home path 0.\n' "$NAME" "$label" "$commits"
    return 0
  fi
  any_failed=1
  {
    printf '%s: FAIL: %s: commits read %s, lines with a home path %s.\n' "$NAME" "$label" "$commits" "$found"
    printf 'Fix: take each listed line out of the commit that added it: git commit --amend for the newest commit, git rebase -i with "edit" for an older one. A later commit that removes the line does not help, because the commit that holds it is pushed. Write a location relative to the repository, take it from an environment variable, or write the name as a placeholder (/Users/<name>). If the path is made up for a fixture, add the file, the exact path and the reason to %s (sh scripts/check-home-paths.sh --help, The list).\n' "$LIST_IN_THE_REPOSITORY"
  } >&2
}

exempt_commit() {
  exempt=
  if [ -f "$script_dir/check-commit-messages.sh" ]; then
    exempt=$(sed -n 's/^NEWEST_EXEMPT_COMMIT=\([0-9a-f][0-9a-f]*\)$/\1/p' "$script_dir/check-commit-messages.sh") || exempt=
  fi
  if [ -n "$exempt" ] && ! git cat-file -e "$exempt^{commit}" 2>/dev/null; then
    exempt=
  fi
}

is_object_id() {
  case $1 in
    '' | *[!0-9a-f]*) return 1 ;;
  esac
  [ ${#1} = 40 ] || [ ${#1} = 64 ]
}

verified() {
  git rev-parse -q --verify "$1^{commit}" >/dev/null 2>&1
}

entries_for_the_account_that_pushes() {
  own_name=
  case ${HOME:-/} in
    / | */) ;;
    *) own_name=$(basename "$HOME") || own_name= ;;
  esac
  [ -n "$own_name" ] || return 0
  run_scan "$allow" /dev/null '' "$own_name" >"$work/own" </dev/null
  own_status=$?
  error=$(sed -n 's/^ERROR //p' "$work/own") || error=
  [ -z "$error" ] || die "cannot use the list: $error"
  [ "$own_status" = 0 ] || die "awk could not read the list (exit $own_status)"
  while read -r tag number file; do
    [ "$tag" = OWN ] || continue
    any_failed=1
    printf '%s: FAIL: line %s of %s, the entry for %s, names the home directory of the account that runs this push. An entry is only for a made-up name that a fixture holds on purpose, never the home path of a person'"'"'s machine: take the entry out of the list and the path out of the commits that hold it, then push again.\n' "$NAME" "$number" "$LIST_IN_THE_REPOSITORY" "$file" >&2
  done <"$work/own"
}

check_push() {
  remote=$1
  git rev-parse --git-dir >/dev/null 2>&1 || die "$PWD is in no git repository"
  exempt_commit
  default_ref=$(git symbolic-ref -q refs/remotes/origin/HEAD) || default_ref=
  if [ -z "$default_ref" ] && verified refs/remotes/origin/main; then
    default_ref=refs/remotes/origin/main
  fi
  default_ref_of_remote=$(git symbolic-ref -q "refs/remotes/$remote/HEAD" 2>/dev/null) || default_ref_of_remote=refs/remotes/$remote/main
  verified "$default_ref_of_remote" || default_ref_of_remote=$default_ref
  [ -n "$default_ref_of_remote" ] && verified "$default_ref_of_remote" || default_ref_of_remote=
  entries_for_the_account_that_pushes
  while IFS= read -r pushed_ref; do
    remote_object=${pushed_ref##* }
    rest=${pushed_ref% *}
    remote_ref=${rest##* }
    rest=${rest% *}
    local_object=${rest##* }
    local_ref=${rest% *}
    if ! { is_object_id "$local_object" && is_object_id "$remote_object"; }; then
      die "cannot read this line that git wrote to the pre-push hook: $pushed_ref"
    fi
    case $local_object in
      *[!0]*) ;;
      *) continue ;;
    esac
    verified "$local_object" || continue
    set -- "$local_object"
    [ -z "$exempt" ] || set -- "$@" "^$exempt"
    case $remote_object in
      *[!0]*)
        if git cat-file -e "$remote_object^{commit}" 2>/dev/null; then set -- "$@" "^$remote_object"; fi
        ;;
    esac
    [ -z "$default_ref_of_remote" ] || set -- "$@" "^$default_ref_of_remote"
    scan "push of $local_ref to $remote_ref" "$@"
  done
  [ "$any_failed" = 0 ]
}

check_range() {
  git rev-parse --git-dir >/dev/null 2>&1 || die "$PWD is in no git repository"
  label=$*
  scan "$label" "$@"
  [ "$any_failed" = 0 ]
}

grammar_fixtures() {
  cat <<'EOF'
FAIL|macos|macos-after-blank|cd /{U}/ada/notes
FAIL|macos|macos-in-double-quotes|"/{U}/ada"
FAIL|macos|macos-in-single-quotes|'/{U}/ada/x'
FAIL|macos|macos-after-equals|HOME=/{U}/ada
FAIL|macos|macos-after-colon|PATH=/bin:/{U}/ada/bin
FAIL|macos|macos-json-escaped-slash|{"p":"\/{U}\/ada"}
FAIL|macos|macos-percent-encoded|path=%2F{U}%2Fada
FAIL|macos|macos-percent-encoded-lower|%2f{U}%2fada
FAIL|macos|macos-after-encoded-byte|q%3D%2F{U}%2Fada
FAIL|macos|macos-after-two-periods|../../{U}/ada
FAIL|macos|macos-file-url|<a href="file:///{U}/ada/x">
FAIL|macos|macos-file-url-one-slash|file:/{U}/ada
FAIL|macos|macos-name-with-digit|/{U}/1ada
FAIL|macos|macos-name-with-underscore|/{U}/_ada
FAIL|macos|macos-name-non-ascii|/{U}/{E}ric
FAIL|macos|macos-in-curly-quotes|{LQ}/{U}/ada{RQ}
FAIL|macos|macos-in-guillemets|{LG}/{U}/ada{RG}
FAIL|macos|macos-at-line-start|/{U}/ada
FAIL|macos|macos-carriage-return|/{U}/ada{CR}
FAIL|macos|macos-after-tab|x{T}/{U}/ada
FAIL|macos|macos-after-bracket|(/{U}/ada)
FAIL|macos|macos-after-scp-host|host:/{U}/ada
FAIL|macos|macos-after-json-newline-escape|{"msg":"panicked\n/{U}/zed/src/main.rs:3"}
FAIL|macos|macos-after-one-letter-flag|cc -I/{U}/zed/include
FAIL|windows-root|windows-root-exact-case|"\{U}\ada"
FAIL|linux|linux-home|cd /{H}/ada/.cache
FAIL|linux|linux-home-after-colon|scp x host:/{H}/ada
FAIL|linux|linux-home-numeric|/{H}/1000
FAIL|linux|linux-after-python-escape|'out\n/{H}/zed/x.py'
FAIL|linux|linux-after-one-letter-flag|rustc -L/{H}/zed/.cache/lib
FAIL|windows-drive|windows-drive-backslash|C:\{U}\ada
FAIL|windows-drive|windows-drive-forward-slash|"C:/{U}/ada"
FAIL|windows-drive|windows-drive-lower-case|c:/{u}/ada
FAIL|windows-drive|windows-drive-upper-case|C:\{UU}\ADA
FAIL|windows-drive|windows-drive-json-doubled|"C:\\{U}\\ada"
FAIL|windows-drive|windows-drive-percent-encoded|C%3A%5C{U}%5Cada
FAIL|windows-drive|windows-drive-long-path|\\?\C:\{U}\ada
FAIL|windows-drive|windows-drive-name-all|C:\{U}\All\x
FAIL|windows-root|windows-root-backslash|\{u}\ada
FAIL|windows-root|windows-root-unc-share|\\host\c$\{U}\ada
FAIL|windows-old|windows-old-drive|C:\{D}\ada
FAIL|windows-old|windows-old-root|\{D}\ada
FAIL|git-bash|git-bash-drive|/c/{U}/ada
FAIL|git-bash|git-bash-upper-drive|/C/{u}/ada
FAIL|wsl-mnt|wsl-mnt-drive|/mnt/c/{U}/ada
FAIL|cygwin|cygwin-drive|/cygdrive/c/{U}/ada
FAIL|wsl-unc|wsl-unc-dollar|\\wsl$\Ubuntu\{H}\ada
FAIL|wsl-unc|wsl-unc-localhost|\\wsl.localhost\Ubuntu-22.04\{H}\ada
FAIL|wsl-unc|wsl-unc-forward-slashes|cd //wsl$/Ubuntu-22.04/{H}/zed
FAIL|wsl-unc|wsl-unc-file-url|file://wsl.localhost/Ubuntu/{H}/zed
FAIL|wsl-unc|wsl-unc-editor-remote|vscode-remote://wsl%2Bubuntu/{H}/zed
FAIL|macos-volume|macos-volume-data|/System/Volumes/Data/{U}/ada
FAIL|macos-volume|macos-volume-named|/Volumes/Macintosh HD/{U}/ada
FAIL|file-url-host|file-url-host-localhost|file://localhost/{U}/ada
FAIL|file-url-host|file-url-host-linux|file://box/{H}/ada
FAIL|file-url-host|file-url-host-drive|file:///C:/{U}/ada
FAIL|url-file-host|url-file-host-editor-link|vscode://file/{U}/ada/x.rs:3
FAIL|dash|dash-macos|-{U}-ada-Documents-x
FAIL|dash|dash-linux|"-{H}-ada-x"
FAIL|dash|dash-after-slash|projects/-{U}-ada-x
FAIL|dash|dash-windows|C--{U}-ada-x
PASS|-|directory-alone-in-prose|under /{U}/ or /{H}/
PASS|-|directory-alone-at-end|/{U}/
PASS|-|directory-alone-quoted|"/{H}/"
PASS|-|directory-alone-in-curly-quotes|{LQ}/{U}/{RQ}
PASS|-|placeholder-angle|/{U}/<name>
PASS|-|placeholder-dollar|/{U}/$USER
PASS|-|placeholder-braced|/{H}/${USER}/x
PASS|-|placeholder-percent|C:\{U}\%USERNAME%
PASS|-|placeholder-star|/{H}/*/x
PASS|-|placeholder-curly|/{U}/{name}
PASS|-|placeholder-dots|/{U}/...
PASS|-|placeholder-dot-name|/{H}/.config
PASS|-|own-directory-shared|/{U}/Shared/x
PASS|-|own-directory-public|C:\{U}\Public\x
PASS|-|own-directory-default-user|C:\{U}\Default User\x
PASS|-|own-directory-all-users|C:\{U}\All {U}\x
PASS|-|own-directory-dash|-{U}-Shared-x
PASS|-|web-address-path|https://example.com/{H}/index.html
PASS|-|web-address-lower-users|https://api.github.com/{u}/ada
PASS|-|relative-directory|src/{H}/page.tsx
PASS|-|relative-from-dot|./{H}/page.tsx
PASS|-|macos-lower-case-is-not-read|/{u}/ada
PASS|-|xml-closing-tag|</{U}>
PASS|-|regex-source|[/\\]+({U}|{H})[/\\]+
PASS|-|regex-escape-as-name|Regex::new(r"^/{U}/\w+/")
PASS|-|regex-escape-in-sed|sed -E 's#/{H}/\S+#~#g'
PASS|-|hyphen-and-letter-inside-a-word|foo-I/{U}/zed
PASS|-|long-flag|--{H}-dir
PASS|-|word-alone|{U} and {H}
PASS|-|environment-variable|$HOME/x and ~/x
PASS|-|git-bash-needs-a-drive-letter|/ab/{U}/ada
EOF
}

expand_fixture() {
  e_acute=$(printf '\303\251')
  left_quote=$(printf '\342\200\234')
  right_quote=$(printf '\342\200\235')
  left_guillemet=$(printf '\302\253')
  right_guillemet=$(printf '\302\273')
  carriage_return=$(printf '\r')
  tab=$(printf '\t')
  printf '%s\n' "$1" | sed -e 's|{UU}|USERS|g' -e 's|{U}|Users|g' -e 's|{u}|users|g' -e 's|{H}|home|g' \
    -e 's|{D}|Documents and Settings|g' -e "s|{E}|$e_acute|g" -e "s|{LQ}|$left_quote|g" -e "s|{RQ}|$right_quote|g" \
    -e "s|{LG}|$left_guillemet|g" -e "s|{RG}|$right_guillemet|g" -e "s|{CR}|$carriage_return|g" -e "s|{T}|$tab|g"
}

fixture_block_of_the_table() {
  for f in $(run_scan "$work/empty-list" /dev/null 1); do
    printf '%s:' "$f"
    while IFS='|' read -r want form fixture text; do
      [ "$want" = FAIL ] && [ "$form" = "$f" ] && printf ' %s' "$fixture"
    done <"$work/table"
    printf '\n'
  done
}

fixture_block_of_the_help() {
  help_text | sed -n '/^FIXTURES-BEGIN$/,/^FIXTURES-END$/p' | sed -e '1d' -e '$d'
}

wrong=
rejected=0

verdict() {
  if [ "$2" = "$3" ]; then
    [ "$2" = PASS ] || [ "$2" = 0 ] || rejected=$((rejected + 1))
  else
    wrong="$wrong $1"
  fi
  printf 'case %s/%s want=%s got=%s\n' "$NAME" "$1" "$2" "$3"
}

self_test_grammar() {
  printf '# no entry\n' >"$work/empty-list"
  grammar_fixtures >"$work/table"
  count=0
  : >"$work/lines"
  while IFS='|' read -r want form fixture text; do
    count=$((count + 1))
    expand_fixture "$text" >>"$work/lines"
  done <"$work/table"
  {
    printf '\001commit %s\n' 0000000000000000000000000000000000000000
    printf 'diff --git a/fixtures.txt b/fixtures.txt\nnew file mode 100644\n--- /dev/null\n+++ b/fixtures.txt\n'
    printf '@@ -0,0 +1,%s @@\n' "$count"
    sed 's/^/+/' "$work/lines"
  } >"$work/grammar.diff"
  run_scan "$work/empty-list" "$work/grammar.diff" >"$work/grammar.hits" ||
    die 'awk could not scan the grammar fixtures'
  n=0
  while IFS='|' read -r want form fixture text; do
    n=$((n + 1))
    got=$(sed -n "s/^HIT 0* $n \\([^ ]*\\) fixtures.txt\$/\\1/p" "$work/grammar.hits") || got=
    if [ "$want" = FAIL ]; then
      verdict "$fixture" "FAIL $form" "FAIL ${got:-none}"
    elif [ -n "$got" ]; then
      verdict "$fixture" PASS "FAIL $got"
    else
      verdict "$fixture" PASS PASS
    fi
  done <"$work/table"
  fixture_block_of_the_table >"$work/block-table"
  fixture_block_of_the_help >"$work/block-help"
  if cmp -s "$work/block-table" "$work/block-help"; then
    printf 'case %s/help-names-the-fixtures-of-each-form want=same got=same\n' "$NAME"
  else
    wrong="$wrong help-names-the-fixtures-of-each-form"
    printf 'case %s/help-names-the-fixtures-of-each-form want=same got=different; the help needs:\n' "$NAME"
    cat "$work/block-table"
  fi
}

in_repo() {
  git -C "$repo" -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath="$work/no-hooks" "$@"
}

commit_all() {
  in_repo add -A . && in_repo commit -q -m "$1" && in_repo rev-parse HEAD
}

write() {
  mkdir -p "$(dirname "$repo/$1")" && expand_fixture "$2" >"$repo/$1"
}

scan_case() {
  case_name=$1
  want_exit=$2
  want_text=$3
  shift 3
  (cd "$repo" && sh "$self" "$@") >"$work/out-$case_name" 2>&1
  code=$?
  got=$code
  case $(cat "$work/out-$case_name") in
    *"$want_text"*) ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict "$case_name" "$want_exit" "$got"
}

self_test_commits() {
  repo=$work/repo
  mkdir -p "$work/no-hooks" || die 'mkdir failed'
  git init -q "$repo" || die 'git init failed'
  list=$work/list
  tab=$(printf '\t')
  {
    printf '# made-up paths\n'
    expand_fixture "fixtures/made-up.txt$tab/{U}/ada${tab}a made-up name for the example"
  } >"$list"

  write README 'plain text'
  write fixtures/made-up.txt 'see /{U}/ada for the example'
  root_commit=$(commit_all 'root') || die 'commit failed'
  scan_case root-commit-reads-against-the-empty-tree 1 'fixtures/made-up.txt:1: a home path (macos)' --allow "$work/empty-list" "$root_commit"
  scan_case listed-fixture-passes 0 'commits read 1, lines with a home path 0' --allow "$list" "$root_commit"

  write notes.txt 'a'
  printf 'b\n' >>"$repo/notes.txt"
  expand_fixture 'cd /{H}/bob' >>"$repo/notes.txt"
  c=$(commit_all 'text line') || die 'commit failed'
  scan_case text-line-names-file-and-line 1 'notes.txt:3: a home path (linux)' --allow "$list" "$c^!"
  short=$(short_id "$c") || die 'cut failed'
  scan_case text-line-names-the-commit 1 "$short notes.txt:3" --allow "$list" "$c^!"

  mkdir -p "$repo/sub" || die 'mkdir failed'
  printf 'x\n' >"$repo/sub/x"
  in_repo add sub/x || die 'git add failed'
  (cd "$repo/sub" && GIT_CONFIG_COUNT=8 \
    GIT_CONFIG_KEY_0=diff.noprefix GIT_CONFIG_VALUE_0=true \
    GIT_CONFIG_KEY_1=color.ui GIT_CONFIG_VALUE_1=always \
    GIT_CONFIG_KEY_2=diff.relative GIT_CONFIG_VALUE_2=true \
    GIT_CONFIG_KEY_3=diff.suppressBlankEmpty GIT_CONFIG_VALUE_3=true \
    GIT_CONFIG_KEY_4=core.quotepath GIT_CONFIG_VALUE_4=true \
    GIT_CONFIG_KEY_5=diff.mnemonicPrefix GIT_CONFIG_VALUE_5=true \
    GIT_CONFIG_KEY_6=log.showRoot GIT_CONFIG_VALUE_6=false \
    GIT_CONFIG_KEY_7=diff.renames GIT_CONFIG_VALUE_7=copies \
    sh "$self" --allow "$list" "$c^!") >"$work/out-settings-of-the-clone" 2>&1
  code=$?
  case $(cat "$work/out-settings-of-the-clone") in
    *'notes.txt:3: a home path (linux)'*) got=$code ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict settings-of-the-clone-and-a-subdirectory 1 "$got"
  in_repo reset -q || die 'git reset failed'

  printf 'see ' >>"$repo/fixtures/made-up.txt"
  expand_fixture '{LQ}/{U}/ada{RQ} again' >>"$repo/fixtures/made-up.txt"
  c=$(commit_all 'listed path again') || die 'commit failed'
  scan_case listed-path-added-again-passes 0 'lines with a home path 0' --allow "$list" "$c^!"

  expand_fixture 'and /{U}/carol' >>"$repo/fixtures/made-up.txt"
  c=$(commit_all 'new name in a listed file') || die 'commit failed'
  scan_case new-name-in-a-listed-file-fails 1 'fixtures/made-up.txt:3: a home path (macos)' --allow "$list" "$c^!"

  expand_fixture 'PNG@{CR}@/{U}/dave@' | tr '@' '\000' >"$repo/image.bin"
  c=$(commit_all 'binary') || die 'commit failed'
  scan_case binary-file-by-its-bytes 1 'image.bin:1: a home path (macos)' --allow "$list" "$c^!"

  printf '\377\376' >"$repo/utf16.txt"
  expand_fixture 'C:\{U}\erin' | sed 's/./&@/g' | tr '@' '\000' >>"$repo/utf16.txt"
  c=$(commit_all 'utf-16') || die 'commit failed'
  scan_case utf16-text 1 'utf16.txt:1: a home path (windows-drive)' --allow "$list" "$c^!"

  blob=$(expand_fixture '/{U}/frank/target' | tr -d '\n' | in_repo hash-object -w --stdin) || die 'git hash-object failed'
  in_repo update-index --add --cacheinfo "120000,$blob,link" || die 'git update-index failed'
  in_repo commit -q -m 'link' || die 'commit failed'
  c=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  scan_case link-target 1 'link:1: a home path (macos)' --allow "$list" "$c^!"

  dashed=$(expand_fixture '-{U}-gina-project') || die 'expand failed'
  mkdir -p "$repo/$dashed" || die 'mkdir failed'
  printf 'plain\n' >"$repo/$dashed/x.txt" || die 'printf failed'
  c=$(commit_all 'name') || die 'commit failed'
  scan_case name-of-an-added-file 1 'x.txt:0: a home path (dash)' --allow "$list" "$c^!"

  in_repo rm -q -r -- "$dashed" || die 'git rm failed'
  c=$(commit_all 'remove the file with that name') || die 'commit failed'
  scan_case deleted-file-name-passes 0 'lines with a home path 0' --allow "$list" "$c^!"

  printf 'a\nb\n' >"$repo/notes.txt"
  c=$(commit_all 'remove the line') || die 'commit failed'
  scan_case removed-line-passes 0 'lines with a home path 0' --allow "$list" "$c^!"

  expand_fixture '/{H}/hank' >"$repo/later.txt"
  added=$(commit_all 'add a line') || die 'commit failed'
  printf 'plain\n' >"$repo/later.txt"
  c=$(commit_all 'remove it again') || die 'commit failed'
  short=$(short_id "$added") || die 'cut failed'
  scan_case line-removed-later-still-fails 1 "$short later.txt:1: a home path (linux)" --allow "$list" "$added^..$c"

  expand_fixture '/{H}/ivan' >"$repo/mode.txt"
  base_of_mode=$(commit_all 'file for the mode') || die 'commit failed'
  in_repo update-index --chmod=+x mode.txt || die 'git update-index failed'
  in_repo commit -q -m 'mode only' || die 'commit failed'
  c=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  scan_case mode-change-alone-passes 0 'lines with a home path 0' --allow "$list" "$base_of_mode..$c"

  printf '*.lock -diff\n' >"$repo/.gitattributes"
  in_repo add .gitattributes || die 'git add failed'
  in_repo commit -q -m 'attributes' || die 'commit failed'
  expand_fixture 'resolved "/{H}/june/x"' >"$repo/deps.lock"
  c=$(commit_all 'lock file') || die 'commit failed'
  scan_case file-marked-binary-by-attribute 1 'deps.lock:1: a home path (linux)' --allow "$list" "$c^!"

  printf 'plain\r\n' >"$repo/crlf.txt"
  expand_fixture '/{U}/ada{CR}' >>"$repo/crlf.txt"
  expand_fixture '/{U}/lee{CR}' >>"$repo/crlf.txt"
  c=$(commit_all 'carriage returns') || die 'commit failed'
  {
    expand_fixture "crlf.txt$tab/{U}/ada${tab}made up{CR}"
  } >"$work/list-crlf"
  scan_case carriage-return-line-fails 1 'crlf.txt:3: a home path (macos)' --allow "$work/list-crlf" "$c^!"

  in_repo checkout -q -b side || die 'git checkout failed'
  expand_fixture '/{U}/kate' >"$repo/side.txt"
  side_commit=$(commit_all 'side') || die 'commit failed'
  in_repo checkout -q - || die 'git checkout failed'
  printf 'main\n' >"$repo/main.txt"
  commit_all 'main' >/dev/null || die 'commit failed'
  in_repo merge -q --no-ff -m 'merge side' side || die 'git merge failed'
  merge=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  scan_case merge-takes-a-line-from-a-parent 0 'commits read 1, lines with a home path 0' --allow "$list" "$merge^!"
  short=$(short_id "$side_commit") || die 'cut failed'
  scan_case merge-and-its-side-commit 1 "$short side.txt:1" --allow "$list" "$merge^1..$merge"

  in_repo checkout -q -b side2 "$merge^1" || die 'git checkout failed'
  expand_fixture '/{U}/kim' >>"$repo/main.txt"
  commit_all 'side2' >/dev/null || die 'commit failed'
  in_repo checkout -q - || die 'git checkout failed'
  in_repo merge -q --no-ff --no-commit side2 >/dev/null 2>&1
  expand_fixture '/{U}/liam' >>"$repo/main.txt"
  in_repo add main.txt || die 'git add failed'
  in_repo commit -q -m 'merge with a new line' || die 'commit failed'
  merge=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  scan_case merge-adds-a-line-of-its-own 1 'main.txt:3: a home path (macos)' --allow "$list" "$merge^!"
  scan_case merge-reads-no-line-of-a-parent-beside-its-own 1 'commits read 1, lines with a home path 1.' --allow "$list" "$merge^!"

  printf '*.lock -diff\n' >"$repo/.gitattributes"
  printf 'v1\n' >"$repo/merge.lock"
  printf 'v1\000\n' >"$repo/merge.bin"
  commit_all 'binary files before a merge' >/dev/null || die 'commit failed'
  in_repo checkout -q -b side3 || die 'git checkout failed'
  printf 'side3\n' >"$repo/side3.txt"
  commit_all 'side3' >/dev/null || die 'commit failed'
  in_repo checkout -q - || die 'git checkout failed'
  printf 'trunk\n' >"$repo/trunk3.txt"
  commit_all 'trunk3' >/dev/null || die 'commit failed'
  in_repo merge -q --no-ff --no-commit side3 >/dev/null 2>&1
  expand_fixture '/{H}/nell' >>"$repo/merge.lock"
  expand_fixture '/{H}/nell@' | tr '@' '\000' >>"$repo/merge.bin"
  in_repo add merge.lock merge.bin || die 'git add failed'
  in_repo commit -q -m 'merge with lines of its own in binary files' || die 'commit failed'
  merge=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  scan_case merge-adds-a-line-to-a-file-marked-binary 1 'merge.lock:2: a home path (linux)' --allow "$list" "$merge^!"
  scan_case merge-adds-a-line-to-a-file-binary-by-content 1 'merge.bin:2: a home path (linux)' --allow "$list" "$merge^!"

  dashed=$(expand_fixture '-{U}-otto-module') || die 'expand failed'
  in_repo update-index --add --cacheinfo "160000,$merge,$dashed" || die 'git update-index failed'
  in_repo commit -q -m 'submodule' || die 'commit failed'
  c=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  (cd "$repo" && GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=diff.ignoreSubmodules GIT_CONFIG_VALUE_0=all \
    sh "$self" --allow "$list" "$c^!") >"$work/out-submodule-ignored-by-the-clone" 2>&1
  code=$?
  case $(cat "$work/out-submodule-ignored-by-the-clone") in
    *'module:0: a home path (dash)'*) got=$code ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict submodule-name-with-the-clone-ignoring-submodules 1 "$got"
  in_repo rm -q --cached -- "$dashed" || die 'git rm failed'
  in_repo commit -q -m 'remove the submodule' || die 'commit failed'
  printf '[submodule "m"]\n\tpath = %s\n\turl = ./m\n\tignore = all\n' "$dashed" >"$repo/.gitmodules"
  in_repo add .gitmodules || die 'git add failed'
  in_repo commit -q -m 'gitmodules' || die 'commit failed'
  in_repo update-index --add --cacheinfo "160000,$merge,$dashed" || die 'git update-index failed'
  in_repo commit -q -m 'submodule that .gitmodules ignores' || die 'commit failed'
  c=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  scan_case submodule-name-that-gitmodules-ignores 1 'module:0: a home path (dash)' --allow "$list" "$c^!"
  in_repo rm -q --cached -- "$dashed" .gitmodules || die 'git rm failed'
  in_repo commit -q -m 'remove the submodule again' || die 'commit failed'

  in_repo mv fixtures/made-up.txt fixtures/moved.txt || die 'git mv failed'
  c=$(commit_all 'rename') || die 'commit failed'
  scan_case rename-reads-the-file-in-full 1 'fixtures/moved.txt:1: a home path (macos)' --allow "$list" "$c^!"

  mkdir -p "$repo/scripts" || die 'mkdir failed'
  expand_fixture "x.txt$tab/{U}/mia${tab}made up" >"$repo/$LIST_IN_THE_REPOSITORY"
  c=$(commit_all 'list entry') || die 'commit failed'
  cp "$repo/$LIST_IN_THE_REPOSITORY" "$work/list-in-repo" || die 'cp failed'
  scan_case entry-of-the-list-passes-in-the-list 0 'lines with a home path 0' --allow "$work/list-in-repo" "$c^!"
  expand_fixture '# not /{U}/noah' >>"$repo/$LIST_IN_THE_REPOSITORY"
  c=$(commit_all 'comment in the list') || die 'commit failed'
  scan_case comment-of-the-list-fails 1 "$LIST_IN_THE_REPOSITORY:2: a home path (macos)" --allow "$work/list-in-repo" "$c^!"

  printf 'file-without-reason\t' >"$work/list-bad"
  expand_fixture '/{U}/ada' >>"$work/list-bad"
  scan_case list-without-a-tab-and-reason 2 'line 1: an entry is a file' --allow "$work/list-bad" "$c^!"
  expand_fixture "x.txt$tab/{U}/ada$tab " >"$work/list-bad"
  scan_case list-with-an-empty-reason 2 'line 1: the reason is empty' --allow "$work/list-bad" "$c^!"
  expand_fixture "x.txt${tab}src/{U}/ada${tab}a pattern" >"$work/list-bad"
  scan_case list-path-that-is-no-home-path 2 'line 1: the path is not one home path' --allow "$work/list-bad" "$c^!"
  expand_fixture "x.txt$tab/{U}/ada/notes${tab}more than the path" >"$work/list-bad"
  scan_case list-path-with-more-than-the-name 2 'line 1: the path is not one home path' --allow "$work/list-bad" "$root_commit"
  scan_case list-missing 2 'the list cannot be read' --allow "$work/no-such-list" "$c^!"
  scan_case range-unreadable 2 'git log could not read' --allow "$list" no-such-revision
  scan_case usage-without-a-range 64 'Usage:' --allow "$list"
}

stub_messages_check() {
  cat <<'EOF'
#!/bin/sh
cat >"$STUB_RECORD"
printf 'stub messages check: exit %s\n' "${STUB_EXIT:-0}"
exit "${STUB_EXIT:-0}"
EOF
}

push_case() {
  case_name=$1
  want=$2
  want_text=$3
  shift 3
  before=$(git -C "$remote_repo" rev-list --all --count) || die 'git rev-list failed'
  (cd "$repo" && git -c core.hooksPath="$hook_home/.githooks" push "$@") >"$work/out-$case_name" 2>&1
  code=$?
  after=$(git -C "$remote_repo" rev-list --all --count) || die 'git rev-list failed'
  if [ "$code" = 0 ]; then got=pushed; else got=refused; fi
  if [ "$got" = refused ] && [ "$before" != "$after" ]; then got=refused-but-commits-arrived; fi
  case $(cat "$work/out-$case_name") in
    *"$want_text"*) ;;
    *) got="$got-without-the-text" ;;
  esac
  if [ "$want" = refused ] && [ "$got" = refused ]; then
    rejected=$((rejected + 1))
  elif [ "$want" != "$got" ]; then
    wrong="$wrong $case_name"
  fi
  printf 'case %s/%s want=%s got=%s\n' "$NAME" "$case_name" "$want" "$got"
}

self_test_hook() {
  hook=$root/.githooks/pre-push
  [ -f "$hook" ] || die ".githooks/pre-push is missing beside $script_dir"
  hook_home=$work/hook-home
  mkdir -p "$hook_home/.githooks" "$hook_home/scripts" || die 'mkdir failed'
  cp "$hook" "$hook_home/.githooks/pre-push" || die 'cp failed'
  chmod +x "$hook_home/.githooks/pre-push" || die 'chmod failed'
  cp "$self" "$hook_home/scripts/check-home-paths.sh" || die 'cp failed'
  cp "$list" "$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cp failed'
  stub_messages_check >"$hook_home/scripts/check-commit-messages.sh" || die 'cannot write the stub'
  STUB_RECORD=$work/stub-record
  export STUB_RECORD
  remote_repo=$work/remote.git
  git init -q --bare "$remote_repo" || die 'git init failed'
  in_repo checkout -q -b trunk "$root_commit" || die 'git checkout failed'
  in_repo remote add proof "$remote_repo" || die 'git remote add failed'
  in_repo push -q --no-verify proof trunk:refs/heads/main >/dev/null 2>&1 || die 'the seed push failed'
  in_repo fetch -q proof || die 'git fetch from the bare remote failed'

  in_repo checkout -q -b bad trunk || die 'git checkout failed'
  expand_fixture '/{H}/olga' >"$repo/bad.txt"
  bad=$(commit_all 'bad') || die 'commit failed'
  push_case new-branch-with-a-home-path refused 'bad.txt:1: a home path (linux)' proof bad

  in_repo checkout -q -b clean trunk || die 'git checkout failed'
  printf 'plain\n' >"$repo/clean.txt"
  commit_all 'clean' >/dev/null || die 'commit failed'
  push_case clean-branch pushed 'push of refs/heads/clean to refs/heads/clean: commits read 1, lines with a home path 0' proof clean

  in_repo checkout -q -b clean2 trunk || die 'git checkout failed'
  printf 'plain\n' >"$repo/clean2.txt"
  commit_all 'clean2' >/dev/null || die 'commit failed'
  push_case two-refs-one-with-a-home-path refused 'bad.txt:1' proof clean2 bad
  push_case force-push-over-main refused 'bad.txt:1' proof +bad:refs/heads/main
  push_case delete-a-branch pushed '' proof :refs/heads/clean

  in_repo push -q --no-verify proof bad:refs/heads/old-bad >/dev/null 2>&1 || die 'the push without the hook failed'
  in_repo checkout -q -b over-old-bad bad || die 'git checkout failed'
  printf 'plain\n' >"$repo/over.txt"
  commit_all 'over old bad' >/dev/null || die 'commit failed'
  push_case update-of-a-ref-whose-remote-copy-holds-the-line pushed 'commits read 1, lines with a home path 0' proof over-old-bad:refs/heads/old-bad

  in_repo tag -a -m 'tag' t1 "$bad" || die 'git tag failed'
  push_case annotated-tag-on-a-commit-with-a-home-path refused 'bad.txt:1' proof refs/tags/t1

  in_repo checkout -q -b removed trunk || die 'git checkout failed'
  expand_fixture '/{H}/pia' >"$repo/removed.txt"
  commit_all 'add' >/dev/null || die 'commit failed'
  printf 'plain\n' >"$repo/removed.txt"
  commit_all 'remove again' >/dev/null || die 'commit failed'
  push_case line-removed-by-a-later-commit refused 'removed.txt:1' proof removed

  in_repo checkout -q -b listed trunk || die 'git checkout failed'
  expand_fixture 'and /{U}/ada' >>"$repo/fixtures/made-up.txt"
  commit_all 'listed' >/dev/null || die 'commit failed'
  push_case listed-fixture-line pushed 'lines with a home path 0' proof listed

  in_repo checkout -q -b messages trunk || die 'git checkout failed'
  printf 'plain\n' >"$repo/messages.txt"
  commit_all 'messages' >/dev/null || die 'commit failed'
  STUB_EXIT=1 push_case messages-check-fails-and-the-scan-still-runs refused 'push of refs/heads/messages to refs/heads/messages: commits read 1, lines with a home path 0' proof messages
  case $(cat "$STUB_RECORD") in
    "refs/heads/messages "*" refs/heads/messages "*) got=same ;;
    *) got=other ;;
  esac
  verdict messages-check-reads-the-same-lines same "$got"

  cp "$work/list-bad" "$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cp failed'
  printf 'x\tnot a path\treason\n' >"$hook_home/$LIST_IN_THE_REPOSITORY"
  push_case scan-cannot-run refused 'cannot check, so the commits count as refused' proof messages
  cp "$list" "$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cp failed'

  in_repo checkout -q -b own trunk || die 'git checkout failed'
  printf 'plain\n' >"$repo/own.txt"
  commit_all 'own' >/dev/null || die 'commit failed'
  own_name=$(basename "$HOME") || die 'basename failed'
  {
    cat "$list"
    printf 'x.txt\t/Users/%s\tthe account that runs the self-test\n' "$own_name"
  } >"$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cannot write the list'
  push_case list-entry-for-the-pushing-account refused 'names the home directory of the account that runs this push' proof own
  scan_case list-entry-for-the-running-account-in-a-range 0 'lines with a home path 0' --allow "$hook_home/$LIST_IN_THE_REPOSITORY" "$root_commit"
  cp "$list" "$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cp failed'

  mv "$hook_home/scripts/check-home-paths.sh" "$work/scanner-aside" || die 'mv failed'
  push_case scanner-missing-beside-the-hook pushed 'the scan for home paths did not run' proof messages
  mv "$work/scanner-aside" "$hook_home/scripts/check-home-paths.sh" || die 'mv failed'

  (cd "$repo" && printf 'not a line that git writes\n' | sh "$self" --allow "$list" --pre-push proof) >"$work/out-unreadable-line" 2>&1
  code=$?
  verdict pre-push-line-that-git-does-not-write 2 "$code"
}

self_test() {
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 || {
    printf 'FAIL self-test:%s (the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.)\n' "$NAME"
    exit 1
  }
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_COMMON_DIR
  self_dir=$(cd "$script_dir" && pwd -P) || die 'cannot name the directory of this script'
  self_name=$(basename "$0") || die 'cannot name this script'
  self=$self_dir/$self_name
  self_test_grammar
  self_test_commits
  self_test_hook
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$NAME" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$NAME" "$rejected"
}

allow=$root/$LIST_IN_THE_REPOSITORY
mode=range
remote=
any_failed=0
while [ $# -gt 0 ]; do
  case $1 in
    --help | -h)
      help_text
      exit 0
      ;;
    --self-test)
      mode='self-test'
      shift
      ;;
    --allow)
      [ $# -ge 2 ] || usage '--allow needs a file'
      allow=$2
      shift 2
      ;;
    --pre-push)
      [ $# -ge 2 ] || usage '--pre-push needs the name or the URL of the remote'
      mode='pre-push'
      remote=$2
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*) usage "unknown option $1" ;;
    *) break ;;
  esac
done

make_work
scan_program >"$work/scan.awk" || die 'cannot write the scan program'
case $mode in
  self-test)
    [ $# = 0 ] || usage '--self-test takes no argument'
    self_test
    ;;
  pre-push)
    [ $# = 0 ] || usage '--pre-push takes the remote and no range'
    check_push "$remote"
    ;;
  range)
    [ $# -gt 0 ] || usage 'name a range of commits'
    check_range "$@"
    ;;
esac
