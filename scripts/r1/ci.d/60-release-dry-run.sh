#!/bin/sh
# jobs: macos
# rule: The documented release command works on every commit: in a throwaway clone with no remote its dry run passes on a named branch and changes nothing, --execute refuses a branch other than main, and on main it makes one commit Release <next> with every package at <next>, one tag v<next> and the changelog entries grouped by type, and leaves the tree clean.
set -u
name=60-release-dry-run
types='Added Changed Deprecated Removed Fixed Security'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

unreleased_texts() {
  awk '/^## \[Unreleased\]/ { on = 1; next } /^## / { on = 0 } on && /^- [A-Za-z]+: / { sub(/^- [A-Za-z]+: /, ""); print }' "$1" | LC_ALL=C sort
}

released_section_defect() {
  problem=$(LC_ALL=C awk -v version="$2" -v types="$types" '
    function say(text) { if (!told) print text; told = 1 }
    BEGIN { n = split(types, order, " "); for (i = 1; i <= n; i++) rank[order[i]] = i }
    $0 == "## [Unreleased]" { section = "unreleased"; next }
    index($0, "## [" version "] - ") == 1 { section = "release"; found = 1; next }
    /^## / { section = ""; next }
    section == "unreleased" && /^- / { say("## [Unreleased] holds an entry after the release") }
    section != "release" { next }
    /^### / {
      heading = substr($0, 5)
      if (!(heading in rank)) say("the section of " version " has the heading " $0)
      else if (heading in seen) say("the section of " version " has " $0 " twice")
      else if (rank[heading] < last) say("the section of " version " has " $0 " out of order")
      seen[heading] = 1
      last = rank[heading]
      next
    }
    /^- (Added|Changed|Deprecated|Removed|Fixed|Security): / { say("the section of " version " holds a typed entry, so its entries were not grouped"); next }
    /^- / { if (!last) say("the section of " version " holds an entry above every ### heading"); sub(/^- /, ""); print > "/dev/stderr" }
    END { if (!found) say("CHANGELOG.md has no heading of " version) }
  ' "$1" 2>"$work/grouped-texts")
  [ -z "$problem" ] || { printf '%s\n' "$problem"; return 1; }
  LC_ALL=C sort "$work/grouped-texts" | cmp -s - "$3" || {
    printf 'the entry texts of the section of %s differ from those of ## [Unreleased] before the release\n' "$2"
    return 1
  }
}

snapshot() {
  git -C "$1" status --porcelain --untracked-files=all
  git -C "$1" rev-parse HEAD
  git -C "$1" tag --list
  git -C "$1" for-each-ref --format='%(refname) %(objectname)'
}

release_commit_refusal() {
  snapshot "$1" >"$work/before-the-release-commit"
  (cd "$1" && sh scripts/release.sh --dry-run) >"$work/dry-run-on-the-release-commit.txt" 2>&1
  refusal=$?
  snapshot "$1" >"$work/after-the-release-commit"
  [ "$refusal" = 1 ] || { printf 'the dry run on the release commit exited %s, not 1\n' "$refusal"; return 1; }
  grep -q 'holds no entry, so there is nothing to release' "$work/dry-run-on-the-release-commit.txt" ||
    { printf 'the dry run on the release commit did not refuse with its no entry line\n'; return 1; }
  cmp -s "$work/before-the-release-commit" "$work/after-the-release-commit" ||
    { printf 'the dry run on the release commit changed the status, HEAD, a tag or a ref of the clone\n'; return 1; }
}

measure() {
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_COMMON_DIR
  cd "$CI_D_REPO" || exit 2
  pin=$(git show HEAD:scripts/prerequisites.sh | sed -n 's/^cargo_release_version=\([0-9.]*\)$/\1/p')
  [ -n "$pin" ] || fail "scripts/prerequisites.sh at HEAD holds no line cargo_release_version="
  installed=$(cargo release --version 2>/dev/null)
  if [ "$installed" != "cargo-release $pin" ]; then
    [ "${CI:-}" = true ] && fail "cargo-release $pin is not installed; the Prerequisites step installs it"
    printf 'SKIP %s (cargo-release %s is not installed; run sh scripts/prerequisites.sh)\n' "$name" "$pin"
    exit 77
  fi
  head=$(git rev-parse HEAD) || fail "git rev-parse HEAD failed"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  printf 'throwaway clone under %s\n' "$work"
  clone=$work/clone
  git clone --quiet --no-hardlinks "$CI_D_REPO" "$clone" || fail "git clone of the checkout failed"
  git -C "$clone" remote remove origin || fail "git remote remove origin failed in the clone"
  git -C "$clone" checkout -q -B release-check "$head" || fail "git checkout in the clone failed"
  (cd "$clone" && GIT_AUTHOR_NAME='Halvo CI' GIT_AUTHOR_EMAIL=ci@halvo.invalid GIT_COMMITTER_NAME='Halvo CI' GIT_COMMITTER_EMAIL=ci@halvo.invalid git var GIT_COMMITTER_IDENT) >/dev/null 2>&1 ||
    fail "git has no name and email for the release commit in the throwaway clone"
  remotes=$(git -C "$clone" remote)
  [ -z "$remotes" ] || fail "the throwaway clone has a remote"
  entries=$(awk '{ sub(/[ \t\r]+$/, "") } $0 == "## [Unreleased]" { on = 1; next } /^## / { on = 0 } on && /^- / { n++ } END { print n + 0 }' "$clone/CHANGELOG.md")
  subject=$(git -C "$clone" log -1 --format=%s)
  version=$(cd "$clone" && CI_D_REPO=$clone sh scripts/r1/ci.d/61-version.sh --print)
  if [ "$entries" = 0 ] && [ "$subject" = "Release $version" ]; then
    found=$(release_commit_refusal "$clone") || fail "$found"
    printf 'release commit: the dry run refuses with its no entry line and changes nothing: true\n'
    printf 'PASS %s\n' "$name"
    exit 0
  fi

  snapshot "$clone" >"$work/before-dry-run"
  (cd "$clone" && sh scripts/release.sh --dry-run) >"$work/dry-run.txt" 2>&1
  status=$?
  snapshot "$clone" >"$work/after-dry-run"
  [ "$status" = 0 ] || fail "sh scripts/release.sh --dry-run exited $status on a named branch"
  next=$(sed -n 's/^next version: //p' "$work/dry-run.txt")
  case $next in
    ''|*[!0-9.]*) fail "the dry run printed no line next version: <version>" ;;
  esac
  grep -q -x "tag: v$next" "$work/dry-run.txt" || fail "the dry run printed no line tag: v<next version>"
  grep -q -x "commit message: Release $next" "$work/dry-run.txt" || fail "the dry run printed no line commit message: Release <next version>"
  ! grep -E '^(next version|commit message|tag|tag message): ' "$work/dry-run.txt" | grep -q '{{' || fail "a line of the dry run's commit message or tag holds a template"
  cmp -s "$work/before-dry-run" "$work/after-dry-run" || fail "the dry run changed the status, HEAD, a tag or a ref of the clone"
  printf 'dry run on a named branch: exit 0, next version and tag lines: true, clone unchanged: true\n'

  (cd "$clone" && GIT_AUTHOR_NAME='Halvo CI' GIT_AUTHOR_EMAIL=ci@halvo.invalid GIT_COMMITTER_NAME='Halvo CI' GIT_COMMITTER_EMAIL=ci@halvo.invalid sh scripts/release.sh --execute) >"$work/execute-off-main.txt" 2>&1
  status=$?
  snapshot "$clone" >"$work/after-execute-off-main"
  [ "$status" != 0 ] || fail "sh scripts/release.sh --execute exited 0 on a branch other than main"
  cmp -s "$work/before-dry-run" "$work/after-execute-off-main" || fail "--execute on a branch other than main changed the status, HEAD, a tag or a ref of the clone"
  printf 'execute on a branch other than main: refused with exit %s, clone unchanged: true\n' "$status"

  git -C "$clone" checkout -q -B main "$head" || fail "git checkout -B main in the clone failed"
  unreleased_texts "$clone/CHANGELOG.md" >"$work/unreleased-before.txt"
  tags_before=$(git -C "$clone" tag --list | grep -c .)
  (cd "$clone" && GIT_AUTHOR_NAME='Halvo CI' GIT_AUTHOR_EMAIL=ci@halvo.invalid GIT_COMMITTER_NAME='Halvo CI' GIT_COMMITTER_EMAIL=ci@halvo.invalid sh scripts/release.sh --execute) >"$work/execute-on-main.txt" 2>&1
  status=$?
  [ "$status" = 0 ] || fail "sh scripts/release.sh --execute exited $status on main"
  commits=$(git -C "$clone" rev-list --count "$head..HEAD")
  [ "$commits" = 1 ] || fail "--execute on main made $commits commits"
  subject=$(git -C "$clone" log -1 --format=%s)
  [ "$subject" = "Release $next" ] || fail "the release commit's subject is not Release <next version>"
  tags_after=$(git -C "$clone" tag --list | grep -c .)
  [ "$tags_after" = $((tags_before + 1)) ] || fail "--execute on main made $((tags_after - tags_before)) tags"
  at_head=$(git -C "$clone" tag --points-at HEAD)
  [ "$at_head" = "v$next" ] || fail "the tag at the release commit is not v<next version>"
  changed=$(git -C "$clone" status --porcelain --untracked-files=all)
  [ -z "$changed" ] || fail "the clone is not clean after --execute"
  printed=$(cd "$clone" && CI_D_REPO=$clone sh scripts/r1/ci.d/61-version.sh --print)
  [ "$printed" = "$next" ] || fail "61-version.sh --print in the clone does not print the next version: $printed"
  readme=$(grep -c -F "which is \`$next\` now" "$clone/README.md")
  [ "$readme" = 1 ] || fail "README.md does not name the next version once under ## Version"
  (cd "$clone" && sh scripts/check-commit-messages.sh HEAD~1..HEAD) >"$work/commit-messages.txt" 2>&1 ||
    fail "scripts/check-commit-messages.sh refuses the release commit"
  (cd "$clone" && sh scripts/check-changelog.sh) >"$work/changelog.txt" 2>&1 || fail "scripts/check-changelog.sh refuses CHANGELOG.md after the release"
  (cd "$clone" && sh scripts/check-changelog.sh --base HEAD~1) >"$work/changelog-base.txt" 2>&1 ||
    fail "scripts/check-changelog.sh --base HEAD~1 refuses the release commit"
  found=$(released_section_defect "$clone/CHANGELOG.md" "$next" "$work/unreleased-before.txt") || fail "$found"
  headings=$(awk -v version="$next" 'index($0, "## [" version "] - ") == 1 { on = 1; next } /^## / { on = 0 } on && /^### / { n++ } END { print n + 0 }' "$clone/CHANGELOG.md")
  entries=$(grep -c . "$work/unreleased-before.txt")
  found=$(release_commit_refusal "$clone") || fail "on the release commit that the execute case made: $found"
  printf 'execute on main: one commit Release <next>, one tag v<next>, clean tree, one version: true; entries grouped: %s under %s headings\n' "$entries" "$headings"
  printf 'PASS %s\n' "$name"
}

self_test() {
  root=${CI_D_REPO:-$(pwd)}
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  mkdir "$work/bin"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_LOG"
case "$1 $2" in
  'release version') printf '   Upgrading halvo from 1.2.3 to 1.2.4\n' ;;
  'release config')
    if [ -n "${STUB_CONFIG:-}" ]; then
      printf '%s\n' "$STUB_CONFIG"
    else
      printf 'tag-name = "v{{version}}"\ntag-message = "Release {{version}}"\n'
      if [ "${3:-}" = --config ]; then cat "$4"; else printf 'pre-release-commit-message = "Release {{version}}"\n'; fi
    fi
    ;;
  'release '*) printf '%s\n' "${STUB_TEXT:-   Upgrading halvo from 1.2.3 to 1.2.4}"; exit "${STUB_STATUS:-0}" ;;
  *) exit 97 ;;
esac
EOF
  chmod +x "$work/bin/cargo"
  intro='# Changelog

All notable changes are recorded in this file.

## [Unreleased]
'
  repository() {
    repo=$work/repo-$1
    mkdir -p "$repo/scripts" || exit 2
    cp "$root/scripts/release.sh" "$root/scripts/check-changelog.sh" "$repo/scripts/" || exit 2
    printf '%s\n%s\n' "$intro" "$2" >"$repo/CHANGELOG.md"
    git init -q "$repo" && git -C "$repo" add -A && git -C "$repo" commit -q -m fixture || exit 2
  }
  release_case() {
    case_name=$1 want_status=$2 want_cargo=$3 want_text=$4
    shift 4
    log=$work/argv-$case_name
    status=0
    (cd "$repo" && PATH="$work/bin:$PATH" STUB_LOG=$log CARGO_HOME=$work/no-cargo-home sh scripts/release.sh "$@") >"$work/out-$case_name" 2>&1 || status=$?
    called=no
    [ -f "$log" ] && grep -q '^release ' "$log" && called=yes
    got=FAIL
    [ "$status" = "$want_status" ] && [ "$called" = "$want_cargo" ] && grep -q -F -e "$want_text" "$work/out-$case_name" && got=PASS
    if [ "$want_status" = 0 ]; then verdict "$case_name" PASS "$got"; elif [ "$got" = PASS ]; then verdict "$case_name" FAIL FAIL; else verdict "$case_name" FAIL PASS; fi
  }
  typed='- Fixed: Mend the first fault
- Added: Add the feature
- Fixed: Mend the second fault'
  repository typed "$typed"
  release_case dry-run-calls-cargo 0 yes 'next version: 1.2.4' --dry-run
  release_case dry-run-minor-calls-cargo 0 yes 'tag: v1.2.4' --dry-run minor
  release_case dry-run-and-execute 64 no usage --dry-run --execute
  release_case level-x 64 no usage --dry-run -x
  release_case level-a-version 64 no usage --dry-run 9.9.9
  release_case level-bogus 64 no usage --execute bogus
  release_case third-argument 64 no usage --dry-run patch extra
  release_case no-argument 64 no usage
  help_status=0
  (cd "$repo" && sh scripts/release.sh --help) >"$work/help.txt" 2>&1 || help_status=$?
  missing=
  for fact in '--dry-run' '--execute' 'changes no tracked file' 'target/release-commit-message.toml' 'main only' 'one commit and one tag' 'pushes nothing' 'git ls-remote' 'no remote named origin' 'defaults to patch' "owner's own steps"; do
    grep -q -F -e "$fact" "$work/help.txt" || missing="$missing [$fact]"
  done
  got=FAIL
  [ "$help_status" = 0 ] && [ -z "$missing" ] && got=PASS
  verdict help-states-what-a-person-who-runs-the-script-needs PASS "$got"
  release_case help-with-another-argument 64 no usage --help patch
  grep -q -e '--execute --no-confirm --allow-branch main' "$work/argv-dry-run-calls-cargo" && got=FAIL || got=PASS
  verdict dry-run-has-no-branch-guard PASS "$got"
  STUB_TEXT='   Upgrading halvo from 1.2.3 to 1.2.4' release_case execute-has-the-branch-guard 0 yes 'tag: v1.2.4' --execute
  grep -q -e '--execute --no-confirm --allow-branch main' "$work/argv-execute-has-the-branch-guard" && got=PASS || got=FAIL
  verdict execute-passes-allow-branch-main PASS "$got"
  STUB_STATUS=1 STUB_TEXT='error: io: failed to lookup address information' release_case no-network 1 yes index.crates.io --dry-run
  STUB_TEXT='- Fixed: Refuse a commit message whose template stayed Unrendered' release_case entry-that-holds-the-word-unrendered 0 yes 'commit message: Release 1.2.4' --dry-run
  template_case() {
    case_name=$1
    shift
    log=$work/argv-$case_name
    status=0
    (cd "$repo" && PATH="$work/bin:$PATH" STUB_LOG=$log CARGO_HOME=$work/no-cargo-home sh scripts/release.sh "$@") >"$work/out-$case_name" 2>&1 || status=$?
    got=PASS
    if [ "$status" = 1 ] && grep -q 'would hold a template' "$work/out-$case_name" && ! grep -q -E '^release (patch|minor|major) --config' "$log"; then got=FAIL; fi
    verdict "$case_name" FAIL "$got"
  }
  STUB_CONFIG='tag-name = "v{{crate_version}}"
tag-message = "Release {{version}}"
pre-release-commit-message = "Release 1.2.4"' template_case template-left-in-the-tag-name --dry-run
  STUB_CONFIG='tag-name = "v{{version}}"
tag-message = "Release {{version}}"
pre-release-commit-message = "Release {{version}} of {{crate_name}}"' template_case template-left-in-the-commit-message --execute
  STUB_CONFIG='tag-name = "v{{version}}"
tag-message = "Release {{date}}"
pre-release-commit-message = "Release 1.2.4"' template_case template-left-in-the-tag-message --execute
  STUB_STATUS=1 STUB_TEXT='error: the pre-release hook exited 1' release_case other-failure-prints-its-error-line 1 yes 'error: the pre-release hook exited 1' --dry-run
  grep -q index.crates.io "$work/out-other-failure-prints-its-error-line" && got=FAIL || got=PASS
  verdict other-failure-names-no-network PASS "$got"
  repository heading '### Fixed
- Mend the fault'
  release_case heading-inside-unreleased 1 no 'breaks a rule' --dry-run
  repository untyped '- Mend the fault'
  release_case untyped-entry 1 no 'breaks a rule' --execute
  repository empty ''
  release_case empty-unreleased 1 no 'holds no entry' --dry-run
  repository tagged "$typed
## [1.2.3] - 2026-11-01

- Added: Add the store"
  git -C "$repo" tag v1.2.3
  release_case released-sections-as-at-the-tag 0 yes 'next version: 1.2.4' --dry-run
  printf '%s\n' '- Added: Slip a line into a released section' >>"$repo/CHANGELOG.md"
  git -C "$repo" commit -q -a -m 'Add a line below a release'
  release_case line-below-a-released-section 1 no 'differ from those at v1.2.3' --dry-run
  repository crlf-after-a-release "$typed
## [1.2.3] - 2026-11-01

- Added: Add the store"
  git -C "$repo" tag v1.2.3
  sed 's/$/\r/' "$repo/CHANGELOG.md" >"$repo/CHANGELOG.crlf" && mv "$repo/CHANGELOG.crlf" "$repo/CHANGELOG.md"
  git -C "$repo" commit -q -a -m 'Check out the changelog with CRLF'
  release_case second-release-in-a-crlf-tree 0 yes 'next version: 1.2.4' --dry-run
  refused_before_cargo_release() {
    case_name=$1 want_text=$2
    shift 2
    log=$work/argv-$case_name
    status=0
    (cd "$repo" && PATH="$work/bin:$PATH" STUB_LOG=$log CARGO_HOME=$work/no-cargo-home sh scripts/release.sh "$@") >"$work/out-$case_name" 2>&1 || status=$?
    got=PASS
    if [ "$status" = 1 ] && grep -q -F -e "$want_text" "$work/out-$case_name" && ! grep -q -E '^release (patch|minor|major) --config' "$log"; then got=FAIL; fi
    verdict "$case_name" FAIL "$got"
  }
  repository heading-of-the-next-version "$typed
## [1.2.4] - 2020-01-01

- Added: Add the store"
  refused_before_cargo_release heading-of-the-next-version-exists 'already holds a heading of 1.2.4' --dry-run
  repository behind-origin "$typed"
  git -C "$repo" checkout -q -B main
  git clone -q --bare "$repo" "$work/origin-of-behind.git" || exit 2
  git -C "$repo" remote add origin "$work/origin-of-behind.git"
  git -C "$repo" commit -q --allow-empty -m 'A commit that origin gets'
  git -C "$repo" push -q origin main
  git -C "$repo" checkout -q -B main HEAD~1
  refused_before_cargo_release main-one-commit-behind-origin 'is not the newest commit of main on origin' --execute
  git -C "$repo" checkout -q -B main origin/main
  release_case main-at-the-newest-commit-of-origin 0 yes 'next version: 1.2.4' --execute
  repository shallow-source "$typed"
  git -C "$repo" commit -q --allow-empty -m 'A second commit'
  git clone -q --depth 1 "file://$repo" "$work/repo-shallow" || exit 2
  repo=$work/repo-shallow
  shallow_state() {
    git -C "$repo" for-each-ref --format='%(refname) %(objectname)'
    git -C "$repo" tag --list
    cat "$repo/.git/shallow"
  }
  shallow_state >"$work/shallow-before"
  release_case dry-run-in-a-shallow-clone 1 no 'git fetch --unshallow --tags origin' --dry-run
  shallow_state >"$work/shallow-after"
  cmp -s "$work/shallow-before" "$work/shallow-after" && got=PASS || got=FAIL
  verdict dry-run-in-a-shallow-clone-changes-nothing PASS "$got"

  group_case() {
    case_name=$1 want=$2 dry=$3 version=$4 before=$5 expected=$6
    printf '%s\n' "$before" >"$work/group-$case_name.md"
    cp "$work/group-$case_name.md" "$work/group-$case_name.before"
    mkdir -p "$work/group-$case_name/scripts"
    cp "$root/scripts/release.sh" "$work/group-$case_name/scripts/"
    cp "$work/group-$case_name.md" "$work/group-$case_name/CHANGELOG.md"
    status=0
    (cd "$work/group-$case_name" && NEW_VERSION=$version DRY_RUN=$dry sh scripts/release.sh --group-changelog) >"$work/group-$case_name.out" 2>&1 || status=$?
    got=FAIL
    if [ "$status" = 0 ] && printf '%s\n' "$expected" | cmp -s - "$work/group-$case_name/CHANGELOG.md"; then got=PASS; fi
    verdict "$case_name" "$want" "$got"
  }
  grouped_out='# Changelog

## [Unreleased]

## [1.2.4] - 2026-11-14

### Added

- Add the feature

### Fixed

- Mend the first fault
- Mend the second fault

## [1.2.3] - 2026-11-01

- Added: Add the store'
  group_case two-fixed-around-one-added PASS false 1.2.4 "# Changelog

## [Unreleased]

## [1.2.4] - 2026-11-14

$typed

## [1.2.3] - 2026-11-01

- Added: Add the store" "$grouped_out"
  group_case grouped-under-unreleased-before-the-rename PASS false 1.2.4 "# Changelog

## [Unreleased]

$typed" "# Changelog

## [Unreleased]

### Added

- Add the feature

### Fixed

- Mend the first fault
- Mend the second fault"
  group_case dry-run-writes-nothing FAIL true 1.2.4 "# Changelog

## [Unreleased]

$typed" "# Changelog

## [Unreleased]

### Added

- Add the feature

### Fixed

- Mend the first fault
- Mend the second fault"
  grep -q -x '### Fixed' "$work/group-dry-run-writes-nothing.out" && cmp -s "$work/group-dry-run-writes-nothing.md" "$work/group-dry-run-writes-nothing/CHANGELOG.md" && got=PASS || got=FAIL
  verdict dry-run-prints-the-grouped-section PASS "$got"
  trailing_before=$(printf '%s\n' "# Changelog" "" "## [Unreleased] " "" "$typed")
  group_case heading-with-a-trailing-space PASS false 1.2.4 "$trailing_before" "# Changelog

## [Unreleased]

### Added

- Add the feature

### Fixed

- Mend the first fault
- Mend the second fault"
  crlf_before=$(printf '%s\n' "# Changelog" "" "## [Unreleased]" "" "## [1.2.4] - 2026-11-14" "" "$typed" | sed 's/$/\r/')
  group_case work-tree-with-crlf PASS false 1.2.4 "$crlf_before" "# Changelog

## [Unreleased]

## [1.2.4] - 2026-11-14

### Added

- Add the feature

### Fixed

- Mend the first fault
- Mend the second fault"
  group_case untyped-line-stops-the-hook FAIL false 1.2.4 "# Changelog

## [Unreleased]

## [1.2.4] - 2026-11-14

- Mend the fault" "# Changelog"
  cmp -s "$work/group-untyped-line-stops-the-hook.md" "$work/group-untyped-line-stops-the-hook/CHANGELOG.md" && got=PASS || got=FAIL
  verdict untyped-line-leaves-the-file PASS "$got"
  status=0
  (cd "$work/group-untyped-line-stops-the-hook" && env -u NEW_VERSION sh scripts/release.sh --group-changelog) >/dev/null 2>&1 || status=$?
  [ "$status" = 64 ] && got=FAIL || got=PASS
  verdict hook-without-new-version FAIL "$got"

  section_case() {
    case_name=$1 want=$2 text=$3
    printf '%s\n' "$text" >"$work/section-$case_name.md"
    printf '%s\n' 'Add the feature' 'Mend the first fault' 'Mend the second fault' | LC_ALL=C sort >"$work/section-$case_name.texts"
    got=FAIL
    released_section_defect "$work/section-$case_name.md" 1.2.4 "$work/section-$case_name.texts" >/dev/null && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  section_case grouped-release PASS "$grouped_out"
  section_case renamed-heading-alone FAIL "# Changelog

## [Unreleased]

## [1.2.4] - 2026-11-14

$typed"
  section_case heading-twice FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

### Fixed

- Mend the first fault

### Added

- Add the feature

### Fixed

- Mend the second fault"
  section_case heading-twice-in-order FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

### Added

- Add the feature

### Added

- Mend the first fault
- Mend the second fault"
  released_part=$(printf '%s\n' "$grouped_out" | sed -n '/^## \[1.2.4\]/,$p')
  section_case entry-also-left-in-unreleased FAIL "## [Unreleased]

- Added: Add the feature

$released_part"
  section_case headings-out-of-order FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

### Fixed

- Mend the first fault
- Mend the second fault

### Added

- Add the feature"
  section_case entry-left-in-unreleased FAIL "## [Unreleased]

- Added: Add the feature

## [1.2.4] - 2026-11-14

### Fixed

- Mend the first fault
- Mend the second fault"
  section_case an-entry-lost FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

### Fixed

- Mend the first fault
- Mend the second fault"
  section_case no-section-of-the-version FAIL "## [Unreleased]

## [1.2.3] - 2026-11-14

### Added

- Add the feature"

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
