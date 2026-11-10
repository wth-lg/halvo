#!/bin/sh
# Probe for a scratch branch. It prints what happens to a byte that is not valid
# UTF-8 on each route from this shell to git, then runs the commands of the
# windows step to their end. It removes nothing and always exits 0.
# Usage: sh probe-windows-byte.sh [root of the checkout]
set -u

root=${1:-$(cd "$(dirname "$0")/.." && pwd)}

hex() {
  od -An -tx1 | tr -d ' \n'
  echo
}

section() {
  printf '\n== %s\n' "$1"
}

section 'versions and locale'
git --version
sh --version 2>&1 | sed -n 1p
uname -a
for tool in git sh od sed awk grep mktemp find; do
  printf '%s is %s\n' "$tool" "$(command -v "$tool")"
done
printf 'LANG=%s LC_ALL=%s LC_CTYPE=%s MSYSTEM=%s MSYS=%s TMPDIR=%s TMP=%s TEMP=%s\n' \
  "${LANG-<unset>}" "${LC_ALL-<unset>}" "${LC_CTYPE-<unset>}" "${MSYSTEM-<unset>}" "${MSYS-<unset>}" \
  "${TMPDIR-<unset>}" "${TMP-<unset>}" "${TEMP-<unset>}"
locale 2>&1
printf 'charmap: %s\n' "$(locale charmap 2>&1)"
printf 'charmap under LC_ALL=en_US.UTF-8: %s\n' "$(LC_ALL=en_US.UTF-8 locale charmap 2>&1)"
if command -v powershell >/dev/null 2>&1; then
  printf 'ANSI code page of Windows: %s\n' "$(powershell -NoProfile -Command '[System.Text.Encoding]::Default.CodePage' 2>&1 | tr -d '\r')"
  printf 'console code page: %s\n' "$(cmd //c chcp 2>&1 | tr -d '\r')"
fi
printf 'system attributes file: %s\n' "$(git var GIT_ATTR_SYSTEM 2>&1)"
printf 'attributes file of the user: %s\n' "$(git var GIT_ATTR_GLOBAL 2>&1)"
git config --show-origin --show-scope --get-regexp '^core\.(autocrlf|comment.*|attributesfile)$' 2>&1

work=$(mktemp -d "${TMPDIR:-/tmp}/probe-windows-byte.XXXXXX") || exit 0
printf 'scratch directory, left in place: %s\n' "$work"

routes() {
  label=$1 value=$2
  section "routes of the value $label"
  printf 'in this shell: '
  printf '%s' "$value" | hex
  printf 'environment, to a child sh: '
  V=$value sh -c 'printf %s "$V"' | hex
  if command -v powershell >/dev/null 2>&1; then
    printf 'environment, to a native program (UTF-16 code units, decimal): '
    V=$value powershell -NoProfile -Command '[int[]][char[]]$env:V' 2>&1 | tr '\r\n' '  '
    echo
  fi
  for locale in '' C en_US.UTF-8; do
    printf 'environment GIT_CONFIG_VALUE_0, to git, LC_ALL=[%s]: ' "$locale"
    (
      if [ -n "$locale" ]; then export LC_ALL="$locale"; fi
      export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.commentString GIT_CONFIG_VALUE_0="$value"
      git config --get core.commentString | hex
    )
  done
  printf 'command line -c, to git: '
  git -c core.commentString="$value" config --get core.commentString | hex
  printf '[core]\n\tcommentString = "%s"\n' "$value" >"$work/config-$label"
  printf 'file that GIT_CONFIG_GLOBAL names, to git: '
  GIT_CONFIG_GLOBAL="$work/config-$label" git config --get core.commentString | hex
  printf 'file that include.path in GIT_CONFIG_VALUE_0 names, to git: '
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0="$work/config-$label" git config --get core.commentString | hex
  printf 'file that --file names, to git: '
  git config --file "$work/config-$label" --get core.commentString | hex
  printf 'standard input, to git: '
  git config --file - --get core.commentString <"$work/config-$label" | hex
}

byte_e9=$(printf '\351')
routes e9 "$byte_e9"
routes 80 "$(printf '\200')"
routes 9f "$(printf '\237')"
routes a0 "$(printf '\240')"
routes ff "$(printf '\377')"
routes c3a9 "$(printf '\303\251')"
routes semicolon ';'

section 'the pipeline of git_comment_string under LC_ALL=en_US.UTF-8, value from a file'
config_file=$(mktemp "${TMPDIR:-/tmp}/probe-comment-string.XXXXXX")
printf 'mktemp exit=%s file=%s\n' "$?" "$config_file"
printf '[core]\n\tcommentString = "%s"\n' "$byte_e9" >"$config_file"
got=$(
  export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0="$config_file" LC_ALL=en_US.UTF-8
  git config --get-regexp '^core\.comment(char|string)$' 2>/dev/null | LC_ALL=C sed -n '$s/^[^ ]* //p'
)
printf 'read: '
printf '%s' "$got" | hex
if [ "$got" = "$byte_e9" ]; then echo 'the value from the file: SAME BYTE'; else echo 'the value from the file: DIFFERENT'; fi
printf 'origin that git reports: '
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0="$config_file" git config --show-origin --show-scope --get-regexp '^core\.comment' 2>&1 | hex
got=$(
  export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.commentString GIT_CONFIG_VALUE_0="$byte_e9" LC_ALL=en_US.UTF-8
  git config --get-regexp '^core\.comment(char|string)$' 2>/dev/null | LC_ALL=C sed -n '$s/^[^ ]* //p'
)
printf 'the same pipeline, value from the environment, read: '
printf '%s' "$got" | hex
if [ "$got" = "$byte_e9" ]; then echo 'the value from the environment: SAME BYTE'; else echo 'the value from the environment: DIFFERENT'; fi

section 'attribute sources'
git init -q "$work/attributes"
(
  cd "$work/attributes" || exit 0
  printf '#!/bin/sh\necho\n' >tool
  git add tool
  printf 'git rev-parse --git-path info/attributes: %s\n' "$(git rev-parse --git-path info/attributes)"
  printf 'no source: '
  git check-attr --cached eol -- tool 2>&1
  printf '* text eol=lf\n' >"$work/attributes-of-the-user"
  printf 'core.attributesFile from GIT_CONFIG_VALUE_0: '
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.attributesFile GIT_CONFIG_VALUE_0="$work/attributes-of-the-user" git check-attr --cached eol -- tool 2>&1
  printf 'the same, with -c core.attributesFile=/dev/null and GIT_ATTR_NOSYSTEM=1: '
  GIT_ATTR_NOSYSTEM=1 GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.attributesFile GIT_CONFIG_VALUE_0="$work/attributes-of-the-user" git -c core.attributesFile=/dev/null check-attr --cached eol -- tool 2>&1
  mkdir -p "$work/xdg/git"
  cp "$work/attributes-of-the-user" "$work/xdg/git/attributes"
  printf 'XDG_CONFIG_HOME/git/attributes: '
  XDG_CONFIG_HOME="$work/xdg" git check-attr --cached eol -- tool 2>&1
  printf 'the same, with -c core.attributesFile=/dev/null and GIT_ATTR_NOSYSTEM=1: '
  GIT_ATTR_NOSYSTEM=1 XDG_CONFIG_HOME="$work/xdg" git -c core.attributesFile=/dev/null check-attr --cached eol -- tool 2>&1
  printf '* text eol=lf\n' >.git/info/attributes
  printf 'info/attributes, with the same switches: '
  GIT_ATTR_NOSYSTEM=1 git -c core.attributesFile=/dev/null check-attr --cached eol -- tool 2>&1
)

section 'git clone with and without --no-local'
git clone -q "$root" "$work/clone-local"
printf 'clone exit=%s; object files with more than one link: %s\n' "$?" "$(find "$work/clone-local/.git/objects" -type f -links +1 | wc -l | tr -d ' ')"
git clone -q --no-local "$root" "$work/clone-no-local"
printf 'clone --no-local exit=%s; object files with more than one link: %s\n' "$?" "$(find "$work/clone-no-local/.git/objects" -type f -links +1 | wc -l | tr -d ' ')"

section 'git commit -v through the commit-msg hook, comment string from the config file of the repository'
end_to_end=$work/end-to-end
git init -q "$end_to_end"
mkdir -p "$end_to_end/.githooks" "$end_to_end/scripts"
cp "$root/.githooks/commit-msg" "$end_to_end/.githooks/commit-msg"
cp "$root/scripts/check-commit-messages.sh" "$end_to_end/scripts/check-commit-messages.sh"
chmod +x "$end_to_end/.githooks/commit-msg"
(
  cd "$end_to_end" || exit 0
  git config core.hooksPath .githooks
  git config commit.gpgsign false
  printf 'one\nCo-Authored-By: Probe <probe@halvo.invalid>\nthree\n' >notes.txt
  git add notes.txt
  git commit -q --no-verify -m 'Start the repository'
  printf 'first commit, without the hook: exit=%s\n' "$?"
  printf 'one\nCo-Authored-By: Probe <probe@halvo.invalid>\nthree changed\n' >notes.txt
  GIT_EDITOR=true git commit -q -v -e -m 'Change line three' notes.txt
  printf 'git commit -v with the default comment string: exit=%s\n' "$?"
  printf 'scissors line of that message file: '
  grep -a '>8' .git/COMMIT_EDITMSG | hex
  printf '[core]\n\tcommentString = "\351"\n' >>.git/config
  printf 'git config now reads: '
  git config --get core.commentString | hex
  printf 'one\nCo-Authored-By: Probe <probe@halvo.invalid>\nthree changed twice\n' >notes.txt
  GIT_EDITOR=true git commit -q -v -e -m 'Change line three again' notes.txt
  printf 'git commit -v with the byte e9 as comment string: exit=%s\n' "$?"
  printf 'scissors line of that message file: '
  LC_ALL=C grep '>8' .git/COMMIT_EDITMSG | hex
  printf 'commits in the repository: %s\n' "$(git rev-list --count HEAD)"
)

section 'the commands of the windows step, each one run to its end'
clone=$work/autocrlf-clone
git clone --quiet -c core.autocrlf=true "$root" "$clone"
printf 'clone exit=%s\n' "$?"
(
  cd "$clone" || exit 0
  line_endings=$(git ls-files --eol -- '*.sh' '.githooks/*')
  printf 'files listed: %s; checked out with CRLF: %s\n' \
    "$(printf '%s\n' "$line_endings" | grep -c .)" \
    "$(printf '%s\n' "$line_endings" | awk '$2 == "w/crlf" || $2 == "w/mixed"' | grep -c .)"
  for self_test in check-commit-messages check-changelog check-line-endings check-own-source-reads; do
    sh "scripts/$self_test.sh" --self-test >"$work/$self_test.self-test.txt" 2>&1
    code=$?
    printf '%s.sh --self-test: exit=%s, PASS lines=%s, other lines:\n' "$self_test" "$code" "$(grep -c '^PASS' "$work/$self_test.self-test.txt")"
    grep -v '^PASS' "$work/$self_test.self-test.txt"
  done
  sh scripts/check-line-endings.sh
  printf 'check-line-endings.sh: exit=%s\n' "$?"
  git config core.hooksPath .githooks
  git commit --quiet --allow-empty -m 'Probe the commit-msg hook with a clean message'
  printf 'commit with a clean message: exit=%s\n' "$?"
  refusal=$(git commit --quiet --allow-empty -m 'Probe the commit-msg hook with a trailer' -m 'Co-Authored-By: Probe <probe@halvo.invalid>' 2>&1)
  printf 'commit with a trailer: exit=%s, output:\n%s\n' "$?" "$refusal"
)

section 'end of the probe'
exit 0
