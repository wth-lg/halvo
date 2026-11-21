#!/bin/sh
set -eu

synopsis() {
  cat <<'EOF'
  sh scripts/check-changelog.sh
      Check the structure of CHANGELOG.md and the lines of the entry files under
      changelog.d/, in the work tree of the clone that holds the current directory.
  sh scripts/check-changelog.sh --base <ref>
      Also check the work tree against the commit <ref>: it adds an entry file
      that <ref> lacks, and CHANGELOG.md from its ## [Unreleased] heading down is
      as at <ref>. A CHANGELOG.md with a release that <ref> lacks is the commit of
      a release, which needs neither.
  sh scripts/check-changelog.sh --self-test
      Run five cases of the rules (an entry file passes, none fails, a changed
      Unreleased list fails, an entry line of another form fails, the commit of a
      release passes) and one case of each of scripts/changelog-entry.sh and
      scripts/changelog-join.sh, in throwaway repositories under TMPDIR, which are
      left in place. The structure of CHANGELOG.md has no case.
EOF
}

help_text() {
  cat <<'EOF'
Checks the changelog, so that no two pull requests change the same lines.

A pull request records its changes in one new entry file, changelog.d/<name>.md,
named after its branch, with one line for each type of change that it makes. It
does not change CHANGELOG.md from the ## [Unreleased] heading down. At a release,
sh scripts/changelog-join.sh puts the lines of every entry file at the end of the
list under that heading and removes the files. sh scripts/changelog-entry.sh moves
the lines that a branch added under that heading into the entry file of the branch.

  - Every file under changelog.d/ other than changelog.d/README.md is an entry
    file, named changelog.d/<name>.md with a name of letters, digits, ".", "_"
    and "-" that does not start with ".". It holds at least one line, and each line is an entry,
    "- <Type>: <text>", with Type one of Added, Changed, Deprecated, Removed,
    Fixed, Security, and a text that does not start with a type word and a colon.
  - CHANGELOG.md has exactly one "## [Unreleased]" heading, above every release
    heading, and no other line holds that text: release.toml replaces it and
    allows it once. Below it are entries and blank lines only, no entry text is
    there twice, and no entry is above the heading.
  - A release section, "## [x.y.z] - YYYY-MM-DD", holds such entries, or
    "- <text>" entries under "### <Type>" headings, each heading at most once.
    No version has two release headings.
The check reads the work tree, with the files that git does not track yet and
that .gitignore does not name.

EOF
  synopsis
  cat <<'EOF'

Exit codes: 0 the changelog passes, 1 it breaks a rule, 2 the check could not
run, 64 bad usage. The check and its self-test change nothing in the clone, so
they are safe to run twice.
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

entry_form='"- <Type>: <text>" with Type one of Added, Changed, Deprecated, Removed, Fixed, Security'

structure_errors() {
  LC_ALL=C awk '
    function report(at_line, problem, fix) {
      printf "CHANGELOG.md:%d: %s Fix: %s\n", at_line, problem, fix
    }
    BEGIN {
      type = "(Added|Changed|Deprecated|Removed|Fixed|Security)"
      typed_entry = "^- " type ": [^ \t]"
      type_heading = "^### " type "$"
      release_heading = "^## \\[[0-9]+\\.[0-9]+\\.[0-9]+\\] - [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$"
      entry_form = ENVIRON["ENTRY_FORM"]
    }
    { sub(/[ \t\r]+$/, "") }
    index($0, "## [Unreleased]") && $0 != "## [Unreleased]" {
      report(NR, "this line holds the text \"## [Unreleased]\" and is not that heading. release.toml replaces the text and allows it once in the file.", "reword the line, for example to \"the Unreleased heading\".")
    }
    $0 == "## [Unreleased]" {
      if (unreleased_line) {
        report(NR, "this is a second \"## [Unreleased]\" heading; the first is on line " unreleased_line ".", "delete this line and the entries below it, and write those entries into an entry file under changelog.d/.")
      } else {
        unreleased_line = NR
        if (first_release_line) report(NR, "\"## [Unreleased]\" is below the release heading on line " first_release_line ".", "move this heading and its entries above line " first_release_line ".")
      }
      section = "unreleased"
      next
    }
    $0 ~ release_heading {
      if (!first_release_line) first_release_line = NR
      version = $0
      sub(/\] - .*$/, "]", version)
      if (version in release_line) report(NR, "the release " substr(version, 4) " already has a heading on line " release_line[version] ".", "delete this line, or give it the version of another release.")
      else release_line[version] = NR
      section = "release"
      split("", heading_line)
      under_type_heading = 0
      next
    }
    $0 ~ /^[ \t]*$/ { next }
    section == "" {
      if ($0 ~ typed_entry || $0 ~ type_heading) report(NR, "this line is above the \"## [Unreleased]\" heading.", "move an entry into an entry file under changelog.d/, and delete a ### heading.")
      next
    }
    section == "unreleased" {
      if ($0 ~ typed_entry) {
        text = $0
        sub(/^- [A-Za-z]+: /, "", text)
        if (text ~ ("^" type ":")) report(NR, "the text of this entry starts with a type word and a colon, which the release would read as a second type.", "reword the entry so that its text does not start with a type word and a colon.")
        if (text in entry_line) report(NR, "the text of this entry is already on line " entry_line[text] ".", "delete one of the two lines.")
        else entry_line[text] = NR
        unreleased_entries++
      } else if ($0 ~ /^###/) {
        report(NR, "\"" $0 "\" is a heading inside ## [Unreleased], and that section holds entries only.", "delete this line and start each entry that was under it with its type, as " entry_form ".")
      } else if ($0 ~ /^- /) {
        report(NR, "this entry does not have the form " entry_form ".", "rewrite it in that form.")
      } else {
        report(NR, "this line is not an entry.", "write each entry on one line, as " entry_form ". A release heading has the form \"## [x.y.z] - YYYY-MM-DD\".")
      }
      next
    }
    {
      if ($0 ~ type_heading) {
        if ($0 in heading_line) report(NR, "\"" $0 "\" is already on line " heading_line[$0] " of this release section.", "move the entries below this line under line " heading_line[$0] " and delete this line.")
        else heading_line[$0] = NR
        under_type_heading = 1
      } else if ($0 ~ /^#/) {
        report(NR, "\"" $0 "\" is not a heading of a release section.", "use ### Added, ### Changed, ### Deprecated, ### Removed, ### Fixed or ### Security, or the form \"## [x.y.z] - YYYY-MM-DD\" for a release.")
      } else if (!($0 ~ typed_entry || (under_type_heading && $0 ~ /^- [^ \t]/))) {
        if ($0 ~ /^- /) report(NR, "this entry has no type and no \"### <Type>\" heading above it in its release section.", "write it as " entry_form ", or put it under such a heading.")
        else report(NR, "this line is not an entry and not a heading.", "write each entry on one line.")
      }
    }
    END {
      if (!unreleased_line) {
        if (first_release_line) report(first_release_line, "no \"## [Unreleased]\" heading is above this release heading.", "add the line \"## [Unreleased]\" and an empty line above this line.")
        else report(NR ? NR : 1, "the file has no \"## [Unreleased]\" heading.", "add the line \"## [Unreleased]\" below the introduction.")
      }
      printf "entries %d\n", unreleased_entries
    }
  '
}

entry_file_errors() {
  path=$1
  [ -s "$path" ] || {
    printf '%s: the file holds no entry. Fix: write one line for each type of change that your branch makes, as %s.\n' "$path" "$entry_form"
    return 0
  }
  LC_ALL=C awk -v path="$path" '
    BEGIN { type = "(Added|Changed|Deprecated|Removed|Fixed|Security)" }
    { sub(/[ \t\r]+$/, "") }
    $0 !~ ("^- " type ": [^ \t]") {
      printf "%s:%d: this line is not an entry of the form %s. Fix: rewrite it in that form, one line for each type of change, and delete an empty line.\n", path, FNR, ENVIRON["ENTRY_FORM"]
      next
    }
    {
      text = $0
      sub(/^- [A-Za-z]+: /, "", text)
      if (text ~ ("^" type ":")) printf "%s:%d: the text of this entry starts with a type word and a colon, which the release would read as a second type. Fix: reword the entry so that its text does not start with a type word and a colon.\n", path, FNR
    }
  ' "$path"
}

range_verdict() {
  base_changelog=$1 changelog=$2
  base_line_count=$(($(printf '%s\n' "$base_changelog" | wc -l)))
  {
    printf '%s\n' "$base_changelog"
    printf '%s\n' "$changelog"
  } | LC_ALL=C awk -v base_line_count="$base_line_count" '
    BEGIN { release_heading = "^## \\[[0-9]+\\.[0-9]+\\.[0-9]+\\] - " }
    { sub(/[ \t\r]+$/, "") }
    function version_of(heading) {
      sub(/\] - .*$/, "]", heading)
      return heading
    }
    NR <= base_line_count {
      if ($0 ~ release_heading) base_version[version_of($0)] = 1
      if ($0 == "## [Unreleased]") base_on = 1
      if (base_on) base_tail[++base_n] = $0
      next
    }
    {
      line_number = NR - base_line_count
      if ($0 ~ release_heading && !(version_of($0) in base_version)) release = 1
      if ($0 == "## [Unreleased]") head_on = 1
      if (head_on) {
        head_tail[++head_n] = $0
        head_line[head_n] = line_number
      }
    }
    END {
      if (release) { print "release"; exit }
      while (base_n && base_tail[base_n] == "") base_n--
      while (head_n && head_tail[head_n] == "") head_n--
      for (i = 1; i <= base_n || i <= head_n; i++) {
        if (i > base_n || i > head_n || base_tail[i] != head_tail[i]) {
          print "changed " (i <= head_n ? head_line[i] : (head_n ? head_line[head_n] : line_number))
          exit
        }
      }
      print "same"
    }
  '
}

check() {
  root=$(git rev-parse --show-toplevel 2>/dev/null) ||
    die 'the current directory is not inside the work tree of a git clone'
  cd "$root" || die "cannot enter $root"
  [ -f CHANGELOG.md ] || die "no such file: $root/CHANGELOG.md"
  changelog=$(cat CHANGELOG.md) || die "cannot read $root/CHANGELOG.md"
  export ENTRY_FORM="$entry_form"
  structure=$(printf '%s\n' "$changelog" | structure_errors) || die 'awk could not read CHANGELOG.md'
  entries=$(printf '%s\n' "$structure" | sed -n '$s/^entries //p')
  errors=$(printf '%s\n' "$structure" | sed '$d')

  listed=$(git -c core.quotePath=false ls-files --cached --others --exclude-standard -- changelog.d) ||
    die 'git ls-files could not list changelog.d'
  base_files=
  if [ -n "$base_ref" ]; then
    base_commit=$(git rev-parse --verify --quiet --end-of-options "$base_ref^{commit}") ||
      die "the base $base_ref is not a commit in this clone. Name a ref that this clone has, for example origin/main after git fetch origin (in CI, fetch-depth: 0 on actions/checkout)."
    base_changelog=$(git cat-file blob "$base_commit:CHANGELOG.md") ||
      die "git cat-file could not read CHANGELOG.md at $base_ref"
    base_files=$(git -c core.quotePath=false ls-tree -r --name-only "$base_commit" -- changelog.d) ||
      die "git ls-tree could not list changelog.d at $base_ref"
  fi
  entry_files=0 new_entry_files=0
  nl='
'
  while IFS= read -r path; do
    [ -n "$path" ] && [ -f "$path" ] || continue
    [ "$path" != changelog.d/README.md ] || continue
    name=${path#changelog.d/}
    stem=${name%.md}
    case $stem in
      "$name"|''|.*|*[!A-Za-z0-9._-]*)
        errors="$errors$nl$path: this file is not an entry file: an entry file is named changelog.d/<name>.md, with a name of letters, digits, \".\", \"_\" and \"-\". Fix: rename the file so, or move it out of changelog.d/."
        continue
        ;;
    esac
    entry_files=$((entry_files + 1))
    found=$(entry_file_errors "$path") || die "awk could not read $path"
    [ -z "$found" ] || errors="$errors$nl$found"
    case "$nl$base_files$nl" in
      *"$nl$path$nl"*) ;;
      *) new_entry_files=$((new_entry_files + 1)) ;;
    esac
  done <<EOF
$listed
EOF

  summary="entries under ## [Unreleased]: $entries, entry files: $entry_files"
  if [ -n "$base_ref" ]; then
    verdict=$(range_verdict "$base_changelog" "$changelog") || die 'awk could not compare CHANGELOG.md with the base'
    case $verdict in
      release) summary="$summary, the commit of a release compared with $base_ref" ;;
      same|changed\ *)
        summary="$summary, new compared with $base_ref: $new_entry_files"
        [ "$verdict" = same ] ||
          errors="$errors${nl}CHANGELOG.md:${verdict#changed }: the file differs here from $base_ref, and a pull request does not change CHANGELOG.md from the ## [Unreleased] heading down. Fix: run sh scripts/changelog-entry.sh in your branch: it moves the lines that the branch added under that heading into changelog.d/<branch>.md and sets CHANGELOG.md to the file of origin/main. Record a correction of a released entry in an entry file too."
        [ "$new_entry_files" -gt 0 ] ||
          errors="$errors${nl}changelog.d/: the work tree adds no entry file that $base_ref lacks. Fix: add changelog.d/<branch>.md, named after your branch, with one line for each type of change that it makes, as $entry_form."
        ;;
      *) die "the comparison with the base printed: $verdict" ;;
    esac
  fi
  errors=$(printf '%s\n' "$errors" | sed '/^$/d')
  if [ -n "$errors" ]; then
    printf '%s\n' "$errors" >&2
    printf 'check-changelog: FAIL: errors: %d.\n' "$(printf '%s\n' "$errors" | wc -l)" >&2
    exit 1
  fi
  printf 'check-changelog: PASS: %s.\n' "$summary"
}

leave_the_repository_that_git_variables_name() {
  repository_variables=$(git rev-parse --local-env-vars) || die 'git rev-parse could not list the variables that name a repository'
  for repository_variable in $repository_variables; do
    unset "$repository_variable"
  done
}

self_test() {
  script_directory=$(cd "$(dirname "$0")" && pwd) || die 'cannot find the directory of this script'
  leave_the_repository_that_git_variables_name
  fixture=$(mktemp -d "${TMPDIR:-/tmp}/check-changelog-self-test.XXXXXX") || die 'mktemp could not make a directory'
  printf 'check-changelog: the fixture repositories are under %s, which is left in place\n' "$fixture"
  export GIT_AUTHOR_NAME='Halvo CI' GIT_AUTHOR_EMAIL=ci@halvo.invalid GIT_COMMITTER_NAME='Halvo CI' GIT_COMMITTER_EMAIL=ci@halvo.invalid
  mkdir "$fixture/no-hooks" "$fixture/origin" "$fixture/origin/changelog.d" || die "mkdir failed in $fixture"
  introduction='# Changelog

Each pull request adds an entry file under changelog.d/.

## [Unreleased]

- Added: Add the store'
  release='## [0.0.1] - 2026-11-10

- Added: Start the app'
  main_line='- Changed: Name the constants'
  commit_all() {
    git -C "$1" add -A || die "git add failed in $1"
    git -C "$1" -c commit.gpgsign=false -c core.hooksPath="$fixture/no-hooks" commit -q -m "$2" || die "git could not commit in $1"
  }
  printf '%s\n\n%s\n' "$introduction" "$release" >"$fixture/origin/CHANGELOG.md"
  printf 'One entry file for each pull request.\n' >"$fixture/origin/changelog.d/README.md"
  git init -q "$fixture/origin" || die "git init failed in $fixture"
  git -C "$fixture/origin" symbolic-ref HEAD refs/heads/main || die "git symbolic-ref failed in $fixture/origin"
  git -C "$fixture/origin" config core.autocrlf false || die "git config failed in $fixture/origin"
  commit_all "$fixture/origin" 'Start'
  printf '%s\n%s\n\n%s\n' "$introduction" "$main_line" "$release" >"$fixture/origin/CHANGELOG.md"
  commit_all "$fixture/origin" 'Add a line on main'

  make_clone() {
    clone=$fixture/$1
    git clone -q -c core.autocrlf=false "$fixture/origin" "$clone" || die "git could not make the clone $clone"
    git -C "$clone" checkout -q -B "$2" "${3:-origin/main}" || die "git checkout failed in $clone"
  }
  check_range() {
    (cd "$clone" && sh "$script_directory/check-changelog.sh" --base origin/main)
  }
  failures=0
  expect() {
    name=$1 want_status=$2 want_text=$3
    shift 3
    status=0
    output=$("$@" 2>&1) || status=$?
    case $status:$output in
      "$want_status:"*"$want_text"*) printf 'PASS %s\n' "$name" ;;
      *)
        printf 'FAIL %s (expected exit %s and the text "%s"; found exit %s and this output)\n%s\n' "$name" "$want_status" "$want_text" "$status" "$output"
        failures=$((failures + 1))
        ;;
    esac
  }

  make_clone with-an-entry-file unit
  printf '%s\n' '- Fixed: Close the race' >"$clone/changelog.d/unit.md"
  commit_all "$clone" 'Add an entry file'
  expect 'a range that adds an entry file passes' 0 'new compared with origin/main: 1.' check_range

  make_clone without-an-entry-file unit
  printf 'notes\n' >"$clone/notes.txt"
  commit_all "$clone" 'Add notes'
  expect 'a range that adds no entry file fails' 1 'changelog.d/: the work tree adds no entry file that origin/main lacks. Fix:' check_range

  make_clone with-a-line-under-unreleased unit
  printf '%s\n' '- Fixed: Close the race' >"$clone/changelog.d/unit.md"
  printf '%s\n%s\n%s\n\n%s\n' "$introduction" "$main_line" '- Fixed: Close the race' "$release" >"$clone/CHANGELOG.md"
  commit_all "$clone" 'Add an entry file and a line under Unreleased'
  expect 'a range that changes the Unreleased list fails' 1 'CHANGELOG.md:9: the file differs here from origin/main' check_range

  make_clone with-a-line-of-another-form unit
  printf '%s\n' '- Fixed: Close the race' '- Feature: Add the store' >"$clone/changelog.d/unit.md"
  commit_all "$clone" 'Add an entry file with an untyped line'
  expect 'a line of another form in an entry file fails' 1 'changelog.d/unit.md:2: this line is not an entry' check_range

  make_clone release main
  sed 's/^## \[Unreleased\]$/## [Unreleased]\
\
## [0.0.2] - 2026-11-21/' "$fixture/origin/CHANGELOG.md" >"$clone/CHANGELOG.md" || die 'sed could not write the release'
  commit_all "$clone" 'Release 0.0.2'
  expect 'the commit of a release passes without an entry file' 0 'the commit of a release compared with origin/main.' check_range

  entry_case() {
    entry_file=$clone/changelog.d/unit-two-lines.md
    (cd "$clone" && sh "$script_directory/changelog-entry.sh") || return 1
    after_first=$(git -C "$clone" status --porcelain && cat "$entry_file" "$clone/CHANGELOG.md") || return 1
    (cd "$clone" && sh "$script_directory/changelog-entry.sh") || return 1
    after_second=$(git -C "$clone" status --porcelain && cat "$entry_file" "$clone/CHANGELOG.md") || return 1
    [ "$(cat "$entry_file")" = "$(printf '%s\n' '- Fixed: Close the race' '- Security: Rotate the key')" ] || { echo 'the entry file does not hold the two lines'; return 1; }
    git -C "$clone" diff --quiet origin/main -- CHANGELOG.md || { echo 'CHANGELOG.md differs from origin/main'; return 1; }
    [ "$after_first" = "$after_second" ] || { echo 'the second run changed the clone'; return 1; }
    echo 'the entry file holds the two lines, CHANGELOG.md is as at origin/main, and a second run changes nothing'
  }
  make_clone entry unit/two-lines origin/main~1
  printf '%s\n%s\n%s\n\n%s\n' "$introduction" '- Fixed: Close the race' '- Security: Rotate the key' "$release" >"$clone/CHANGELOG.md"
  commit_all "$clone" 'Add two lines under Unreleased'
  expect 'changelog-entry.sh moves the two lines of a branch into its entry file, and a second run changes nothing' 0 'a second run changes nothing' entry_case

  join_case() {
    (cd "$clone" && sh "$script_directory/changelog-join.sh") || return 1
    printf '%s\n%s\n%s\n%s\n%s\n\n%s\n' "$introduction" "$main_line" '- Added: Add the page' '- Changed: Name the page' '- Fixed: Close the race' "$release" >"$fixture/joined.md"
    cmp "$fixture/joined.md" "$clone/CHANGELOG.md" || { echo 'CHANGELOG.md is not the joined file'; return 1; }
    [ "$(git -C "$clone" ls-files -- changelog.d)" = changelog.d/README.md ] || { echo 'the index holds an entry file'; return 1; }
    [ ! -e "$clone/changelog.d/a-page.md" ] && [ ! -e "$clone/changelog.d/b-race.md" ] || { echo 'the work tree holds an entry file'; return 1; }
    (cd "$clone" && sh "$script_directory/check-changelog.sh") || return 1
    echo 'the lines are at the end of the list, in the order of the file names, and the files are gone'
  }
  make_clone join main
  printf '%s\n' '- Fixed: Close the race' >"$clone/changelog.d/b-race.md"
  printf '%s\n' '- Added: Add the page' '- Changed: Name the page' >"$clone/changelog.d/a-page.md"
  commit_all "$clone" 'Add two entry files'
  expect 'changelog-join.sh puts the lines of the entry files at the end of the Unreleased list and removes the files' 0 'and the files are gone' join_case

  if [ "$failures" -gt 0 ]; then
    printf 'check-changelog: self-test FAIL: cases misjudged: %d\n' "$failures"
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
