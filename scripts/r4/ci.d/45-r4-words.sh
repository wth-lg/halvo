#!/bin/sh
# jobs: macos windows
# rule: Every sentence of the words table, of the 18 state fixtures and of the two view headings matches no pattern of scripts/r4/words/never-read.txt, holds at most 20 words and holds no reserved term of another run in docs/r4/words.md.
set -u
name=45-r4-words

case $0 in
  */*) here=${0%/*} ;;
  *) here=. ;;
esac
here=$(cd "$here" && pwd) || exit 2
repo=${CI_D_REPO:-$(cd "$here/../../.." && pwd)}
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
  if ! node_ok; then
    printf 'FAIL %s (node)\n' "$name"
    exit 1
  fi
  mkdir -p "$artifacts" || exit 2
  node "$repo/scripts/r4/words/lint.mjs" --repo "$repo" --artifacts "$artifacts"
}

node_case() {
  bin=$work/$1
  mkdir -p "$bin" || exit 2
  if [ -n "$2" ]; then
    printf '#!/bin/sh\necho %s\n' "$2" >"$bin/node" || exit 2
    chmod +x "$bin/node" || exit 2
  fi
  out=$(PATH="$bin" CI_D_REPO="$repo" CI_D_ARTIFACTS="$work/$1-artifacts" "$shell" "$here/$name.sh")
  code=$?
  if [ "$code:$out" = "1:FAIL $name (node)" ]; then
    printf 'case %s want=FAIL:node got=FAIL:node\n' "$1"
    rejected=$((rejected + 1))
  else
    printf 'case %s want=FAIL:node got=exit-%s wrong\n' "$1" "$code"
    wrong=1
  fi
}

self_test() {
  if ! node_ok; then
    printf 'FAIL self-test:%s (node)\n' "$name"
    exit 1
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  shell=$(command -v sh) || exit 2
  rejected=0
  wrong=0
  node_case no-node ''
  node_case node-older-than-22 v20.19.0
  result=$(node "$repo/scripts/r4/words/lint.mjs" --self-test "$work/lint" --repo "$repo")
  code=$?
  last=${result##*
}
  printf '%s' "${result%"$last"}"
  case $code:$last in
    "0:PASS self-test:$name ("*" failing fixtures rejected)") ;;
    *)
      printf 'FAIL self-test:%s (lint)\n' "$name"
      exit 1
      ;;
  esac
  if [ "$wrong" -ne 0 ]; then
    printf 'FAIL self-test:%s (node cases)\n' "$name"
    exit 1
  fi
  k=${last#"PASS self-test:$name ("}
  k=${k%% *}
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$((k + rejected))"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
