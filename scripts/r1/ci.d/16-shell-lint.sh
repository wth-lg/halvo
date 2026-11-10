#!/bin/sh
# jobs: macos
# rule: shellcheck reports nothing for a tracked *.sh file, and no masked return status under scripts/r1-gate, scripts/ci and scripts/r1/ci.d.
set -u
name=16-shell-lint

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

have_shellcheck() {
  if ! command -v shellcheck >/dev/null 2>&1 && [ "${CI:-}" = true ]; then
    HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install shellcheck >"$CI_D_ARTIFACTS/brew-install.txt" 2>&1
  fi
  command -v shellcheck >/dev/null 2>&1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  if ! have_shellcheck; then
    printf 'SKIP %s (shellcheck is not installed)\n' "$name"
    exit 77
  fi
  version=$(shellcheck --version) || fail "shellcheck --version failed"
  version=$(printf '%s\n' "$version" | sed -n 's/^version: //p')
  printf 'shellcheck version: %s\n' "$version"
  lists=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"

  git ls-files -z -- '*.sh' >"$lists/tracked" || fail "git ls-files failed"
  [ -s "$lists/tracked" ] || fail "git lists no tracked *.sh file"
  xargs -0 shellcheck -x <"$lists/tracked" || fail "shellcheck -x reports a finding in a tracked *.sh file"

  git ls-files -z -- 'scripts/r1-gate/*.sh' 'scripts/ci/*.sh' 'scripts/r1/ci.d/*.sh' >"$lists/strict" || fail "git ls-files failed"
  [ -s "$lists/strict" ] || fail "git lists no script under scripts/r1-gate, scripts/ci and scripts/r1/ci.d"
  xargs -0 shellcheck -x --enable=check-extra-masked-returns <"$lists/strict" ||
    fail "shellcheck reports a masked return status under scripts/r1-gate, scripts/ci or scripts/r1/ci.d"

  tracked=$(tr -cd '\000' <"$lists/tracked" | wc -c)
  strict=$(tr -cd '\000' <"$lists/strict" | wc -c)
  printf 'tracked *.sh files: %s, of them in the second pass: %s\n' "$((tracked))" "$((strict))"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    CI_D_REPO=$work/$1
    export CI_D_REPO
    measure 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

# shellcheck disable=SC2016
tree() {
  repo=$work/$1
  mkdir -p "$repo/scripts/ci" "$repo/scripts/r1-gate" || exit 2
  git -C "$repo" init -q >/dev/null 2>&1
  shift
  for file in "$@"; do
    case $file in
      *clean.sh) printf '#!/bin/sh\nprintf "ok\\n"\n' >"$repo/$file" ;;
      *unquoted.sh) printf '#!/bin/sh\necho $(date)\n' >"$repo/$file" ;;
      *masked.sh) printf '#!/bin/sh\nprintf "%%s\\n" "$(date)"\n' >"$repo/$file" ;;
    esac
    git -C "$repo" add "$file" >/dev/null 2>&1
  done
}

self_test() {
  if ! have_shellcheck; then
    printf 'FAIL self-test:%s (shellcheck is not installed)\n' "$name"
    exit 1
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  tree clean scripts/ci/clean.sh scripts/clean.sh
  check clean PASS 'tracked *.sh files: 2, of them in the second pass: 1'
  tree unquoted-substitution-in-a-gate-script scripts/ci/clean.sh scripts/r1-gate/unquoted.sh
  check unquoted-substitution-in-a-gate-script FAIL 'shellcheck -x reports a finding in a tracked *.sh file)'
  tree unquoted-substitution-elsewhere scripts/ci/clean.sh scripts/unquoted.sh
  check unquoted-substitution-elsewhere FAIL 'shellcheck -x reports a finding in a tracked *.sh file)'
  tree masked-return-in-a-gate-script scripts/ci/clean.sh scripts/r1-gate/masked.sh
  check masked-return-in-a-gate-script FAIL 'shellcheck reports a masked return status'
  tree masked-return-elsewhere scripts/ci/clean.sh scripts/masked.sh
  check masked-return-elsewhere PASS
  tree no-tracked-script
  check no-tracked-script FAIL 'git lists no tracked *.sh file)'
  tree no-script-in-the-second-pass scripts/clean.sh
  check no-script-in-the-second-pass FAIL 'git lists no script under scripts/r1-gate, scripts/ci and scripts/r1/ci.d)'

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
