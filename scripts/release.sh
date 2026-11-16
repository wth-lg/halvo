#!/bin/sh
#   sh scripts/release.sh --dry-run [patch|minor|major]
#   sh scripts/release.sh --execute [patch|minor|major]
set -eu
cd "$(dirname "$0")/.."
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

usage() {
  echo "usage: sh scripts/release.sh --dry-run|--execute [patch|minor|major], or --help" >&2
  exit 64
}

help_text() {
  cat <<'EOF'
Releases Halvo with cargo-release, one version for every package of the workspace.

  sh scripts/release.sh --dry-run [patch|minor|major]
      Prints the next version, the commit message, the tag and the CHANGELOG.md
      section that --execute would write, its entries grouped by type. It
      changes no tracked file: the script writes target/release-commit-message.toml,
      and cargo writes its own files under target/.
  sh scripts/release.sh --execute [patch|minor|major]
      Works on the branch main only. It sets every package of the workspace to
      the next version, moves the entries of ## [Unreleased] under a heading of
      that version, grouped by type, and makes one commit and one tag v<version>.
      It pushes nothing. When the clone has a remote named origin, it first
      reads main's newest commit there with git ls-remote and refuses unless
      HEAD is that commit; a clone with no remote named origin is the one case
      without that read.
      Before that read, and before any fetch, it refuses a tracked file that
      differs from HEAD, a tracked file that carries an index flag in
      git ls-files -v (assume-unchanged or skip-worktree), and a clone where git
      can name no committer.
      In a shallow clone it fetches the whole history and the tags from origin
      first (git fetch --unshallow --tags origin), after the checks that read no
      history; a dry run refuses a shallow clone and names that command.
      Just before cargo-release it runs scripts/r1/ci.d/54-tag-needs-store4.sh
      with the coming tag, and while crates/halvo-capture/src/store/db.rs holds
      the statement that creates the search index events_fts, which store-4
      takes out, it stops there with that check's line and changes nothing; a
      dry run prints the line where --execute would stop and exits 0.
      When cargo-release fails, the script puts the clone back to the commit it
      started from and deletes the tags that the run made, and says so.
  sh scripts/release.sh --help
      Prints this text.

The level defaults to patch. Pushing the commit and the tag, signing and
publishing are the owner's own steps. The settings are in release.toml and
src-tauri/release.toml.
EOF
}

fail() {
  echo "release.sh: $1" >&2
  exit 1
}

error_chain() {
  awk '/^error:/ { print; under = 0; next } /^Caused by:/ { print; under = 1; next } under && /^[ \t]/ { print; next } { under = 0 }'
}

failure_cause() {
  first=$(printf '%s\n' "$2" | sed -n '/^error:/{p;q;}')
  [ -n "$first" ] || first=$(printf '%s\n' "$2" | awk 'NF { last = $0 } END { print last }')
  cause="$1 failed: ${first:-it printed nothing}"
  chain=$(printf '%s\n' "$2" | error_chain)
  if printf '%s\n' "$chain" | grep -q -F -e 'Connection refused' -e 'connection reset' -e 'Could not resolve host' -e "Couldn't resolve host" \
    -e 'failed to resolve address' -e 'failed to lookup address' -e 'dns error' -e 'timed out' -e 'Network is unreachable' -e 'failed to connect'; then
    cause="$cause. Every dry run and every release asks index.crates.io about each package of the workspace and needs the network: check that this computer is online and run the same command again."
  fi
  printf '%s\n' "$cause"
}

cargo_failed() {
  cause=$(failure_cause "$1" "$2")
  fail "$cause"
}

new_tags() {
  git tag --list | while IFS= read -r name; do
    printf '%s\n' "$tags_before" | grep -q -x -F -e "$name" || printf '%s\n' "$name"
  done
}

undo_run() {
  head_now=$(git rev-parse HEAD)
  tracked_now=$(git status --porcelain --untracked-files=no)
  made=$(new_tags)
  if [ "$head_now" = "$recorded" ] && [ -z "$tracked_now" ] && [ -z "$made" ]; then
    printf 'nothing was changed\n'
    return
  fi
  undone=yes
  git reset -q --hard "$recorded" || undone=no
  for name in $made; do
    git tag -d "$name" >/dev/null || undone=no
  done
  if [ "$undone" = yes ]; then
    printf 'the files, the commit and the tag that this run made are undone; nothing was released\n'
    return
  fi
  command="git -C '$PWD' reset --hard $recorded"
  for name in $made; do
    command="$command && git -C '$PWD' tag -d $name"
  done
  printf 'this run could not undo what it made; run: %s\n' "$command"
}

has_a_remote_named_origin() {
  git remote get-url origin >/dev/null 2>&1
}

unreleased_entries() {
  awk '{ sub(/[ \t\r]+$/, "") } $0 == "## [Unreleased]" { on = 1; next } /^## / { on = 0 } on && /^- / { n++ } END { print n + 0 }' CHANGELOG.md
}

released_sections() {
  awk '{ sub(/[ \t\r]+$/, "") } /^## \[[0-9]/ { on = 1 } on'
}

group_changelog() {
  [ -n "${NEW_VERSION:-}" ] || {
    echo "release.sh: --group-changelog is the pre-release hook of cargo-release, which sets NEW_VERSION" >&2
    exit 64
  }
  grouped=$(LC_ALL=C awk -v version="$NEW_VERSION" '
    { raw[NR] = $0; sub(/[ \t\r]+$/, ""); line[NR] = $0 }
    END {
      eol = (raw[1] ~ /\r$/) ? "\r" : ""
      for (i = 1; i <= NR; i++) if (index(line[i], "## [" version "] - ") == 1) { start = i; break }
      if (!start) for (i = 1; i <= NR; i++) if (line[i] == "## [Unreleased]") { start = i; break }
      if (!start) { print "error: CHANGELOG.md has no heading ## [" version "] and no ## [Unreleased]" > "/dev/stderr"; exit 1 }
      stop = NR + 1
      for (i = start + 1; i <= NR; i++) if (line[i] ~ /^## /) { stop = i; break }
      split("Added Changed Deprecated Removed Fixed Security", types, " ")
      for (i = start + 1; i < stop; i++) {
        if (line[i] ~ /^[ \t\r]*$/) continue
        typed = 0
        for (t = 1; t <= 6; t++) {
          prefix = "- " types[t] ": "
          if (index(line[i], prefix) == 1 && length(line[i]) > length(prefix)) {
            entries[t] = entries[t] "- " substr(line[i], length(prefix) + 1) eol "\n"
            typed = 1
            break
          }
        }
        if (!typed) { print "error: CHANGELOG.md:" i ": this line is not a typed entry, - <Type>: <text>, so the release cannot group it" > "/dev/stderr"; bad = 1 }
      }
      if (bad) exit 1
      for (i = 1; i <= start; i++) print raw[i]
      for (t = 1; t <= 6; t++) if (entries[t] != "") printf "%s\n### %s%s\n%s\n%s", eol, types[t], eol, eol, entries[t]
      if (stop <= NR) {
        print eol
        for (i = stop; i <= NR; i++) print raw[i]
      }
    }
  ' CHANGELOG.md) || exit 1
  if [ "${DRY_RUN:-}" = false ]; then
    printf '%s\n' "$grouped" >CHANGELOG.md
    return
  fi
  echo "CHANGELOG.md section $NEW_VERSION that --execute writes, grouped by type:"
  printf '%s\n' "$grouped" | awk -v version="$NEW_VERSION" '
    { sub(/\r$/, "") }
    !on && (index($0, "## [" version "] - ") == 1 || $0 == "## [Unreleased]") { on = 1; next }
    on && /^## / { exit }
    on'
}

if [ "${1:-}" = --help ]; then
  [ $# -eq 1 ] || usage
  help_text
  exit 0
fi

if [ "${1:-}" = --group-changelog ]; then
  [ $# -eq 1 ] || usage
  group_changelog
  exit 0
fi

[ $# -ge 1 ] && [ $# -le 2 ] || usage
mode=$1
level=${2:-patch}
case "$mode" in
  --dry-run|--execute) ;;
  *) usage ;;
esac
case "$level" in
  patch|minor|major) ;;
  *) usage ;;
esac

if ! lint=$(sh scripts/check-changelog.sh 2>&1); then
  printf '%s\n' "$lint" >&2
  fail "CHANGELOG.md breaks a rule of scripts/check-changelog.sh, so nothing was released"
fi
entries=$(unreleased_entries)
[ "$entries" -gt 0 ] || fail "## [Unreleased] in CHANGELOG.md holds no entry, so there is nothing to release"

plan=$(cargo release version "$level" 2>&1) || {
  printf '%s\n' "$plan" >&2
  cargo_failed "cargo release version $level" "$plan"
}
next=$(printf '%s\n' "$plan" | sed -n 's/^ *Upgrading halvo from [^ ]* to \([^ ]*\).*/\1/p')
case "$next" in
  ''|*[!0-9.]*) printf '%s\n' "$plan" >&2; fail "cargo release version $level names no single next version of the package halvo" ;;
esac
headings=$(awk -v version="$next" '{ sub(/[ \t\r]+$/, "") } index($0, "## [" version "] ") == 1 || $0 == "## [" version "]" { n++ } END { print n + 0 }' CHANGELOG.md)
[ "$headings" = 0 ] ||
  fail "CHANGELOG.md already holds a heading of $next, so the release would add a second one: remove the heading ## [$next] and move its entries to the end of the list under ## [Unreleased], or release a higher level; nothing was released"
mkdir -p target
settings=target/release-commit-message.toml
printf 'pre-release-commit-message = "Release %s"\n' "$next" >"$settings"
config=$(cargo release config --config "$settings" 2>/dev/null) || fail "cargo release config failed, so the commit message and the tag cannot be read"
rendered() {
  printf '%s\n' "$config" | sed -n "s/^$1 = \"\(.*\)\"\$/\1/p" | head -n 1 | sed "s/{{version}}/$next/g"
}
message=$(rendered pre-release-commit-message)
tag=$(rendered tag-name)
tag_message=$(rendered tag-message)
for pair in "commit message:$message" "tag name:$tag" "tag message:$tag_message"; do
  case "${pair#*:}" in
    ''|*'{{'*) fail "the ${pair%%:*} would hold a template or nothing ('${pair#*:}'), so nothing was released" ;;
  esac
done

if [ "$mode" = --execute ]; then
  tracked=$(git status --porcelain --untracked-files=no)
  if [ -n "$tracked" ]; then
    first_path=$(printf '%s\n' "$tracked" | head -n 1 | cut -c4-)
    fail "tracked files differ from HEAD (the first is $first_path); commit or stash them first; nothing was changed"
  fi
  flagged=$(git ls-files -v | awk '!/^H / { print; exit }')
  if [ -n "$flagged" ]; then
    flagged_path=${flagged#? }
    fail "the tracked file $flagged_path carries the index flag ${flagged%% *} in git ls-files -v, which can hide a local edit from git status and from the undo of a failed run; clear it with git update-index --no-assume-unchanged --no-skip-worktree -- '$flagged_path' and run this again; nothing was changed"
  fi
  git var GIT_COMMITTER_IDENT >/dev/null 2>&1 ||
    fail "git can name no committer in this clone: set your name and email in git's global configuration and run this again; nothing was changed"
fi

shallow=$(git rev-parse --is-shallow-repository)
if [ "$shallow" = true ]; then
  [ "$mode" = --execute ] ||
    fail "this clone is shallow, so the released sections of CHANGELOG.md cannot be compared with the last tag; a dry run changes nothing, so run git fetch --unshallow --tags origin and then the dry run again"
  git fetch --quiet --unshallow --tags origin ||
    fail "this clone is shallow and git fetch --unshallow --tags origin failed, so the released sections of CHANGELOG.md cannot be compared with the last tag"
fi
last_tag=$(git describe --tags --abbrev=0 --match 'v[0-9]*' HEAD 2>/dev/null) || last_tag=
if [ -n "$last_tag" ]; then
  at_tag=$(git show "$last_tag:CHANGELOG.md" | released_sections)
  now=$(released_sections <CHANGELOG.md)
  [ "$now" = "$at_tag" ] ||
    fail "the released sections of CHANGELOG.md differ from those at $last_tag; a change of a released entry is a new entry under ## [Unreleased]"
fi

if [ "$mode" = --execute ] && has_a_remote_named_origin; then
  newest=$(git ls-remote origin refs/heads/main) || fail "git ls-remote origin refs/heads/main failed, so main's newest commit cannot be read; nothing was released"
  newest=$(printf '%s\n' "$newest" | cut -f 1)
  [ -n "$newest" ] || fail "origin has no branch main, so main's newest commit cannot be read; nothing was released"
  here=$(git rev-parse HEAD)
  if [ "$here" != "$newest" ]; then
    if git merge-base --is-ancestor "$here" "$newest" 2>/dev/null; then
      fail "main here is behind origin's main ($newest): run git pull --ff-only origin main, then run this again; nothing was released"
    elif git merge-base --is-ancestor "$newest" "$here" 2>/dev/null; then
      ahead=$(git rev-list --count "$newest..$here")
      fail "main here holds $ahead commits that origin's main does not have (git log origin/main..main lists them); a release starts from origin's newest main: get those commits merged by a pull request first, or release from a fresh clone; nothing was released"
    else
      fail "HEAD and origin's main have diverged, or this clone lacks origin's newest commit ($newest): run git fetch origin main and compare; nothing was released"
    fi
  fi
fi

store4_check=scripts/r1/ci.d/54-tag-needs-store4.sh
store4_status=0
store4=$(CI_D_REPO=$PWD GITHUB_REF_TYPE=tag GITHUB_REF_NAME=$tag sh "$store4_check" 2>&1) || store4_status=$?
if [ "$store4_status" -ne 0 ]; then
  store4_cause=$(printf '%s\n' "$store4" | sed -n 's/^FAIL 54-tag-needs-store4 (\(.*\))$/\1/p')
  [ -n "$store4_cause" ] ||
    store4_cause="$store4_check exited $store4_status and printed no FAIL line, so it cannot say that the tag $tag may be made"
  if [ "$mode" = --execute ]; then
    printf '%s\n' "$store4" >&2
    fail "$store4_cause; nothing was released"
  fi
  printf 'release.sh: --execute would stop here: %s\n' "$store4_cause" >&2
fi

if [ "$mode" = --execute ]; then
  recorded=$(git rev-parse HEAD)
  tags_before=$(git tag --list)
  set -- "$level" --config "$settings" --execute --no-confirm --allow-branch main
else
  set -- "$level" --config "$settings" --no-confirm
fi
status=0
out=$(cargo release "$@" 2>&1) || status=$?
printf '%s\n' "$out"
if [ "$status" -ne 0 ]; then
  cause=$(failure_cause cargo-release "$out")
  [ "$mode" = --execute ] || fail "$cause"
  undone=$(undo_run)
  fail "$cause; $undone"
fi
if [ "$mode" = --execute ]; then
  at_head=$(git tag --points-at HEAD)
  echo "tag at HEAD: $at_head"
  if [ "$at_head" != "$tag" ]; then
    undone=$(undo_run)
    fail "the tag at the release commit is $at_head, not $tag; $undone"
  fi
fi
echo
echo "next version: $next"
echo "commit message: $message"
echo "tag: $tag"
echo "tag message: $tag_message"
