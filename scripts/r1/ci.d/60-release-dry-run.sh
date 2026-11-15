#!/bin/sh
# jobs: macos
# rule: The documented release command works on every commit: in a throwaway clone with no remote its dry run passes on a named branch and changes nothing, --execute refuses a branch other than main, and on main it makes one commit Release <next> with every package at <next>, one tag v<next> and the changelog entries grouped by type, and leaves the tree clean; on a release commit (no Unreleased entry, the subject Release <version>) it requires instead that the dry run refuses with its no entry line and changes nothing.
set -u
name=60-release-dry-run
types='Added Changed Deprecated Removed Fixed Security'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

unreleased_texts() {
  awk '{ sub(/[ \t\r]+$/, "") } $0 == "## [Unreleased]" { on = 1; next } /^## / { on = 0 } on && /^- [A-Za-z]+: / { sub(/^- [A-Za-z]+: /, ""); print }' "$1" | LC_ALL=C sort
}

released_section_defect() {
  problem=$(LC_ALL=C awk -v version="$2" -v types="$types" '
    function say(text) { if (!told) print text; told = 1 }
    BEGIN { n = split(types, order, " "); for (i = 1; i <= n; i++) rank[order[i]] = i }
    { sub(/[ \t\r]+$/, "") }
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
  LC_ALL=C sort "$work/grouped-texts" >"$work/grouped-sorted"
  cmp -s "$work/grouped-sorted" "$3" && return 0
  only_before=$(LC_ALL=C comm -23 "$3" "$work/grouped-sorted" | head -n 1)
  if [ -n "$only_before" ]; then
    printf 'the section of %s differs from ## [Unreleased] before the release: only before the release: %s\n' "$2" "$only_before"
  else
    only_after=$(LC_ALL=C comm -13 "$3" "$work/grouped-sorted" | head -n 1)
    printf 'the section of %s differs from ## [Unreleased] before the release: only after: %s\n' "$2" "$only_after"
  fi
  return 1
}

with_release_line() {
  said=$(awk 'NF { last = $0 } END { print last }' "$2")
  printf '%s; release.sh said: %s\n' "$1" "${said:-nothing}"
}

fail_after() {
  line=$(with_release_line "$1" "$2")
  fail "$line"
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
  cp "$work/dry-run.txt" "${CI_D_ARTIFACTS:-$work}/" 2>/dev/null
  [ "$status" = 0 ] || fail_after "sh scripts/release.sh --dry-run exited $status on a named branch" "$work/dry-run.txt"
  next=$(sed -n 's/^next version: //p' "$work/dry-run.txt")
  case $next in
    ''|*[!0-9.]*) fail_after "the dry run printed no line next version: <version>" "$work/dry-run.txt" ;;
  esac
  grep -q -x "tag: v$next" "$work/dry-run.txt" || fail_after "the dry run printed no line tag: v<next version>" "$work/dry-run.txt"
  grep -q -x "commit message: Release $next" "$work/dry-run.txt" || fail_after "the dry run printed no line commit message: Release <next version>" "$work/dry-run.txt"
  ! grep -E '^(next version|commit message|tag|tag message): ' "$work/dry-run.txt" | grep -q '{{' || fail_after "a line of the dry run's commit message or tag holds a template" "$work/dry-run.txt"
  cmp -s "$work/before-dry-run" "$work/after-dry-run" || fail_after "the dry run changed the status, HEAD, a tag or a ref of the clone" "$work/dry-run.txt"
  printf 'dry run on a named branch: exit 0, next version and tag lines: true, clone unchanged: true\n'

  (cd "$clone" && GIT_AUTHOR_NAME='Halvo CI' GIT_AUTHOR_EMAIL=ci@halvo.invalid GIT_COMMITTER_NAME='Halvo CI' GIT_COMMITTER_EMAIL=ci@halvo.invalid sh scripts/release.sh --execute) >"$work/execute-off-main.txt" 2>&1
  status=$?
  snapshot "$clone" >"$work/after-execute-off-main"
  cp "$work/execute-off-main.txt" "${CI_D_ARTIFACTS:-$work}/" 2>/dev/null
  [ "$status" != 0 ] || fail_after "sh scripts/release.sh --execute exited 0 on a branch other than main" "$work/execute-off-main.txt"
  cmp -s "$work/before-dry-run" "$work/after-execute-off-main" || fail_after "--execute on a branch other than main changed the status, HEAD, a tag or a ref of the clone" "$work/execute-off-main.txt"
  printf 'execute on a branch other than main: refused with exit %s, clone unchanged: true\n' "$status"

  git -C "$clone" checkout -q -B main "$head" || fail "git checkout -B main in the clone failed"
  unreleased_texts "$clone/CHANGELOG.md" >"$work/unreleased-before.txt"
  tags_before=$(git -C "$clone" tag --list | grep -c .)
  (cd "$clone" && GIT_AUTHOR_NAME='Halvo CI' GIT_AUTHOR_EMAIL=ci@halvo.invalid GIT_COMMITTER_NAME='Halvo CI' GIT_COMMITTER_EMAIL=ci@halvo.invalid sh scripts/release.sh --execute) >"$work/execute-on-main.txt" 2>&1
  status=$?
  cp "$work/execute-on-main.txt" "${CI_D_ARTIFACTS:-$work}/" 2>/dev/null
  [ "$status" = 0 ] || fail_after "sh scripts/release.sh --execute exited $status on main" "$work/execute-on-main.txt"
  commits=$(git -C "$clone" rev-list --count "$head..HEAD")
  [ "$commits" = 1 ] || fail_after "--execute on main made $commits commits" "$work/execute-on-main.txt"
  subject=$(git -C "$clone" log -1 --format=%s)
  [ "$subject" = "Release $next" ] || fail_after "the release commit's subject is not Release <next version>" "$work/execute-on-main.txt"
  tags_after=$(git -C "$clone" tag --list | grep -c .)
  [ "$tags_after" = $((tags_before + 1)) ] || fail_after "--execute on main made $((tags_after - tags_before)) tags" "$work/execute-on-main.txt"
  at_head=$(git -C "$clone" tag --points-at HEAD)
  [ "$at_head" = "v$next" ] || fail_after "the tag at the release commit is not v<next version>" "$work/execute-on-main.txt"
  changed=$(git -C "$clone" status --porcelain --untracked-files=all)
  first_changed=$(printf '%s\n' "$changed" | head -n 1 | cut -c4-)
  [ -z "$changed" ] || fail_after "the clone is not clean after --execute (the first changed path is $first_changed)" "$work/execute-on-main.txt"
  printed=$(cd "$clone" && CI_D_REPO=$clone sh scripts/r1/ci.d/61-version.sh --print)
  [ "$printed" = "$next" ] || fail "61-version.sh --print in the clone does not print the next version: $printed"
  readme=$(grep -c -F "which is \`$next\` now" "$clone/README.md")
  [ "$readme" = 1 ] || fail "README.md does not name the next version once under ## Version"
  (cd "$clone" && sh scripts/check-commit-messages.sh HEAD~1..HEAD) >"$work/commit-messages.txt" 2>&1 ||
    fail "scripts/check-commit-messages.sh refuses the release commit"
  (cd "$clone" && sh scripts/check-changelog.sh) >"$work/changelog.txt" 2>&1 || fail "scripts/check-changelog.sh refuses CHANGELOG.md after the release"
  (cd "$clone" && sh scripts/check-changelog.sh --base HEAD~1) >"$work/changelog-base.txt" 2>&1 ||
    fail "scripts/check-changelog.sh --base HEAD~1 refuses the release commit"
  found=$(released_section_defect "$clone/CHANGELOG.md" "$next" "$work/unreleased-before.txt") || fail_after "$found" "$work/execute-on-main.txt"
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
  'release version')
    case ${3:-} in
      patch) printf '   Upgrading halvo from 1.2.3 to 1.2.4\n' ;;
      minor) printf '   Upgrading halvo from 1.2.3 to 1.3.0\n' ;;
      major) printf '   Upgrading halvo from 1.2.3 to 2.0.0\n' ;;
      *) printf 'error: the stub knows no level %s\n' "${3:-}"; exit 98 ;;
    esac
    ;;
  'metadata --no-deps')
    printf '{"packages":[{"name":"halvo","version":"1.2.3","id":"path+file:///w/src-tauri#halvo@1.2.3"}],"workspace_members":["path+file:///w/src-tauri#halvo@1.2.3"]}\n'
    ;;
  'release config')
    if [ -n "${STUB_CONFIG:-}" ]; then
      printf '%s\n' "$STUB_CONFIG"
    else
      printf 'tag-name = "v{{version}}"\ntag-message = "Release {{version}}"\n'
      if [ "${3:-}" = --config ]; then cat "$4"; else printf 'pre-release-commit-message = "Release {{version}}"\n'; fi
    fi
    ;;
  'release '*)
    case " $* " in
      *' --execute '*)
        case ${STUB_ACTION:-} in
          touch) printf 'changed by the stub\n' >>CHANGELOG.md ;;
          commit) printf 'changed by the stub\n' >>CHANGELOG.md && git commit -q -a -m 'Release 1.2.4' ;;
          commit-tag) printf 'changed by the stub\n' >>CHANGELOG.md && git commit -q -a -m 'Release 1.2.4' && git tag "${STUB_TAG:-v1.2.4}" ;;
        esac
        ;;
    esac
    printf '%s\n' "${STUB_TEXT:-   Upgrading halvo from 1.2.3 to 1.2.4}"
    exit "${STUB_STATUS:-0}"
    ;;
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
  typed='- Fixed: Mend the first fault
- Added: Add the feature
- Fixed: Mend the second fault'
  repository typed "$typed"
  release_case dry-run-calls-cargo 0 yes 'next version: 1.2.4' --dry-run
  release_case dry-run-minor-calls-cargo 0 yes 'tag: v1.3.0' --dry-run minor
  release_case dry-run-major-calls-cargo 0 yes 'next version: 2.0.0' --dry-run major
  got=FAIL
  grep -q '^release minor --config ' "$work/argv-dry-run-minor-calls-cargo" && grep -q '^release major --config ' "$work/argv-dry-run-major-calls-cargo" && got=PASS
  verdict the-level-reaches-cargo-release PASS "$got"
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
  typed_repo=$repo
  repository branch-guard "$typed"
  STUB_ACTION=commit-tag
  export STUB_ACTION
  STUB_TEXT='   Upgrading halvo from 1.2.3 to 1.2.4' release_case execute-has-the-branch-guard 0 yes 'tag: v1.2.4' --execute
  unset STUB_ACTION
  repo=$typed_repo
  grep -q -e '--execute --no-confirm --allow-branch main' "$work/argv-execute-has-the-branch-guard" && got=PASS || got=FAIL
  verdict execute-passes-allow-branch-main PASS "$got"
  grep -q -E '^release patch --config target/release-commit-message[.]toml --execute ' "$work/argv-execute-has-the-branch-guard" && got=PASS || got=FAIL
  verdict execute-passes-the-config-of-the-commit-message PASS "$got"
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
  upgrading='   Upgrading workspace to version 0.0.1
   Upgrading halvo-control from 0.0.0 to 0.0.1 (inherited from workspace)
   Upgrading halvo-connect from 0.0.0 to 0.0.1
   Upgrading halvo-web from 0.0.0 to 0.0.1'
  network_case() {
    case_name=$1 want_index=$2 want_line=$3
    STUB_STATUS=1 STUB_TEXT=$4 release_case "$case_name" 1 yes "$want_line" --dry-run
    named=no
    grep -q index.crates.io "$work/out-$case_name" && named=yes
    got=FAIL
    [ "$named" = "$want_index" ] && grep -q -F -e "release.sh: cargo-release failed: $want_line" "$work/out-$case_name" && got=PASS
    verdict "$case_name-names-the-index-$want_index" PASS "$got"
  }
  readme_error="error: for \`which is \`[0-9]+\\.[0-9]+\\.[0-9]+\` now\` in '../README.md', at least 1 replacements expected, found 0"
  network_case replacement-failure no "$readme_error" "$upgrading

$readme_error"
  network_case dirty-tree no 'error: uncommitted changes detected, please resolve before release:' "error: uncommitted changes detected, please resolve before release:
         crates/halvo-connect/Cargo.toml (Status(WT_MODIFIED))
$upgrading"
  network_case hook-refusal no 'error: CHANGELOG.md:315: this line is not a typed entry' "$upgrading
error: CHANGELOG.md:315: this line is not a typed entry, - <Type>: <text>, so the release cannot group it
error: release of halvo aborted by non-zero return of prerelease hook."
  network_case connection-refused yes 'error: io: Connection refused (os error 61)' 'error: io: Connection refused (os error 61)'
  grep -q 'check that this computer is online and run the same command again' "$work/out-connection-refused" && got=PASS || got=FAIL
  verdict connection-refused-says-what-to-do PASS "$got"
  STUB_STATUS=1 STUB_TEXT='error: the pre-release hook exited 1' release_case other-failure-prints-its-error-line 1 yes 'error: the pre-release hook exited 1' --dry-run
  grep -q index.crates.io "$work/out-other-failure-prints-its-error-line" && got=FAIL || got=PASS
  verdict other-failure-names-no-network PASS "$got"
  repository heading '### Fixed
- Mend the fault'
  release_case heading-inside-unreleased 1 no 'breaks a rule' --dry-run
  refusal_case() {
    case_name=$1 want=$2
    mkdir -p "$work/refusal-$case_name/scripts"
    printf '%s\n' '#!/bin/sh' "$3" >"$work/refusal-$case_name/scripts/release.sh"
    git init -q "$work/refusal-$case_name" && git -C "$work/refusal-$case_name" add -A && git -C "$work/refusal-$case_name" commit -q -m 'Release 1.2.3' || exit 2
    got=FAIL
    release_commit_refusal "$work/refusal-$case_name" >"$work/refusal-$case_name.out" && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  refusal_case release-commit-refused-with-the-no-entry-line PASS "echo 'release.sh: ## [Unreleased] in CHANGELOG.md holds no entry, so there is nothing to release' >&2; exit 1"
  refusal_case release-commit-no-entry-line-with-exit-0 FAIL "echo 'release.sh: ## [Unreleased] in CHANGELOG.md holds no entry, so there is nothing to release' >&2; exit 0"
  refusal_case release-commit-refused-with-another-line FAIL "echo 'release.sh: cargo-release failed: error: io: Connection refused' >&2; exit 1"
  refusal_case release-commit-passed-by-the-dry-run FAIL "echo 'next version: 1.2.4'"
  refusal_case release-commit-refused-after-a-tag FAIL "git tag v1.2.4; echo 'release.sh: ## [Unreleased] in CHANGELOG.md holds no entry, so there is nothing to release' >&2; exit 1"
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
  refused_before_cargo_release line-below-a-released-section 'differ from those at v1.2.3' --dry-run
  repository crlf-after-a-release "$typed
## [1.2.3] - 2026-11-01

- Added: Add the store"
  git -C "$repo" tag v1.2.3
  sed 's/$/\r/' "$repo/CHANGELOG.md" >"$repo/CHANGELOG.crlf" && mv "$repo/CHANGELOG.crlf" "$repo/CHANGELOG.md"
  git -C "$repo" commit -q -a -m 'Check out the changelog with CRLF'
  release_case second-release-in-a-crlf-tree 0 yes 'next version: 1.2.4' --dry-run
  repository heading-of-the-next-version "$typed
## [1.2.4] - 2020-01-01

- Added: Add the store"
  refused_before_cargo_release heading-of-the-next-version-exists 'remove the heading ## [1.2.4]' --dry-run
  repository behind-origin "$typed"
  git -C "$repo" checkout -q -B main
  git clone -q --bare "$repo" "$work/origin-of-behind.git" || exit 2
  git -C "$repo" remote add origin "$work/origin-of-behind.git"
  git -C "$repo" commit -q --allow-empty -m 'A commit that origin gets'
  git -C "$repo" push -q origin main
  git -C "$repo" checkout -q -B main HEAD~1
  refused_before_cargo_release main-one-commit-behind-origin 'run git pull --ff-only origin main, then run this again' --execute
  git -C "$repo" checkout -q -B main origin/main
  git -C "$repo" commit -q --allow-empty -m 'A commit that origin lacks'
  refused_before_cargo_release main-one-commit-ahead-of-origin 'main here holds 1 commits that origin' --execute
  git -C "$repo" checkout -q -B main HEAD~1
  git clone -q "$work/origin-of-behind.git" "$work/second-clone-of-behind" || exit 2
  git -C "$work/second-clone-of-behind" commit -q --allow-empty -m 'A commit that this clone lacks'
  git -C "$work/second-clone-of-behind" push -q origin HEAD:main
  refused_before_cargo_release newest-commit-of-origin-absent-here 'run git fetch origin main and compare' --execute
  git -C "$repo" fetch -q origin
  git -C "$repo" commit -q --allow-empty -m 'A commit beside the newest of origin'
  refused_before_cargo_release main-diverged-from-origin 'have diverged' --execute
  git -C "$repo" checkout -q -B main origin/main
  STUB_ACTION=commit-tag
  export STUB_ACTION
  release_case main-at-the-newest-commit-of-origin 0 yes 'next version: 1.2.4' --execute
  unset STUB_ACTION
  mkdir -p "$work/gitbin"
  STUB_REAL_GIT=$(command -v git)
  export STUB_REAL_GIT
  cat >"$work/gitbin/git" <<'GITSTUB'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_GIT_LOG"
exec "$STUB_REAL_GIT" "$@"
GITSTUB
  chmod +x "$work/gitbin/git"
  logged_case() {
    case_name=$1
    shift
    status=0
    (cd "$repo" && PATH="$work/gitbin:$work/bin:$PATH" STUB_ACTION=commit-tag STUB_LOG=$work/argv-$case_name STUB_GIT_LOG=$work/git-$case_name CARGO_HOME=$work/no-cargo-home sh scripts/release.sh "$@") >"$work/out-$case_name" 2>&1 || status=$?
    : >>"$work/argv-$case_name"
    : >>"$work/git-$case_name"
  }
  repository no-remote "$typed"
  git -C "$repo" checkout -q -B main
  logged_case no-remote-makes-no-ls-remote --execute
  got=FAIL
  [ "$status" = 0 ] && ! grep -q -w ls-remote "$work/git-no-remote-makes-no-ls-remote" && got=PASS
  verdict no-remote-makes-no-ls-remote PASS "$got"
  repository no-level "$typed"
  logged_case no-level-gives-patch --dry-run
  got=FAIL
  [ "$status" = 0 ] && grep -q -x 'release version patch' "$work/argv-no-level-gives-patch" && grep -q '^release patch --config' "$work/argv-no-level-gives-patch" && got=PASS
  verdict no-level-gives-patch PASS "$got"
  repository pushes-nothing "$typed"
  git -C "$repo" checkout -q -B main
  git clone -q --bare "$repo" "$work/origin-of-pushes-nothing.git" || exit 2
  git -C "$repo" remote add origin "$work/origin-of-pushes-nothing.git"
  git -C "$repo" fetch -q origin
  logged_case no-call-pushes --execute
  got=FAIL
  [ "$status" = 0 ] && grep -q -w ls-remote "$work/git-no-call-pushes" && ! grep -q -w push "$work/git-no-call-pushes" "$work/argv-no-call-pushes" && got=PASS
  verdict no-call-pushes PASS "$got"
  execute_case() {
    case_name=$1
    shift
    repository "$case_name" "$typed"
    git -C "$repo" checkout -q -B main
    recorded=$(git -C "$repo" rev-parse HEAD)
    log=$work/argv-$case_name
    status=0
    (cd "$repo" && env "$@" PATH="$work/bin:$PATH" STUB_LOG="$log" CARGO_HOME="$work/no-cargo-home" sh scripts/release.sh --execute) >"$work/out-$case_name" 2>&1 || status=$?
    : >>"$log"
    head_now=$(git -C "$repo" rev-parse HEAD)
    tracked_now=$(git -C "$repo" status --porcelain --untracked-files=no)
    tags_now=$(git -C "$repo" tag --list)
  }
  put_back() {
    got=FAIL
    [ "$status" = 1 ] && [ "$head_now" = "$recorded" ] && [ -z "$tracked_now" ] && [ -z "$tags_now" ] &&
      grep -q -F -e "$2" "$work/out-$1" && grep -q 'are undone; nothing was released' "$work/out-$1" && got=PASS
    verdict "$1" PASS "$got"
  }
  execute_case hook-refuses-the-commit STUB_ACTION=touch STUB_STATUS=1 'STUB_TEXT=pre-commit: refused for a case'
  put_back hook-refuses-the-commit 'cargo-release failed: pre-commit: refused for a case'
  execute_case commits-then-fails STUB_ACTION=commit STUB_STATUS=1 'STUB_TEXT=error: a stand-in failure after the commit'
  put_back commits-then-fails 'cargo-release failed: error: a stand-in failure after the commit'
  execute_case tag-fails-after-the-commit STUB_ACTION=commit-tag STUB_STATUS=1 'STUB_TEXT=error: a stand-in failure after the tag'
  put_back tag-fails-after-the-commit 'cargo-release failed: error: a stand-in failure after the tag'
  execute_case another-tag-at-the-release-commit STUB_ACTION=commit-tag STUB_TAG=halvo-v1.2.4
  put_back another-tag-at-the-release-commit 'the tag at the release commit is halvo-v1.2.4, not v1.2.4'
  execute_case failure-that-changed-nothing STUB_STATUS=1 'STUB_TEXT=error: a stand-in failure before any change'
  got=FAIL
  [ "$status" = 1 ] && [ "$head_now" = "$recorded" ] && [ -z "$tracked_now" ] && grep -q 'nothing was changed' "$work/out-failure-that-changed-nothing" && got=PASS
  verdict failure-that-changed-nothing PASS "$got"
  mkdir -p "$work/home-without-a-committer"
  printf '[user]\n\tuseConfigOnly = true\n' >"$work/home-without-a-committer/.gitconfig"
  execute_case no-committer -u GIT_AUTHOR_NAME -u GIT_AUTHOR_EMAIL -u GIT_COMMITTER_NAME -u GIT_COMMITTER_EMAIL -u EMAIL \
    HOME="$work/home-without-a-committer" GIT_CONFIG_GLOBAL="$work/home-without-a-committer/.gitconfig" GIT_CONFIG_NOSYSTEM=1
  got=PASS
  if [ "$status" = 1 ] && grep -q "set your name and email in git's global configuration" "$work/out-no-committer" &&
    grep -q 'nothing was changed' "$work/out-no-committer" && ! grep -q -E '^release (patch|minor|major) --config' "$work/argv-no-committer"; then got=FAIL; fi
  verdict no-committer FAIL "$got"
  repository tracked-change-before-the-run "$typed"
  git -C "$repo" checkout -q -B main
  printf '\n' >>"$repo/scripts/check-changelog.sh"
  refused_before_cargo_release tracked-change-before-the-run 'tracked files differ from HEAD' --execute
  changed_still=$(git -C "$repo" status --porcelain --untracked-files=no)
  [ -n "$changed_still" ] && got=PASS || got=FAIL
  verdict tracked-change-before-the-run-is-kept PASS "$got"
  repository release-commit-without-its-tag "$typed"
  git -C "$repo" checkout -q -B main
  git -C "$repo" commit -q --allow-empty -m 'Release 1.2.3'
  refused_before_cargo_release release-commit-without-its-tag 'git reset --hard HEAD~1' --dry-run
  git -C "$repo" tag v1.2.3
  release_case release-commit-with-its-tag 0 yes 'next version: 1.2.4' --dry-run
  repository heading-in-a-shallow-clone "$typed
## [1.2.4] - 2020-01-01

- Added: Add the store"
  git -C "$repo" checkout -q -B main
  git -C "$repo" commit -q --allow-empty -m 'A second commit'
  git clone -q --depth 1 "file://$repo" "$work/shallow-with-a-heading" || exit 2
  repo=$work/shallow-with-a-heading
  { git -C "$repo" for-each-ref; git -C "$repo" tag --list; cat "$repo/.git/shallow"; } >"$work/heading-shallow-before"
  refused_before_cargo_release heading-refuses-a-shallow-execute-before-any-fetch 'remove the heading ## [1.2.4]' --execute
  { git -C "$repo" for-each-ref; git -C "$repo" tag --list; cat "$repo/.git/shallow"; } >"$work/heading-shallow-after"
  cmp -s "$work/heading-shallow-before" "$work/heading-shallow-after" && got=PASS || got=FAIL
  verdict heading-refusal-made-no-fetch PASS "$got"
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
  refused_before_cargo_release dry-run-in-a-shallow-clone 'git fetch --unshallow --tags origin' --dry-run
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
  trailing_after=$(printf '%s\n' "# Changelog" "" "## [Unreleased] " "" "### Added" "" "- Add the feature" "" "### Fixed" "" "- Mend the first fault" "- Mend the second fault")
  group_case heading-with-a-trailing-space PASS false 1.2.4 "$trailing_before" "$trailing_after"
  crlf_before=$(printf '%s\n' "# Changelog" "" "## [Unreleased]" "" "## [1.2.4] - 2026-11-14" "" "$typed" | awk '{ printf "%s\r\n", $0 }')
  crlf_after=$(printf '%s\n' "# Changelog" "" "## [Unreleased]" "" "## [1.2.4] - 2026-11-14" "" "### Added" "" "- Add the feature" "" "### Fixed" "" "- Mend the first fault" "- Mend the second fault" | awk '{ printf "%s\r\n", $0 }')
  group_case work-tree-with-crlf PASS false 1.2.4 "$crlf_before" "$crlf_after"
  outside_before=$(printf '%s\n' "# Changelog " "" "## [Unreleased]" "" "## [1.2.4] - 2026-11-14" "" "$typed" "" "## [1.2.3] - 2026-11-01" "" "- Added: Add the store ")
  outside_after=$(printf '%s\n' "# Changelog " "" "## [Unreleased]" "" "## [1.2.4] - 2026-11-14" "" "### Added" "" "- Add the feature" "" "### Fixed" "" "- Mend the first fault" "- Mend the second fault" "" "## [1.2.3] - 2026-11-01" "" "- Added: Add the store ")
  group_case lines-outside-the-section-are-kept PASS false 1.2.4 "$outside_before" "$outside_after"
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
  texts_case() {
    case_name=$1 want=$2
    printf '%s\n' "$3" >"$work/texts-$case_name.before"
    printf '%s\n' "$4" >"$work/texts-$case_name.after"
    unreleased_texts "$work/texts-$case_name.before" >"$work/texts-$case_name.sorted"
    got=FAIL
    released_section_defect "$work/texts-$case_name.after" 1.2.4 "$work/texts-$case_name.sorted" >"$work/texts-$case_name.out" && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  grouped_tail='## [Unreleased]

## [1.2.4] - 2026-11-14

### Fixed

- Mend the fault'
  blank_entry=$(printf '%s\n' '## [Unreleased]' '' '- Fixed: Mend the fault  ')
  texts_case entry-that-ends-in-two-blanks PASS "$blank_entry" "$grouped_tail"
  tab_entry=$(printf '%s\n' '## [Unreleased]' ''; printf -- '- Fixed: Mend the fault\t\n')
  texts_case entry-that-ends-in-a-tab PASS "$tab_entry" "$grouped_tail"
  crlf_entry=$(printf '%s\n' '## [Unreleased]' '' '- Fixed: Mend the fault' | awk '{ printf "%s\r\n", $0 }')
  crlf_tail=$(printf '%s\n' "$grouped_tail" | awk '{ printf "%s\r\n", $0 }')
  texts_case changelog-with-crlf PASS "$crlf_entry" "$crlf_tail"
  other_tail=$(printf '%s\n' "$grouped_tail" | sed 's/Mend the fault/Mend another fault/')
  texts_case an-entry-that-differs FAIL "$blank_entry" "$other_tail"
  grep -q -F -e 'only before the release: Mend the fault' "$work/texts-an-entry-that-differs.out" && got=PASS || got=FAIL
  verdict the-first-entry-that-differs-is-named PASS "$got"
  printf '%s\n' '   Upgrading halvo from 1.2.3 to 1.2.4' "release.sh: cargo-release failed: error: for \`which is \` in '../README.md', at least 1 replacements expected, found 0" '' >"$work/release-output.txt"
  line=$(with_release_line 'sh scripts/release.sh --dry-run exited 1 on a named branch' "$work/release-output.txt")
  case $line in
    *"release.sh: cargo-release failed: error: for"*"at least 1 replacements expected"*) got=PASS ;;
    *) got=FAIL ;;
  esac
  verdict fail-line-carries-the-last-line-of-release-sh PASS "$got"
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
  section_case a-heading-of-no-type FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

### Added

- Add the feature

### Mended

- Mend the first fault
- Mend the second fault"
  section_case an-empty-heading-of-no-type-first FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

### Mended

### Added

- Add the feature

### Fixed

- Mend the first fault
- Mend the second fault"
  section_case an-entry-above-every-heading FAIL "## [Unreleased]

## [1.2.4] - 2026-11-14

- Add the feature

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
