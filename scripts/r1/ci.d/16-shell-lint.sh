#!/bin/sh
# jobs: macos
# rule: shellcheck at its pinned version reports nothing for a tracked *.sh file, and no masked return status under scripts/r1-gate, scripts/ci and scripts/r1/ci.d.
set -u
name=16-shell-lint
pinned_version=0.11.0

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
  [ "$version" = "$pinned_version" ] ||
    fail "shellcheck $version runs here, and the pinned version is $pinned_version: the runner image changed the tool, so read its release notes, then move the pin in scripts/r1/ci.d/16-shell-lint.sh"
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
  out=$(PATH=$path CI=$ci CI_D_REPO=$work/$1 CI_D_ARTIFACTS=$work/artifacts "$shell" "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
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
  path=$PATH
  ci=
}

# shellcheck disable=SC2016
tree() {
  repo=$work/$1
  mkdir -p "$repo" || exit 2
  git -C "$repo" init -q >/dev/null 2>&1
  shift
  for file in "$@"; do
    mkdir -p "$repo/${file%/*}" || exit 2
    case $file in
      *clean.sh) printf '#!/bin/sh\nword=ok\nprintf "%%s\\n" "$word"\n' >"$repo/$file" ;;
      *unquoted.sh) printf '#!/bin/sh\necho $(date)\n' >"$repo/$file" ;;
      *masked.sh) printf '#!/bin/sh\nprintf "%%s\\n" "$(date)"\n' >"$repo/$file" ;;
      *part.inc) printf 'word=ok\nexport word\n' >"$repo/$file" ;;
      *main.sh) printf '#!/bin/sh\n# shellcheck source=scripts/ci/part.inc\n. scripts/ci/part.inc\nprintf "%%s\\n" "$word"\n' >"$repo/$file" ;;
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
  mkdir -p "$work/artifacts" "$work/no-tools" "$work/tools" "$work/old-tools"
  shell=$(command -v sh) || exit 2
  STUB_REAL=$(command -v shellcheck) || exit 2
  STUB_TOOLS=$work/tools
  export STUB_REAL STUB_TOOLS
  cat >"$work/tools/brew" <<'EOF'
#!/bin/sh
[ "$*" = 'install shellcheck' ] || exit 1
printf '#!/bin/sh\nexec "%s" "$@"\n' "$STUB_REAL" >"$STUB_TOOLS/shellcheck"
chmod +x "$STUB_TOOLS/shellcheck"
EOF
  chmod +x "$work/tools/brew"
  path=$PATH
  ci=
  rejected=0
  wrong=

  tree clean scripts/ci/clean.sh scripts/clean.sh
  check clean PASS 'tracked *.sh files: 2, of them in the second pass: 1'
  tree unquoted-substitution-in-a-gate-script scripts/ci/clean.sh scripts/r1-gate/unquoted.sh
  check unquoted-substitution-in-a-gate-script FAIL 'shellcheck -x reports a finding in a tracked *.sh file)'
  tree unquoted-substitution-elsewhere scripts/ci/clean.sh scripts/unquoted.sh
  check unquoted-substitution-elsewhere FAIL 'shellcheck -x reports a finding in a tracked *.sh file)'
  tree unquoted-substitution-outside-scripts scripts/ci/clean.sh tools/unquoted.sh
  check unquoted-substitution-outside-scripts FAIL 'shellcheck -x reports a finding in a tracked *.sh file)'
  tree sourced-file-is-followed scripts/ci/main.sh scripts/ci/part.inc
  check sourced-file-is-followed PASS 'tracked *.sh files: 1, of them in the second pass: 1'
  tree masked-return-in-a-gate-script scripts/ci/clean.sh scripts/r1-gate/masked.sh
  check masked-return-in-a-gate-script FAIL 'shellcheck reports a masked return status'
  tree masked-return-in-a-gate-subdirectory scripts/ci/clean.sh scripts/r1-gate/helper/masked.sh
  check masked-return-in-a-gate-subdirectory FAIL 'shellcheck reports a masked return status'
  tree masked-return-in-the-driver-directory scripts/ci/clean.sh scripts/ci/masked.sh
  check masked-return-in-the-driver-directory FAIL 'shellcheck reports a masked return status'
  tree masked-return-in-a-ci-d-script scripts/ci/clean.sh scripts/r1/ci.d/masked.sh
  check masked-return-in-a-ci-d-script FAIL 'shellcheck reports a masked return status'
  tree masked-return-elsewhere scripts/ci/clean.sh scripts/masked.sh
  check masked-return-elsewhere PASS
  tree no-tracked-script
  check no-tracked-script FAIL 'git lists no tracked *.sh file)'
  tree no-script-in-the-second-pass scripts/clean.sh
  check no-script-in-the-second-pass FAIL 'git lists no script under scripts/r1-gate, scripts/ci and scripts/r1/ci.d)'
  tree another-shellcheck-version scripts/ci/clean.sh
  printf '#!/bin/sh\ncase "%s1" in --version) printf "ShellCheck\\nversion: 0.10.0\\n" ;; *) exec "%s" "%s@" ;; esac\n' "\$" "$STUB_REAL" "\$" >"$work/old-tools/shellcheck"
  chmod +x "$work/old-tools/shellcheck"
  path=$work/old-tools:$PATH
  check another-shellcheck-version FAIL 'shellcheck 0.10.0 runs here, and the pinned version is 0.11.0'
  tree shellcheck-missing scripts/ci/clean.sh
  path=$work/no-tools
  check shellcheck-missing SKIP 'SKIP 16-shell-lint (shellcheck is not installed)'
  tree shellcheck-installed-in-ci scripts/ci/clean.sh
  path=$work/tools:/usr/bin:/bin
  ci=true
  check shellcheck-installed-in-ci PASS 'tracked *.sh files: 1, of them in the second pass: 1'

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
