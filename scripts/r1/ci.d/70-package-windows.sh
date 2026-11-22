#!/bin/sh
# jobs: windows
# rule: The one command npm run tauri build -- -- --locked, started with no set variable whose name starts with APPLE_ or TAURI_SIGNING_ or is HALVO_WINDOWS_SIGN_COMMAND and with no tracked file under src-tauri that names externalBin, signCommand or certificateThumbprint, and tried three times at most, makes exactly one installer *-setup.exe under release/bundle/nsis of the target directory, newer than the start of the build, whose manifest asks for the level asInvoker at each requestedExecutionLevel and for no other, and which the 7-Zip of the runner's image unpacks, with no install, into files that hold one halvo.exe with each helper that packaging/helpers.d names, with .exe, in its directory, each read for a number of bytes above zero before anything starts it, and no other file in that directory whose name ends in .exe but the uninstall.exe of the installer template, and 62-build.sh --files passes on those executables (no stand-in or test-seam marker, and the version by 61-version.sh --check, which starts each with --version, but for a helper whose file of packaging/helpers.d says version=none, which is read and not started).
set -u
name=70-package-windows
config_keys='externalBin signCommand certificateThumbprint'
attempts=3
# The uninstaller that the NSIS template of the Tauri bundler writes; 7-Zip unpacks it beside halvo.exe.
template_executables='uninstall.exe'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

signing_defect() {
  environment=$(env) || { printf 'env could not list the environment\n'; return 1; }
  set_name=$(printf '%s\n' "$environment" | LC_ALL=C awk -F = '/^(APPLE_|TAURI_SIGNING_)[A-Za-z0-9_]*=./ || /^HALVO_WINDOWS_SIGN_COMMAND=./ { print $1; exit }')
  [ -z "$set_name" ] && return 0
  printf 'the signing variable %s is set, and this check builds unsigned\n' "$set_name"
  return 1
}

config_defect() {
  for key in $config_keys; do
    found=$(git -C "$1" grep -l -F -e "$key" -- src-tauri)
    status=$?
    case $status in
      0) ;;
      1) continue ;;
      *) printf 'git grep could not read the tracked files under src-tauri\n'; return 1 ;;
    esac
    case $key in
      externalBin) why='scripts/tauri.js gives the build its external binaries for that build alone, and cargo test fails on a clean checkout when a tracked configuration names one' ;;
      *) why='the installer is built unsigned, and sign_step in scripts/tauri.js is the place of signing' ;;
    esac
    printf 'the tracked file %s names %s; %s\n' "${found%%
*}" "$key" "$why"
    return 1
  done
}

helper_names() {
  for file in packaging/helpers.d/*; do
    if [ -f "$file" ]; then printf '%s\n' "${file##*/}"; fi
  done
}

installers() {
  for file in "$1"/*-setup.exe; do
    if [ -f "$file" ]; then printf '%s\n' "$file"; fi
  done
}

installer_defect() {
  found=$(installers "$1")
  count=$(printf '%s\n' "$found" | grep -c .)
  [ "$count" != 0 ] || { printf 'the build made no installer *-setup.exe under release/bundle/nsis\n'; return 1; }
  [ "$count" = 1 ] || { printf 'release/bundle/nsis holds %s installers *-setup.exe, and this check reads exactly one\n' "$count"; return 1; }
  newer=$(find "$found" -newer "$2") || { printf 'find could not compare the installer with the start of the build\n'; return 1; }
  [ -n "$newer" ] || { printf '%s is older than the start of this build\n' "${found##*/}"; return 1; }
  printf '%s\n' "$found"
}

manifest_defect() {
  elements=$(LC_ALL=C grep -a -o -E '<requestedExecutionLevel[^>]*>' "$1")
  [ -n "$elements" ] || { printf '%s carries no requestedExecutionLevel in its manifest\n' "${1##*/}"; return 1; }
  levels=$(printf '%s\n' "$elements" | sed -n 's/.*[[:space:]]level="\([^"]*\)".*/\1/p')
  element_count=$(printf '%s\n' "$elements" | grep -c .)
  level_count=$(printf '%s\n' "$levels" | grep -c .)
  [ "$level_count" = "$element_count" ] || { printf 'a requestedExecutionLevel of the manifest of %s names no level\n' "${1##*/}"; return 1; }
  other=$(printf '%s\n' "$levels" | grep -v -x -F asInvoker | head -n 1)
  [ -z "$other" ] || { printf 'the manifest of %s asks for the level %s, and a per-user installer asks for asInvoker alone\n' "${1##*/}" "$other"; return 1; }
  printf 'installer manifest: requestedExecutionLevel asInvoker, %s of %s\n' "$level_count" "$element_count"
}

content_defect() {
  content=$1
  shift
  apps=$(find "$content" -type f -name halvo.exe) || { printf 'find could not list the unpacked installer\n'; return 1; }
  count=$(printf '%s\n' "$apps" | grep -c .)
  if [ "$count" != 1 ]; then
    executables=$(cd "$content" && find . -type f -name '*.exe' | sed 's|^\./||' | sort | paste -s -d ' ' -)
    printf 'the installer holds %s files halvo.exe, and this check reads exactly one (its executables: %s)\n' "$count" "${executables:-none}"
    return 1
  fi
  for helper in "$@"; do
    [ -f "${apps%/*}/$helper.exe" ] || { printf 'the installer holds no %s.exe beside halvo.exe\n' "$helper"; return 1; }
  done
  # Every entry beside halvo.exe whose name ends in .exe in any case, of any type and with a dot name
  # too, is halvo.exe, a helper with .exe or a file of the installer template.
  for entry in "${apps%/*}"/* "${apps%/*}"/.[!.]* "${apps%/*}"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    case ${entry##*/} in
      *.[eE][xX][eE]) ;;
      *) continue ;;
    esac
    for executable in halvo "$@"; do
      [ "${entry##*/}" = "$executable.exe" ] && continue 2
    done
    for executable in $template_executables; do
      [ "${entry##*/}" = "$executable" ] && continue 2
    done
    printf 'the installer holds %s beside halvo.exe, which is neither halvo.exe, a helper of packaging/helpers.d with .exe nor a file of the installer template (%s)\n' "${entry##*/}" "$template_executables"
    return 1
  done
  printf '%s\n' "${apps%/*}"
}

bytes_of() {
  read_bytes=$(wc -c <"$1") || { printf 'wc could not read %s\n' "${1##*/}"; return 1; }
  read_bytes=$(printf '%s' "$read_bytes" | tr -d '[:blank:]')
  case $read_bytes in
    '' | *[!0-9]*) printf 'wc gave no number of bytes for %s\n' "${1##*/}"; return 1 ;;
  esac
  [ "$read_bytes" -gt 0 ] || { printf '%s holds no byte\n' "${1##*/}"; return 1; }
  printf '%s\n' "$read_bytes"
}

release_defect() {
  directory=$1
  shift
  names=$*
  set -- "$directory/halvo.exe"
  for helper in $names; do
    set -- "$@" "$directory/$helper.exe"
  done
  read_output=$(sh scripts/r1/ci.d/62-build.sh --files "$@")
  status=$?
  if [ "$status" -eq 0 ]; then
    printf '%s\n' "$read_output" | grep -v -E '^(PASS|FAIL) 62-build'
    return 0
  fi
  cause=$(printf '%s\n' "$read_output" | sed -n 's/^FAIL 62-build (\(.*\))$/\1/p')
  printf '62-build --files: %s\n' "${cause:-exit $status}"
  return 1
}

build() {
  : >"$CI_D_ARTIFACTS/build.txt"
  attempt=1
  while :; do
    printf 'build attempt %s of %s\n' "$attempt" "$attempts" >>"$CI_D_ARTIFACTS/build.txt"
    npm run tauri build -- -- --locked >>"$CI_D_ARTIFACTS/build.txt" 2>&1
    status=$?
    printf 'build attempt %s of %s: exit %s\n' "$attempt" "$attempts" "$status"
    [ "$status" = 0 ] && return 0
    [ "$attempt" -lt "$attempts" ] || return 1
    attempt=$((attempt + 1))
  done
}

measure() {
  cd "$CI_D_REPO" || exit 2
  found=$(signing_defect) || fail "$found"
  found=$(config_defect .) || fail "$found"
  helpers=$(helper_names)
  [ -n "$helpers" ] || fail "packaging/helpers.d names no helper"
  sevenzip=$(command -v 7z) || sevenzip='/c/Program Files/7-Zip/7z.exe'
  [ -x "$sevenzip" ] || fail "no 7z on PATH and no $sevenzip; the windows runner's image holds 7-Zip"
  banner=$("$sevenzip" i | grep -m 1 '7-Zip')
  printf '7-Zip: %s\n' "${banner:-unknown}"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  : >"$work/before-build" || exit 2
  build || {
    cause=$(grep -m 1 -E '^(error|Error|scripts/tauri.js:)' "$CI_D_ARTIFACTS/build.txt")
    fail "the build call failed $attempts times: ${cause:-see build.txt in the artifacts}"
  }
  installer=$(installer_defect "${CARGO_TARGET_DIR:-target}/release/bundle/nsis" "$work/before-build") || fail "$installer"
  bytes=$(wc -c <"$installer" | tr -d ' ')
  printf 'installer: %s bytes=%s\n' "${installer##*/}" "$bytes"
  found=$(manifest_defect "$installer") || fail "$found"
  printf '%s\n' "$found"
  cp "$installer" "$CI_D_ARTIFACTS/" || fail "cp could not copy the installer to the artifacts"
  cp "$installer" "$work/installer.exe" || fail "cp could not copy the installer"
  (cd "$work" && "$sevenzip" x -y -bd -ocontent installer.exe) >"$CI_D_ARTIFACTS/7z.txt" 2>&1
  status=$?
  case $status in
    0) printf '7-Zip unpacked the installer: exit 0\n' ;;
    1)
      warning=$(grep -m 1 -i 'warning' "$CI_D_ARTIFACTS/7z.txt")
      printf '7-Zip unpacked the installer with a warning, exit 1: %s\n' "${warning:-see 7z.txt in the artifacts}"
      ;;
    *)
      cause=$(grep -m 1 -i -E 'error|cannot|unsupported' "$CI_D_ARTIFACTS/7z.txt")
      fail "7-Zip could not unpack the installer, exit $status: ${cause:-see 7z.txt in the artifacts}"
      ;;
  esac
  (cd "$work/content" && find . -type f | sed 's|^\./||' | sort) >"$CI_D_ARTIFACTS/installer-files.txt" || fail "find could not list the unpacked installer"
  file_count=$(grep -c . "$CI_D_ARTIFACTS/installer-files.txt")
  printf 'installer files unpacked by 7-Zip: %s\n' "$file_count"
  # shellcheck disable=SC2086
  directory=$(content_defect "$work/content" $helpers) || fail "$directory"
  for executable in halvo $helpers; do
    bytes=$(bytes_of "$directory/$executable.exe") || fail "$bytes"
    printf 'installer holds %s.exe bytes=%s\n' "$executable" "$bytes"
  done
  # shellcheck disable=SC2086
  read_output=$(release_defect "$directory" $helpers) || fail "$read_output"
  printf '%s\n' "$read_output"
  printf 'PASS %s\n' "$name"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  stand_in() {
    mkdir -p "$work/$1" || exit 2
    printf '%s\n' '#!/bin/sh' 'exit 0' >"$work/$1/$2" || exit 2
    chmod +x "$work/$1/$2" || exit 2
  }
  answer() {
    got=unexpected-exit-$2
    [ "$2" = 0 ] && got=PASS
    [ "$2:$3" = "1:$4" ] && got=FAIL
    verdict "$1" FAIL "$got"
  }

  mkdir -p "$work/tree/target/release/bundle/nsis" || exit 2
  : >"$work/tree/before-build" || exit 2
  output=$(installer_defect "$work/tree/target/release/bundle/nsis" "$work/tree/before-build" 2>"$work/tree.err")
  answer build-tree-without-an-installer $? "$output" 'the build made no installer *-setup.exe under release/bundle/nsis'

  stand_in content-without-the-daemon/Halvo halvo.exe
  output=$(content_defect "$work/content-without-the-daemon" halvo-capture 2>"$work/content.err")
  answer installer-content-without-the-daemon $? "$output" 'the installer holds no halvo-capture.exe beside halvo.exe'

  # The list of packaging/helpers.d as the tree holds it, and a content with every helper of it but the runner.
  helpers=$(helper_names)
  stand_in content-without-the-runner/Halvo halvo.exe
  for helper in $(printf '%s\n' "$helpers" | grep -v -x -F halvo-runner); do
    stand_in content-without-the-runner/Halvo "$helper.exe"
  done
  # shellcheck disable=SC2086
  output=$(content_defect "$work/content-without-the-runner" $helpers 2>"$work/runner.err")
  answer installer-content-without-the-runner $? "$output" 'the installer holds no halvo-runner.exe beside halvo.exe'

  # The template's uninstall.exe stands there as in a real installer, and the one more file sorts after it.
  for executable in halvo $helpers uninstall updater; do
    stand_in content-with-one-more-executable/Halvo "$executable.exe"
  done
  # shellcheck disable=SC2086
  output=$(content_defect "$work/content-with-one-more-executable" $helpers 2>"$work/more.err")
  answer installer-content-with-one-more-executable $? "$output" 'the installer holds updater.exe beside halvo.exe, which is neither halvo.exe, a helper of packaging/helpers.d with .exe nor a file of the installer template (uninstall.exe)'

  stand_in daemon-with-the-test-seam halvo.exe
  stand_in daemon-with-the-test-seam halvo-capture.exe
  printf '%s\n' '# halvo-capture:test-seam:this-build-has-it!' >>"$work/daemon-with-the-test-seam/halvo-capture.exe" || exit 2
  output=$(release_defect "$work/daemon-with-the-test-seam" halvo-capture 2>"$work/seam.err")
  answer daemon-with-the-test-seam $? "$output" '62-build --files: halvo-capture.exe holds the marker halvo-capture:test-seam:this-build-has-it!'

  config_repo=$work/config
  mkdir -p "$config_repo/src-tauri" || exit 2
  git init -q "$config_repo" || exit 2
  printf '%s\n' '{"bundle":{"windows":{"signCommand":"stand-in-sign %1"}}}' >"$config_repo/src-tauri/tauri.windows.conf.json" || exit 2
  git -C "$config_repo" add -- src-tauri/tauri.windows.conf.json || exit 2
  output=$(config_defect "$config_repo")
  code=$?
  got=unexpected-exit-$code
  [ "$code" = 0 ] && got=PASS
  case $code:$output in
    "1:the tracked file src-tauri/tauri.windows.conf.json names signCommand; "*) got=FAIL ;;
  esac
  verdict configuration-with-a-sign-command FAIL "$got"

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
