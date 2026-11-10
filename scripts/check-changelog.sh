#!/bin/sh
set -eu

synopsis() {
  cat <<'EOF'
  sh scripts/check-changelog.sh
      Check the structure of CHANGELOG.md at the root of the clone that holds the
      current directory.
  sh scripts/check-changelog.sh --base <ref>
      Check the structure, and that the file has an entry under ## [Unreleased]
      that CHANGELOG.md at <ref> lacks. A file with a release heading that <ref>
      lacks is the commit of a release and needs no such entry. Also refuse an
      entry that <ref> lacks inside the section of a release that <ref> already
      has, whatever the date on its heading: a merge can put a new entry there,
      and a correction of a released entry is a new entry under ## [Unreleased].
  sh scripts/check-changelog.sh --self-test
      Prove each rule on fixtures. Needs no repository.
EOF
}

help_text() {
  cat <<'EOF'
Checks CHANGELOG.md, so that an entry cannot sit in the wrong place.
  - The file has exactly one "## [Unreleased]" heading, above every release heading,
    and no other line holds that text: release.toml replaces it and allows it once.
  - Below it are entries and blank lines only. An entry is one line,
    "- <Type>: <text>", with Type one of Added, Changed, Deprecated, Removed,
    Fixed, Security. No entry text is there twice, and no entry is above the
    heading.
  - A release section, "## [x.y.z] - YYYY-MM-DD", holds such entries, or
    "- <text>" entries under "### <Type>" headings, each heading at most once.
    No version has two release headings.
A new entry goes at the end of the list under "## [Unreleased]". The lint does
not check where in the list an entry is.

EOF
  synopsis
  cat <<'EOF'

Exit codes: 0 the file passes, 1 it breaks a rule, 2 the check could not run,
64 bad usage. The check only reads, so it is safe to run twice.
EOF
}

die() {
  printf 'check-changelog: error: %s\n' "$1" >&2
  exit 2
}

usage() {
  printf 'check-changelog: %s\n' "$1" >&2
  synopsis >&2
  exit 64
}

changelog_verdict() {
  base=$1 base_changelog=$2 changelog=$3
  base_line_count=0
  if [ -n "$base" ]; then
    base_line_count=$(($(printf '%s\n' "$base_changelog" | wc -l)))
  fi
  {
    [ -z "$base" ] || printf '%s\n' "$base_changelog"
    printf '%s\n' "$changelog"
  } | CHANGELOG_BASE=$base LC_ALL=C awk -v base_line_count="$base_line_count" '
    function report(at_line, problem, fix) {
      printf "CHANGELOG.md:%d: %s Fix: %s\n", at_line, problem, fix
      error_count++
    }
    BEGIN {
      base = ENVIRON["CHANGELOG_BASE"]
      type = "(Added|Changed|Deprecated|Removed|Fixed|Security)"
      typed_entry = "^- " type ": [^ \t]"
      type_heading = "^### " type "$"
      release_heading = "^## \\[[0-9]+\\.[0-9]+\\.[0-9]+\\] - [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$"
      entry_form = "\"- <Type>: <text>\" with Type one of Added, Changed, Deprecated, Removed, Fixed, Security"
    }
    { sub(/[ \t\r]+$/, "") }
    function release_version(heading) {
      sub(/\] - .*$/, "]", heading)
      return heading
    }
    NR <= base_line_count {
      base_has_line[$0] = 1
      if ($0 ~ release_heading) base_has_release[release_version($0)] = 1
      next
    }
    { line_number = NR - base_line_count }
    index($0, "## [Unreleased]") && $0 != "## [Unreleased]" {
      report(line_number, "this line holds the text \"## [Unreleased]\" and is not that heading. release.toml replaces the text and allows it once in the file.", "reword the line, for example to \"the Unreleased heading\".")
    }
    $0 == "## [Unreleased]" {
      if (unreleased_line) {
        report(line_number, "this is a second \"## [Unreleased]\" heading; the first is on line " unreleased_line ".", "delete this line and move the entries below it to the end of the list under line " unreleased_line ".")
      } else {
        unreleased_line = line_number
        append_below_line = line_number
        if (first_release_line) report(line_number, "\"## [Unreleased]\" is below the release heading on line " first_release_line ".", "move this heading and its entries above line " first_release_line ".")
      }
      section = "unreleased"
      next
    }
    $0 ~ release_heading {
      if (!first_release_line) first_release_line = line_number
      version = release_version($0)
      if (version in release_line) report(line_number, "the release " substr(version, 4) " already has a heading on line " release_line[version] ".", "delete this line, or give it the version of another release.")
      else release_line[version] = line_number
      if (!($0 in base_has_line)) release_is_new = 1
      base_has_this_release = (version in base_has_release)
      section = "release"
      split("", heading_line)
      under_type_heading = 0
      next
    }
    $0 ~ /^[ \t]*$/ { next }
    section == "" {
      if ($0 ~ typed_entry || $0 ~ type_heading) report(line_number, "this line is above the \"## [Unreleased]\" heading.", "move an entry to the end of the list under that heading, and delete a ### heading.")
      next
    }
    section == "unreleased" {
      if ($0 ~ typed_entry) {
        text = $0
        sub(/^- [A-Za-z]+: /, "", text)
        if (text in entry_line) report(line_number, "the text of this entry is already on line " entry_line[text] ".", "delete one of the two lines.")
        else entry_line[text] = line_number
        unreleased_entries++
        if (!($0 in base_has_line)) new_unreleased_entries++
        append_below_line = line_number
      } else if ($0 ~ /^###/) {
        report(line_number, "\"" $0 "\" is a heading inside ## [Unreleased], and that section holds entries only.", "delete this line and start each entry that was under it with its type, as " entry_form ".")
      } else if ($0 ~ /^- /) {
        report(line_number, "this entry does not have the form " entry_form ".", "rewrite it in that form.")
      } else {
        report(line_number, "this line is not an entry.", "write each entry on one line, as " entry_form ". A release heading has the form \"## [x.y.z] - YYYY-MM-DD\".")
      }
      next
    }
    {
      if ($0 ~ type_heading) {
        if ($0 in heading_line) report(line_number, "\"" $0 "\" is already on line " heading_line[$0] " of this release section.", "move the entries below this line under line " heading_line[$0] " and delete this line.")
        else heading_line[$0] = line_number
        under_type_heading = 1
      } else if ($0 ~ /^#/) {
        report(line_number, "\"" $0 "\" is not a heading of a release section.", "use ### Added, ### Changed, ### Deprecated, ### Removed, ### Fixed or ### Security, or the form \"## [x.y.z] - YYYY-MM-DD\" for a release.")
      } else if ($0 ~ typed_entry || (under_type_heading && $0 ~ /^- [^ \t]/)) {
        if (base_has_this_release && !($0 in base_has_line)) {
          new_released_entry_line[++new_released_entries] = line_number
          new_released_entry_version[new_released_entries] = substr(version, 4)
        }
      } else if ($0 ~ /^- /) {
        report(line_number, "this entry has no type and no \"### <Type>\" heading above it in its release section.", "write it as " entry_form ", or put it under such a heading.")
      } else {
        report(line_number, "this line is not an entry and not a heading.", "write each entry on one line.")
      }
    }
    END {
      if (!unreleased_line) {
        if (first_release_line) report(first_release_line, "no \"## [Unreleased]\" heading is above this release heading.", "add the line \"## [Unreleased]\" and an empty line above this line.")
        else report(line_number ? line_number : 1, "the file has no \"## [Unreleased]\" heading.", "add the line \"## [Unreleased]\" below the introduction, and the entries below that line.")
      } else {
        for (i = 1; i <= new_released_entries; i++) report(new_released_entry_line[i], "this entry is new compared with " base " and sits in the section of release " new_released_entry_version[i] ", which " base " already has.", "move it to the end of the list under ## [Unreleased], below line " append_below_line ". If it corrects a released entry, put that entry back as it was and record the correction as a new entry there.")
        if (base != "" && !new_unreleased_entries && !release_is_new) report(append_below_line, "no entry under ## [Unreleased] is new compared with " base ".", "add a line below this one for each type of change that your branch makes, as " entry_form ".")
      }
      if (error_count) {
        printf "check-changelog: FAIL: errors in CHANGELOG.md: %d.\n", error_count
        exit 1
      }
      printf "check-changelog: PASS: entries under ## [Unreleased]: %d", unreleased_entries
      if (base != "") printf ", new compared with %s: %d", base, new_unreleased_entries
      printf ".\n"
    }
  '
}

check() {
  root=$(git rev-parse --show-toplevel 2>/dev/null) ||
    die 'the current directory is not inside the work tree of a git clone'
  [ -f "$root/CHANGELOG.md" ] || die "no such file: $root/CHANGELOG.md"
  changelog=$(cat "$root/CHANGELOG.md") || die "cannot read $root/CHANGELOG.md"
  base_changelog=
  if [ -n "$base_ref" ]; then
    base_commit=$(git rev-parse --verify --quiet --end-of-options "$base_ref^{commit}") ||
      die "the base $base_ref is not a commit in this clone. Name a ref that this clone has, for example origin/main after git fetch origin (in CI, fetch-depth: 0 on actions/checkout)."
    base_changelog=$(git cat-file blob "$base_commit:CHANGELOG.md") ||
      die "git cat-file could not read CHANGELOG.md at $base_ref"
  fi
  status=0
  verdict=$(changelog_verdict "$base_ref" "$base_changelog" "$changelog") || status=$?
  case $status in
    0) printf '%s\n' "$verdict" ;;
    1) printf '%s\n' "$verdict" >&2; exit 1 ;;
    *) die "awk exited $status while reading the changelog" ;;
  esac
}

self_test() {
  failures=0
  nl='
'
  carriage_return=$(printf '\r')
  introduction="# Changelog${nl}${nl}Add an entry at the end of the list.${nl}"
  unreleased="$introduction${nl}## [Unreleased]${nl}${nl}"
  release="## [0.0.1] - 2026-11-10${nl}${nl}"
  flat_list="${unreleased}- Added: Add the store${nl}- Fixed: Close the race"
  released="$unreleased$release- Added: Add the store${nl}- Fixed: Close the race"

  expect() {
    name=$1 want_error_lines=$2 base=$3 base_changelog=$4 changelog=$5
    want_status=1
    [ -n "$want_error_lines" ] || want_status=0
    status=0
    verdict=$(changelog_verdict "$base" "$base_changelog" "$changelog") || status=$?
    error_lines=$(printf '%s\n' "$verdict" | sed -n 's/^CHANGELOG\.md:\([0-9][0-9]*\): .* Fix: .*$/\1/p' | tr '\n' ' ')
    error_lines=${error_lines% }
    if [ "$status" = "$want_status" ] && [ "$error_lines" = "$want_error_lines" ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected exit %s and errors with a fix on lines [%s]; found exit %s, lines [%s] and this output)\n%s\n' "$name" "$want_status" "$want_error_lines" "$status" "$error_lines" "$verdict"
      failures=$((failures + 1))
    fi
  }

  expect_text() {
    name=$1 want_text=$2 base=$3 base_changelog=$4 changelog=$5
    verdict=$(changelog_verdict "$base" "$base_changelog" "$changelog") || :
    case $verdict in
      *"$want_text"*) printf 'PASS %s\n' "$name" ;;
      *)
        printf 'FAIL %s (expected the text "%s" in this output)\n%s\n' "$name" "$want_text" "$verdict"
        failures=$((failures + 1))
        ;;
    esac
  }

  expect 'a flat list of typed entries is accepted' '' '' '' "$flat_list"
  expect 'an entry of each of the six types is accepted' '' '' '' "${unreleased}- Added: A${nl}- Changed: B${nl}- Deprecated: C${nl}- Removed: D${nl}- Fixed: E${nl}- Security: F"
  expect 'a line of spaces between two entries is accepted' '' '' '' "${unreleased}- Added: Add the store${nl}   ${nl}- Fixed: Close the race"
  expect 'a release section of typed entries, as release.toml writes it, is accepted' '' '' '' "${unreleased}- Changed: Name the constants${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race"
  expect 'an empty Unreleased section above a release section with ### headings is accepted' '' '' '' "$unreleased$release### Added${nl}${nl}- Add the store${nl}${nl}### Fixed${nl}${nl}- Close the race"
  expect 'a list with CRLF line endings, as core.autocrlf=true checks it out, is accepted' '' '' '' "$(printf '%s\n' "$flat_list" | sed "s/\$/$carriage_return/")"
  expect 'spaces at the end of a heading and of an entry are accepted' '' '' '' "$introduction${nl}## [Unreleased]  ${nl}${nl}- Added: Add the store  "
  expect 'a sentence that quotes the Unreleased heading is refused, because release.toml allows the text once' 3 '' '' "# Changelog${nl}${nl}Add an entry under \`## [Unreleased]\`.${nl}${nl}## [Unreleased]${nl}${nl}- Added: Add the store"
  expect 'a release section with each of the six ### headings is accepted' '' '' '' "$unreleased$release### Added${nl}- A${nl}### Changed${nl}- B${nl}### Deprecated${nl}- C${nl}### Removed${nl}- D${nl}### Fixed${nl}- E${nl}### Security${nl}- F"
  expect 'two release sections that each hold the same ### heading are accepted' '' '' '' "$unreleased$release### Fixed${nl}${nl}- Close the race${nl}${nl}## [0.0.0] - 2026-11-01${nl}${nl}### Fixed${nl}${nl}- Quote the arguments"
  expect 'a ### heading inside Unreleased is refused' 7 '' '' "${unreleased}### Added${nl}${nl}- Added: Add the store"
  expect 'entries cut off below a ### heading by a blank line, as on main at d7b245a, are refused' '7 9 11 12' '' '' "${unreleased}### Fixed${nl}${nl}- Say in the CI echo that exempt commits are not checked${nl}${nl}- Clamp a storage cap read from consent.json to the minimum so a zero cap cannot shred the store${nl}- Add SQLCipher and AEAD dependencies for the encrypted segment store"
  expect 'a ### heading twice in one release section is refused' 13 '' '' "$unreleased$release### Fixed${nl}${nl}- Close the race${nl}${nl}### Fixed${nl}${nl}- Quote the arguments"
  expect 'an entry twice is refused' 9 '' '' "$flat_list${nl}- Added: Add the store"
  expect 'one entry text under two types is refused' 8 '' '' "${unreleased}- Added: Add the store${nl}- Changed: Add the store"
  expect 'an entry with no type is refused' 7 '' '' "${unreleased}- Add the store"
  expect 'an entry with an unknown type is refused' 7 '' '' "${unreleased}- Feature: Add the store"
  expect 'an entry that has a type and no text is refused' 7 '' '' "${unreleased}- Added:"
  expect 'a release heading with no date is refused' 9 '' '' "${unreleased}- Added: Add the store${nl}${nl}## [0.0.1]${nl}${nl}- Fixed: Close the race"
  expect 'an Unreleased heading with more text on its line is refused' '5 7 7' '' '' "$introduction${nl}## [Unreleased] - soon${nl}${nl}- Added: Add the store"
  expect 'an entry and a ### heading above the Unreleased heading are refused' '3 5' '' '' "# Changelog${nl}${nl}### Added${nl}${nl}- Added: Add the store${nl}${nl}## [Unreleased]${nl}${nl}- Fixed: Close the race"
  expect 'one release version on two headings is refused' 11 '' '' "$unreleased$release- Added: Add the store${nl}${nl}$release- Fixed: Close the race"
  expect 'a ### heading with more text and the entry below it are refused' '9 11' '' '' "$unreleased$release### Added later${nl}${nl}- Add the store"
  expect 'an untyped entry in a second release section, below no heading of that section, is refused' 15 '' '' "$unreleased$release### Fixed${nl}${nl}- Close the race${nl}${nl}## [0.0.0] - 2026-11-01${nl}${nl}- Quote the arguments"
  expect 'a line that continues an entry is refused' 8 '' '' "${unreleased}- Added: Add the store${nl}  and its tests"
  expect 'two ## [Unreleased] headings are refused' 10 '' '' "$flat_list${nl}${nl}## [Unreleased]${nl}${nl}- Changed: Name the constants"
  expect '## [Unreleased] below a release heading is refused' 9 '' '' "$introduction${nl}$release- Added: Add the store${nl}${nl}## [Unreleased]${nl}${nl}- Fixed: Close the race"
  expect 'a file with a release and no ## [Unreleased] heading is refused' 5 '' '' "$introduction${nl}$release- Added: Add the store"
  expect 'a file with no ## heading is refused' 1 '' '' '# Changelog'
  expect 'an untyped entry above the headings and an unknown ### heading in a release section are refused' '9 11 13' '' '' "$unreleased$release- Add the store${nl}${nl}### Internal${nl}${nl}- Rename a module"
  expect 'with a base, an entry that the base lacks is accepted' '' origin/main "$flat_list" "$flat_list${nl}- Changed: Name the constants"
  expect 'with a base, a file with no new entry is refused at the line to add one below' 8 origin/main "$flat_list" "$flat_list"
  expect 'with a base, a new entry that a release moved out of Unreleased is refused' '11 5' origin/main "$released" "$released${nl}- Changed: Name the constants"
  expect 'with a base, the commit of a release needs no new entry' '' origin/main "$flat_list" "$released"
  expect 'with a base, a new entry in a release section that the base has is refused, also when Unreleased has a new entry' 13 origin/main "$released" "${unreleased}- Security: Rotate the key${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race${nl}- Changed: Name the constants"
  expect 'with a base, a new entry in a release section that the base has under another date is refused' 13 origin/main "$released" "${unreleased}- Security: Rotate the key${nl}${nl}## [0.0.1] - 2026-11-11${nl}${nl}- Added: Add the store${nl}- Fixed: Close the race${nl}- Changed: Name the constants"
  expect 'with a base, the commit of a release does not excuse a new entry in an older release section' 14 origin/main "${unreleased}- Changed: Name the constants${nl}${nl}## [0.0.0] - 2026-11-01${nl}${nl}- Added: Add the store" "$unreleased$release- Changed: Name the constants${nl}${nl}## [0.0.0] - 2026-11-01${nl}${nl}- Added: Add the store${nl}- Fixed: Close the race"
  expect 'with a base, the commit of a release may add an entry to the section that it creates' '' origin/main "$flat_list" "$released${nl}- Changed: Name the constants"
  expect_text 'the error for an entry in a release section of the base names the last line of the Unreleased list' 'below line 7.' origin/main "$released" "${unreleased}- Security: Rotate the key${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race${nl}- Changed: Name the constants"
  expect 'with a base, a new untyped entry under a ### heading of a release that the base has is refused' 14 origin/main "$unreleased$release### Fixed${nl}${nl}- Close the race" "${unreleased}- Added: Add the store${nl}${nl}$release### Fixed${nl}${nl}- Close the race${nl}- Quote the arguments"
  expect 'with a base, a copy in a release section of an entry that the base has under Unreleased is refused' 14 origin/main "${unreleased}- Fixed: Release the lock${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race" "${unreleased}- Fixed: Release the lock${nl}- Changed: Name the constants${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race${nl}- Fixed: Release the lock"
  expect_text 'the error for such a copy says to delete it and names the line of the entry under Unreleased' 'delete this line: the same entry is on line 7 ' origin/main "${unreleased}- Fixed: Release the lock${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race" "${unreleased}- Fixed: Release the lock${nl}- Changed: Name the constants${nl}${nl}$release- Added: Add the store${nl}- Fixed: Close the race${nl}- Fixed: Release the lock"
  expect 'with a base, another date on the heading of a release that the base has is not the commit of a release' 7 origin/main "${unreleased}- Changed: Name the constants${nl}${nl}$release- Added: Add the store" "${unreleased}- Changed: Name the constants${nl}${nl}## [0.0.1] - 2026-11-11${nl}${nl}- Added: Add the store"

  if [ "$failures" -gt 0 ]; then
    printf 'check-changelog: self-test FAIL: fixtures misjudged: %d\n' "$failures"
    return 1
  fi
  printf 'check-changelog: self-test PASS\n'
}

mode=check
base_ref=
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) mode=self-test ;;
    --base)
      [ $# -ge 2 ] || usage '--base needs a ref'
      [ -n "$2" ] || usage 'the ref of --base is empty'
      base_ref=$2
      shift
      ;;
    -h|--help) help_text; exit 0 ;;
    *) usage "unknown argument: $1" ;;
  esac
  shift
done

case $mode in
  self-test)
    [ -z "$base_ref" ] || usage '--self-test and --base exclude each other'
    self_test
    ;;
  check) check ;;
esac
