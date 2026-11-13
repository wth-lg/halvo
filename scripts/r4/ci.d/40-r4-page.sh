#!/bin/sh
# jobs: macos windows
# rule: Every view file keeps to the rules of a view file, and the committed record of --record, whose stamp is that of the files it read, shows every state of ui/tests/fixtures/coord/ passing the six checks of the page harness and the status-region rule, which each push runs again on states that its commit picks.
set -u
name=40-r4-page
nested=self-test-of-$name
own_per_push=4
needs_per_push=4

usage() {
  printf 'FAIL %s (usage)\n' "$name"
  exit 1
}

mode=measure
case $# in
  0) ;;
  1)
    case $1 in
      --self-test) mode=self_test ;;
      --record) mode=record ;;
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac

prefix=
[ "$mode" = self_test ] && prefix=self-test:

fail() {
  printf 'FAIL %s%s (%s)\n' "$prefix" "$name" "$1"
  exit 1
}

case $0 in
  */*) here=${0%/*} ;;
  *) here=. ;;
esac
here=$(cd "$here" 2>/dev/null && pwd) || fail setup
repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  case $here in
    *?/scripts/r4/ci.d) repo=${here%/scripts/r4/ci.d} ;;
    *) fail setup ;;
  esac
fi
artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
[ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
page=$repo/ui/tests/r4/page

node_ok() {
  command -v node >/dev/null 2>&1 || return 1
  version=$(node --version 2>/dev/null) || return 1
  major=${version#v}
  major=${major%%.*}
  case $major in
    '' | *[!0123456789]*) return 1 ;;
  esac
  [ "$major" -ge 22 ]
}

inputs() {
  git -C "$repo" status --porcelain -- ui/app ui/tests tools/r4-page docs/r4/page-features.txt scripts/r4/ci.d/$name.sh >"$artifacts/inputs-$1.txt" 2>/dev/null || return 1
  [ ! -s "$artifacts/inputs-$1.txt" ]
}

tracked_same() {
  git -C "$repo" diff --quiet HEAD -- 2>/dev/null
}

acorn() {
  dir=$artifacts/acorn
  mkdir -p "$dir" || fail setup
  if [ -f "$dir/node_modules/acorn/package.json" ] && cmp -s "$page/package-lock.json" "$dir/package-lock.json"; then
    return 0
  fi
  cp "$page/package.json" "$page/package-lock.json" "$dir/" || fail setup
  (cd "$dir" && npm ci --ignore-scripts --no-audit --no-fund --cache "$dir/npm-cache" >"$artifacts/npm.log" 2>&1) || fail acorn
}

browsers() {
  if [ -z "${R4_PAGE_BROWSERS:-}" ] && [ ! -d "$repo/tools/r4-page/.browsers" ]; then
    R4_PAGE_BROWSERS=$artifacts/browsers
    export R4_PAGE_BROWSERS
    sh "$repo/tools/r4-page/fetch.sh" "$R4_PAGE_BROWSERS" >"$artifacts/fetch.log" 2>&1 || fail browser
  fi
}

last_of() {
  sed -n '$p' "$1"
}

measure() {
  node_ok || fail node
  if git -C "$repo" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    inputs before || fail dirty
    guarded=yes
  else
    guarded=no
  fi
  acorn
  node "$page/check.mjs" files --repo "$repo" --acorn "$artifacts/acorn" >"$artifacts/files.txt" 2>&1
  code=$?
  cat "$artifacts/files.txt"
  last=$(last_of "$artifacts/files.txt")
  case $code:$last in
    "0:check result=ok") ;;
    "1:check result="*) fail "${last#check result=}" ;;
    *) fail files ;;
  esac
  browsers
  [ "$guarded" = yes ] || fail setup
  commit=$(git -C "$repo" rev-parse HEAD 2>/dev/null) || fail setup
  node "$page/check.mjs" push --repo "$repo" --commit "$commit" --own "$own_per_push" --needs "$needs_per_push" >"$artifacts/page.txt" 2>"$artifacts/page-stderr.txt"
  code=$?
  cat "$artifacts/page.txt"
  last=$(last_of "$artifacts/page.txt")
  if [ "$guarded" = yes ]; then
    tracked_same || fail dirty
    inputs after || fail dirty
  fi
  case $code:$last in
    "0:check result=ok") ;;
    "1:check result="*) fail "${last#check result=}" ;;
    *) fail page ;;
  esac
  printf 'PASS %s\n' "$name"
}

record() {
  node_ok || fail node
  git -C "$repo" rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail setup
  inputs before || fail dirty
  browsers
  node "$page/check.mjs" record --repo "$repo" >"$artifacts/record.txt" 2>"$artifacts/record-stderr.txt"
  code=$?
  cat "$artifacts/record.txt"
  last=$(last_of "$artifacts/record.txt")
  case $code:$last in
    "0:check result=ok") ;;
    "1:check result="*) fail "${last#check result=}" ;;
    *) fail page ;;
  esac
  printf 'PASS %s\n' "$name"
}

fake_node() {
  mkdir -p "$work/$1" || fail setup
  if [ -n "$2" ]; then
    printf '#!/bin/sh\necho %s\n' "$2" >"$work/$1/node" || fail setup
    chmod +x "$work/$1/node" || fail setup
  fi
}

script_case() {
  label=$1
  want=$2
  shift 2
  out=$(CI_D_JOB=$nested "$@" 2>/dev/null)
  code=$?
  last=${out##*
}
  got=exit-$code
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=FAIL:${got%)}
      ;;
    "1:FAIL self-test:$name ("*")")
      got=${last#"FAIL self-test:$name ("}
      got=FAIL:${got%)}
      ;;
  esac
  line="case $label want=$want got=$got"
  if [ "$got" = "$want" ]; then
    [ "$want" = PASS ] || rejected=$((rejected + 1))
  else
    line="$line wrong"
    wrong=$((wrong + 1))
  fi
  printf '%s\n' "$line"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null) || fail setup
  [ "${CI_D_JOB:-}" = "$nested" ] && fail nested
  node_ok || fail node
  acorn
  browsers
  node "$page/check.mjs" selftest --repo "$repo" --acorn "$artifacts/acorn" >"$work/page-cases.txt" 2>"$artifacts/selftest-stderr.txt"
  code=$?
  cat "$work/page-cases.txt"
  last=$(last_of "$work/page-cases.txt")
  case $code:$last in
    "0:self-test rejected="*" wrong=0") ;;
    *) fail cases ;;
  esac
  k=${last#self-test rejected=}
  k=${k%% *}
  counted=$(grep -c ' want=[a-z_]* got=[a-z_]*$' "$work/page-cases.txt")
  passing=$(grep -c ' want=pass got=pass$' "$work/page-cases.txt")
  [ "$((counted - passing))" = "$k" ] || fail count
  rejected=0
  wrong=0
  shell=$(command -v sh) || fail setup
  printf 'x\n' >"$work/blocker" || fail setup
  fake_node bin-none ''
  fake_node bin-21 v21.7.0
  mkdir -p "$work/ok-artifacts" "$work/dirty/ui/app" || fail setup
  git -C "$work/dirty" init -q >/dev/null 2>&1 || fail setup
  printf 'x\n' >"$work/dirty/ui/app/untracked.js" || fail setup
  me="$here/$name.sh"
  script_case usage-an-unknown-argument FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --bogus
  script_case usage-a-second-argument FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --self-test extra
  script_case setup-artifacts-under-a-file FAIL:setup env CI_D_ARTIFACTS="$work/blocker/a" "$shell" "$me"
  script_case setup-no-temporary-directory FAIL:setup env TMPDIR="$work/blocker" "$shell" "$me" --self-test
  script_case a-self-test-inside-the-self-test FAIL:nested env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --self-test
  as0="f=\$1; shift; . \"\$f\""
  script_case setup-no-repository-above-the-script FAIL:setup env CI_D_REPO= CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" -c "$as0" "$work/$name.sh" "$me"
  script_case no-node FAIL:node env PATH="$work/bin-none" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case an-input-that-differs-from-the-commit FAIL:dirty env CI_D_REPO="$work/dirty" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case node-21 FAIL:node env PATH="$work/bin-21:$PATH" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  [ "$wrong" -eq 0 ] || fail cases
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$((k + rejected))"
}

"$mode"
