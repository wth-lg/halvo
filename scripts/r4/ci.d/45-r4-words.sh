#!/bin/sh
# jobs: macos windows
# rule: Every sentence of the words table, of the state fixtures and of the two view headings matches no pattern of scripts/r4/words/never-read.txt, holds at most 20 words and holds no reserved term of another run in docs/r4/words.md.
# lane: free
set -u
name=45-r4-words
nested=self-test-of-$name

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
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac

setup() {
  if [ "$mode" = self_test ]; then
    printf 'FAIL self-test:%s (setup)\n' "$name"
  else
    printf 'FAIL %s (setup)\n' "$name"
  fi
  exit 1
}

case $0 in
  */*) here=${0%/*} ;;
  *) here=. ;;
esac
here=$(cd "$here" 2>/dev/null && pwd) || setup
repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  case $here in
    *?/scripts/r4/ci.d) repo=${here%/scripts/r4/ci.d} ;;
    *) setup ;;
  esac
fi
artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}

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

measure() {
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || setup
  if ! node_ok; then
    printf 'FAIL %s (node)\n' "$name"
    exit 1
  fi
  node "$here/../words/lint.mjs" --repo "$repo" --artifacts "$artifacts"
}

fake_node() {
  mkdir -p "$work/$1" || setup
  if [ -n "$2" ]; then
    printf '#!/bin/sh\necho %s\n' "$2" >"$work/$1/node" || setup
    chmod +x "$work/$1/node" || setup
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
  cases="$cases$line
"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null) || setup
  if [ "${CI_D_JOB:-}" = "$nested" ]; then
    printf 'FAIL self-test:%s (nested)\n' "$name"
    exit 1
  fi
  if ! node_ok; then
    printf 'FAIL self-test:%s (node)\n' "$name"
    exit 1
  fi
  shell=$(command -v sh) || setup
  printf 'x\n' >"$work/blocker" || setup
  fake_node bin-none ''
  fake_node bin-20 v20.19.0
  fake_node bin-21 v21.7.0
  mkdir -p "$work/ok-artifacts" || setup
  result=$(node "$here/../words/lint.mjs" --self-test "$work/lint" --repo "$repo")
  code=$?
  last=${result##*
}
  body=${result%"$last"}
  printf '%s' "$body"
  case $code:$last in
    "0:PASS self-test:$name ("*" failing fixtures rejected)") ;;
    *)
      printf 'FAIL self-test:%s (lint)\n' "$name"
      exit 1
      ;;
  esac
  k=${last#"PASS self-test:$name ("}
  k=${k%% *}
  rejected=0
  wrong=0
  cases=
  me="$here/$name.sh"
  script_case usage-an-unknown-argument FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --bogus
  script_case usage-a-second-argument FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" '' extra
  script_case usage-the-self-test-and-more FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --self-test extra
  script_case setup-artifacts-under-a-file FAIL:setup env CI_D_ARTIFACTS="$work/blocker/a" "$shell" "$me"
  script_case setup-no-temporary-directory FAIL:setup env TMPDIR="$work/blocker" "$shell" "$me" --self-test
  script_case a-self-test-inside-the-self-test FAIL:nested env PATH="$work/bin-20:$PATH" "$shell" "$me" --self-test
  script_case a-self-test-with-no-job-is-not-nested FAIL:node env CI_D_JOB= PATH="$work/bin-20:$PATH" "$shell" "$me" --self-test
  as0="f=\$1; shift; . \"\$f\""
  script_case setup-own-directory-not-entered FAIL:setup env CI_D_REPO="$work/lint/base" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" -c "$as0" "$work/no-such-directory/$name.sh" "$me"
  script_case setup-no-repository-above-the-script FAIL:setup env CI_D_REPO= CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" -c "$as0" "$work/$name.sh" "$me"
  script_case order-usage-before-setup FAIL:usage env CI_D_ARTIFACTS="$work/blocker/a" "$shell" "$me" --bogus
  script_case order-setup-before-node FAIL:setup env PATH="$work/bin-20:$PATH" CI_D_ARTIFACTS="$work/blocker/a" "$shell" "$me"
  script_case order-node-before-list FAIL:node env PATH="$work/bin-20:$PATH" CI_D_REPO="$work/lint/list-with-a-broken-pattern" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case no-node FAIL:node env PATH="$work/bin-none" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case node-20 FAIL:node env PATH="$work/bin-20" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case node-21 FAIL:node env PATH="$work/bin-21" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case measure-passes-on-a-clean-tree PASS env CI_D_REPO="$work/lint/base" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case measure-fails-on-a-never-read-word FAIL:never_read env CI_D_REPO="$work/lint/never-read-in-the-table" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  if [ "$wrong" -ne 0 ]; then
    printf 'FAIL self-test:%s (script cases)\n' "$name"
    exit 1
  fi
  line="PASS self-test:$name ($((k + rejected)) failing fixtures rejected)"
  printf '%s%s' "$body" "$cases" >"$work/cases.txt" || setup
  counted=$(grep -c ' want=FAIL:[a-z_]* got=FAIL:[a-z_]*$' "$work/cases.txt")
  n=${line#*(}
  n=${n%% *}
  if [ "$n" != "$counted" ]; then
    printf 'FAIL self-test:%s (count)\n' "$name"
    exit 1
  fi
  printf '%s\n' "$line"
}

"$mode"
