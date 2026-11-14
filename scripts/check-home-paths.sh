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
expression (/Users/\w+) and passes; at the end of a line such a letter is a
name. When one blank and more name characters follow a name up to a
separator, a quotation mark or the end of the line, as in
"C:\Users\<first> <last>", the path that the scan matches goes on to there;
a name of three or more words, or of two words that end at another
character, is matched up to its first blank. Any other first character
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
  unc-share      two backslashes, a host, a backslash, then Users, Users$,
                 home or home$ in any case, a separator: the share of a file
                 server, also in the long spelling \\?\UNC\<host>\ that
                 Rust's std::fs::canonicalize returns
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
JSON or a Python string), after the prefix /@fs of a dev server, right after
an escape sequence of a terminal (a CSI sequence: ESC, an opening square
bracket, parameters and a final letter, as in a colour code; an OSC sequence
that BEL or ESC and a backslash ends), also where ESC is written as \x1b,
\033, \e, \u{1b} (Rust's Debug spelling, either case) or the JSON escape of
the code 27, and after a flag: one or two
hyphens and letters at the start of a line or after a blank, a quote, an
equals sign, a comma or an opening bracket (-I/Users/<name>,
-isystem/Users/<name>). They do not count inside a path below another
directory (src/home/<name>, my-app/home/<name>) or in the path of a web
address (https://<host>/home/<name>).
url-file-host counts anywhere. dash counts at the start of a line and after
a blank, a tab, a quote, a backtick, a slash, a backslash, an equals sign, a
colon, a comma, or an opening bracket of any kind.

The self-test fixtures that hold each form (form: fixture...):
FIXTURES-BEGIN
wsl-unc: wsl-unc-dollar wsl-unc-localhost wsl-unc-forward-slashes wsl-unc-file-url wsl-unc-editor-remote
unc-share: unc-share-users unc-share-home-dollar unc-share-in-a-diff-line unc-share-users-dollar unc-share-in-the-long-spelling
macos-volume: macos-volume-data macos-volume-named
file-url-host: file-url-host-localhost file-url-host-linux file-url-host-drive
url-file-host: url-file-host-editor-link
cygwin: cygwin-drive
wsl-mnt: wsl-mnt-drive
git-bash: git-bash-drive git-bash-upper-drive
windows-old: windows-old-drive windows-old-root
windows-drive: windows-drive-backslash windows-drive-forward-slash windows-drive-lower-case windows-drive-upper-case windows-drive-json-doubled windows-drive-percent-encoded windows-drive-long-path windows-drive-name-all windows-drive-one-letter-name-at-the-end
windows-root: windows-root-exact-case windows-root-backslash windows-root-unc-share windows-root-one-letter-name-at-the-end
macos: macos-after-blank macos-in-double-quotes macos-in-single-quotes macos-after-equals macos-after-colon macos-json-escaped-slash macos-percent-encoded macos-percent-encoded-lower macos-after-encoded-byte macos-after-two-periods macos-file-url macos-file-url-one-slash macos-name-with-digit macos-name-with-underscore macos-name-non-ascii macos-in-curly-quotes macos-in-guillemets macos-at-line-start macos-carriage-return macos-after-tab macos-after-bracket macos-after-scp-host macos-after-json-newline-escape macos-after-one-letter-flag macos-after-colour-escapes-of-rustc macos-after-a-colour-escape-in-json macos-after-a-colour-escape-p095 macos-after-a-colour-escape-q015 macos-after-an-osc-8-link-in-json macos-after-an-escape-written-x1b macos-after-a-long-flag macos-after-the-dev-server-prefix macos-after-the-rust-debug-escape macos-after-the-rust-debug-escape-upper-case
linux: linux-home linux-home-after-colon linux-home-numeric linux-after-python-escape linux-after-one-letter-flag linux-after-colour-escapes-q016 linux-after-an-escape-written-033 linux-after-a-double-hyphen-flag
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
end of a line changes no answer. A line is read in pieces of 1,000
characters, each with the 320 before it and the 1,000 after it in view, so
the time grows with the length of a line and with the count of its matches,
and a path that goes on for more than 1,000 characters past the start of its
piece is cut there.
The settings of a clone that change what git
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

The list. scripts/home-path-allow.txt as the scanned commit holds it: in the
pre-push mode the pushed commit of each ref (for a tag, the commit that it
names), in the range mode the first commit that git rev-list <revision>...
prints (for <base>..<head>, <head>), and never the work tree or the index. A
commit without the file has an empty list, and a range that names no commit
reads none. --allow <file> names a file in its place, to try an entry before
it is committed; the hook passes no --allow. The list holds one entry per
line: the path of a file as git prints it with core.quotepath=off, a tab, the
home path as this script matches it (from the first character of the form to
the end of the name, or to where a name of two words ends, as
C:\Users\<first> <last>), a tab, and the reason. A line that starts with #
and an empty line are skipped, a carriage return at the end of a line is
dropped, and a byte-order mark at the start of the list is read as none. An
entry allows its path on each line of its file that holds it, and nothing
else: no other path, no other file, no longer name. An entry is not one
line, because a line number moves with every edit above it. In a line of
the list, a path that is the path of an entry is no hit, and the rest of the
line is read. A line with fewer than three fields, with an empty file or
reason, or with a path that is not one home path as a whole makes the list
malformed, and the scan cannot check. An entry is of one of three kinds, and
its reason says which: a made-up name that a fixture holds on purpose; the
public account of a hosting service's runner; or a line that holds no home
path and that the grammar reads as one (a route at the root of a site, a
relative path through a directory called home or Users, the home of a
container's account, -home-<word> after a blank). It is never the home path
of a person's machine. In the pre-push mode the scan refuses an entry that a
commit of the push adds to the list, read from the lines that those commits
add to it, when its name is the last part of $HOME or the leading part of
that last part that the grammar takes as a name, in any case. HOME is read
with either separator and without its trailing separators; when a commit of
the push adds an entry and HOME is unset or empty, the scan cannot check. An
added entry whose file and path the list of the default branch holds, as
this clone last fetched it, is not read for this, whatever its reason and
its place; when the clone knows no default branch for the remote, or that
branch holds no list, every added entry is read. The range mode does not
apply the rule, because the account of a hosted runner in CI is no person's.

Output and exit. A hit prints the commit, the file, the line number and the
form, and never the text of the line. Exit 0 when no added line holds a home
path that the list does not allow, 1 when one does, 2 when the check could
not run (git failed, a range or a line that git wrote cannot be read, the
list of the commit is malformed or a list that --allow names is missing, git
cannot resolve an object that a push
names, or a commit adds an entry while HOME is unset or empty), 64 on bad
usage. Every run that the scan counts as passed ends with one PASS line,
also when it reads no commit. In the pre-push mode 1 and 2 both make
.githooks/pre-push refuse the push, and so does an exit 0 without that PASS
line. A temporary directory of the script goes by its own trap.

Limits. The scan reads the forms that a tool of this repository writes by
itself on macOS and Windows, and the plain forms of the three systems. A form
that reaches a file only because a person wrote it that way is a named
limit. It does not find a path spelled in a way that the grammar does not
hold: /users/<name> or /USERS/<name> in another case, a share written with
slashes (//<host>/Users/<name>, which reads as an address without its
scheme), /var/home/<name>, /usr/home/<name>, /export/home/<name> and
/host_mnt/Users/<name>, the colon path of the classic Mac file system
(<volume>:Users:<name>:), a path after a digit or after the host of an
address (:8080/Users/<name>), the listing of an archive without its first
slash, the short form DOCUME~1, a name whose first character is
percent-encoded, an HTML entity or a \u escape for a separator, base64, a
compressed file (zip, gzip, a PNG chunk), UTF-32, and a path that a person
spells so that it does not read as one: a path broken over two lines, a join
of strings, a letter of the root as a percent code, /./ or /Shared/../, a
quoted name. It does not find a path after an escape sequence in a spelling
outside the six above (the byte ESC, the JSON escape, \u{1b}, \x1b, \033 and
\e): the caret notation that cat -v and an editor show, a short octal such
as \33, the spellings \U0000001b and \X1B, the name of the byte written as a
word, the percent code and the HTML entity of the byte, and the 8-bit form of
CSI; no tool of the two systems writes a path after one of these by itself,
and each reaches a file through a viewer that a person ran or through a line
that a person wrote. It does not find a path after a sequence that is
neither CSI nor OSC (ESC, an opening parenthesis, B), after a CSI cut before
its final byte or with an intermediate byte, or after an OSC whose text is
longer than the 300 characters that the scan looks back. It does not find a
path that starts with a variable in the dollar style and no braces
($env:SystemDrive\Users\<name>), because the root form does not count after
a letter; no tool prints that variable unexpanded before an account name,
and the percent style is read. It does not find a share whose name is none
of Users, Users$, home and home$, or the device spelling with a period in the
place of the question mark (\\.\UNC\). It does not find a path after a flag
that follows another character than the start of a line, a blank, a
quotation mark, an equals sign, a comma or an opening bracket (after a
semicolon or a vertical bar), after a flag that holds a digit, or after a
flag of more than 40 letters, nor after the prefix of a dev server with its
sign as a percent code (/%40fs/). It matches a name of three or more words,
or of two words that end at another character than a separator, a quotation
mark or the end of the line, up to its first blank, so an entry for the
first word allows that line. It does not find an account name outside a
path, such as an email address or a host name. It reads no commit message,
no tag message, no author or committer, no blob or tree that a tag names
without a commit, and no commit of a submodule, which is another
repository's. It refuses lines that hold no home path and that the grammar
reads as one: a relative path through a directory called home or Users
(../home/<page>), a route at the root of a site (/home/<page>), the home of a
container's or a runner's account (/home/<service>), and -home-<word> after
a blank; give each an entry of the third kind. A placeholder that is a word
(username after /Users/) is refused: write it with a sign (/Users/<name>). A
file whose name holds a tab, a line break, a quote or a backslash is listed
by the name that git prints, in quotes and with its escapes ("a\tb.txt").
The name of the exempt commit is read from the copy of
scripts/check-commit-messages.sh that runs, not from the pushed commits. The
hook does not run for git push --no-verify, or in a clone where
sh scripts/prerequisites.sh has not set core.hooksPath, and when this script
is not beside .githooks/pre-push the hook says that the scan did not run and
lets the push go on: run the scanner of a checkout of main on the range,
sh <checkout of main>/scripts/check-home-paths.sh origin/main..HEAD.
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
  NAME_CHARACTER = "([^[:space:][:cntrl:][:punct:]]|[._-])"
  CHUNK = 1000
  OVERLAP = 1000
  LOOKBACK = 320
  ESCAPE = "(\033|\\\\u001[bB]|\\\\u\\{1[bB]\\}|\\\\x1[bB]|\\\\033|\\\\[eE])"
  CSI_END = ESCAPE "\\[[0-9;:?<=>]*[@-~]$"
  OSC_END = ESCAPE "\\][^\007\033]*(\007|" ESCAPE "\\\\|\\\\u000[7]|\\\\x07|\\\\007|\\\\a)$"
  add("wsl-unc", 1, S "wsl(\\$|\\.localhost)" S "[^/\\\\%]+" S "home" S, "path")
  add("wsl-unc", 1, S "wsl(%2b|\\+)[^/\\\\%]+" S "home" S, "path")
  add("unc-share", 1, "\\\\\\\\(\\\\)*(\\?(\\\\)+unc(\\\\)+)?[^/\\\\%[:space:]]+(\\\\)+(users|home)\\$?" S, "path")
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
function after_an_escape_sequence(text, p,   from) {
  from = p - 300
  if (from < 1) from = 1
  return substr(text, from, p - from) ~ ("(" CSI_END "|" OSC_END ")")
}
function after_a_flag(text, p,   q, letters) {
  q = p - 1
  letters = 0
  while (q >= 1 && substr(text, q, 1) ~ /[A-Za-z]/) {
    q--
    if (++letters > 40) return 0
  }
  if (q < 1 || substr(text, q, 1) != "-") return 0
  q--
  if (q >= 1 && substr(text, q, 1) == "-") q--
  return q < 1 || index(" \t\"'`=,([{<", substr(text, q, 1)) > 0
}
function starts_here(text, p, k,   c) {
  if (k == "any" || p == 1) return 1
  c = substr(text, p - 1, 1)
  if (k == "word") return index(" \t\"'`/\\=:,([{<", c) > 0
  if (c ~ /[A-Za-z0-9_-]/) {
    if (p >= 4 && substr(text, p - 3, 1) == "%" && substr(text, p - 2, 2) ~ /^[0-9A-Fa-f][0-9A-Fa-f]$/) return 1
    if (p >= 5 && substr(text, p - 4, 4) == "/@fs") return 1
    if (after_an_escape_sequence(text, p)) return 1
    if (c !~ /[A-Za-z]/ || p < 3) return 0
    if (substr(text, p - 2, 1) == "\\") return 1
    return after_a_flag(text, p)
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
function name_after_a_blank(text, at,   s, n, rest) {
  s = substr(text, at, 300)
  if (!match(s, "^ " NAME_CHARACTER "+")) return ""
  n = RLENGTH
  rest = substr(s, n + 1)
  if (rest ~ "^([/\\\\\"']|%2[Ff]|%5[Cc])" || rest ~ /^\r?$/) return substr(s, 1, n)
  return ""
}
function scan_text(text, file, use_list,   lowered, size, first, own_end, base, seg, segl, i, t, off, rest, lp, p, n, m, nm, k, adv, out, j) {
  lowered = tolower(text)
  nmatch = 0
  if (!index(lowered, "users") && !index(lowered, "home") && !index(lowered, "documents and settings")) return ""
  split("", covered)
  out = ""
  size = length(text)
  for (first = 1; first <= size; first += CHUNK) {
    own_end = first + CHUNK - 1
    base = first - LOOKBACK
    if (base < 1) base = 1
    seg = substr(text, base, first - base + CHUNK + OVERLAP)
    segl = tolower(seg)
    for (i = 1; i <= nforms; i++) {
      t = low[i] ? segl : seg
      off = 0
      rest = t
      while (match(rest, re[i])) {
        lp = off + RSTART
        n = RLENGTH
        adv = RSTART + n - 1
        p = base + lp - 1
        if (p > own_end) break
        if (p >= first && !(p in covered) && starts_here(seg, lp, kind_of[i])) {
          m = substr(seg, lp, n)
          nm = name_in(m, i)
          k = name_length(nm)
          m = substr(m, 1, length(m) - length(nm) + k)
          n = length(m)
          if (k == 1 && substr(seg, lp + n - 2, 1) == "\\" && substr(seg, lp + n, 1) != "" && index("+*?{", substr(seg, lp + n, 1)) > 0) k = 0
          if (k > 0 && !own_directory(substr(nm, 1, k), substr(seg, lp + n, 8), i)) {
            m = m name_after_a_blank(seg, lp + n)
            n = length(m)
            for (j = p; j < p + n; j++) covered[j] = 1
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
    if (n == 1 && substr(rec, 1, 3) == "\357\273\277") rec = substr(rec, 4)
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
    allowed[file SUBSEP path] = 1
    allowed[list_path SUBSEP path] = 1
  }
  if (rc < 0) malformed(0, "the list cannot be read")
  close(list)
}
function load_base_list(   rc, rec, k, rest, j) {
  n_base = 0
  while ((rc = (getline rec < base_list)) > 0) {
    n_base++
    sub(/\r$/, "", rec)
    if (n_base == 1 && substr(rec, 1, 3) == "\357\273\277") rec = substr(rec, 4)
    k = index(rec, "\t")
    if (!k) continue
    rest = substr(rec, k + 1)
    j = index(rest, "\t")
    if (!j) continue
    in_base[substr(rec, 1, k - 1) SUBSEP substr(rest, 1, j - 1)] = 1
  }
  close(base_list)
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
function added_entry(n, text,   rec, k, rest, path, name, entry_file) {
  rec = text
  sub(/\r$/, "", rec)
  if (n == 1 && substr(rec, 1, 3) == "\357\273\277") rec = substr(rec, 4)
  if (rec == "" || substr(rec, 1, 1) == "#") return
  k = index(rec, "\t")
  if (!k) return
  entry_file = substr(rec, 1, k - 1)
  rest = substr(rec, k + 1)
  k = index(rest, "\t")
  if (!k) return
  path = substr(rest, 1, k - 1)
  if ((entry_file SUBSEP path) in in_base) return
  scan_text(path, "", 0)
  if (nmatch != 1 || last_match != path) return
  if (own_unknown) {
    print "OWN-UNKNOWN " substr(commit, 1, 12) " " n
    return
  }
  name = tolower(last_name)
  if (name == own_full || name == own_cut) print "OWN " substr(commit, 1, 12) " " n
}
function without_the_path_of_an_entry(text,   k, rest, j, path) {
  k = index(text, "\t")
  if (!k) return text
  rest = substr(text, k + 1)
  j = index(rest, "\t")
  if (!j) return text
  path = substr(rest, 1, j - 1)
  if (!((list_path SUBSEP path) in allowed)) return text
  return substr(text, 1, k) substr(rest, j)
}
function check(n, text,   forms) {
  forms = scan_text(file == list_path ? without_the_path_of_an_entry(text) : text, file, 1)
  if (forms != "") {
    hits++
    print "HIT " substr(commit, 1, 12) " " n " " forms " " file
  }
  if (own_check && n > 0 && file == list_path) added_entry(n, text)
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
  own_full = tolower(own_full)
  own_cut = ""
  if (match(own_full, "^" NAMED)) {
    own_cut = substr(own_full, 1, RLENGTH)
    own_cut = substr(own_cut, 1, name_length(own_cut))
  }
  if (base_list != "") load_base_list()
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
  awk -v list="$1" -v list_path="$LIST_IN_THE_REPOSITORY" -v list_forms="${3:-}" -v own_check="$own_check" \
    -v own_full="$own_full" -v own_unknown="$own_unknown" -v base_list="$base_list" -f "$work/scan.awk" "$2"
}

list_of_commit() {
  scan_list=$allow
  [ -z "$allow_given" ] || return 0
  scan_list=$work/list-of-commit
  : >"$scan_list"
  [ -n "$1" ] || return 0
  if git cat-file -e "$1:$LIST_IN_THE_REPOSITORY" 2>/dev/null </dev/null; then
    git cat-file blob "$1:$LIST_IN_THE_REPOSITORY" >"$scan_list" </dev/null || die "git cannot read $LIST_IN_THE_REPOSITORY in commit $1"
  fi
}

scan_stream() {
  run_scan "$scan_list" "$1" >"$2" </dev/null
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
  own_entries=0
  while read -r tag commit number forms file; do
    case $tag in
      HIT) printf '%s: %s %s:%s: a home path (%s)\n' "$NAME" "$commit" "$file" "$number" "$forms" >&2 ;;
      OWN-UNKNOWN) die "commit $commit adds an entry to $LIST_IN_THE_REPOSITORY at line $number, and HOME is unset or empty, so the scan cannot tell whether the entry names the home directory of the account that pushes" ;;
      OWN)
        own_entries=$((own_entries + 1))
        printf '%s: %s %s:%s: this entry names the home directory of the account that runs this push. An entry is only for a made-up name that a fixture holds on purpose, the public account of a hosting service'"'"'s runner, or a line that holds no home path and reads as one, never the home path of a person'"'"'s machine: take the entry out of the commit that added it, and the path out of the commits that hold it.\n' "$NAME" "$commit" "$LIST_IN_THE_REPOSITORY" "$number" >&2
        ;;
    esac
  done <"$work/hits"
  if [ "$found" = 0 ] && [ "$own_entries" = 0 ]; then
    printf '%s: PASS: %s: commits read %s, lines with a home path 0.\n' "$NAME" "$label" "$commits"
    return 0
  fi
  any_failed=1
  {
    printf '%s: FAIL: %s: commits read %s, lines with a home path %s.\n' "$NAME" "$label" "$commits" "$found"
    [ "$own_entries" = 0 ] || printf '%s: FAIL: %s: added entries of the list that name the home directory of the account that pushes: %s.\n' "$NAME" "$label" "$own_entries"
    printf 'Fix: take each listed line out of the commit that added it: git commit --amend for the newest commit, git rebase -i with "edit" for an older one. A later commit that removes the line does not help, because the commit that holds it is pushed. Write a location relative to the repository, take it from an environment variable, or write the name as a placeholder (/Users/<name>). If the path is made up for a fixture, is the public account of a hosting service'"'"'s runner, or is no home path but reads as one (a route at the root of a site, a relative path through a directory called home), add the file, the exact path and a reason that says which to %s and commit it: the list is read from the commit that is pushed or scanned, never from the work tree; to try an entry first, run the range mode with --allow <file> (sh scripts/check-home-paths.sh --help, The list).\n' "$LIST_IN_THE_REPOSITORY"
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

account_that_pushes() {
  own_check=1
  own_full=
  own_unknown=
  home=${HOME-}
  while :; do
    case $home in
      */ | *\\) home=${home%?} ;;
      *) break ;;
    esac
  done
  own_full=${home##*/}
  own_full=${own_full##*\\}
  [ -n "$own_full" ] || own_unknown=1
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
  base_list=$work/list-of-the-default-branch
  : >"$base_list"
  if [ -n "$default_ref_of_remote" ] && git cat-file -e "$default_ref_of_remote:$LIST_IN_THE_REPOSITORY" 2>/dev/null </dev/null; then
    git cat-file blob "$default_ref_of_remote:$LIST_IN_THE_REPOSITORY" >"$base_list" </dev/null || die "git cannot read $LIST_IN_THE_REPOSITORY on $default_ref_of_remote"
  fi
  account_that_pushes
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
    if ! verified "$local_object"; then
      git cat-file -e "$local_object" 2>/dev/null || die "git cannot resolve the object $local_object that the push names for $local_ref"
      continue
    fi
    set -- "$local_object"
    [ -z "$exempt" ] || set -- "$@" "^$exempt"
    case $remote_object in
      *[!0]*)
        if git cat-file -e "$remote_object^{commit}" 2>/dev/null; then set -- "$@" "^$remote_object"; fi
        ;;
    esac
    [ -z "$default_ref_of_remote" ] || set -- "$@" "^$default_ref_of_remote"
    list_of_commit "$local_object^{commit}"
    scan "push of $local_ref to $remote_ref" "$@"
  done
  [ "$any_failed" = 0 ] || return 1
  printf '%s: PASS: the push to %s adds no line with a home path that the list does not allow.\n' "$NAME" "$remote"
}

check_range() {
  git rev-parse --git-dir >/dev/null 2>&1 || die "$PWD is in no git repository"
  label=$*
  first=$(git rev-list -n 1 --end-of-options "$@" -- 2>/dev/null </dev/null) || first=
  list_of_commit "$first"
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
FAIL|macos|macos-after-colour-escapes-of-rustc|{ESC}[0m{ESC}[1m{ESC}[38;5;12m--> {ESC}[0m/{U}/zed/.cargo/registry/src/lib.rs:3:5
FAIL|macos|macos-after-a-colour-escape-in-json|{"rendered":"{JE}[0m/{U}/zed/proj/src/main.rs"}
FAIL|macos|macos-after-a-colour-escape-p095|{ESC}[31m/{U}/Linus_T/x{ESC}[0m
FAIL|macos|macos-after-a-colour-escape-q015|{ESC}[0m/{U}/ada/x
FAIL|macos|macos-after-an-osc-8-link-in-json|{"t":"{JE}]8;;https://example.com{JB}/{U}/zed/notes.txt{JE}]8;;{JB}"}
FAIL|macos|macos-after-an-escape-written-x1b|\x1b[31m/{U}/zed/x
FAIL|macos|macos-after-a-long-flag|cc -isystem/{U}/zed/include
FAIL|macos|macos-after-the-dev-server-prefix|http://localhost:5173/@fs/{U}/zed/proj/src/main.ts
FAIL|macos|macos-after-the-rust-debug-escape|left: "\u{1b}[0m/{U}/zed/proj/src/main.rs"
FAIL|macos|macos-after-the-rust-debug-escape-upper-case|right: "\u{1B}[31m/{U}/zed/x"
FAIL|windows-root|windows-root-exact-case|"\{U}\ada"
FAIL|linux|linux-home|cd /{H}/ada/.cache
FAIL|linux|linux-home-after-colon|scp x host:/{H}/ada
FAIL|linux|linux-home-numeric|/{H}/1000
FAIL|linux|linux-after-python-escape|'out\n/{H}/zed/x.py'
FAIL|linux|linux-after-one-letter-flag|rustc -L/{H}/zed/.cache/lib
FAIL|linux|linux-after-colour-escapes-q016|{ESC}[0m{ESC}[35m/{H}/ada/proj/src/a.rs{ESC}[0m:3:
FAIL|linux|linux-after-an-escape-written-033|\033[1m/{H}/zed/x
FAIL|linux|linux-after-a-double-hyphen-flag|ld --rpath/{H}/zed/lib
FAIL|windows-drive|windows-drive-backslash|C:\{U}\ada
FAIL|windows-drive|windows-drive-forward-slash|"C:/{U}/ada"
FAIL|windows-drive|windows-drive-lower-case|c:/{u}/ada
FAIL|windows-drive|windows-drive-upper-case|C:\{UU}\ADA
FAIL|windows-drive|windows-drive-json-doubled|"C:\\{U}\\ada"
FAIL|windows-drive|windows-drive-percent-encoded|C%3A%5C{U}%5Cada
FAIL|windows-drive|windows-drive-long-path|\\?\C:\{U}\ada
FAIL|windows-drive|windows-drive-name-all|C:\{U}\All\x
FAIL|windows-drive|windows-drive-one-letter-name-at-the-end|C:\{U}\j
FAIL|windows-root|windows-root-backslash|\{u}\ada
FAIL|windows-root|windows-root-unc-share|\\host\c$\{U}\ada
FAIL|windows-root|windows-root-one-letter-name-at-the-end|%SystemDrive%\{U}\X
FAIL|unc-share|unc-share-users|cwd: \\FILESRV\{U}\j2\x
FAIL|unc-share|unc-share-home-dollar|\\FILESRV\{H}$\ada\x
FAIL|unc-share|unc-share-in-a-diff-line|@@ -1 +1 @@ \\FILESRV\{U}\j2\
FAIL|unc-share|unc-share-users-dollar|\\FILESRV\{U}$\zed
FAIL|unc-share|unc-share-in-the-long-spelling|\\?\UNC\FILESRV\{U}\zed\x
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
PASS|-|hyphen-word-before-a-directory|my-app/{H}/index
PASS|-|hyphen-straight-before-a-form|x-/{U}/ada
PASS|-|bracket-and-letter-without-an-escape|x[31m/{U}/zed/x
PASS|-|unc-share-of-another-word|\\FILESRV\homework\ada
PASS|-|unc-share-of-another-hidden-share|\\FILESRV\Media$\zed
PASS|-|unc-share-in-the-long-spelling-of-another-share|\\?\UNC\FILESRV\Media\zed
PASS|-|rust-debug-escape-before-a-relative-path|\u{1b}[0mdocs/{U}/guide.md
PASS|-|colour-escape-before-a-relative-path|{ESC}[0mdocs/{U}/guide.md
PASS|-|dev-server-prefix-of-another-name|/@fsx/{U}/zed
PASS|-|long-flag|--{H}-dir
PASS|-|word-alone|{U} and {H}
PASS|-|environment-variable|$HOME/x and ~/x
PASS|-|git-bash-needs-a-drive-letter|/ab/{U}/ada
EOF
}

expand_fixture() {
  e_acute=$(printf '\303\251')
  escape=$(printf '\033')
  no_break_space=$(printf '\302\240')
  json_escape=$(printf '\\\\%s' u001b)
  json_bell=$(printf '\\\\%s' u0007)
  left_quote=$(printf '\342\200\234')
  right_quote=$(printf '\342\200\235')
  left_guillemet=$(printf '\302\253')
  right_guillemet=$(printf '\302\273')
  carriage_return=$(printf '\r')
  tab=$(printf '\t')
  printf '%s\n' "$1" | sed -e 's|{UU}|USERS|g' -e 's|{U}|Users|g' -e 's|{u}|users|g' -e 's|{H}|home|g' \
    -e 's|{D}|Documents and Settings|g' -e "s|{NB}|$no_break_space|g" -e "s|{E}|$e_acute|g" -e "s|{ESC}|$escape|g" -e "s|{JE}|$json_escape|g" -e "s|{JB}|$json_bell|g" -e "s|{LQ}|$left_quote|g" -e "s|{RQ}|$right_quote|g" \
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

  new_case_repo twin
  expand_fixture 'notes in C:\{U}\Ann Sam\Desktop\todo.txt' >"$case_repo/twin.txt"
  expand_fixture 'and C:\{U}\Ann Sam\Documents' >>"$case_repo/twin.txt"
  expand_fixture 'notes in C:\{U}\Ann Sam\Desktop\todo.txt' >"$case_repo/other.txt"
  twin=$(commit_case 'twin') || die 'commit failed'
  expand_fixture "twin.txt${tab}C:\\{U}\\Ann${tab}made up" >"$work/list-ann"
  expand_fixture "twin.txt${tab}C:\\{U}\\Ann Sam${tab}made up" >"$work/list-ann-sam"
  printf 'other.txt\t' >>"$work/list-ann-sam"
  expand_fixture "C:\\{U}\\Ann${tab}made up" >>"$work/list-ann-sam"
  case_scan entry-for-the-shorter-name-refuses-a-name-with-a-blank 1 'twin.txt:1: a home path (windows-drive)' --allow "$work/list-ann" "$twin"
  case_scan entry-for-the-longer-text-allows-two-lines-of-its-file 1 'lines with a home path 1.' --allow "$work/list-ann-sam" "$twin"
  case_scan entry-allows-no-other-file 1 'other.txt:1: a home path (windows-drive)' --allow "$work/list-ann-sam" "$twin"
  case $(uname -s) in
    MINGW* | MSYS* | CYGWIN*) printf 'case %s/file-name-with-a-tab not run: this system has no such file name\n' "$NAME" ;;
    *)
      tab_name="a${tab}b.txt"
      expand_fixture 'see /{U}/ada' >"$case_repo/$tab_name"
      tab_commit=$(commit_case 'a name with a tab') || die 'commit failed'
      case_scan file-name-with-a-tab-unlisted 1 '"a\tb.txt":1: a home path (macos)' --allow "$work/empty-list" "$tab_commit^!"
      rejected=$((rejected - 1))
      expand_fixture "\"a\\tb.txt\"$tab/{U}/ada${tab}made up" >"$work/list-tab-name"
      case_scan file-name-with-a-tab-listed-by-its-quoted-name 0 'lines with a home path 0' --allow "$work/list-tab-name" "$tab_commit^!"
      ;;
  esac
  mkdir -p "$case_repo/scripts" || die 'mkdir failed'
  expand_fixture "twin.txt${tab}C:\\{U}\\Ann Sam${tab}made up" >"$work/list-ann-sam-alone"
  cp "$work/list-ann-sam-alone" "$case_repo/$LIST_IN_THE_REPOSITORY" || die 'cp failed'
  list_line=$(commit_case 'the list holds an entry for a name with a blank') || die 'commit failed'
  case_scan line-of-the-list-with-an-entry-for-a-name-with-a-blank 0 'lines with a home path 0' --allow "$work/list-ann-sam-alone" "$list_line^!"

  new_case_repo blank-ends
  expand_fixture 'cd "C:\{U}\Ann Sam"' >"$case_repo/quoted.txt"
  expand_fixture 'USERPROFILE=C:\{U}\Ann Sam' >"$case_repo/line-end.txt"
  expand_fixture 'see /{U}/ada now' >"$case_repo/one-word-more.txt"
  blank_ends=$(commit_case 'names that go on after a blank') || die 'commit failed'
  {
    expand_fixture "quoted.txt${tab}C:\\{U}\\Ann${tab}made up"
    expand_fixture "line-end.txt${tab}C:\\{U}\\Ann${tab}made up"
    expand_fixture "one-word-more.txt${tab}/{U}/ada now${tab}made up, with the word after it"
  } >"$work/list-shorter"
  {
    expand_fixture "quoted.txt${tab}C:\\{U}\\Ann Sam${tab}made up"
    expand_fixture "line-end.txt${tab}C:\\{U}\\Ann Sam${tab}made up"
    expand_fixture "one-word-more.txt${tab}/{U}/ada now${tab}made up, with the word after it"
  } >"$work/list-longer"
  case_scan entry-for-the-shorter-name-refuses-a-quoted-name-with-a-blank 1 'quoted.txt:1: a home path (windows-drive)' --allow "$work/list-shorter" "$blank_ends"
  case_scan entry-for-the-shorter-name-refuses-a-name-with-a-blank-at-the-end 1 'line-end.txt:1: a home path (windows-drive)' --allow "$work/list-shorter" "$blank_ends"
  case_scan entries-for-the-longer-texts-allow-a-quote-and-the-end-of-a-line 0 'lines with a home path 0' --allow "$work/list-longer" "$blank_ends"

  new_case_repo pieces
  awk 'BEGIN { printf "/Volumes/"; for (i = 0; i < 2500; i++) printf "v"; printf "/%s/zed/x\n", "Users" }' >"$case_repo/volume-2500.txt" || die 'awk failed'
  awk 'BEGIN { printf "/Volumes/"; for (i = 0; i < 900; i++) printf "v"; printf "/%s/zed/x\n", "Users" }' >"$case_repo/volume-900.txt" || die 'awk failed'
  awk 'BEGIN { for (i = 0; i < 995; i++) printf "x"; printf " /%s/zed/x\n", "Users" }' >"$case_repo/across-1000.txt" || die 'awk failed'
  awk 'BEGIN { for (i = 0; i < 1995; i++) printf "x"; printf " /%s/zed/x\n", "Users" }' >"$case_repo/across-2000.txt" || die 'awk failed'
  awk 'BEGIN { for (i = 0; i < 999; i++) printf "x"; printf "a/%s/zed/x\n", "Users" }' >"$case_repo/after-a-letter-of-the-piece-before.txt" || die 'awk failed'
  pieces=$(commit_case 'paths at the edges of a piece') || die 'commit failed'
  case_scan path-cut-at-its-piece-passes 1 'lines with a home path 3.' --allow "$work/empty-list" "$pieces"
  case_scan path-of-900-letters-is-read 1 'volume-900.txt:1: a home path (macos-volume)' --allow "$work/empty-list" "$pieces"
  case_scan path-across-the-1000th-character 1 'across-1000.txt:1: a home path (macos)' --allow "$work/empty-list" "$pieces"
  case_scan path-across-the-2000th-character 1 'across-2000.txt:1: a home path (macos)' --allow "$work/empty-list" "$pieces"

  new_case_repo list-of-the-commit
  mkdir -p "$case_repo/scripts" || die 'mkdir failed'
  cp "$self" "$case_repo/scripts/check-home-paths.sh" || die 'cp failed'
  expand_fixture 'see /{U}/wendy' >"$case_repo/w.txt"
  in_case_repo add w.txt || die 'git add failed'
  in_case_repo commit -q -m 'a line' || die 'commit failed'
  expand_fixture "w.txt$tab/{U}/wendy${tab}made up" >"$case_repo/$LIST_IN_THE_REPOSITORY"
  (cd "$case_repo" && HOME=$work/homes/nobody sh scripts/check-home-paths.sh 'HEAD^!') >"$work/out-range-list-in-the-work-tree-alone" 2>&1
  code=$?
  case $(cat "$work/out-range-list-in-the-work-tree-alone") in
    *'w.txt:1: a home path (macos)'*) got=$code ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict range-entry-in-the-work-tree-alone-allows-nothing 1 "$got"
  (cd "$case_repo" && HOME=$work/homes/nobody sh scripts/check-home-paths.sh --allow "$LIST_IN_THE_REPOSITORY" 'HEAD^!') >"$work/out-range-allow" 2>&1
  verdict range-entry-tried-with-allow 0 "$?"

  new_case_repo listed
  expand_fixture 'see /{U}/ada for the example' >"$case_repo/made-up.txt"
  expand_fixture 'see /{U}/ada{NB}now' >"$case_repo/no-break.txt"
  listed=$(commit_case 'listed') || die 'commit failed'
  u=Users
  printf 'made-up.txt\t/%s/ada\tmade up\r\n\r\nno-break.txt\t/%s/ada\tmade up\r\n' "$u" "$u" >"$work/list-crlf-empty"
  case_scan list-checked-out-with-crlf-and-an-empty-line 0 'lines with a home path 0' --allow "$work/list-crlf-empty" "$listed"
  printf '\t/%s/ada\tmade up\n' "$u" >"$work/list-no-file"
  case_scan entry-with-an-empty-file 2 'line 1: the file is empty' --allow "$work/list-no-file" "$listed"
  printf '\357\273\277made-up.txt\t/%s/ada\tmade up\nno-break.txt\t/%s/ada\tmade up\n' "$u" "$u" >"$work/list-bom"
  case_scan list-that-starts-with-a-byte-order-mark 0 'lines with a home path 0' --allow "$work/list-bom" "$listed"
  printf 'made-up.txt\t/%s/ada\tmade up\n' "$u" >"$work/list-no-nbsp"
  case_scan listed-path-followed-by-a-no-break-space 1 'no-break.txt:1: a home path (macos)' --allow "$work/list-no-nbsp" "$listed"
  case_scan listed-path-followed-by-a-no-break-space-passes-with-its-entry 0 'lines with a home path 0' --allow "$work/list-bom" "$listed"

  new_case_repo replaced
  printf 'a\nb\nc\n' >"$case_repo/r.txt"
  commit_case 'three lines' >/dev/null || die 'commit failed'
  { printf 'a\n'; expand_fixture '/{H}/rhea'; printf 'c\n'; } >"$case_repo/r.txt"
  replaced=$(commit_case 'replace line 2') || die 'commit failed'
  case_scan replaced-line-reports-its-own-line 1 'r.txt:2: a home path (linux)' --allow "$work/empty-list" "$replaced^!"
  root_of_replaced=$(in_case_repo rev-list --max-parents=0 HEAD) || die 'git rev-list failed'
  expand_fixture "conv.txt diff=lower" >"$case_repo/.gitattributes"
  expand_fixture '/{U}/tess' >"$case_repo/conv.txt"
  converted=$(commit_case 'a file with a text conversion') || die 'commit failed'
  (cd "$case_repo" && GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=diff.lower.textconv GIT_CONFIG_VALUE_0='sed -e s/Users/users/' \
    HOME=$work/homes/nobody sh "$self" --allow "$work/empty-list" "$converted^!") >"$work/out-textconv" 2>&1
  code=$?
  case $(cat "$work/out-textconv") in
    *'conv.txt:1: a home path (macos)'*) got=$code ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict clone-that-converts-the-text-of-a-file 1 "$got"
  new_case_repo root
  expand_fixture '/{H}/rory' >"$case_repo/root.txt"
  only_root=$(commit_case 'root') || die 'commit failed'
  (cd "$case_repo" && GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=log.showRoot GIT_CONFIG_VALUE_0=false \
    HOME=$work/homes/nobody sh "$self" --allow "$work/empty-list" "$only_root") >"$work/out-show-root" 2>&1
  code=$?
  case $(cat "$work/out-show-root") in
    *'root.txt:1: a home path (linux)'*) got=$code ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict root-commit-in-a-clone-with-show-root-off 1 "$got"
  [ -n "$root_of_replaced" ] || die 'no root commit'

  new_case_repo wide
  awk 'BEGIN { for (i = 0; i < 20000; i++) printf "/%s/zed%d ", "Users", i; printf "\n" }' >"$case_repo/wide.txt" || die 'awk failed'
  wide=$(commit_case 'one line with 20000 paths') || die 'commit failed'
  timed_case one-line-with-20000-paths 'wide.txt:1: a home path (macos)' "$wide^!"
  awk 'BEGIN { for (i = 0; i < 8000; i++) printf "see /%s/zed%d/x\n", "Users", i }' >"$case_repo/tall.txt" || die 'awk failed'
  tall=$(commit_case '8000 lines with a path') || die 'commit failed'
  timed_case 8000-lines-with-a-path 'lines with a home path 8000.' "$tall^!"
}

short_id() {
  printf '%s' "$1" | cut -c1-12
}

timed_case() {
  started=$(date +%s) || die 'date failed'
  (cd "$case_repo" && HOME=$work/homes/nobody sh "$self" --allow "$work/empty-list" "$3") >"$work/out-$1" 2>&1
  code=$?
  ended=$(date +%s) || die 'date failed'
  seconds=$((ended - started))
  got=$code
  case $(cat "$work/out-$1") in
    *"$2"*) ;;
    *) got="$code-without-the-text" ;;
  esac
  [ "$seconds" -le 120 ] || got="$got-after-$seconds-s"
  printf 'seconds %s/%s %s\n' "$NAME" "$1" "$seconds"
  verdict "$1" 1 "$got"
}

in_case_repo() {
  git -C "$case_repo" -c commit.gpgsign=false -c core.hooksPath="$work/no-hooks" "$@"
}

new_case_repo() {
  case_repo=$work/$1
  git init -q "$case_repo" || die 'git init failed'
}

commit_case() {
  in_case_repo add -A . || die 'git add failed'
  in_case_repo commit -q -m "$1" || die 'commit failed'
  in_case_repo rev-parse HEAD
}

case_scan() {
  case_name=$1
  want_exit=$2
  want_text=$3
  shift 3
  (cd "$case_repo" && HOME=$work/homes/nobody sh "$self" "$@") >"$work/out-$case_name" 2>&1
  code=$?
  got=$code
  case $(cat "$work/out-$case_name") in
    *"$want_text"*) ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict "$case_name" "$want_exit" "$got"
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
  if [ "$push_home" = UNSET ]; then
    (cd "$repo" && env -u HOME git -c core.hooksPath="$hook_home/.githooks" push "$@") >"$work/out-$case_name" 2>&1
  else
    (cd "$repo" && HOME=$push_home git -c core.hooksPath="$hook_home/.githooks" push "$@") >"$work/out-$case_name" 2>&1
  fi
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
  push_home=$work/homes/nobody
  remote_repo=$work/remote.git
  git init -q --bare "$remote_repo" || die 'git init failed'
  in_repo checkout -q -b trunk "$root_commit" || die 'git checkout failed'
  u=Users
  mkdir -p "$repo/scripts" || die 'mkdir failed'
  {
    cat "$list"
    printf 'fixtures/made-up.txt\t/%s/sam\ta made-up name that the base list holds\n' "$u"
    printf 'hosted.txt\t/%s/runner\tthe public account of the runners of a hosting service\n' "$u"
  } >"$repo/$LIST_IN_THE_REPOSITORY" || die 'cannot write the list'
  trunk_list=$(commit_all 'the list') || die 'commit failed'
  printf 'fixtures/made-up.txt\t/%s/kim\ta made-up name that main adds\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  kim_commit=$(commit_all 'an entry for kim') || die 'commit failed'
  cp "$repo/$LIST_IN_THE_REPOSITORY" "$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cp failed'
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

  in_repo checkout -q -b malformed trunk || die 'git checkout failed'
  printf 'x\tnot a path\treason\n' >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'a malformed list' >/dev/null || die 'commit failed'
  push_case scan-cannot-run refused 'cannot check, so the commits count as refused' proof malformed

  in_repo checkout -q -b in-the-work-tree trunk || die 'git checkout failed'
  expand_fixture 'see /{U}/wendy' >"$repo/wendy.txt"
  commit_all 'a line that an uncommitted entry would allow' >/dev/null || die 'commit failed'
  printf 'wendy.txt\t/%s/wendy\tmade up\n' "$u" >>"$hook_home/$LIST_IN_THE_REPOSITORY"
  printf 'wendy.txt\t/%s/wendy\tmade up\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  push_case entry-in-the-work-tree-alone refused 'wendy.txt:1: a home path (macos)' proof in-the-work-tree
  in_repo add "$LIST_IN_THE_REPOSITORY" || die 'git add failed'
  push_case entry-staged-and-not-committed refused 'wendy.txt:1: a home path (macos)' proof in-the-work-tree
  in_repo commit -q -m 'the entry, after the line' || die 'commit failed'
  push_case entry-committed-after-the-line-in-the-same-push pushed 'the push to proof adds no line' proof in-the-work-tree
  cp "$repo/$LIST_IN_THE_REPOSITORY" "$work/list-with-wendy" || die 'cp failed'
  in_repo checkout -q -b xena-line trunk || die 'git checkout failed'
  expand_fixture 'see /{U}/xena' >"$repo/xena.txt"
  commit_all 'a line for xena' >/dev/null || die 'commit failed'
  in_repo checkout -q -b xena-entry trunk || die 'git checkout failed'
  printf 'xena.txt\t/%s/xena\tmade up\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'an entry for xena' >/dev/null || die 'commit failed'
  push_case line-that-only-the-other-ref-lists refused 'xena.txt:1: a home path (macos)' proof xena-line xena-entry
  in_repo checkout -q -b no-list "$root_commit" || die 'git checkout failed'
  printf 'plain\n' >"$repo/no-list.txt"
  commit_all 'a commit without the list' >/dev/null || die 'commit failed'
  push_case pushed-commit-without-the-list pushed 'the push to proof adds no line' proof no-list
  expand_fixture 'see /{U}/yuri' >"$repo/yuri.txt"
  commit_all 'a line without the list' >/dev/null || die 'commit failed'
  push_case pushed-commit-without-the-list-and-a-line refused 'yuri.txt:1: a home path (macos)' proof no-list
  in_repo checkout -q trunk || die 'git checkout failed'
  cp "$repo/$LIST_IN_THE_REPOSITORY" "$hook_home/$LIST_IN_THE_REPOSITORY" || die 'cp failed'

  push_home=$work/homes/kim
  in_repo checkout -q -b moved trunk || die 'git checkout failed'
  { sed -n '$p' "$repo/$LIST_IN_THE_REPOSITORY"; sed '$d' "$repo/$LIST_IN_THE_REPOSITORY"; } >"$work/moved-list"
  cp "$work/moved-list" "$repo/$LIST_IN_THE_REPOSITORY" || die 'cp failed'
  commit_all 'move the line of an entry that main holds' >/dev/null || die 'commit failed'
  push_case moved-line-of-an-entry-that-main-holds pushed 'the push to proof adds no line' proof moved
  in_repo checkout -q -b copied "$trunk_list" || die 'git checkout failed'
  in_repo cherry-pick -x "$kim_commit" >/dev/null 2>&1 || die 'git cherry-pick failed'
  push_case copy-of-the-commit-that-added-an-entry-of-main pushed 'the push to proof adds no line' proof copied
  in_repo checkout -q -b reworded trunk || die 'git checkout failed'
  sed 's/a made-up name that main adds$/the same made-up name, its reason written again/' "$repo/$LIST_IN_THE_REPOSITORY" >"$work/reworded-list"
  cp "$work/reworded-list" "$repo/$LIST_IN_THE_REPOSITORY" || die 'cp failed'
  commit_all 'write the reason of an entry again' >/dev/null || die 'commit failed'
  push_case reason-of-an-entry-of-main-written-again pushed 'the push to proof adds no line' proof reworded
  in_repo checkout -q -b kim-elsewhere trunk || die 'git checkout failed'
  printf 'x.txt\t/%s/kim\ta made-up name\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'an entry for kim that main lacks' >/dev/null || die 'commit failed'
  push_case entry-that-main-lacks refused 'this entry names the home directory of the account that runs this push' proof kim-elsewhere
  in_repo checkout -q -b kim-other-path trunk || die 'git checkout failed'
  printf 'fixtures/made-up.txt\tC:\\%s\\kim\tthe file of an entry of main and another path\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'an entry with the file of an entry of main and another path' >/dev/null || die 'commit failed'
  push_case entry-with-the-file-of-an-entry-of-main-and-another-path refused 'this entry names the home directory of the account that runs this push' proof kim-other-path

  in_repo checkout -q -b add-signs trunk || die 'git checkout failed'
  {
    printf 'o.txt\t/%s/o\ta made-up name\n' "$u"
    printf 'zed.txt\t/%s/zed\ta made-up name\n' "$u"
    printf 'ann.txt\t/%s/ann\ta made-up name\n' "$u"
  } >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'entries for names before a sign' >/dev/null || die 'commit failed'
  push_home="$work/homes/o'brien"
  push_case added-entry-for-an-account-with-an-apostrophe refused 'this entry names the home directory of the account that runs this push' proof add-signs
  push_home="$work/homes/zed@corp"
  push_case added-entry-for-an-account-with-an-at-sign refused 'this entry names the home directory of the account that runs this push' proof add-signs
  push_home="$work/homes/ann,b"
  push_case added-entry-for-an-account-with-a-comma refused 'this entry names the home directory of the account that runs this push' proof add-signs
  case $(uname -s) in
    MINGW* | MSYS* | CYGWIN*) printf 'case %s/added-entry-for-an-account-with-a-curly-apostrophe not run: this system converts a HOME outside ASCII\n' "$NAME" ;;
    *)
      push_home="$work/homes/zed$(printf '\342\200\231')s"
      push_case added-entry-for-an-account-with-a-curly-apostrophe refused 'this entry names the home directory of the account that runs this push' proof add-signs
      rejected=$((rejected - 1))
      ;;
  esac
  push_home=$work/homes/nobody
  in_repo checkout -q -b plain-as-sam trunk || die 'git checkout failed'
  printf 'plain\n' >"$repo/plain-as-sam.txt"
  commit_all 'plain' >/dev/null || die 'commit failed'
  push_home=$work/homes/sam
  push_case account-named-like-an-entry-pushes-a-plain-commit pushed 'the push to proof adds no line' proof plain-as-sam
  push_case account-named-like-an-entry-deletes-a-ref pushed '' proof :refs/heads/plain-as-sam
  push_home=$work/homes/runner
  push_case account-named-runner-pushes-a-plain-commit pushed 'the push to proof adds no line' proof plain-as-sam
  in_repo checkout -q -b add-sam trunk || die 'git checkout failed'
  mkdir -p "$repo/scripts" || die 'mkdir failed'
  printf 'x.txt\t/%s/sam\ta made-up name\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'an entry for sam' >/dev/null || die 'commit failed'
  push_home=$work/homes/sam
  push_case added-entry-for-the-account-that-pushes refused 'this entry names the home directory of the account that runs this push' proof add-sam
  push_home=$work/homes/sam/
  push_case added-entry-with-a-trailing-slash-in-home refused 'this entry names the home directory of the account that runs this push' proof add-sam
  push_home="C:\\$u\\sam\\"
  push_case added-entry-with-home-in-the-windows-form refused 'this entry names the home directory of the account that runs this push' proof add-sam
  case $(uname -s) in
    MINGW* | MSYS* | CYGWIN*)
      shell_home=$(env -u HOME sh -c 'printenv HOME >/dev/null && echo set || echo unset') || shell_home=unknown
      git_home=$(env -u HOME git -c alias.home-state='!printenv HOME >/dev/null && echo set || echo unset' home-state) || git_home=unknown
      printf 'case %s/added-entry-with-home-unset left out, not counted: with HOME unset before them, HOME is %s in a shell of this system and %s in a command that git runs\n' "$NAME" "$shell_home" "$git_home"
      ;;
    *)
      push_home=UNSET
      push_case added-entry-with-home-unset refused 'HOME is unset or empty' proof add-sam
      rejected=$((rejected - 1))
      ;;
  esac
  in_repo checkout -q -b add-ann-sam trunk || die 'git checkout failed'
  mkdir -p "$repo/scripts" || die 'mkdir failed'
  printf 'y.txt\t/%s/Ann\ta made-up name\n' "$u" >>"$repo/$LIST_IN_THE_REPOSITORY"
  commit_all 'an entry for the first word of an account' >/dev/null || die 'commit failed'
  push_home="$work/homes/Ann Sam"
  push_case added-entry-for-an-account-of-two-words refused 'this entry names the home directory of the account that runs this push' proof add-ann-sam
  push_home=$work/homes/nobody
  in_repo checkout -q add-sam || die 'git checkout failed'
  sam_commit=$(in_repo rev-parse HEAD) || die 'git rev-parse failed'
  (cd "$repo" && HOME=$work/homes/sam sh "$self" --allow "$hook_home/$LIST_IN_THE_REPOSITORY" "$sam_commit^!") >"$work/out-added-entry-in-a-range" 2>&1
  verdict added-entry-for-the-account-in-a-range 0 "$?"

  mv "$hook_home/scripts/check-home-paths.sh" "$work/scanner-aside" || die 'mv failed'
  : >"$hook_home/scripts/check-home-paths.sh"
  push_case scanner-with-no-bytes-beside-the-hook refused 'without its PASS line' proof messages
  mv "$work/scanner-aside" "$hook_home/scripts/check-home-paths.sh" || die 'mv failed'
  (cd "$repo" && printf 'refs/heads/x %s refs/heads/x %s\n' 1234567890123456789012345678901234567890 0000000000000000000000000000000000000000 |
    HOME=$work/homes/nobody sh "$self" --allow "$list" --pre-push proof) >"$work/out-unresolvable-object" 2>&1
  code=$?
  case $(cat "$work/out-unresolvable-object") in
    *'git cannot resolve the object'*) got=$code ;;
    *) got="$code-without-the-text" ;;
  esac
  verdict pushed-object-that-git-cannot-resolve 2 "$got"

  exempt_home=$work/exempt-home/scripts
  mkdir -p "$exempt_home" || die 'mkdir failed'
  cp "$self" "$exempt_home/check-home-paths.sh" || die 'cp failed'
  in_repo checkout -q -b exempt trunk || die 'git checkout failed'
  expand_fixture '/{H}/ezra' >"$repo/exempt.txt"
  exempt_commit=$(commit_all 'the exempt commit') || die 'commit failed'
  expand_fixture '/{H}/lena' >"$repo/later.txt"
  later_commit=$(commit_all 'a later commit') || die 'commit failed'
  printf 'NEWEST_EXEMPT_COMMIT=%s\n' "$exempt_commit" >"$exempt_home/check-commit-messages.sh"
  (cd "$repo" && printf 'refs/heads/exempt %s refs/heads/exempt %s\n' "$later_commit" 0000000000000000000000000000000000000000 |
    HOME=$work/homes/nobody sh "$exempt_home/check-home-paths.sh" --allow "$list" --pre-push proof) >"$work/out-exempt" 2>&1
  case $(cat "$work/out-exempt") in
    *exempt.txt*) got=the-exempt-commit-read ;;
    *'later.txt:1: a home path (linux)'*) got=the-later-commit-alone ;;
    *) got=neither ;;
  esac
  verdict exempt-commit-left-out-and-a-later-one-read the-later-commit-alone "$got"
  [ "$got" != the-later-commit-alone ] || rejected=$((rejected + 1))

  case $(uname -s) in
    MINGW* | MSYS* | CYGWIN*) printf 'case %s/scan-ended-by-term not run: this system emulates a FIFO\n' "$NAME" ;;
    *)
      mkfifo "$work/fifo" || die 'mkfifo failed'
      sleep 30 >"$work/fifo" &
      writer=$!
      (cd "$repo" && HOME=$work/homes/nobody exec sh "$self" --allow "$list" --pre-push proof) <"$work/fifo" >"$work/out-term" 2>&1 &
      scanner=$!
      sleep 1
      kill -TERM "$scanner" || die 'kill failed'
      wait "$scanner"
      code=$?
      kill "$writer" 2>/dev/null
      wait "$writer" 2>/dev/null
      if [ "$code" = 0 ]; then got=0; else got=not-0; fi
      printf 'case %s/scan-ended-by-term want=not-0 got=%s\n' "$NAME" "$got"
      [ "$got" = not-0 ] || wrong="$wrong scan-ended-by-term"
      ;;
  esac

  mv "$hook_home/scripts/check-home-paths.sh" "$work/scanner-aside" || die 'mv failed'
  push_case scanner-missing-beside-the-hook pushed 'the scan for home paths did not run' proof messages
  mv "$work/scanner-aside" "$hook_home/scripts/check-home-paths.sh" || die 'mv failed'

  (cd "$repo" && printf 'not a line that git writes\n' | HOME=$work/homes/nobody sh "$self" --allow "$list" --pre-push proof) >"$work/out-unreadable-line" 2>&1
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

allow=
mode=range
remote=
any_failed=0
own_check=
own_full=
own_unknown=
base_list=
scan_list=
allow_given=
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
      allow_given=1
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
