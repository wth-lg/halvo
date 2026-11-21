#!/bin/sh
set -eu

help_text() {
  cat <<'EOF'
Joins the entry files under changelog.d/ into CHANGELOG.md, for the commit of a release.

  sh scripts/changelog-join.sh
      Puts the lines of every entry file that git tracks, changelog.d/<name>.md
      other than changelog.d/README.md, in the order of the file names and each
      file in its own order, at the end of the list under the ## [Unreleased]
      heading of CHANGELOG.md. It skips empty lines. Then it removes the entry
      files with git rm and stages CHANGELOG.md. It commits nothing.
  sh scripts/changelog-join.sh --help
      Prints this text.

With no entry file it changes nothing, so a second run changes nothing.
Check the result with sh scripts/check-changelog.sh, which refuses an entry
text that is under the heading twice.

Exit codes: 0 done or nothing to do, 2 it could not run, 64 bad usage.
EOF
}

die() {
  printf 'changelog-join: error: %s\n' "$1" >&2
  exit 2
}

case $# in
  0) ;;
  1)
    case $1 in
      -h|--help) help_text; exit 0 ;;
      *) printf 'changelog-join: unknown argument: %s; it takes none but --help\n' "$1" >&2; exit 64 ;;
    esac
    ;;
  *) printf 'changelog-join: it takes no argument but --help\n' >&2; exit 64 ;;
esac

root=$(git rev-parse --show-toplevel 2>/dev/null) || die 'the current directory is not inside the work tree of a git clone'
cd "$root" || die "cannot enter $root"
[ -f CHANGELOG.md ] || die "no such file: $root/CHANGELOG.md"
tracked=$(git -c core.quotePath=false ls-files -- changelog.d) || die 'git ls-files could not list changelog.d'
files=$(printf '%s\n' "$tracked" | LC_ALL=C awk '/^changelog\.d\/[^\/]+\.md$/ && $0 != "changelog.d/README.md"' | LC_ALL=C sort)
if [ -z "$files" ]; then
  printf 'changelog-join: no entry file under changelog.d/; nothing to join.\n'
  exit 0
fi
lines=$(printf '%s\n' "$files" | while IFS= read -r path; do
  [ -f "$path" ] || die "git tracks $path, and the work tree lacks it: restore it with git checkout -- '$path', or stage its removal"
  LC_ALL=C sed -e 's/[[:space:]]*$//' -e '/^$/d' "$path" || die "sed could not read $path"
done) || exit 2
joined=$(JOIN_LINES=$lines LC_ALL=C awk '
  { raw[NR] = $0; line = $0; sub(/[ \t\r]+$/, "", line) }
  !start && line == "## [Unreleased]" { start = NR; next }
  start && !stop && line ~ /^## / { stop = NR }
  start && !stop && line ~ /^- / { last = NR }
  END {
    if (!start) { print "CHANGELOG.md has no line ## [Unreleased]" > "/dev/stderr"; exit 1 }
    eol = (raw[1] ~ /\r$/) ? "\r" : ""
    count = split(ENVIRON["JOIN_LINES"], add, "\n")
    at = last ? last : start
    for (i = 1; i <= at; i++) print raw[i]
    if (!last) print eol
    for (i = 1; i <= count; i++) print add[i] eol
    if (!last && at + 1 <= NR && raw[at + 1] !~ /^[ \t\r]*$/) print eol
    for (i = at + 1; i <= NR; i++) print raw[i]
  }
' CHANGELOG.md) || die 'awk could not join the entry files into CHANGELOG.md'
printf '%s\n' "$joined" >CHANGELOG.md || die 'cannot write CHANGELOG.md'
printf '%s\n' "$files" | while IFS= read -r path; do
  git rm -q -- "$path" || die "git rm could not remove $path"
done || exit 2
git add -- CHANGELOG.md || die 'git add could not stage CHANGELOG.md'
printf 'changelog-join: joined %d lines of %d entry files at the end of the list under the Unreleased heading, and removed the files. Both are staged: run sh scripts/check-changelog.sh, then commit with the release.\n' \
  "$(($(printf '%s\n' "$lines" | wc -l)))" "$(($(printf '%s\n' "$files" | wc -l)))"
