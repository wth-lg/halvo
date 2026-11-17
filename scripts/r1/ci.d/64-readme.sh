#!/bin/sh
# jobs: macos
# rule: The README keeps its four fixed headings and one version anchor, names only scripts that exist, and says under Prerequisites that the command uses no sudo.
set -u
name=64-readme

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

anchor_pattern() {
  sed -n 's/.*file = "\.\.\/README\.md", search = "\([^"]*\)".*/\1/p' "$1" >"$work/anchor-raw" || return 1
  entries=$(grep -c '' "$work/anchor-raw") || entries=0
  [ "$entries" = 1 ] || return 1
  sed 's/\\\\/\\/g' "$work/anchor-raw"
}

readme_defects() {
  ANCHOR=$2 VERSION=$3 awk '
    BEGIN { re = ENVIRON["ANCHOR"]; version = ENVIRON["VERSION"] }
    { sub(/\r$/, "") }
    /^```/ { fence = !fence }
    !fence && /^## / {
      heading = substr($0, 4)
      sub(/[ \t]+$/, "", heading)
      section = heading
      seen[heading]++
      if (!(heading in first)) first[heading] = NR
    }
    !fence && section == "Prerequisites" && /no sudo/ { no_sudo++ }
    {
      rest = $0
      while (rest != "" && match(rest, re) && RLENGTH > 0) {
        anchors++
        anchor_section = section
        anchor_text = substr(rest, RSTART, RLENGTH)
        rest = substr(rest, RSTART + RLENGTH)
      }
    }
    END {
      n = split("Prerequisites Build Version Release", fixed, " ")
      present = 0
      for (i = 1; i <= n; i++) {
        count = seen[fixed[i]] + 0
        if (count != 1) printf "problem: README.md holds %d headings ## %s, not 1\n", count, fixed[i]
        else present++
      }
      if (present == n) {
        for (i = 2; i <= n; i++) if (first[fixed[i]] < first[fixed[i - 1]]) {
          printf "problem: ## %s stands before ## %s; the fixed headings are Prerequisites, Build, Version and Release in that order\n", fixed[i], fixed[i - 1]
        }
      }
      if (!no_sudo) print "problem: the Prerequisites section of README.md does not say that the command uses no sudo"
      if (anchors != 1) {
        printf "problem: README.md holds %d matches of the version anchor of src-tauri/release.toml, not 1\n", anchors
      } else if (anchor_section != "Version") {
        printf "problem: the version anchor stands under ## %s, not under ## Version\n", anchor_section
      } else if (match(anchor_text, /[0-9]+\.[0-9]+\.[0-9]+/) && substr(anchor_text, RSTART, RLENGTH) != version) {
        printf "problem: the version anchor names %s, and sh scripts/r1/ci.d/61-version.sh --print gives %s\n", substr(anchor_text, RSTART, RLENGTH), version
      }
    }
  ' "$1"
}

script_defects() {
  readme=$1
  listing=$2
  grep -o -E '[A-Za-z0-9_./-]+\.(sh|js|mjs|py)([^A-Za-z0-9_]|$)' "$readme" >"$work/tokens" || :
  sed 's/[^A-Za-z0-9_]$//' "$work/tokens" >"$work/paths"
  while IFS= read -r path; do
    path=${path#./}
    case $path in
      */*) found=$(grep -c -x -F -e "$path" "$listing") || found=0 ;;
      *) found=$(NAME=$path awk -F/ '$NF == ENVIRON["NAME"] { n++ } END { print n + 0 }' "$listing") || found=0 ;;
    esac
    [ "$found" != 0 ] || printf 'problem: README.md names %s, which is no tracked file\n' "$path"
  done <"$work/paths"
  grep -o -E 'run-ci-d\.sh +(macos|windows)( +--self-test)?( +[0-9][0-9]-[a-z0-9-]+)+' "$readme" >"$work/runs" || :
  tr ' ' '\n' <"$work/runs" >"$work/words"
  grep -E '^[0-9][0-9]-[a-z0-9-]+$' "$work/words" >"$work/checks" || :
  while IFS= read -r check_name; do
    found=$(grep -c -E "^scripts/[^/]+/ci\\.d/$check_name\\.sh\$" "$listing") || found=0
    [ "$found" != 0 ] || printf 'problem: README.md names the check %s, and no scripts/*/ci.d/%s.sh is tracked\n' "$check_name" "$check_name"
  done <"$work/checks"
}

measure() {
  root=$1
  cd "$root" || fail "cannot enter the checkout"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  git ls-files >"$work/listing" || fail "git ls-files failed"
  [ -f README.md ] || fail "README.md does not exist"
  [ -f src-tauri/release.toml ] || fail "src-tauri/release.toml does not exist"
  cp README.md "$work/README.md" || fail "cannot read README.md"
  cp src-tauri/release.toml "$work/release.toml" || fail "cannot read src-tauri/release.toml"
  before=$(git status --porcelain --untracked-files=no) || fail "git status failed"
  version_status=0
  version=$(sh scripts/r1/ci.d/61-version.sh --print 2>"$work/version.err") || version_status=$?
  after=$(git status --porcelain --untracked-files=no) || fail "git status failed"
  [ "$before" = "$after" ] || fail "a tracked file differs after sh scripts/r1/ci.d/61-version.sh --print"
  [ "$version_status" = 0 ] || fail "sh scripts/r1/ci.d/61-version.sh --print exited $version_status"
  case $version in
    '' | *[!0-9.]*) fail "sh scripts/r1/ci.d/61-version.sh --print gives no single version" ;;
  esac
  pattern=$(anchor_pattern "$work/release.toml") || fail "src-tauri/release.toml holds no single pre-release-replacements entry for ../README.md"
  readme_defects "$work/README.md" "$pattern" "$version" >"$work/defects" || fail "awk failed"
  script_defects "$work/README.md" "$work/listing" >>"$work/defects"
  cat "$work/defects"
  problems=$(grep -c '^problem' "$work/defects") || problems=0
  printf 'problems %s\n' "$problems"
  [ "$problems" = 0 ] || fail "$problems problems in README.md"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(CI_D_REPO=$work/$1 sh "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

good_readme() {
  printf '# Product\n\nIntro.\n\n## Prerequisites\n\n\140\140\140sh\nsh scripts/prerequisites.sh\n\140\140\140\n\nIt installs as the current user, with no sudo.\n\n'
  printf '## Build\n\nRun \140sh scripts/ci/run-ci-d.sh macos 10-format 11-clippy\140 and tools/x/scripts/fetch.sh.\n\nThe config is src-tauri/tauri.conf.json.\n\n'
  printf '\140\140\140sh\n## Version\n\140\140\140\n\n'
  printf '## Version\n\nThe version is in Cargo.toml, which is \1401.2.3\140 now.\n\n'
  printf '## Release\n\nRun release.sh.\n'
}

fixture() {
  mkdir -p "$work/$1/scripts/ci" "$work/$1/scripts/r1/ci.d" "$work/$1/tools/x/scripts" "$work/$1/src-tauri" || exit 2
  good_readme >"$work/$1/README.md" || exit 2
  printf '[package]\n' >"$work/$1/Cargo.toml" || exit 2
  for file in scripts/prerequisites.sh scripts/release.sh scripts/ci/run-ci-d.sh scripts/r1/ci.d/10-format.sh scripts/r1/ci.d/11-clippy.sh tools/x/scripts/fetch.sh; do
    printf '#!/bin/sh\n' >"$work/$1/$file" || exit 2
  done
  printf '#!/bin/sh\nprintf "1.2.3\\n"\n' >"$work/$1/scripts/r1/ci.d/61-version.sh" || exit 2
  printf 'pre-release-replacements = [\n  { file = "../CHANGELOG.md", search = "## \\\\[Unreleased\\\\]", replace = "x", exactly = 1 },\n  { file = "../README.md", search = "which is \140[0-9]+\\\\.[0-9]+\\\\.[0-9]+\140 now", replace = "which is \140{{version}}\140 now", exactly = 1 },\n]\n' >"$work/$1/src-tauri/release.toml" || exit 2
}

track() {
  git -C "$work/$1" init -q || exit 2
  git -C "$work/$1" add -A || exit 2
}

edit() {
  sed "$2" "$work/$1/$3" >"$work/$1/$3.new" || exit 2
  mv "$work/$1/$3.new" "$work/$1/$3" || exit 2
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  track good
  check good PASS 'problems 0'

  fixture crlf
  edit crlf 's/$/\r/' README.md
  track crlf
  check crlf PASS 'problems 0'

  fixture another-heading
  printf '\n## Notes\n\nMore.\n' >>"$work/another-heading/README.md" || exit 2
  track another-heading
  check another-heading PASS 'problems 0'

  fixture no-release
  edit no-release 's/^## Release$/## Shipping/' README.md
  track no-release
  check no-release FAIL 'README.md holds 0 headings ## Release, not 1'

  fixture second-version-heading
  printf '\n## Version\n\nAgain.\n' >>"$work/second-version-heading/README.md" || exit 2
  track second-version-heading
  check second-version-heading FAIL 'README.md holds 2 headings ## Version, not 1'

  fixture build-after-version
  edit build-after-version 's/^## Build$/## Swap/;s/^## Version$/## Build/;s/^## Swap$/## Version/' README.md
  track build-after-version
  check build-after-version FAIL '## Version stands before ## Build'

  fixture two-anchors
  printf '\nIt was 1.2.3, which is \1401.2.3\140 now.\n' >>"$work/two-anchors/README.md" || exit 2
  track two-anchors
  check two-anchors FAIL 'README.md holds 2 matches of the version anchor of src-tauri/release.toml, not 1'

  fixture no-anchor
  edit no-anchor 's/which is .1\.2\.3. now/is 1.2.3/' README.md
  track no-anchor
  check no-anchor FAIL 'README.md holds 0 matches of the version anchor of src-tauri/release.toml, not 1'

  fixture anchor-under-build
  edit anchor-under-build '/which is .1\.2\.3. now/d' README.md
  bt=$(printf '\140')
  edit anchor-under-build "s/^Run ${bt}sh scripts/It is 1.2.3, which is ${bt}1.2.3${bt} now. Run ${bt}sh scripts/" README.md
  track anchor-under-build
  check anchor-under-build FAIL 'the version anchor stands under ## Build, not under ## Version'

  fixture other-version
  printf '#!/bin/sh\nprintf "1.2.4\\n"\n' >"$work/other-version/scripts/r1/ci.d/61-version.sh" || exit 2
  track other-version
  check other-version FAIL 'the version anchor names 1.2.3, and sh scripts/r1/ci.d/61-version.sh --print gives 1.2.4'

  fixture version-fails
  printf '#!/bin/sh\nprintf "FAIL 61-version (cargo metadata failed)\\n"\nexit 1\n' >"$work/version-fails/scripts/r1/ci.d/61-version.sh" || exit 2
  track version-fails
  check version-fails FAIL 'sh scripts/r1/ci.d/61-version.sh --print exited 1'

  fixture version-writes
  printf '#!/bin/sh\nprintf "x\\n" >>README.md\nprintf "1.2.3\\n"\n' >"$work/version-writes/scripts/r1/ci.d/61-version.sh" || exit 2
  track version-writes
  check version-writes FAIL 'a tracked file differs after sh scripts/r1/ci.d/61-version.sh --print'

  fixture part-of-a-path
  printf '\nSee ci/run-ci-d.sh.\n' >>"$work/part-of-a-path/README.md" || exit 2
  track part-of-a-path
  check part-of-a-path FAIL 'README.md names ci/run-ci-d.sh, which is no tracked file'

  fixture no-sudo
  edit no-sudo 's/, with no sudo\././' README.md
  track no-sudo
  check no-sudo FAIL 'problem: the Prerequisites section of README.md does not say that the command uses no sudo'

  fixture windows-form
  printf '\nRun sh scripts/ci/run-ci-d.sh windows 99-nothing too.\n' >>"$work/windows-form/README.md" || exit 2
  track windows-form
  check windows-form FAIL 'README.md names the check 99-nothing, and no scripts/*/ci.d/99-nothing.sh is tracked'

  fixture self-test-form
  printf '\nRun sh scripts/ci/run-ci-d.sh macos --self-test 99-nothing too.\n' >>"$work/self-test-form/README.md" || exit 2
  track self-test-form
  check self-test-form FAIL 'README.md names the check 99-nothing, and no scripts/*/ci.d/99-nothing.sh is tracked'

  for ending in js mjs py; do
    fixture "missing-$ending"
    printf '\nRun tools/x/missing.%s too.\n' "$ending" >>"$work/missing-$ending/README.md" || exit 2
    track "missing-$ending"
    check "missing-$ending" FAIL "README.md names tools/x/missing.$ending, which is no tracked file"
  done

  fixture two-anchors-on-one-line
  printf '\nIt was 1.2.3, which is \1401.2.3\140 now, which is \1401.2.3\140 now.\n' >>"$work/two-anchors-on-one-line/README.md" || exit 2
  track two-anchors-on-one-line
  check two-anchors-on-one-line FAIL 'README.md holds 3 matches of the version anchor of src-tauri/release.toml, not 1'

  fixture no-readme-entry
  edit no-readme-entry '/README/d' src-tauri/release.toml
  track no-readme-entry
  check no-readme-entry FAIL 'src-tauri/release.toml holds no single pre-release-replacements entry for ../README.md'

  fixture missing-script
  printf '\nRun scripts/r1-gate/run.sh too.\n' >>"$work/missing-script/README.md" || exit 2
  track missing-script
  check missing-script FAIL 'README.md names scripts/r1-gate/run.sh, which is no tracked file'

  fixture untracked-script
  printf '\nRun scripts/r1-gate/run.sh too.\n' >>"$work/untracked-script/README.md" || exit 2
  track untracked-script
  mkdir -p "$work/untracked-script/scripts/r1-gate" || exit 2
  printf '#!/bin/sh\n' >"$work/untracked-script/scripts/r1-gate/run.sh" || exit 2
  check untracked-script FAIL 'README.md names scripts/r1-gate/run.sh, which is no tracked file'

  fixture missing-bare-script
  edit missing-bare-script 's/Run release.sh/Run publish.sh/' README.md
  track missing-bare-script
  check missing-bare-script FAIL 'README.md names publish.sh, which is no tracked file'

  fixture missing-check
  edit missing-check 's/10-format 11-clippy/10-format 99-nothing/' README.md
  track missing-check
  check missing-check FAIL 'README.md names the check 99-nothing, and no scripts/*/ci.d/99-nothing.sh is tracked'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure "$CI_D_REPO" ;;
  *) exit 64 ;;
esac
