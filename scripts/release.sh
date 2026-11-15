#!/bin/sh
# Releases Halvo with cargo-release, one version for every package of the workspace.
#   sh scripts/release.sh --dry-run [patch|minor|major]
#       Prints what the release would do and changes no tracked file (cargo may create the
#       ignored target/): the next version, the tag, and the CHANGELOG.md section that
#       --execute would write from ## [Unreleased], with its entries grouped by type.
#   sh scripts/release.sh --execute [patch|minor|major]
#       On the branch main only: sets every package of the workspace to the next version,
#       moves the entries of ## [Unreleased] in CHANGELOG.md under a heading of that version,
#       grouped by type, commits, and tags v<version>. Pushes nothing.
# The bump level defaults to patch. Settings are in release.toml and src-tauri/release.toml.
set -eu
cd "$(dirname "$0")/.."
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

usage() {
  echo "usage: sh scripts/release.sh --dry-run|--execute [patch|minor|major]" >&2
  exit 64
}

fail() {
  echo "release.sh: $1" >&2
  exit 1
}

cargo_failed() {
  first=$(printf '%s\n' "$2" | grep -i -m 1 'error')
  cause="$1 failed: ${first:-it printed no error line}"
  if printf '%s\n' "$2" | grep -q -i -E 'lookup|resolve|connect|network|timed out|dns'; then
    cause="$cause. Every dry run and every release asks index.crates.io about each package of the workspace and needs the network."
  fi
  fail "$cause"
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
    { sub(/[ \t\r]+$/, ""); line[NR] = $0 }
    END {
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
            entries[t] = entries[t] "- " substr(line[i], length(prefix) + 1) "\n"
            typed = 1
            break
          }
        }
        if (!typed) { print "error: CHANGELOG.md:" i ": this line is not a typed entry, - <Type>: <text>, so the release cannot group it" > "/dev/stderr"; bad = 1 }
      }
      if (bad) exit 1
      for (i = 1; i <= start; i++) print line[i]
      for (t = 1; t <= 6; t++) if (entries[t] != "") printf "\n### %s\n\n%s", types[t], entries[t]
      if (stop <= NR) {
        print ""
        for (i = stop; i <= NR; i++) print line[i]
      }
    }
  ' CHANGELOG.md) || exit 1
  if [ "${DRY_RUN:-}" = false ]; then
    printf '%s\n' "$grouped" >CHANGELOG.md
    return
  fi
  echo "CHANGELOG.md section $NEW_VERSION that --execute writes, grouped by type:"
  printf '%s\n' "$grouped" | awk -v version="$NEW_VERSION" '
    !on && (index($0, "## [" version "] - ") == 1 || $0 == "## [Unreleased]") { on = 1; next }
    on && /^## / { exit }
    on'
}

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

plan=$(cargo release version "$level" 2>&1) || {
  printf '%s\n' "$plan" >&2
  cargo_failed "cargo release version $level" "$plan"
}
next=$(printf '%s\n' "$plan" | sed -n 's/^ *Upgrading halvo from [^ ]* to \([^ ]*\).*/\1/p')
case "$next" in
  ''|*[!0-9.]*) printf '%s\n' "$plan" >&2; fail "cargo release version $level names no single next version of the package halvo" ;;
esac
mkdir -p target
settings=target/release-commit-message.toml
printf 'pre-release-commit-message = "Release %s"\n' "$next" >"$settings"

if [ "$mode" = --execute ]; then
  set -- "$level" --config "$settings" --execute --no-confirm --allow-branch main
else
  set -- "$level" --config "$settings" --no-confirm
fi
status=0
out=$(cargo release "$@" 2>&1) || status=$?
printf '%s\n' "$out"
if [ "$status" -ne 0 ]; then
  cargo_failed cargo-release "$out"
fi
case "$out" in
  *Unrendered*) fail "cargo-release left a template unrendered (the line Unrendered above), so a commit message or a tag would hold it" ;;
esac

tag=$(cargo release config 2>/dev/null | sed -n 's/^tag-name = "\(.*\)"$/\1/p' | head -1 | sed "s/{{version}}/$next/g")
case "$tag" in
  ''|*'{{'*) fail "could not resolve tag-name from cargo release config: '$tag'" ;;
esac
echo
echo "next version: $next"
echo "tag: $tag"
