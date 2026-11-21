#!/bin/sh
set -eu

help_text() {
  cat <<'EOF'
Moves the changelog lines of a branch out of CHANGELOG.md into its entry file.

  sh scripts/changelog-entry.sh
      Run it in the branch of a unit that added lines under the ## [Unreleased]
      heading of CHANGELOG.md. It takes the lines below that heading which the
      work tree has and which neither the merge base of HEAD with origin/main
      nor origin/main has, and appends each that the file lacks to
      changelog.d/<branch>.md, where <branch> is the name of the branch with
      every sign other than a letter, a digit, ".", "_" and "-" turned into "-".
      When CHANGELOG.md differs from origin/main from that heading down, it sets
      the whole file to the one of origin/main, so a change of the branch above
      the heading goes too: read git diff --cached. It stages both files and
      commits nothing.
  sh scripts/changelog-entry.sh --help
      Prints this text.

A second run finds no line to move and changes nothing. Run git fetch origin
first, so that origin/main is the newest main. Then commit, and check with
sh scripts/check-changelog.sh --base origin/main.

Exit codes: 0 done or nothing to do, 2 it could not run, 64 bad usage.
EOF
}

die() {
  printf 'changelog-entry: error: %s\n' "$1" >&2
  exit 2
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
main=$(git rev-parse --verify --quiet 'refs/remotes/origin/main^{commit}') ||
  die 'this clone has no origin/main: run git fetch origin, then this again'
branch=$(git symbolic-ref --quiet --short HEAD) || die 'HEAD is not on a branch: check out the branch of your unit'
[ "$branch" != main ] || die 'HEAD is on main: run this in the branch of your unit'
base=$(git merge-base HEAD "$main") || die 'HEAD has no merge base with origin/main'
name=$(printf '%s' "$branch" | LC_ALL=C tr -c 'A-Za-z0-9._-' '-')
entry_file=changelog.d/$name.md
[ -f CHANGELOG.md ] || die "no such file: $root/CHANGELOG.md"

base_changelog=$(git cat-file blob "$base:CHANGELOG.md") || die 'git could not read CHANGELOG.md at the merge base'
main_changelog=$(git cat-file blob "$main:CHANGELOG.md") || die 'git could not read CHANGELOG.md at origin/main'
part=$(printf '\001')
moved=$({
  printf '%s\n%s\n%s\n%s\n' "$base_changelog" "$part" "$main_changelog" "$part"
  [ ! -f "$entry_file" ] || { cat "$entry_file"; printf '\n'; }
  printf '%s\n' "$part"
  cat CHANGELOG.md
} | LC_ALL=C awk -v part="$part" '
  $0 == part { file++; on = 0; next }
  { sub(/[ \t\r]+$/, "") }
  file == 2 { known[$0] = 1; next }
  $0 == "## [Unreleased]" { on = 1; next }
  /^## / { on = 0 }
  !on || $0 == "" { next }
  file < 2 { known[$0] = 1; next }
  !($0 in known) { known[$0] = 1; print }
') || die 'awk could not read CHANGELOG.md or the entry file'

tail_of() {
  LC_ALL=C awk '{ sub(/[ \t\r]+$/, "") } $0 == "## [Unreleased]" { on = 1 } on'
}
here_tail=$(tail_of <CHANGELOG.md) || die 'awk could not read CHANGELOG.md'
main_tail=$(printf '%s\n' "$main_changelog" | tail_of) || die 'awk could not read CHANGELOG.md of origin/main'

if [ -z "$moved" ] && [ "$here_tail" = "$main_tail" ]; then
  if [ -f "$entry_file" ]; then
    printf 'changelog-entry: nothing to move: CHANGELOG.md is as at origin/main from the Unreleased heading down, and %s holds the lines of %s.\n' "$entry_file" "$branch"
  else
    printf 'changelog-entry: nothing to move: %s added no line under the Unreleased heading. Write %s by hand, one line for each type of change.\n' "$branch" "$entry_file"
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
if [ "$here_tail" != "$main_tail" ]; then
  git checkout "$main" -- CHANGELOG.md || die 'git checkout could not set CHANGELOG.md to the file of origin/main'
fi
count=0
[ -z "$moved" ] || count=$(($(printf '%s\n' "$moved" | wc -l)))
printf 'changelog-entry: moved %d lines into %s, and CHANGELOG.md is the file of origin/main (%s). Both are staged: read git diff --cached, then commit.\n' "$count" "$entry_file" "$main"
