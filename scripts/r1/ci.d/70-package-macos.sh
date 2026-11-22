#!/bin/sh
# jobs: macos
# rule: The one command npm run tauri build -- -- --locked, started with no set variable whose name starts with APPLE_ or TAURI_SIGNING_ or is HALVO_WINDOWS_SIGN_COMMAND and with no tracked file under src-tauri that names externalBin or signingIdentity, makes Halvo.app under release/bundle/macos of the target directory with Contents/Info.plist, whose CFBundleIdentifier is the identifier of src-tauri/tauri.conf.json and whose CFBundleShortVersionString and CFBundleVersion are what 61-version.sh --print prints, with halvo and each helper that packaging/helpers.d names as an executable file in Contents/MacOS and no other entry there, and 62-build.sh --files passes on those executables (no stand-in or test-seam marker, no import of the call that holds secure input, and the version by 61-version.sh --check, which starts each with --version, but for a helper whose file of packaging/helpers.d says version=none, which is read and not started).
set -u
name=70-package-macos
config_keys='externalBin signingIdentity'

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
      *) why='the bundle is built unsigned, and sign_step in scripts/tauri.js is the place of signing' ;;
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

bundle_defect() {
  app=$1 identifier=$2 version=$3
  shift 3
  plist=$app/Contents/Info.plist
  [ -d "$app" ] || { printf 'the build made no Halvo.app\n'; return 1; }
  [ -f "$plist" ] || { printf 'Halvo.app holds no Contents/Info.plist\n'; return 1; }
  for executable in halvo "$@"; do
    file=$app/Contents/MacOS/$executable
    if [ -L "$file" ] || [ ! -f "$file" ] || [ ! -x "$file" ]; then
      printf 'Halvo.app holds no executable file Contents/MacOS/%s\n' "$executable"
      return 1
    fi
  done
  # Every entry of Contents/MacOS, of any type and with a dot name too, is halvo or a helper.
  for entry in "$app/Contents/MacOS"/* "$app/Contents/MacOS"/.[!.]* "$app/Contents/MacOS"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    for executable in halvo "$@"; do
      [ "${entry##*/}" = "$executable" ] && continue 2
    done
    printf 'Halvo.app holds Contents/MacOS/%s, which is neither halvo nor a helper of packaging/helpers.d\n' "${entry##*/}"
    return 1
  done
  value=$(plutil -extract CFBundleIdentifier raw -o - "$plist") || { printf 'plutil could not read the CFBundleIdentifier of Info.plist\n'; return 1; }
  [ "$value" = "$identifier" ] || { printf 'the CFBundleIdentifier of Info.plist is not the identifier of src-tauri/tauri.conf.json\n'; return 1; }
  for key in CFBundleShortVersionString CFBundleVersion; do
    value=$(plutil -extract "$key" raw -o - "$plist") || { printf 'plutil could not read the %s of Info.plist\n' "$key"; return 1; }
    [ "$value" = "$version" ] || { printf 'the %s of Info.plist is not what 61-version.sh --print prints\n' "$key"; return 1; }
  done
  helpers=$*
  set -- "$app/Contents/MacOS/halvo"
  for helper in $helpers; do
    set -- "$@" "$app/Contents/MacOS/$helper"
  done
  read_output=$(sh scripts/r1/ci.d/62-build.sh --files "$@")
  status=$?
  printf '%s\n' "$read_output" | grep -v -E '^(PASS|FAIL) 62-build' >&2
  [ "$status" -eq 0 ] && return 0
  cause=$(printf '%s\n' "$read_output" | sed -n 's/^FAIL 62-build (\(.*\))$/\1/p')
  printf '62-build --files: %s\n' "${cause:-exit $status}"
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  found=$(signing_defect) || fail "$found"
  found=$(config_defect .) || fail "$found"
  : >"$CI_D_ARTIFACTS/build.txt"
  if ! npm run tauri build -- -- --locked >>"$CI_D_ARTIFACTS/build.txt" 2>&1; then
    cause=$(grep -m 1 -E '^(error|Error|scripts/tauri.js:)' "$CI_D_ARTIFACTS/build.txt")
    fail "the build call failed: ${cause:-see build.txt in the artifacts}"
  fi
  helpers=$(helper_names)
  [ -n "$helpers" ] || fail "packaging/helpers.d names no helper"
  identifier=$(plutil -extract identifier raw -o - src-tauri/tauri.conf.json) || fail "plutil could not read the identifier of src-tauri/tauri.conf.json"
  version=$(sh scripts/r1/ci.d/61-version.sh --print) || fail "61-version.sh --print derived no version"
  app=${CARGO_TARGET_DIR:-target}/release/bundle/macos/Halvo.app
  # shellcheck disable=SC2086
  found=$(bundle_defect "$app" "$identifier" "$version" $helpers) || fail "$found"
  files=$(find "$app" -type f) || fail "find could not list Halvo.app"
  file_count=$(printf '%s\n' "$files" | grep -c .)
  helper_count=$(printf '%s\n' "$helpers" | grep -c .)
  printf 'Halvo.app files: %s, helpers: %s\n' "$file_count" "$helper_count"
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
  identifier=com.halvo.desktop
  version=0.0.0-stand-in
  stand_in_bundle() {
    app=$work/$1/Halvo.app
    shift
    mkdir -p "$app/Contents/MacOS" || exit 2
    cat >"$app/Contents/Info.plist" <<EOF || exit 2
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>$identifier</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
</dict>
</plist>
EOF
    for executable in "$@"; do
      printf '%s\n' '#!/bin/sh' 'exit 0' >"$app/Contents/MacOS/$executable" || exit 2
      chmod +x "$app/Contents/MacOS/$executable" || exit 2
    done
  }
  # A case names its helpers after the wanted line, or reads halvo-capture alone.
  bundle_case() {
    case_name=$1 want=$2
    shift 2
    [ $# -gt 0 ] || set -- halvo-capture
    output=$(bundle_defect "$app" "$identifier" "$version" "$@" 2>"$work/$case_name.err")
    code=$?
    got=unexpected-exit-$code
    [ "$code" = 0 ] && got=PASS
    [ "$code:$output" = "1:$want" ] && got=FAIL
    verdict "$case_name" FAIL "$got"
  }

  stand_in_bundle without-the-daemon halvo
  bundle_case bundle-without-the-daemon 'Halvo.app holds no executable file Contents/MacOS/halvo-capture'

  # The list of packaging/helpers.d as the tree holds it, and a bundle with every helper of it but the runner.
  helpers=$(helper_names)
  others=$(printf '%s\n' "$helpers" | grep -v -x -F halvo-runner)
  # shellcheck disable=SC2086
  stand_in_bundle without-the-runner halvo $others
  # shellcheck disable=SC2086
  bundle_case bundle-without-the-runner 'Halvo.app holds no executable file Contents/MacOS/halvo-runner' $helpers

  # shellcheck disable=SC2086
  stand_in_bundle with-one-more-executable halvo $helpers halvo-updater
  # shellcheck disable=SC2086
  bundle_case bundle-with-one-more-executable 'Halvo.app holds Contents/MacOS/halvo-updater, which is neither halvo nor a helper of packaging/helpers.d' $helpers

  case $0 in /*) check_script=$0 ;; *) check_script=$PWD/$0 ;; esac
  measure_repo=$work/measure
  mkdir -p "$measure_repo" "$work/artifacts" "$work/tools" || exit 2
  git init -q "$measure_repo" || exit 2
  measure_trace=$work/build-started
  export measure_trace
  for program in npm node cargo rustc; do
    cat >"$work/tools/$program" <<'SH' || exit 2
#!/bin/sh
printf '%s\n' "${0##*/}" >>"$measure_trace"
exit 1
SH
    chmod +x "$work/tools/$program" || exit 2
  done
  output=$(APPLE_SIGNING_IDENTITY=stand-in-identity CI_D_REPO="$measure_repo" CI_D_ARTIFACTS="$work/artifacts" CI_D_JOB=macos PATH="$work/tools:$PATH" sh "$check_script" 2>"$work/measure.err")
  code=$?
  got=unexpected-exit-$code
  case $code:$output in
    "1:FAIL $name (the signing variable "*" is set, and this check builds unsigned)") got=FAIL ;;
  esac
  [ -e "$measure_trace" ] && got=the-build-started
  verdict signing-variable-stops-before-the-build FAIL "$got"

  stand_in_bundle daemon-with-the-test-seam halvo halvo-capture
  printf '%s\n' '# halvo-capture:test-seam:this-build-has-it!' >>"$app/Contents/MacOS/halvo-capture" || exit 2
  bundle_case daemon-with-the-test-seam '62-build --files: halvo-capture holds the marker halvo-capture:test-seam:this-build-has-it!'

  config_repo=$work/config
  mkdir -p "$config_repo/src-tauri" || exit 2
  git init -q "$config_repo" || exit 2
  printf '%s\n' '{"bundle":{"macOS":{"signingIdentity":"stand-in identity"}}}' >"$config_repo/src-tauri/tauri.conf.json" || exit 2
  git -C "$config_repo" add -- src-tauri/tauri.conf.json || exit 2
  output=$(config_defect "$config_repo")
  code=$?
  got=unexpected-exit-$code
  [ "$code" = 0 ] && got=PASS
  case $code:$output in
    "1:the tracked file src-tauri/tauri.conf.json names signingIdentity; "*) got=FAIL ;;
  esac
  verdict tracked-configuration-with-a-signing-identity FAIL "$got"

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
