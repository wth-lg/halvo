#!/bin/sh
set -eu

help_text() {
  cat <<'EOF'
Moves the changelog lines of a branch out of CHANGELOG.md into its entry file.

  git fetch origin
  git merge origin/main
  sh scripts/changelog-entry.sh
      Run the three in this order in the branch of a unit that added lines
      under the ## [Unreleased] heading of CHANGELOG.md. The script refuses,
      with exit 1 and no file changed, unless origin/main holds
      changelog.d/README.md (it is a main of the rule of entry files) and is an
      ancestor of HEAD (the branch has merged it). Then it takes the lines below
      that heading that the work tree has and that origin/main and the entry
      file lack to the letter, and appends them to changelog.d/<branch>.md,
      where <branch> is the name of the branch with every sign other than a
      letter, a digit, ".", "_" and "-" turned into "-". When CHANGELOG.md
      differs from the file of origin/main, it sets the file to that one. It
      stages both files, commits nothing, and prints the commit of origin/main
      that it read and the number of lines that it moved.
  sh scripts/changelog-entry.sh --help
      Prints this text.

A line that the branch reworded, or one that main holds in other words, is
moved too: read git diff --cached and delete such a line from the entry file.
A second run finds no line to move and changes nothing. Then commit, and check
with sh scripts/check-changelog.sh --base origin/main.

Exit codes: 0 done or nothing to do, 1 refused, 2 it could not run, 64 bad usage.
EOF
}

die() {
  printf 'changelog-entry: error: %s\n' "$1" >&2
  exit 2
}

refuse() {
  printf 'changelog-entry: refused: %s Nothing was changed. Fix: run git fetch origin, then git merge origin/main, then sh scripts/changelog-entry.sh.\n' "$1" >&2
  exit 1
}

case $# in
  0) ;;
  1)
    case $1 in
      -h|--help) help_text; exit 0 ;;
      *) printf 'changelog-entry: unknown argument: %s; it takes none but --help\n' "$1" >&2; exit 64 ;;
    esac
    ;;
  *) printf 'changelog-entry: it takes no argument but --help\n' >&2; exit 64 ;;
esac

root=$(git rev-parse --show-toplevel 2>/dev/null) || die 'the current directory is not inside the work tree of a git clone'
cd "$root" || die "cannot enter $root"
branch=$(git symbolic-ref --quiet --short HEAD) || die 'HEAD is not on a branch: check out the branch of your unit'
[ "$branch" != main ] || die 'HEAD is on main: run this in the branch of your unit'
main=$(git rev-parse --verify --quiet 'refs/remotes/origin/main^{commit}') ||
  refuse 'this clone has no origin/main.'
git cat-file -e "$main:changelog.d/README.md" 2>/dev/null ||
  refuse "origin/main ($main) holds no changelog.d/README.md, so it is older than the rule of entry files. In a clone of a local copy, origin is that copy: fetch from a remote whose main holds the rule."
git merge-base --is-ancestor "$main" HEAD ||
  refuse "origin/main ($main) is not an ancestor of HEAD: the branch has not merged it."
name=$(printf '%s' "$branch" | LC_ALL=C tr -c 'A-Za-z0-9._-' '-')
entry_file=changelog.d/$name.md
[ -f CHANGELOG.md ] || die "no such file: $root/CHANGELOG.md"

main_changelog=$(git cat-file blob "$main:CHANGELOG.md") || die 'git could not read CHANGELOG.md at origin/main'
part=$(printf '\001')
moved=$({
  printf '%s\n%s\n' "$main_changelog" "$part"
  [ ! -f "$entry_file" ] || { cat "$entry_file"; printf '\n'; }
  printf '%s\n' "$part"
  cat CHANGELOG.md
} | LC_ALL=C awk -v part="$part" '
  $0 == part { file++; on = 0; next }
  { sub(/[ \t\r]+$/, "") }
  file == 1 { known[$0] = 1; next }
  $0 == "## [Unreleased]" { on = 1; next }
  /^## / { on = 0 }
  !on || $0 == "" { next }
  file == 0 { known[$0] = 1; next }
  !($0 in known) { known[$0] = 1; print }
') || die 'awk could not read CHANGELOG.md or the entry file'

same_as_main=yes
git diff --quiet "$main" -- CHANGELOG.md || same_as_main=no

if [ -z "$moved" ] && [ "$same_as_main" = yes ]; then
  if [ -f "$entry_file" ]; then
    printf 'changelog-entry: read origin/main at %s; moved 0 lines: nothing to move, CHANGELOG.md is the file of origin/main, and %s holds the lines of %s.\n' "$main" "$entry_file" "$branch"
  else
    printf 'changelog-entry: read origin/main at %s; moved 0 lines: nothing to move, %s added no line under the Unreleased heading. Write %s by hand, one line for each type of change.\n' "$main" "$branch" "$entry_file"
  fi
  exit 0
fi

if [ -n "$moved" ]; then
  mkdir -p changelog.d || die 'mkdir could not make changelog.d'
  if [ -s "$entry_file" ] && [ -n "$(tail -c 1 "$entry_file")" ]; then
    printf '\n' >>"$entry_file" || die "cannot write $entry_file"
  fi
  printf '%s\n' "$moved" >>"$entry_file" || die "cannot write $entry_file"
  git add -- "$entry_file" || die "git add could not stage $entry_file"
fi
if [ "$same_as_main" = no ]; then
  git checkout "$main" -- CHANGELOG.md || die 'git checkout could not set CHANGELOG.md to the file of origin/main'
fi
count=0
[ -z "$moved" ] || count=$(($(printf '%s\n' "$moved" | wc -l)))
printf 'changelog-entry: read origin/main at %s; moved %d lines into %s, and CHANGELOG.md is the file of origin/main. Both are staged: read git diff --cached, then commit.\n' "$main" "$count" "$entry_file"
