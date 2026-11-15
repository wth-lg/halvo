#!/bin/sh
# jobs: macos windows
# rule: Every workspace package reports one version in cargo metadata, no workflow and no ci.d script compares --version output with a literal, on a tag ref the tag is v<that version>, and each binary given to --check exits 0 and prints that version.
set -u
name=61-version

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

version_from() {
  members=$(tr -d '\n' <"$1" | sed -n 's/.*"workspace_members":\[\([^]]*\)\].*/\1/p')
  [ -n "$members" ] || { printf 'cargo metadata lists no workspace member\n'; return 1; }
  member_count=$(printf '%s\n' "$members" | tr ',' '\n' | grep -c '"')
  packages=$(tr -d '\n' <"$1" | grep -o -E '\{"name":"[^"]*","version":"[^"]*","id":"' | sed 's/^{"name":"\([^"]*\)","version":"\([^"]*\)".*/\1 \2/')
  package_count=$(printf '%s\n' "$packages" | grep -c ' ')
  [ "$package_count" -eq "$member_count" ] || {
    printf 'cargo metadata lists %s workspace members and this script reads %s packages\n' "$member_count" "$package_count"
    return 1
  }
  versions=$(printf '%s\n' "$packages" | sed 's/^[^ ]* //' | sort -u)
  version_count=$(printf '%s\n' "$versions" | grep -c .)
  if [ "$version_count" -ne 1 ]; then
    lowest=$(printf '%s\n' "$versions" | head -n 1)
    others=$(printf '%s\n' "$packages" | awk -v first="$lowest" '$2 != first { printf "%s%s=%s", sep, $1, $2; sep = " " }')
    printf 'the workspace packages report %s versions; packages that differ from the lowest: %s\n' "$version_count" "$others"
    return 1
  fi
  case $versions in
    *[!0-9A-Za-z.+-]*|'') printf 'the version is not a semver string\n'; return 1 ;;
  esac
  printf '%s\n' "$versions"
}

literal_compares() {
  for file in "$@"; do
    grep -n -E -e '--version[^#]*(=|-eq|-ne)[[:space:]]*["'\'']?v?[0-9]+\.[0-9]+\.[0-9]+' "$file" | sed "s|^\([0-9]*\):.*|$file:\1|"
  done
}

tag_defect() {
  [ "${GITHUB_REF_TYPE:-}" = tag ] || return 0
  [ "${GITHUB_REF_NAME:-}" = "v$1" ] && return 0
  printf 'the tag of this run is not v%s, the derived version\n' "$1"
  return 1
}

binary_defect() {
  [ -f "$1" ] || { printf '%s does not exist\n' "$2"; return 1; }
  printed=$("$1" --version 2>/dev/null)
  status=$?
  [ "$status" -eq 0 ] || { printf '%s --version exited %s\n' "$2" "$status"; return 1; }
  [ "$printed" = "$3" ] || { printf '%s --version does not print the derived version\n' "$2"; return 1; }
}

derive() {
  root=${CI_D_REPO:-}
  [ -n "$root" ] || root=$(git rev-parse --show-toplevel) || exit 2
  cd "$root" || exit 2
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  before=$(git status --porcelain) || fail "git status failed"
  if ! cargo metadata --locked --no-deps --format-version 1 >"$work/metadata.json" 2>"$work/metadata.err"; then
    cause=$(grep -m 1 '^error' "$work/metadata.err")
    fail "cargo metadata --locked --no-deps failed: $cause"
  fi
  after=$(git status --porcelain) || fail "git status failed"
  [ "$before" = "$after" ] || fail "a tracked file differs after cargo metadata"
  found=$(version_from "$work/metadata.json") || fail "$found"
  version=$found
}

measure() {
  derive
  files=$(git ls-files -- .github/workflows 'scripts/*/ci.d/*.sh') || fail "git ls-files failed"
  [ -n "$files" ] || fail "git ls-files lists no workflow and no ci.d script"
  # shellcheck disable=SC2086
  compares=$(literal_compares $files)
  places=$(printf '%s\n' "$compares" | tr '\n' ' ')
  [ -z "$compares" ] || fail "--version output compared with a literal at $places"
  found=$(tag_defect "$version") || fail "$found"
  packages=$(tr -d '\n' <"$work/metadata.json" | grep -o -E '\{"name":"[^"]*","version":"[^"]*","id":"' | grep -c .)
  file_count=$(printf '%s\n' "$files" | grep -c .)
  printf 'workspace packages: %s, versions: 1, workflow and ci.d files read: %s\n' "$packages" "$file_count"
  printf 'PASS %s\n' "$name"
}

check_binaries() {
  derive
  found=$(tag_defect "$version") || fail "$found"
  for binary in "$@"; do
    label=$(basename "$binary")
    found=$(binary_defect "$binary" "$label" "$version") || fail "$found"
    printf '%s --version: the derived version\n' "$label"
  done
  printf 'PASS %s\n' "$name"
}

metadata_fixture() {
  printf '{"packages":['
  sep=
  for pair in "$@"; do
    printf '%s{"name":"%s","version":"%s","id":"path+file:///w/%s#%s","dependencies":[{"name":"serde","source":"registry","req":"^1"}]}' "$sep" "${pair%=*}" "${pair#*=}" "${pair%=*}" "${pair#*=}"
    sep=,
  done
  printf '],"workspace_members":['
  sep=
  for pair in "$@"; do
    printf '%s"path+file:///w/%s#%s"' "$sep" "${pair%=*}" "${pair#*=}"
    sep=,
  done
  printf ']}\n'
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  metadata_case() {
    case_name=$1 want=$2 expected=$3
    shift 3
    metadata_fixture "$@" >"$work/$case_name.json"
    got=FAIL
    found=$(version_from "$work/$case_name.json") && [ "$found" = "$expected" ] && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  metadata_case one-version PASS 1.2.3 halvo=1.2.3 halvo-capture=1.2.3 halvo-control=1.2.3
  metadata_case one-member-differs FAIL 1.2.3 halvo=1.2.3 halvo-capture=1.2.4 halvo-control=1.2.3
  metadata_case only-the-first-differs FAIL 1.2.4 halvo=1.2.4 halvo-capture=1.2.3
  metadata_case one-package PASS 9.9.9 halvo=9.9.9
  : >"$work/empty.json"
  got=FAIL
  version_from "$work/empty.json" >/dev/null && got=PASS
  verdict empty-metadata FAIL "$got"
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 | sed 's/{"name":"halvo-capture","version"/{"version":"1.2.3","name":"halvo-capture","x"/' >"$work/unread.json"
  got=FAIL
  version_from "$work/unread.json" >/dev/null && got=PASS
  verdict a-package-that-this-script-cannot-read FAIL "$got"
  metadata_fixture halvo=1.2.3 | sed 's/"1.2.3"/"1.2 3"/g' >"$work/space.json"
  got=FAIL
  version_from "$work/space.json" >/dev/null && got=PASS
  verdict a-version-with-a-blank FAIL "$got"

  compare_case() {
    case_name=$1 want=$2 text=$3
    printf '%s\n' "$text" >"$work/$case_name.yml"
    got=PASS
    compares=$(literal_compares "$work/$case_name.yml")
    [ -z "$compares" ] || got=FAIL
    verdict "$case_name" "$want" "$got"
  }
  flag=--version
  compare_case workflow-step-of-the-base FAIL "        run: test \"\$(target/release/halvo.exe $flag)\" = 9.9.9"
  compare_case not-equal-in-a-script FAIL "[ \"\$(halvo $flag)\" != '1.2.3' ] && exit 1"
  compare_case double-equals-with-a-v FAIL "[[ \$(halvo $flag) == v1.2.3 ]]"
  compare_case derived-value PASS "test \"\$(halvo $flag)\" = \"\$(sh scripts/r1/ci.d/61-version.sh --print)\""
  compare_case literal-on-another-line PASS "pinned_version=1.7.12"
  compare_case version-printed-not-compared PASS "printf 'tool: %s\\n' \"\$(tool $flag)\" # 1.2.3"

  stub() {
    printf '#!/bin/sh\nprintf "%%s\\n" "%s"\nexit %s\n' "$2" "$3" >"$work/$1"
    chmod +x "$work/$1"
  }
  binary_case() {
    case_name=$1 want=$2
    got=FAIL
    binary_defect "$work/$3" "$3" 1.2.3 >/dev/null && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  stub right 1.2.3 0
  stub right-then-exit-3 1.2.3 3
  stub other 1.2.4 0
  stub with-a-v v1.2.3 0
  binary_case binary-prints-the-version PASS right
  binary_case binary-prints-the-version-and-exits-3 FAIL right-then-exit-3
  binary_case binary-prints-another-version FAIL other
  binary_case binary-prints-a-v FAIL with-a-v
  binary_case binary-missing FAIL missing

  tag_case() {
    case_name=$1 want=$2
    got=FAIL
    GITHUB_REF_TYPE=$3 GITHUB_REF_NAME=$4 tag_defect 1.2.3 >/dev/null && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  tag_case tag-of-another-version FAIL tag v9.9.9
  tag_case tag-of-the-version PASS tag v1.2.3
  tag_case tag-without-the-v FAIL tag 1.2.3
  tag_case branch-named-like-a-tag PASS branch v9.9.9
  tag_case no-ref-type PASS '' ''

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --print)
    derive
    printf '%s\n' "$version"
    ;;
  --check)
    shift
    [ $# -gt 0 ] || { printf 'usage: sh %s --check <binary>...\n' "scripts/r1/ci.d/$name.sh" >&2; exit 64; }
    check_binaries "$@"
    ;;
  '') measure ;;
  *) printf 'usage: sh %s [--print | --check <binary>... | --self-test]\n' "scripts/r1/ci.d/$name.sh" >&2; exit 64 ;;
esac
