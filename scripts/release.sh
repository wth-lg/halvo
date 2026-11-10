#!/bin/sh
# Releases Halvo with cargo-release.
#   sh scripts/release.sh --dry-run [patch|minor|major]
#       Prints what the release would do and writes nothing: the next version, the tag,
#       and the CHANGELOG.md section that --execute would write from ## [Unreleased].
#   sh scripts/release.sh --execute [patch|minor|major]
#       Bumps the version in src-tauri/Cargo.toml, promotes ## [Unreleased] in
#       CHANGELOG.md to the new version, commits, and tags v<version>. Pushes nothing.
# The bump level defaults to patch. Settings are in release.toml.
set -eu
cd "$(dirname "$0")/.."
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

mode=${1:-}
level=${2:-patch}
case "$mode" in
  --dry-run|--execute) ;;
  *) echo "usage: sh scripts/release.sh --dry-run|--execute [patch|minor|major]" >&2; exit 64 ;;
esac

if [ "$mode" = --execute ]; then
  exec cargo release "$level" --execute --no-confirm
fi

# cargo-release is a dry run unless --execute is given. It prints the version bump,
# the diff of the CHANGELOG.md replacement and the tag, and writes nothing.
out=$(mktemp "${TMPDIR:-/tmp}/release.XXXXXX")
if ! cargo release "$level" --no-confirm >"$out" 2>&1; then
  cat "$out"
  rm -f "$out"
  exit 1
fi
cat "$out"

next=$(sed -n 's/.*Upgrading halvo from [0-9.]* to \([0-9.]*\).*/\1/p' "$out" | head -1)
heading=$(sed -n 's/^+\(## \[[0-9.]*\] - [0-9-]*\)$/\1/p' "$out" | head -1)
rm -f "$out"
if [ -z "$next" ] || [ -z "$heading" ]; then
  echo "release.sh: the cargo-release output above has no version bump or changelog heading" >&2
  exit 1
fi

# The dry run does not print the tag name; take it from the tag-name setting that
# cargo-release resolved (release.toml) and fill in the version.
tag=$(cargo release config 2>/dev/null | sed -n 's/^tag-name = "\(.*\)"$/\1/p' | head -1 | sed "s/{{version}}/$next/g")
case "$tag" in
  ''|*'{{'*) echo "release.sh: could not resolve tag-name from cargo release config: '$tag'" >&2; exit 1 ;;
esac

echo
echo "next version: $next"
echo "tag: $tag"
echo "CHANGELOG.md section ## $next that --execute would write from ## [Unreleased]:"
echo
echo "$heading"
awk '/^## \[Unreleased\]/ { in_section = 1; next } /^## / { if (in_section) exit } in_section' CHANGELOG.md
