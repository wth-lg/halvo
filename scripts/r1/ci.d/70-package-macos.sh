#!/bin/sh
# jobs: macos
# rule: The one command npm run tauri build -- -- --locked, started with no set variable whose name starts with APPLE_ or TAURI_SIGNING_ or is HALVO_WINDOWS_SIGN_COMMAND and with no tracked file under src-tauri that names externalBin or signingIdentity, makes Halvo.app under release/bundle/macos of the target directory with Contents/Info.plist, whose CFBundleIdentifier is the identifier of src-tauri/tauri.conf.json and whose CFBundleShortVersionString and CFBundleVersion are what 61-version.sh --print prints, with halvo and each helper that packaging/helpers.d names as an executable file in Contents/MacOS and no other entry there, and 62-build.sh --files passes on those executables (no stand-in or test-seam marker, no import of the call that holds secure input, and the version by 61-version.sh --check, which starts each with --version, but for a helper whose file of packaging/helpers.d says version=none, which is read and not started); the same build leaves exactly one disk image in release/bundle/dmg, newer than the start of the build, which the check attaches read-only and unbrowsed under its scratch directory, copies Halvo.app out of with ditto and detaches, also when it fails; the copy's Info.plist names as LSMinimumSystemVersion the bundle.macOS.minimumSystemVersion of src-tauri/tauri.macos.conf.json, whose major is the MACOS_FLOOR_MAJOR of crates/halvo-control/src/onboarding.rs; codesign --verify --deep --strict passes on the copy and codesign -dv names Signature=adhoc and a bound Info.plist; the copy's halvo, started with --background on a scratch data directory that holds a state file, has within 60 s exactly one process whose path, as ps names it, is the copy's halvo-capture, with that app as its parent, whose status by the copy's ctl reads its pid and store=open, which ctl stop ends with exit 0, after which no halvo-capture of the copy runs for 5 s, and within 60 s of the app's end, by TERM from the check, which ends nothing else, no process whose path lies in the copy runs.
set -u
name=70-package-macos
config_keys='externalBin signingIdentity'
macos_config=src-tauri/tauri.macos.conf.json
floor_source=crates/halvo-control/src/onboarding.rs
wait_s=60
settle_s=5

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

# The seal: codesign --verify --deep --strict passes on the bundle $1, and codesign -dv names an
# ad hoc signature and an Info.plist bound to the seal.
seal_defect() {
  verify=$(codesign --verify --deep --strict "$1" 2>&1)
  status=$?
  if [ "$status" -ne 0 ]; then
    printf 'codesign --verify --deep --strict refuses %s: %s\n' "${1##*/}" "$(printf '%s\n' "$verify" | head -n 1)"
    return 1
  fi
  details=$(codesign -dv "$1" 2>&1) || { printf 'codesign -dv could not read the seal of %s\n' "${1##*/}"; return 1; }
  printf '%s\n' "$details" | grep -q -x -F 'Signature=adhoc' || { printf 'codesign -dv names no ad hoc signature on %s\n' "${1##*/}"; return 1; }
  printf '%s\n' "$details" | grep -q -x -E 'Info\.plist entries=[0-9]+' || { printf 'codesign -dv names no Info.plist bound to the seal of %s\n' "${1##*/}"; return 1; }
}

# The floor: the LSMinimumSystemVersion of the Info.plist $1 is the minimumSystemVersion $2 of
# src-tauri/tauri.macos.conf.json, and its major is $3, the MACOS_FLOOR_MAJOR of the onboarding.
floor_defect() {
  value=$(plutil -extract LSMinimumSystemVersion raw -o - "$1") || { printf 'plutil could not read the LSMinimumSystemVersion of Info.plist\n'; return 1; }
  [ "$value" = "$2" ] || { printf 'the LSMinimumSystemVersion of Info.plist is %s, not the minimumSystemVersion %s of %s\n' "$value" "$2" "$macos_config"; return 1; }
  [ "${value%%.*}" = "$3" ] || { printf 'the major of the LSMinimumSystemVersion %s of Info.plist is not the MACOS_FLOOR_MAJOR %s of %s\n' "$value" "$3" "$floor_source"; return 1; }
}

# The one disk image of the build: exactly one *.dmg in the directory $1, a file newer than the
# file $2, which the check made before the build. It prints the image's path.
image_of() {
  count=0
  image=
  for entry in "$1"/*.dmg; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    count=$((count + 1))
    image=$entry
  done
  [ "$count" -eq 1 ] || { printf 'the build left %s disk images in %s, not one\n' "$count" "$1"; return 1; }
  if [ -L "$image" ] || [ ! -f "$image" ]; then
    printf 'the disk image %s is no file\n' "${image##*/}"
    return 1
  fi
  newer=$(find "$image" -prune -newer "$2") || { printf 'find could not read the time of the disk image %s\n' "${image##*/}"; return 1; }
  [ -n "$newer" ] || { printf 'the disk image %s is not newer than the start of the build\n' "${image##*/}"; return 1; }
  printf '%s\n' "$image"
}

# The pid of the one halvo-capture of the copy $2 whose parent is the app $1, where ps names a
# process by the path that started it; none, more than one, or one with another parent is the
# defect.
daemon_of() {
  listing=$(ps -A -o pid= -o ppid= -o comm=) || { printf 'ps could not list the processes\n'; return 1; }
  printf '%s\n' "$listing" | awk -v app="$1" -v exe="$2/Contents/MacOS/halvo-capture" '
    {
      path = $0
      sub(/^[ \t]*[0-9]+[ \t]+[0-9]+[ \t]+/, "", path)
    }
    path == exe {
      count++
      if ($2 == app) { found = $1 } else { other = $2 }
    }
    END {
      if (count == 0) { print "no halvo-capture of the copy runs"; exit 1 }
      if (count > 1) { printf "%d processes halvo-capture of the copy run, not one\n", count; exit 1 }
      if (found == "") { printf "the halvo-capture of the copy runs with the parent %s, not the app %s\n", other, app; exit 1 }
      print found
    }'
}

# Each process whose path, as ps names it, lies in the copy $1: its pid and its path in the copy.
copy_processes() {
  listing=$(ps -A -o pid= -o comm=) || return 1
  printf '%s\n' "$listing" | awk -v dir="$1/" '
    {
      path = $0
      sub(/^[ \t]*[0-9]+[ \t]+/, "", path)
    }
    index(path, dir) == 1 { printf "%s %s\n", $1, substr(path, length(dir) + 1) }'
}

no_copy_process() {
  left=$(copy_processes "$1") || { printf 'ps could not list the processes\n'; return 1; }
  [ -z "$left" ] && return 0
  printf '%s\n' "$left" | tr '\n' ' '
  return 1
}

gone() {
  ! kill -0 "$1" 2>/dev/null
}

# The status that the copy's ctl reads from the daemon of the data directory $2: it answers with
# the pid $3 and reads the store open.
store_open() {
  text=$("$1/Contents/MacOS/halvo-capture" ctl --data-dir "$2" status 2>&1)
  code=$?
  [ "$code" -eq 0 ] || { printf 'ctl status exited %s\n' "$code"; return 1; }
  if ! printf '%s\n' "$text" | grep -q -x -F "pid=$3"; then
    printf 'ctl status reads %s, not pid=%s\n' "$(printf '%s\n' "$text" | grep -m 1 '^pid=' || printf 'no pid line')" "$3"
    return 1
  fi
  if ! printf '%s\n' "$text" | grep -q -x -F 'store=open'; then
    printf 'ctl status reads %s, not store=open\n' "$(printf '%s\n' "$text" | grep -m 1 '^store=' || printf 'no store line')"
    return 1
  fi
}

# Calls "$@" once a second until it exits 0 or $1 seconds have passed; $answer holds its last
# output and $spent the seconds.
until_ok() {
  bound=$1
  shift
  spent=0
  while :; do
    answer=$("$@") && return 0
    [ "$spent" -ge "$bound" ] && return 1
    sleep 1
    spent=$((spent + 1))
  done
}

detach() {
  hdiutil detach "$1" >>"$CI_D_ARTIFACTS/detach.txt" 2>&1 && return 0
  sleep 2
  hdiutil detach -force "$1" >>"$CI_D_ARTIFACTS/detach.txt" 2>&1
}

# At every end of the check: the app that it started ends by its pid, and the image that it
# attached is detached.
cleanup() {
  if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid" 2>/dev/null
  fi
  if [ -n "$attached" ]; then
    detach "$attached" || printf 'note %s: hdiutil could not detach %s; see detach.txt\n' "$name" "$attached"
  fi
}

# The copy out of the image runs: its halvo, started hidden on a data directory that holds a
# state file, starts exactly one halvo-capture of the copy, whose status reads the store open; ctl
# stop ends it with exit 0, no second child starts, and after the app's end by TERM no process of
# the copy is left.
run_copy() {
  copy=$1 data_dir=$2
  mkdir -m 700 "$data_dir" || fail "mkdir could not make the data directory"
  printf '%s\n' '{"version":1,"desired":"paused"}' >"$data_dir/daemon-state.json" || fail "the state file of the data directory could not be written"
  chmod 600 "$data_dir/daemon-state.json" || fail "chmod could not set the mode of the state file"
  "$copy/Contents/MacOS/halvo" --background --data-dir "$data_dir" >"$CI_D_ARTIFACTS/app.out" 2>"$CI_D_ARTIFACTS/app.err" &
  app_pid=$!
  if ! until_ok "$wait_s" daemon_of "$app_pid" "$copy"; then
    state=runs
    if ! kill -0 "$app_pid" 2>/dev/null; then
      wait "$app_pid"
      state="ended with $?"
      app_pid=
    fi
    fail "$answer $spent s after the app's start, and the app $state; see app.err"
  fi
  daemon_pid=$answer
  printf 'daemon: pid %s, the app %s its parent, %s s after the start\n' "$daemon_pid" "$app_pid" "$spent"
  until_ok "$wait_s" store_open "$copy" "$data_dir" "$daemon_pid" || fail "$answer $spent s after the daemon's start"
  printf 'status: pid=%s store=open, %s s after the daemon was found\n' "$daemon_pid" "$spent"
  "$copy/Contents/MacOS/halvo-capture" ctl --data-dir "$data_dir" stop >"$CI_D_ARTIFACTS/stop.txt" 2>&1
  code=$?
  [ "$code" -eq 0 ] || fail "ctl stop exited $code; see stop.txt"
  until_ok "$wait_s" gone "$daemon_pid" || fail "the daemon $daemon_pid ran $wait_s s past ctl stop"
  printf 'stop: ctl stop exit 0, the daemon gone %s s after it\n' "$spent"
  watched=0
  while [ "$watched" -lt "$settle_s" ]; do
    sleep 1
    watched=$((watched + 1))
    found=$(daemon_of "$app_pid" "$copy") && fail "a second halvo-capture child, pid $found, started within $watched s of the daemon's end"
    [ "$found" = 'no halvo-capture of the copy runs' ] || fail "after the daemon's end: $found"
  done
  printf 'no second child: none in the %s s after the daemon was gone\n' "$settle_s"
  end='ended by TERM with'
  if kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid" 2>/dev/null
    until_ok "$wait_s" gone "$app_pid" || { kill -KILL "$app_pid" 2>/dev/null; fail "the app $app_pid ran $wait_s s past TERM and was killed"; }
  else
    end='had ended before TERM with'
  fi
  wait "$app_pid"
  end="$end exit $?"
  app_pid=
  until_ok "$wait_s" no_copy_process "$copy" || fail "$spent s after the app's end processes of the copy run: $answer"
  printf 'app end: the app %s, no process of the copy left %s s after it\n' "$end" "$spent"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  found=$(signing_defect) || fail "$found"
  found=$(config_defect .) || fail "$found"
  app_pid='' attached=''
  trap cleanup EXIT
  trap 'exit 2' HUP INT TERM
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  work=$(cd "$work" && pwd -P) || fail "the scratch directory has no physical path"
  : >"$work/build-started" || fail "the mark of the build's start could not be written"
  : >"$CI_D_ARTIFACTS/build.txt"
  if ! npm run tauri build -- -- --locked >>"$CI_D_ARTIFACTS/build.txt" 2>&1; then
    cause=$(grep -m 1 -E '^(error|Error|scripts/tauri.js:)' "$CI_D_ARTIFACTS/build.txt")
    fail "the build call failed: ${cause:-see build.txt in the artifacts}"
  fi
  helpers=$(helper_names)
  [ -n "$helpers" ] || fail "packaging/helpers.d names no helper"
  identifier=$(plutil -extract identifier raw -o - src-tauri/tauri.conf.json) || fail "plutil could not read the identifier of src-tauri/tauri.conf.json"
  version=$(sh scripts/r1/ci.d/61-version.sh --print) || fail "61-version.sh --print derived no version"
  bundle_dir=${CARGO_TARGET_DIR:-target}/release/bundle
  app=$bundle_dir/macos/Halvo.app
  # shellcheck disable=SC2086
  found=$(bundle_defect "$app" "$identifier" "$version" $helpers) || fail "$found"
  files=$(find "$app" -type f) || fail "find could not list Halvo.app"
  file_count=$(printf '%s\n' "$files" | grep -c .)
  helper_count=$(printf '%s\n' "$helpers" | grep -c .)
  printf 'Halvo.app files: %s, helpers: %s\n' "$file_count" "$helper_count"
  floor=$(plutil -extract bundle.macOS.minimumSystemVersion raw -o - "$macos_config") || fail "plutil could not read the bundle.macOS.minimumSystemVersion of $macos_config"
  floor_major=$(sed -n 's/^pub const MACOS_FLOOR_MAJOR: u32 = \([0-9][0-9]*\);$/\1/p' "$floor_source")
  [ -n "$floor_major" ] || fail "$floor_source holds no line pub const MACOS_FLOOR_MAJOR: u32 = <number>;"
  image=$(image_of "$bundle_dir/dmg" "$work/build-started") || fail "$image"
  printf 'disk image: %s, bytes: %s\n' "${image##*/}" "$(wc -c <"$image" | tr -d ' ')"
  mkdir "$work/mount" "$work/copy" || fail "mkdir could not make the mount point and the copy's directory"
  hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$work/mount" "$image" >"$CI_D_ARTIFACTS/attach.txt" 2>&1 || fail "hdiutil could not attach the disk image ${image##*/}; see attach.txt"
  attached=$work/mount
  [ -d "$work/mount/Halvo.app" ] || fail "the disk image ${image##*/} holds no Halvo.app"
  copy=$work/copy/Halvo.app
  ditto "$work/mount/Halvo.app" "$copy" || fail "ditto could not copy Halvo.app out of the disk image"
  detach "$work/mount" || fail "hdiutil could not detach the disk image; see detach.txt"
  attached=
  found=$(floor_defect "$copy/Contents/Info.plist" "$floor" "$floor_major") || fail "$found"
  printf 'floor: LSMinimumSystemVersion %s, MACOS_FLOOR_MAJOR %s\n' "$floor" "$floor_major"
  found=$(seal_defect "$copy") || fail "$found"
  codesign -dv "$copy" 2>&1 | grep -E '^(Format|CodeDirectory|Signature|Info\.plist|TeamIdentifier|Sealed Resources)' | sed 's/^/codesign: /'
  run_copy "$copy" "$work/data"
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
  floor=14.0
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
  <key>LSMinimumSystemVersion</key>
  <string>$floor</string>
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

  # A case of a rule below is rejected when its function exits 1 and prints the wanted line.
  rule_case() {
    case_name=$1 want=$2
    shift 2
    output=$("$@" 2>"$work/$case_name.err")
    code=$?
    got=unexpected-exit-$code
    [ "$code" = 0 ] && got=PASS
    [ "$code:$output" = "1:$want" ] && got=FAIL
    verdict "$case_name" FAIL "$got"
  }
  # Runs a function with the stand-in programs of the directory $1 first on PATH; rule_case
  # calls it in a command substitution, so the PATH of the self-test stays as it is.
  with_tools() {
    tools=$1
    shift
    PATH="$tools:$PATH" "$@"
  }
  stand_in_program() {
    directory=$work/$1 program=$2
    shift 2
    mkdir -p "$directory" || exit 2
    printf '%s\n' '#!/bin/sh' "$@" >"$directory/$program" || exit 2
    chmod +x "$directory/$program" || exit 2
  }

  stand_in_bundle refused-by-codesign halvo halvo-capture
  stand_in_program refusing-tools codesign 'echo "code object is not signed at all" >&2' 'exit 1'
  rule_case seal-that-codesign-refuses 'codesign --verify --deep --strict refuses Halvo.app: code object is not signed at all' with_tools "$work/refusing-tools" seal_defect "$app"

  floor=10.13
  stand_in_bundle with-another-floor halvo halvo-capture
  floor=14.0
  rule_case bundle-with-another-floor "the LSMinimumSystemVersion of Info.plist is 10.13, not the minimumSystemVersion 14.0 of $macos_config" floor_defect "$app/Contents/Info.plist" 14.0 14

  image_tree=$work/build-without-a-disk-image
  mkdir -p "$image_tree/release/bundle/dmg" || exit 2
  : >"$image_tree/started" || exit 2
  printf '%s\n' '#!/bin/sh' >"$image_tree/release/bundle/dmg/bundle_dmg.sh" || exit 2
  rule_case build-without-a-disk-image "the build left 0 disk images in $image_tree/release/bundle/dmg, not one" image_of "$image_tree/release/bundle/dmg" "$image_tree/started"

  stand_in_program other-parent-tools ps "echo '  4100     1 /stand-in/Halvo.app/Contents/MacOS/halvo'" "echo '  4101     1 /stand-in/Halvo.app/Contents/MacOS/halvo-capture'"
  rule_case daemon-whose-parent-is-not-the-app 'the halvo-capture of the copy runs with the parent 1, not the app 4100' with_tools "$work/other-parent-tools" daemon_of 4100 /stand-in/Halvo.app

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
