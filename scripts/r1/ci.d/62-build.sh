#!/bin/sh
# jobs: macos windows
# rule: The release build of halvo, halvo-capture, halvo-owner, halvo-analysis and halvo-runner, made by calls that name no other package and no feature, configuration or test target, takes every tauri package that cargo resolves for the host from the crates.io registry, with debug assertions explicitly off in the workspace release profile and no release package override that enables them, with no untracked cargo configuration (.cargo/config or .cargo/config.toml at any depth of the checkout) and no tracked one at any depth that the list allowed_configurations of this script does not name, which holds .cargo/config.toml at the root alone (none outside the checkout is read), prints the derived version from halvo, halvo-capture and halvo-runner (halvo-owner and halvo-analysis have no --version argument, and the check starts neither), changes no file of the checkout, makes halvo-owner and halvo-analysis as files in the directory of halvo-capture, where the daemon looks for a program that it starts, holds no marker of the list markers (a stand-in, the test seam of halvo-capture, the test fence of halvo-web, the loopback route of halvo-connect) by a scan of every shipped file that stands as the backstop, imports no forbidden_import (the Carbon call that holds secure input) in any of the five built macOS executables as read by dyld_info regardless of build flags or CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS (a recognized row contains indentation, an optional hexadecimal ordinal, the symbol, an optional hexadecimal addend, an optional [weak-import] mark and (from library), with internal blanks allowed in the nonempty library name, and the check logs the row, ordinal, addend and weak-import counts per executable and slice, and a tool error, a missing slice header or import header, an empty table or an unrecognized nonblank line fails the check, which trusts dyld_info to enumerate every slice and its named imports, the import table shows what the shipped executable can call through a named import and omits an unreachable hold compiled into the capture library, which check 51 covers with proof builds, and Windows has no import check), and is what the artifacts directory holds, on macOS the app as Halvo.app.tar and on Windows the four executables, and no signing variable is set in its environment, because this run builds unsigned (--rules reads only the work-tree workspace manifest and Cargo configuration paths).
set -u
name=62-build
forbidden_import=_EnableSecureEventInput
shipped_packages='halvo halvo-capture halvo-owner halvo-analysis halvo-runner'
scope_flags='--workspace --all --all-targets --tests --test --benches --bench --examples --example --lib'
feature_flags='--features -F -f --all-features --no-default-features'
configuration_flags='--config -c'
allowed_configurations='.cargo/config.toml'
markers='halvo-standin-module-present
halvo-capture:test-seam:this-build-has-it!
halvo-web:test-fence:this-build-has-it!
halvo-connect-loopback-route-of-tests'
macos_calls='npm run tauri build -- -- --locked
cargo build --release --locked -p halvo-capture --bin halvo-capture
cargo build --release --locked -p halvo-owner --bin halvo-owner
cargo build --release --locked -p halvo-analysis --bin halvo-analysis
cargo build --release --locked -p halvo-runner --bin halvo-runner'
windows_calls='npm run tauri build -- --no-bundle -- --locked
cargo build --release --locked -p halvo-capture --bin halvo-capture
cargo build --release --locked -p halvo-owner --bin halvo-owner
cargo build --release --locked -p halvo-analysis --bin halvo-analysis
cargo build --release --locked -p halvo-runner --bin halvo-runner'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

call_defect() {
  expect_package=0
  for word in $1; do
    if [ "$expect_package" = 1 ]; then
      expect_package=0
      case " $shipped_packages " in
        *" $word "*) continue ;;
        *) printf 'the call names the package %s, which does not ship\n' "$word"; return 1 ;;
      esac
    fi
    case $word in
      -p|--package) expect_package=1 ;;
      --package=*)
        case " $shipped_packages " in
          *" ${word#--package=} "*) ;;
          *) printf 'the call names the package %s, which does not ship\n' "${word#--package=}"; return 1 ;;
        esac
        ;;
      -p?*) printf 'the call names a package in the form %s, which this check does not read\n' "$word"; return 1 ;;
    esac
    case $word in
      --*=*) named_flag=${word%%=*} ;;
      -[!-]?*) rest=${word#-?}; named_flag=${word%"$rest"} ;;
      *) named_flag=$word ;;
    esac
    case " $scope_flags " in
      *" $named_flag "*) printf 'the call has %s, which builds more than the shipped binaries\n' "$word"; return 1 ;;
    esac
    case " $feature_flags " in
      *" $named_flag "*) printf 'the call has %s, which changes the features of what ships\n' "$word"; return 1 ;;
    esac
    case " $configuration_flags " in
      *" $named_flag "*) printf 'the call has %s, which gives the build a configuration that no tracked file holds\n' "$word"; return 1 ;;
    esac
  done
  [ "$expect_package" = 0 ] || { printf 'the call ends after a package flag\n'; return 1; }
}

signing_names='APPLE_SIGNING_IDENTITY APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_API_KEY APPLE_API_KEY_PATH APPLE_API_ISSUER APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID HALVO_WINDOWS_SIGN_COMMAND'

signing_defect() {
  for variable in $signing_names; do
    value=$(printenv "$variable") || continue
    [ -n "$value" ] || continue
    printf 'the signing variable %s is set, and this run builds unsigned\n' "$variable"
    return 1
  done
}

environment_defect() {
  [ -z "${TAURI_CONFIG:-}" ] && return 0
  printf 'the variable TAURI_CONFIG is set, so the build reads a configuration that no tracked file holds\n'
  return 1
}

tauri_source_defect() {
  packages=$(tr -d '\n' <"$1" | grep -o -E '\{"name":"tauri[^"]*","version":"[^"]*","id":"[^"]*"')
  [ -n "$packages" ] || { printf 'cargo metadata lists no package whose name starts with tauri\n'; return 1; }
  other=$(printf '%s\n' "$packages" | grep -v '"id":"registry+https://github.com/rust-lang/crates.io-index#' | sed -n 's/^{"name":"\([^"]*\)".*/\1/p' | head -n 1)
  [ -z "$other" ] || { printf 'cargo resolves the package %s from another source than the crates.io registry\n' "$other"; return 1; }
}

cargo_config_defect() {
  untracked=$1
  shift
  if [ -n "$untracked" ]; then
    first=$(printf '%s\n' "$untracked" | head -n 1)
    printf 'the cargo configuration %s is no tracked file; check 62 refuses an untracked .cargo/config or .cargo/config.toml at any depth of the checkout\n' "$first"
    return 1
  fi
  for file in "$@"; do
    case " $allowed_configurations " in
      *" $file "*) continue ;;
    esac
    printf 'the cargo configuration %s is tracked and is not on the list allowed_configurations of scripts/r1/ci.d/62-build.sh; a unit that needs one adds it to that list with its review\n' "$file"
    return 1
  done
}

marker_defect() {
  for file in "$@"; do
    [ -f "$file" ] || continue
    while IFS= read -r marker; do
      [ -n "$marker" ] || continue
      if LC_ALL=C grep -q -a -F -e "$marker" "$file"; then
        printf '%s holds the marker %s\n' "$(basename "$file")" "$marker"
        return 1
      fi
    done <<EOF
$markers
EOF
  done
}

beside_defect() {
  daemon=$1
  shift
  for child in "$@"; do
    [ -f "$child" ] && continue
    printf 'the build made no %s beside %s, where the daemon looks for it\n' "${child##*/}" "${daemon##*/}"
    return 1
  done
}

release_imports() {
  for binary in "$@"; do
    executable=${binary##*/}
    if ! imports=$(xcrun dyld_info -imports "$binary"); then
      printf 'dyld_info -imports %s:\n%s\n' "$executable" "$imports"
      fail "dyld_info could not read the imports of $executable"
    fi
    result=$(LC_ALL=C awk -v symbol="$forbidden_import" -v executable="$executable" '
      function import_row(line) {
        if (!sub(/^[[:blank:]]+/, "", line)) return 0
        has_ordinal = sub(/^0x([0-9A-F]{4}|[1-9A-F][0-9A-F]{4,7})[[:blank:]]+/, "", line)
        if (!sub(/^[[:alpha:]_.$][[:alnum:]_.$]*/, "", line)) return 0
        has_addend = sub(/^\+0x(0|[1-9A-F][0-9A-F]{0,15})/, "", line)
        if (!sub(/^[[:blank:]]+/, "", line)) return 0
        has_weak_import = sub(/^\[weak-import\][[:blank:]]+/, "", line)
        return line ~ /^\(from [^()[:space:]]+([[:blank:]]+[^()[:space:]]+)*\)$/
      }
      function unfinished() {
        if (state == "rows") return ""
        if (state == "table") return "an empty import table"
        return "no import table"
      }
      function report_table() {
        if (state == "table" || state == "rows")
          printf "dyld_info -imports %s %s rows=%d ordinal=%d addend=%d weak-import=%d\n", executable, slice, rows, ordinals, addends, weak_imports >> "/dev/stderr"
      }
      BEGIN { needle = "(^|[^[:alnum:]_])" symbol "([^[:alnum:]_]|$)" }
      { raw = $0; sub(/\r$/, "") }
      /^[[:blank:]]*$/ { next }
      /^[^[:space:]].* \[[[:alnum:]_.-]+\]:$/ {
        if (state && (reason = unfinished()) != "") exit
        report_table()
        slice = $NF
        rows = ordinals = addends = weak_imports = 0
        state = "slice"
        next
      }
      state == "slice" && /^[[:blank:]]*-imports:[[:blank:]]*$/ { state = "table"; next }
      !state || state == "slice" { reason = "no import table"; exit }
      $0 ~ needle && !matched { matched = raw }
      import_row($0) {
        state = "rows"
        rows++
        ordinals += has_ordinal
        addends += has_addend
        weak_imports += has_weak_import
        next
      }
      { reason = "an unrecognized import listing"; exit }
      END {
        report_table()
        if (matched) { print matched; exit 3 }
        if (!reason) reason = unfinished()
        if (reason) { print reason; exit 1 }
      }
    ' <<EOF
$imports
EOF
    )
    case $? in
      0) ;;
      3) printf 'dyld_info -imports %s:\n%s\n' "$executable" "$result"
         fail "$executable imports $forbidden_import" ;;
      *) printf 'dyld_info -imports %s:\n%s\n' "$executable" "$imports"
         fail "dyld_info returned $result for $executable" ;;
    esac
    printf 'dyld_info -imports %s: absent %s\n' "$executable" "$forbidden_import"
  done
}

tar_defect() {
  [ -s "$1" ] || { printf 'the listing of Halvo.app.tar is empty\n'; return 1; }
  outside=$(awk '{ path = $NF } path != "Halvo.app" && path != "Halvo.app/" && index(path, "Halvo.app/") != 1 { n++ } END { print n + 0 }' "$1")
  [ "$outside" = 0 ] || { printf 'Halvo.app.tar holds %s paths outside Halvo.app/\n' "$outside"; return 1; }
  executable=$(awk '$NF == "Halvo.app/Contents/MacOS/halvo" && $1 ~ /^-..x/ { n++ } END { print n + 0 }' "$1")
  [ "$executable" = 1 ] || { printf 'Halvo.app.tar holds no executable Halvo.app/Contents/MacOS/halvo\n'; return 1; }
}

untracked_configuration_defect() {
  for path in "$@"; do
    case $path in
      src-tauri/tauri*.json*|src-tauri/Tauri*.toml)
        printf 'the untracked file %s is a tauri configuration that the build reads\n' "$path"
        return 1
        ;;
    esac
  done
}

build_root_for() {
  if [ "$1" = tag ]; then
    printf 'fresh\n'
  else
    printf 'cached\n'
  fi
}

run_call() {
  # shellcheck disable=SC2086
  set -- $1
  "$@" >>"$CI_D_ARTIFACTS/build.txt" 2>&1
}

file_rules() {
  git ls-files --error-unmatch -- Cargo.toml >/dev/null 2>&1 || fail "the workspace Cargo.toml must be tracked"
  python=
  for candidate in python3.11 python3 python; do
    probe=$("$candidate" -I -c '
import sys
import tomllib

if sys.version_info < (3, 11):
    sys.exit(1)
answer = sum(tomllib.loads("values = [19, 23]")["values"])
version = ".".join(map(str, sys.version_info[:3]))
sys.stdout.write(f"{answer} {version}")' </dev/null 2>/dev/null) || continue
    version=$(expr "$probe" : '42 \([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$') || continue
    python=$candidate
    break
  done
  [ -n "$python" ] || fail "reading Cargo.toml requires Python 3.11 or later"
  printf 'manifest reader: %s %s\n' "$python" "$version" >&2
  found=$(
    {
      "$python" -I - 2>&1 <<'PY'
import sys
import tomllib

sys.stderr.reconfigure(newline="\n")
try:
    with open("Cargo.toml", "rb") as manifest:
        release = tomllib.load(manifest).get("profile", {}).get("release", {})
    if release.get("debug-assertions") is not False:
        sys.exit("Cargo.toml must set profile.release.debug-assertions to false")
    for package, options in release.get("package", {}).items():
        if options.get("debug-assertions") is True:
            sys.exit(f"Cargo.toml enables release debug assertions for package {package}")
except (OSError, ValueError, AttributeError) as error:
    sys.exit(f"could not read the release profile in Cargo.toml: {error}")
sys.stdout.write("profile-ok")
PY
      printf '/%s' "$?"
    } | LC_ALL=C tr '\000' '?'
  ) || fail "could not read the Cargo.toml reader output"
  case $found in
    profile-ok/0) ;;
    */0) fail "unexpected Cargo.toml reader output: ${found%/0}" ;;
    *) cause=$(printf '%s' "${found%/*}"); fail "$cause" ;;
  esac

  set -- '.cargo/config' '.cargo/config.toml' '*/.cargo/config' '*/.cargo/config.toml'
  untracked_configurations=$(git ls-files --others -- "$@") || fail "git ls-files failed"
  tracked_configurations=$(git ls-files -- "$@") || fail "git ls-files failed"
  found=$(cargo_config_defect "$untracked_configurations") || fail "$found"
  while IFS= read -r configuration; do
    [ -n "$configuration" ] || continue
    found=$(cargo_config_defect '' "$configuration") || fail "$found"
  done <<EOF
$tracked_configurations
EOF
}

rules() {
  printf 'rules: workspace manifest and Cargo configuration paths only (binary imports require the macOS build)\n'
  cd "${CI_D_REPO:-.}" || exit 2
  root=$(git rev-parse --show-toplevel) || fail "no git checkout"
  cd "$root" || exit 2
  file_rules
  printf 'PASS %s\n' "$name"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  case $CI_D_JOB in
    macos) calls=$macos_calls ;;
    windows) calls=$windows_calls ;;
    *) fail "no build for the job $CI_D_JOB" ;;
  esac
  while IFS= read -r call; do
    found=$(call_defect "$call") || fail "$found"
  done <<EOF
$calls
EOF
  found=$(environment_defect) || fail "$found"
  if ! found=$(signing_defect); then
    [ "${CI:-}" = true ] && fail "$found"
    printf 'SKIP %s (%s)\n' "$name" "$found"
    exit 77
  fi
  file_rules
  others=$(git ls-files --others -- src-tauri) || fail "git ls-files failed"
  # shellcheck disable=SC2086
  found=$(untracked_configuration_defect $others) || fail "$found"
  if [ ! -f node_modules/@tauri-apps/cli/package.json ]; then
    [ "${CI:-}" = true ] && fail "node_modules holds no @tauri-apps/cli; the Prerequisites step runs npm ci"
    printf 'SKIP %s (node_modules holds no @tauri-apps/cli; run npm ci)\n' "$name"
    exit 77
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  ruled=$(sh scripts/r1/ci.d/61-version.sh)
  status=$?
  if [ "$status" -ne 0 ]; then
    cause=$(printf '%s\n' "$ruled" | sed -n 's/^FAIL 61-version (\(.*\))$/\1/p')
    fail "before the build: ${cause:-61-version exited $status}"
  fi
  build_root=$(build_root_for "${GITHUB_REF_TYPE:-}")
  if [ "$build_root" = fresh ]; then
    CARGO_TARGET_DIR=$work/target
    CARGO_HOME=$work/cargo-home
    export CARGO_TARGET_DIR CARGO_HOME
  fi
  fresh=false
  [ "$build_root" = cached ] || fresh=true
  printf 'fresh target and cargo home: %s\n' "$fresh"
  target=${CARGO_TARGET_DIR:-target}
  host=$(rustc -vV | sed -n 's/^host: //p')
  [ -n "$host" ] || fail "rustc -vV names no host triple"
  cargo metadata --locked --format-version 1 --filter-platform "$host" >"$work/metadata.json" 2>"$work/metadata.err" || {
    cause=$(grep -m 1 '^error' "$work/metadata.err")
    fail "cargo metadata --locked failed: $cause"
  }
  found=$(tauri_source_defect "$work/metadata.json") || fail "$found"
  before=$(git status --porcelain --untracked-files=all) || fail "git status failed"
  : >"$CI_D_ARTIFACTS/build.txt"
  while IFS= read -r call; do
    if ! run_call "$call"; then
      cause=$(grep -m 1 -E '^(error|Error)' "$CI_D_ARTIFACTS/build.txt")
      fail "the build call failed: ${cause:-see build.txt in the artifacts}"
    fi
  done <<EOF
$calls
EOF
  after=$(git status --porcelain --untracked-files=all) || fail "git status failed"
  [ "$before" = "$after" ] || fail "the build changed a file of the checkout"
  case $CI_D_JOB in
    macos)
      app=$target/release/bundle/macos/Halvo.app
      [ -d "$app" ] || fail "the build made no Halvo.app"
      set -- "$app/Contents/MacOS/halvo" "$target/release/halvo-capture"
      owner=$target/release/halvo-owner
      analysis=$target/release/halvo-analysis
      runner=$target/release/halvo-runner
      found=$(beside_defect "$2" "$owner" "$analysis") || fail "$found"
      release_imports "$@" "$owner" "$analysis" "$runner"
      ;;
    windows)
      set -- "$target/release/halvo.exe" "$target/release/halvo-capture.exe"
      owner=$target/release/halvo-owner.exe
      analysis=$target/release/halvo-analysis.exe
      runner=$target/release/halvo-runner.exe
      found=$(beside_defect "$2" "$owner" "$analysis") || fail "$found"
      ;;
  esac
  checked=$(sh scripts/r1/ci.d/61-version.sh --check "$@" "$runner")
  status=$?
  printf '%s\n' "$checked" | grep -v -E '^(PASS|FAIL) 61-version'
  if [ "$status" -ne 0 ]; then
    cause=$(printf '%s\n' "$checked" | sed -n 's/^FAIL 61-version (\(.*\))$/\1/p')
    fail "${cause:-61-version --check exited $status}"
  fi
  case $CI_D_JOB in
    macos)
      shipped=$(find "$app" -type f) || fail "find could not list Halvo.app"
      shipped="$shipped
$target/release/halvo-capture
$owner
$analysis
$runner"
      ;;
    windows)
      shipped="$1
$2
$owner
$analysis
$runner"
      ;;
  esac
  shipped_count=$(printf '%s\n' "$shipped" | grep -c .)
  found=$(printf '%s\n' "$shipped" | while IFS= read -r file; do marker_defect "$file" || exit 1; done) || fail "$found"
  printf 'shipped files read for the markers: %s\n' "$shipped_count"
  case $CI_D_JOB in
    macos)
      COPYFILE_DISABLE=1 tar -cf "$CI_D_ARTIFACTS/Halvo.app.tar" -C "$target/release/bundle/macos" Halvo.app || fail "tar could not pack Halvo.app"
      tar -tvf "$CI_D_ARTIFACTS/Halvo.app.tar" >"$work/listing.txt" || fail "tar could not list Halvo.app.tar"
      found=$(tar_defect "$work/listing.txt") || fail "$found"
      entries=$(grep -c . "$work/listing.txt")
      printf 'Halvo.app.tar entries: %s\n' "$entries"
      ;;
    windows)
      cp "$1" "$2" "$owner" "$analysis" "$CI_D_ARTIFACTS/" || fail "cp could not copy the four executables"
      for kept in halvo.exe halvo-capture.exe halvo-owner.exe halvo-analysis.exe; do
        [ -f "$CI_D_ARTIFACTS/$kept" ] || fail "the artifacts directory lacks $kept"
      done
      for kept in halvo-capture.exe halvo-owner.exe halvo-analysis.exe; do
        bytes=$(wc -c <"$CI_D_ARTIFACTS/$kept" | tr -d ' ')
        printf '%s bytes=%s\n' "$kept" "$bytes"
      done
      ;;
  esac
  printf 'PASS %s\n' "$name"
}

# Whether the file $1 is a helper with no version argument: its name, without .exe, is the name
# of a file of packaging/helpers.d, the installed name of a helper, and that file holds the line
# version=none. A carriage return at the end of the line, as a Windows checkout can leave one, is
# dropped.
unversioned() {
  helper=${1##*/}
  helper=${helper%.exe}
  [ -f "packaging/helpers.d/$helper" ] || return 1
  tr -d '\r' <"packaging/helpers.d/$helper" | grep -q -x -F -e 'version=none'
}

files_form() {
  [ $# -gt 0 ] || { printf 'usage: sh scripts/r1/ci.d/%s.sh --files <file>...\n' "$name" >&2; exit 64; }
  for file in "$@"; do
    [ -f "$file" ] || fail "${file##*/} is no file"
  done
  found=$(marker_defect "$@") || fail "$found"
  system=$(uname -s) || fail "uname -s failed"
  if [ "$system" = Darwin ]; then
    release_imports "$@"
  fi
  read_files=$#
  for file in "$@"; do
    if unversioned "$file"; then
      printf '%s: read and not started, its file of packaging/helpers.d says version=none\n' "${file##*/}"
    else
      set -- "$@" "$file"
    fi
  done
  shift "$read_files"
  if [ $# -gt 0 ]; then
    checked=$(sh scripts/r1/ci.d/61-version.sh --check "$@")
    status=$?
    printf '%s\n' "$checked" | grep -v -E '^(PASS|FAIL) 61-version'
    if [ "$status" -ne 0 ]; then
      cause=$(printf '%s\n' "$checked" | sed -n 's/^FAIL 61-version (\(.*\))$/\1/p')
      fail "${cause:-61-version --check exited $status}"
    fi
  fi
  printf 'files read: %s\n' "$read_files"
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
  call_case() {
    got=FAIL
    call_defect "$3" >/dev/null && got=PASS
    verdict "$1" "$2" "$got"
  }
  number=0
  while IFS= read -r call; do
    number=$((number + 1))
    call_case "shipped-call-$number" PASS "$call"
  done <<EOF
$macos_calls
$windows_calls
EOF
  call_case gate-package FAIL 'cargo build --release --locked -p halvo-capture -p halvo-agent-gate'
  call_case gate-package-with-equals FAIL 'cargo build --release --locked --package=halvo-agent-gate'
  call_case package-glued-to-its-flag FAIL 'cargo build --release -phalvo-agent-gate'
  call_case runner-package PASS 'cargo build --release --locked -p halvo-runner --bin halvo-runner'
  call_case whole-workspace FAIL 'cargo build --release --locked --workspace'
  call_case test-targets FAIL 'cargo build --release --locked -p halvo-capture --tests'
  call_case all-targets FAIL 'cargo build --release --locked -p halvo-capture --all-targets'
  call_case a-feature FAIL 'cargo build --release --locked -p halvo-capture --features test-seam'
  call_case a-feature-of-tauri FAIL 'npm run tauri build -- --features tauri/unstable'
  call_case a-configuration FAIL 'npm run tauri build -- --config src-tauri/other.json'
  call_case a-package-flag-at-the-end FAIL 'cargo build --release -p'
  call_case gate-package-long-flag FAIL 'cargo build --release --locked --package halvo-agent-gate'
  for listed_flag in $scope_flags $feature_flags $configuration_flags; do
    call_case "the-flag$listed_flag" FAIL "cargo build --release --locked -p halvo-capture $listed_flag value"
    case $listed_flag in
      --*) call_case "the-flag$listed_flag-with-equals" FAIL "cargo build --release --locked -p halvo-capture $listed_flag=value" ;;
      *) call_case "the-flag$listed_flag-glued-to-its-value" FAIL "cargo build --release --locked -p halvo-capture ${listed_flag}value" ;;
    esac
  done
  got=FAIL
  TAURI_CONFIG='{"build":{}}' environment_defect >/dev/null && got=PASS
  verdict tauri-configuration-in-a-variable FAIL "$got"
  got=FAIL
  (unset TAURI_CONFIG; environment_defect >/dev/null) && got=PASS
  verdict environment-without-those-variables PASS "$got"

  printf 'head\000\001\002%s\000tail' 'halvo-standin-module-present' >"$work/standin.bin"
  printf 'head\000%s\377' 'halvo-capture:test-seam:this-build-has-it!' >"$work/seam.bin"
  printf 'head\000\377halvo-standin-module\000present' >"$work/clean.bin"
  printf 'head\000%s\000tail' 'halvo-web:test-fence:this-build-has-it!' >"$work/fence.bin"
  printf 'head\000%s\000tail' 'halvo-connect-loopback-route-of-tests' >"$work/loopback.bin"
  for fixture in standin:FAIL seam:FAIL clean:PASS fence:FAIL loopback:FAIL; do
    got=FAIL
    marker_defect "$work/${fixture%:*}.bin" >/dev/null && got=PASS
    verdict "marker-${fixture%:*}" "${fixture#*:}" "$got"
  done
  mkdir -p "$work/unreadable-tools" && printf '%s\n' '#!/bin/sh' 'echo "grep: Permission denied" >&2' 'exit 2' >"$work/unreadable-tools/grep" && chmod +x "$work/unreadable-tools/grep" || exit 2
  got=$( (PATH="$work/unreadable-tools:$PATH" && marker_defect "$work/clean.bin") 2>"$work/unreadable.err")
  case $?:$got in "1:grep could not read $work/clean.bin, so its markers are unknown") got=FAIL ;; 0:*) got=PASS ;; *) got=unexpected-answer ;; esac
  verdict marker-file-that-cannot-be-read FAIL "$got"

  listing_case() {
    printf '%s\n' "$3" >"$work/$1.txt"
    got=FAIL
    tar_defect "$work/$1.txt" >/dev/null && got=PASS
    verdict "$1" "$2" "$got"
  }
  listing_case tar-of-the-bundle PASS 'drwxr-xr-x  0 runner staff       0 Nov 14 06:00 Halvo.app/
drwxr-xr-x  0 runner staff       0 Nov 14 06:00 Halvo.app/Contents/
-rwxr-xr-x  0 runner staff 9000000 Nov 14 06:00 Halvo.app/Contents/MacOS/halvo'
  listing_case tar-with-contents-at-its-root FAIL 'drwxr-xr-x  0 runner staff       0 Nov 14 06:00 Contents/
-rwxr-xr-x  0 runner staff 9000000 Nov 14 06:00 Contents/MacOS/halvo'
  listing_case tar-without-the-executable-bit FAIL 'drwxr-xr-x  0 runner staff       0 Nov 14 06:00 Halvo.app/
-rw-r--r--  0 runner staff 9000000 Nov 14 06:00 Halvo.app/Contents/MacOS/halvo'
  listing_case tar-with-an-appledouble-entry FAIL 'drwxr-xr-x  0 runner staff       0 Nov 14 06:00 Halvo.app/
-rwxr-xr-x  0 runner staff 9000000 Nov 14 06:00 Halvo.app/Contents/MacOS/halvo
-rw-r--r--  0 runner staff     163 Nov 14 06:00 ./._Halvo.app'
  listing_case empty-tar FAIL ''

  got=FAIL
  untracked_configuration_defect src-tauri/target-note.txt src-tauri/gen/schemas/a.json >/dev/null && got=PASS
  verdict untracked-files-that-are-no-configuration PASS "$got"
  got=FAIL
  untracked_configuration_defect src-tauri/tauri.macos.conf.json >/dev/null && got=PASS
  verdict untracked-platform-configuration FAIL "$got"
  got=FAIL
  untracked_configuration_defect src-tauri/Tauri.toml >/dev/null && got=PASS
  verdict untracked-toml-configuration FAIL "$got"

  for variable in $signing_names; do
    got=FAIL
    (export "$variable=stand-in-value" && signing_defect >/dev/null) && got=PASS
    verdict "signing-name-$variable-set" FAIL "$got"
  done
  for variable in APPLE_SIGNING_IDENTITY APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_API_KEY APPLE_API_KEY_PATH APPLE_API_ISSUER APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID HALVO_WINDOWS_SIGN_COMMAND; do
    got=FAIL
    (export "$variable=stand-in-value" && signing_defect >/dev/null) && got=PASS
    verdict "signing-name-$variable-of-rl-1-set" FAIL "$got"
  done
  got=FAIL
  (HALVO_WINDOWS_SIGN_COMMAND=stand-in-command signing_defect >/dev/null) && got=PASS
  verdict windows-signing-name-set FAIL "$got"
  got=FAIL
  (APPLE_ID='' signing_defect >/dev/null) && got=PASS
  verdict one-signing-name-empty PASS "$got"

  registry='registry+https://github.com/rust-lang/crates.io-index'
  source_case() {
    printf '{"packages":[%s],"workspace_members":[]}\n' "$3" >"$work/$1.json"
    got=FAIL
    tauri_source_defect "$work/$1.json" >/dev/null && got=PASS
    verdict "$1" "$2" "$got"
  }
  source_case tauri-from-the-registry PASS "{\"name\":\"tauri\",\"version\":\"2.12.5\",\"id\":\"$registry#tauri@2.12.5\"},{\"name\":\"tauri-build\",\"version\":\"2.7.4\",\"id\":\"$registry#tauri-build@2.7.4\"}"
  source_case tauri-from-a-path FAIL "{\"name\":\"tauri\",\"version\":\"2.12.5\",\"id\":\"path+file:///w/tauri#2.12.5\"},{\"name\":\"tauri-build\",\"version\":\"2.7.4\",\"id\":\"$registry#tauri-build@2.7.4\"}"
  source_case tauri-build-from-git FAIL "{\"name\":\"tauri\",\"version\":\"2.12.5\",\"id\":\"$registry#tauri@2.12.5\"},{\"name\":\"tauri-build\",\"version\":\"2.7.4\",\"id\":\"git+https://example.invalid/t#abc\"}"
  source_case no-tauri-package FAIL "{\"name\":\"serde\",\"version\":\"1.0.0\",\"id\":\"$registry#serde@1.0.0\"}"
  tracked_config_case() {
    case_name=$1
    shift
    printf '%s\n' "$@" >"$work/$case_name.toml"
    got=FAIL
    cargo_config_defect '' "$work/$case_name.toml" >/dev/null && got=PASS
    verdict "tracked-configuration-$case_name" FAIL "$got"
  }
  tracked_config_case table-header '[source.crates-io]' 'replace-with = "vendored"' '[source.vendored]' 'directory = "no-such-vendor-dir"'
  tracked_config_case dotted-keys-at-top-level 'source.crates-io.replace-with = "vendored"' 'source.vendored.directory = "no-such-vendor-dir"'
  tracked_config_case spaced-table-header '[ source.crates-io ]' 'replace-with = "vendored"' '[ source . vendored ]' 'directory = "no-such-vendor-dir"'
  tracked_config_case inline-table 'source = { crates-io = { replace-with = "vendored" }, vendored = { directory = "no-such-vendor-dir" } }'
  tracked_config_case quoted-key-header '["source"."crates-io"]' 'replace-with = "vendored"' '["source".vendored]' 'directory = "no-such-vendor-dir"'
  tracked_config_case plain-header '[build]' 'jobs = 2'
  tracked_config_case include-line 'include = "other.toml"'
  got=FAIL
  cargo_config_defect '' >/dev/null && got=PASS
  verdict no-tracked-configuration PASS "$got"
  got=FAIL
  (allowed_configurations="$work/plain-header.toml"; cargo_config_defect '' "$work/plain-header.toml" >/dev/null) && got=PASS
  verdict tracked-configuration-on-the-list PASS "$got"
  got=FAIL
  cargo_config_defect '.cargo/config.toml' >/dev/null && got=PASS
  verdict untracked-cargo-configuration-at-the-root FAIL "$got"
  got=FAIL
  cargo_config_defect 'src-tauri/.cargo/config' >/dev/null && got=PASS
  verdict untracked-cargo-configuration-below-the-root FAIL "$got"

  output_has_line() {
    line_feed='
'
    case "$line_feed$2$line_feed" in
      *"$line_feed$1$line_feed"*) return 0 ;;
      *) return 1 ;;
    esac
  }
  got=FAIL
  output_has_line 'partial' 'partial row' && got=PASS
  verdict diagnostic-requires-whole-line FAIL "$got"
  got=FAIL
  output_has_line '-n' '-n' && got=PASS
  verdict diagnostic-allows-leading-dash PASS "$got"
  imports_case() {
    case_name=$1 want=$2 wanted_text=$3
    shift 3
    output=$(
      awk() {
        if [ "${verify_import_locale:-}" = yes ] && [ "${LC_ALL:-}" != C ]; then return 2; fi
        command awk "$@"
      }
      xcrun() {
        [ "$#" = 3 ] && [ "$1" = dyld_info ] && [ "$2" = -imports ] || return 2
        cat "$3" 2>/dev/null || return 1
        return "${tool_status:-0}"
      }
      release_imports "$@" 2>"$work/imports.err"
    )
    code=$?
    case $code:$output in
      0:*"$wanted_text"*) got=PASS ;;
      1:*"FAIL $name ($wanted_text)"*) got=FAIL ;;
      *) got="unexpected-exit-$code" ;;
    esac
    if grep -Ev '^dyld_info -imports [^[:space:]]+ \[[[:alnum:]_.-]+\]: rows=[0-9]+ ordinal=[0-9]+ addend=[0-9]+ weak-import=[0-9]+$' "$work/imports.err" >/dev/null; then
      got=unexpected-stderr
    fi
    if [ "${expect_counts+x}" = x ]; then
      observed_counts=$(cat "$work/imports.err") || exit 2
      [ "$observed_counts" = "$expect_counts" ] || got=unexpected-table-counts
    fi
    if [ -n "${diagnostic_binary:-}" ]; then
      output_has_line "dyld_info -imports $diagnostic_binary:" "$output" || got=missing-diagnostic-header
    fi
    if [ "$got" = "$want" ] && [ -n "${expect_row:-}" ]; then
      output_has_line "$expect_row" "$output" || got=missing-row
    fi
    verdict "$case_name" "$want" "$got"
  }
  clean='halvo [arm64]:
    -imports:
      _free  (from libSystem)'
  printf '%s\n' "$clean" "      ${forbidden_import}Extra  (from Carbon)" >"$work/halvo"
  printf '%s\n' "$clean" >"$work/halvo-capture"
  imports_case clean-imports PASS 'dyld_info -imports halvo-capture: absent' "$work/halvo" "$work/halvo-capture"
  printf '%s\n' 'halvo-capture [x86_64]:' '    -imports:' "      $forbidden_import  (from Carbon)" >>"$work/halvo-capture"
  hold_row="      $forbidden_import  (from Carbon)"
  diagnostic_binary=halvo-capture expect_row="$hold_row" imports_case capture-imports-hold FAIL "halvo-capture imports $forbidden_import" "$work/halvo" "$work/halvo-capture"
  printf '%s\n' "      $forbidden_import  (from Carbon)" >>"$work/halvo"
  expect_row="$hold_row" imports_case app-imports-hold FAIL "halvo imports $forbidden_import" "$work/halvo" "$work/halvo-capture"
  printf '%s\n' 'unrecognized import listing' >"$work/halvo"
  diagnostic_binary=halvo imports_case unreadable-import-list FAIL 'dyld_info returned no import table for halvo' "$work/halvo"
  imports_case import-tool-failed FAIL 'dyld_info could not read the imports of missing' "$work/missing"
  printf '%s\n' 'incomplete tool output' >"$work/halvo"
  tool_status=7 diagnostic_binary=halvo expect_row='incomplete tool output' imports_case import-tool-failure-shows-stdout FAIL 'dyld_info could not read the imports of halvo' "$work/halvo"
  import_row_case() {
    printf '%s\n' "$clean" "$2" >"$work/halvo"
    expect_row="$2" imports_case "$1" FAIL "halvo imports $forbidden_import" "$work/halvo"
  }
  import_row_case address-before-import "      0x00001000 $forbidden_import  (from Carbon)"
  import_row_case index-before-import "      1 $forbidden_import  (from Carbon)"
  import_row_case star-after-import "      $forbidden_import*  (from Carbon)"
  import_row_case comma-after-import "      $forbidden_import,  (from Carbon)"
  cr=$(printf '\r')
  cr_after_import_case() {
    row="      $forbidden_import"
    printf '%s\n' "$clean" "$row$cr" >"$work/halvo"
    cr_count=$(tr -dc '\r' <"$work/halvo" | wc -c)
    printf 'a\r\n' >"$work/cr-probe"
    captured=$(cat "$work/cr-probe") || captured=failed
    width=$(printf 'a\r\n' | LC_ALL=C awk '{ print length($0) }') || width=failed
    case $captured in
      a) captured_width=1 ;;
      "a$cr") captured_width=2 ;;
      *) captured_width=invalid ;;
    esac
    printf 'cr-after-import: fixture CR count %d, captured width %s, awk width %s\n' "$cr_count" "$captured_width" "$width"
    if ! [ "$cr_count" -eq 1 ]; then
      verdict cr-after-import FAIL invalid-fixture
      return
    fi
    case $captured_width:$width in
      2:2) row=$row$cr ;;
      1:1|1:2|2:1) ;;
      *) verdict cr-after-import FAIL invalid-reader-probe; return ;;
    esac
    expect_row=$row imports_case cr-after-import FAIL "halvo imports $forbidden_import" "$work/halvo"
  }
  cr_after_import_case
  import_row_case dollar-after-import "      $forbidden_import\$suffix  (from Carbon)"
  import_row_case dot-after-import "      $forbidden_import.suffix  (from Carbon)"
  slice='halvo [arm64]:'
  other_slice='halvo [x86_64]:'
  header='    -imports:'
  no_table='dyld_info returned no import table for halvo'
  empty_table='dyld_info returned an empty import table for halvo'
  bad_layout='dyld_info returned an unrecognized import listing for halvo'
  table_case() {
    table_name=$1 table_want=$2 table_reason=$3 table_row=$4
    shift 4
    printf '%s\n' "$@" >"$work/halvo"
    expect_row="$table_row" imports_case "$table_name" "$table_want" "$table_reason" "$work/halvo"
  }
  clean_row='      _free  (from libSystem)'
  clean_result='dyld_info -imports halvo: absent'
  bad_row_case() {
    table_case "$1" FAIL "$bad_layout" "$2" "$slice" "$header" "$2"
  }
  bad_row_case import-row-without-library '      _free'
  bad_row_case import-row-with-empty-library '      _free  (from )'
  bad_row_case import-row-without-indent '_free  (from libSystem)'
  bad_row_case import-row-with-leading-digit '      2name  (from libSystem)'
  bad_row_case import-row-with-punctuation '      _fr!ee  (from libSystem)'
  bad_row_case import-row-touching-origin '      _free(from libSystem)'
  bad_row_case import-row-wrong-origin '      _free  (into libSystem)'
  bad_row_case import-row-library-parenthesis '      _free  (from lib(System))'
  bad_row_case import-row-library-close-paren '      _free  (from lib)System)'
  bad_row_case import-row-library-open-paren '      _free  (from lib(System)'
  bad_row_case import-row-library-only-blanks '      _free  (from   )'
  bad_row_case import-row-embedded-cr "      _fr${cr}ee  (from libSystem)"
  vertical_tab=$(printf '\013')
  bad_row_case vertical-tab-is-not-a-blank-row "$vertical_tab"
  bad_row_case import-row-trailing-text '      _free  (from libSystem) extra'
  bad_row_case import-short-ordinal '      0x001  _free  (from libSystem)'
  bad_row_case import-ordinal-no-prefix '      0001  _free  (from libSystem)'
  bad_row_case import-ordinal-bad-hex '      0x00G1  _free  (from libSystem)'
  bad_row_case import-lowercase-ordinal '      0x00a1  _free  (from libSystem)'
  bad_row_case import-ordinal-without-gap '      0x0001_free  (from libSystem)'
  bad_row_case import-oversized-ordinal '      0x100000000  _free  (from libSystem)'
  bad_row_case import-empty-addend '      _free+0x  (from libSystem)'
  bad_row_case import-addend-no-prefix '      _free+10  (from libSystem)'
  bad_row_case import-addend-nonhex '      _free+0xG  (from libSystem)'
  bad_row_case import-padded-addend '      _free+0x01  (from libSystem)'
  bad_row_case import-lowercase-addend '      _free+0xab  (from libSystem)'
  bad_row_case import-oversized-addend '      _free+0x10000000000000000  (from libSystem)'
  bad_row_case import-wrong-mark '      _free [weak] (from libSystem)'
  bad_row_case import-repeated-mark '      _free [weak-import] [weak-import] (from libSystem)'
  bad_row_case import-mark-without-gap '      _free [weak-import](from libSystem)'
  bad_row_case import-mark-touching-symbol '      _free[weak-import] (from libSystem)'
  bad_row_case import-library-starts-with-blank '      _free  (from  libSystem)'
  bad_row_case import-library-ends-with-blank '      _free  (from libSystem )'
  shape_cases() {
    table_case "$1-clean" PASS "$clean_result" '' "$slice" "$header" "$2"
    table_case "$1-hold" FAIL "halvo imports $forbidden_import" "$3" "$slice" "$header" "$2" "$3"
  }
  shape_cases weak-import '      _NSAppearanceNameDarkAqua [weak-import] (from AppKit)' \
    "      $forbidden_import [weak-import] (from AppKit)"
  shape_cases ordinal-import '      0x0000  __NSConcreteGlobalBlock  (from libSystem)' \
    "      0x0000  $forbidden_import  (from libSystem)"
  shape_cases addend-import '      0x0001  __ZTVN10__cxxabiv120__si_class_type_infoE+0x10  (from libc++)' \
    "      0x0001  $forbidden_import+0x10  (from libc++)"
  shape_cases spaced-library-import "      _OBJC_CLASS_\$_Alfred1PasswordManager  (from Alfred Framework)" \
    "      $forbidden_import  (from Alfred Framework)"
  table_case printable-import-limits PASS "$clean_result" '' "$slice" "$header" \
    '      0xFFFF  _free+0x0  (from libSystem)' '      0x10000  _free+0xA [weak-import] (from Two Words)' \
    '      0xFFFFFFFF  _free+0xFFFFFFFFFFFFFFFF  (from libSystem)' '      _free+0x1  (from libSystem)'
  table_case indented-slice-header FAIL "$bad_layout" "  $slice" "$clean" "  $slice" "$header" "$clean_row"
  table_case slice-with-trailing-text FAIL "$bad_layout" "$slice extra" "$clean" "$slice extra" "$header" "$clean_row"
  table_case slice-with-empty-arch FAIL "$bad_layout" 'halvo []:' "$clean" 'halvo []:' "$header" "$clean_row"
  table_case slice-without-path-separator FAIL "$bad_layout" 'halvo[arm64]:' "$clean" 'halvo[arm64]:' "$header" "$clean_row"
  table_case slice-without-colon FAIL "$bad_layout" 'halvo [arm64]' "$clean" 'halvo [arm64]' "$header" "$clean_row"
  table_case slice-with-invalid-arch FAIL "$bad_layout" 'halvo [arm/64]:' "$clean" 'halvo [arm/64]:' "$header" "$clean_row"
  table_case header-with-surrounding-text FAIL "$no_table" 'prefix -imports: suffix' "$slice" 'prefix -imports: suffix' "$clean_row"
  table_case header-with-prefix FAIL "$no_table" 'prefix -imports:' "$slice" 'prefix -imports:' "$clean_row"
  table_case header-with-suffix FAIL "$no_table" '-imports: suffix' "$slice" '-imports: suffix' "$clean_row"
  table_case dotted-architecture PASS "$clean_result" '' 'halvo [arm64e.x1]:' "$header" "$clean_row"
  table_case hyphenated-architecture PASS "$clean_result" '' 'halvo [arch-1]:' "$header" "$clean_row"
  table_case underscored-architecture PASS "$clean_result" '' 'halvo [x86_64]:' "$header" "$clean_row"
  table_case unindented-import-header PASS "$clean_result" '' "$slice" '-imports:' "$clean_row"
  table_case padded-import-header PASS "$clean_result" '' "$slice" ' -imports:  ' "$clean_row"
  LC_ALL=POSIX verify_import_locale=yes table_case imports-use-c-locale PASS "$clean_result" '' 'x [a]:' "$header" "$clean_row"
  tab=$(printf '\t')
  table_case tab-separated-import PASS "$clean_result" '' "$slice" "${tab}-imports:$tab" "$tab" "${tab}_free${tab}(from libSystem)"
  table_case supported-symbol-characters PASS "$clean_result" '' "$slice" "$header" \
    "      \$x7  (from libSystem)" '      .  (from libSystem)' '      A1_b.2  (from libSystem)'
  table_case longer-import-names PASS "$clean_result" '' "$slice" "$header" \
    "      _2$forbidden_import  (from Carbon)" "      _Pre_$forbidden_import  (from Carbon)" \
    "      ${forbidden_import}7  (from Carbon)" "      ${forbidden_import}_  (from Carbon)"
  import_row_case standalone-import-at-line-boundaries "$forbidden_import"
  import_row_case following-punctuation-import "      _other,$forbidden_import  (from Carbon)"
  printf '%s\n' "$header" '      _free  (from libSystem)' >"$work/halvo"
  expect_counts='' expect_row="$header" imports_case import-table-without-slice FAIL "$no_table" "$work/halvo"
  printf '%s\n' "$slice" "$hold_row" "$header" '      _free  (from libSystem)' >"$work/halvo"
  expect_counts='' expect_row="$hold_row" imports_case import-before-header FAIL "$no_table" "$work/halvo"
  printf '%s\n' "$slice" "$header" >"$work/halvo"
  expect_counts='dyld_info -imports halvo [arm64]: rows=0 ordinal=0 addend=0 weak-import=0' expect_row="$header" imports_case empty-import-table FAIL "$empty_table" "$work/halvo"
  printf '%s\n' "$clean" "$other_slice" >"$work/halvo"
  expect_row="$other_slice" imports_case last-slice-without-header FAIL "$no_table" "$work/halvo"
  printf '%s\n' "$slice" "$header" "$clean" >"$work/halvo"
  expect_row="$header" imports_case first-table-empty FAIL "$empty_table" "$work/halvo"
  printf '%s\n' "$slice" "$other_slice" "$header" '      _free  (from libSystem)' >"$work/halvo"
  expect_row="$slice" imports_case first-slice-without-header FAIL "$no_table" "$work/halvo"
  other_layout='      0x00001000 _free  (from libSystem)'
  printf '%s\n' "$slice" "$header" "$other_layout" >"$work/halvo"
  expect_row="$other_layout" imports_case unexpected-import-layout FAIL "$bad_layout" "$work/halvo"
  printf '%s\n' "$clean" "$header" >"$work/halvo"
  expect_row="$header" imports_case repeated-import-header FAIL "$bad_layout" "$work/halvo"
  printf '%s\n' "$clean" "$other_slice" "$header" "      other$forbidden_import  (from Carbon)" >"$work/halvo"
  imports_case clean-imports-both-slices PASS 'dyld_info -imports halvo: absent' "$work/halvo"
  printf '%s\r\n' "$slice" '' "$header" "      .\$entry  (from libSystem)" "      ${forbidden_import}_2  (from Carbon)" >"$work/halvo"
  imports_case clean-imports-crlf PASS 'dyld_info -imports halvo: absent' "$work/halvo"
  printf '%s\n' "$slice" "$header" '      _free  (from Text 0x0001  Meta [weak-import] Extra+0x8)' \
    '      0x0039  ____chkstk_darwin [weak-import] (from libSystem)' '      _open+0x8  (from Foundation)' \
    '      0x0001  _another+0x10 [weak-import] (from Two Words)' "$other_slice" "$header" "$clean_row" >"$work/halvo"
  printf '%s\n' 'halvo-capture [arm64]:' "$header" "$clean_row" >"$work/halvo-capture"
  expect_counts='dyld_info -imports halvo [arm64]: rows=4 ordinal=2 addend=2 weak-import=2
dyld_info -imports halvo [x86_64]: rows=1 ordinal=0 addend=0 weak-import=0
dyld_info -imports halvo-capture [arm64]: rows=1 ordinal=0 addend=0 weak-import=0' \
    imports_case shape-counts-by-binary-and-slice PASS 'dyld_info -imports halvo-capture: absent' "$work/halvo" "$work/halvo-capture"
  first_hold="      $forbidden_import [weak-import] (from Carbon)"
  printf '%s\n' "$slice" "$header" "$first_hold" '      0x0001  _other+0xA  (from Core)' "$other_slice" "$header" "$hold_row" >"$work/halvo"
  expect_counts='dyld_info -imports halvo [arm64]: rows=2 ordinal=1 addend=1 weak-import=1
dyld_info -imports halvo [x86_64]: rows=1 ordinal=0 addend=0 weak-import=0' expect_row="$first_hold" \
    imports_case shape-counts-include-rows-after-hold FAIL "halvo imports $forbidden_import" "$work/halvo"

  rules_repo=$work/rules
  mkdir -p "$rules_repo/crates/member" "$rules_repo/crates/member/.cargo" || exit 2
  git init -q "$rules_repo" || exit 2
  case $0 in /*) check_script=$0 ;; *) check_script=$PWD/$0 ;; esac
  rules_case() {
    answer=$(cd "${rules_directory:-$rules_repo}" && CI_D_REPO=. CI_D_JOB=rules sh "$check_script" --rules 2>"$work/rules.err")
    code=$?
    case $answer in
      'rules: workspace manifest and Cargo configuration paths only (binary imports require the macOS build)
'*)
        case $code:$answer in
          0:*"PASS $name") got=PASS ;;
          1:*"FAIL $name ($3"*) got=FAIL ;;
          *) got="unexpected-exit-$code" ;;
        esac ;;
      *) got="unexpected-rules-output-$code" ;;
    esac
    if [ "$got" = "$2" ] && [ -n "${expect_reader:-}" ]; then
      reader_line=$(cat "$work/rules.err") || exit 2
      [ "$reader_line" = "$expect_reader" ] || got=unexpected-reader
    fi
    if [ -s "$work/probe-input" ]; then got=unexpected-stdin; fi
    verdict "$1" "$2" "$got"
  }
  root_manifest=$rules_repo/Cargo.toml
  member_manifest=$rules_repo/crates/member/Cargo.toml
  profile_off='[profile.release]
debug-assertions = false'
  need_profile='Cargo.toml must set profile.release.debug-assertions to false'
  printf '%s\n' "$profile_off" >"$root_manifest"
  rules_case untracked-workspace-manifest FAIL 'the workspace Cargo.toml must be tracked'
  printf '%s\n' '[workspace]' 'members = ["crates/*"]' >"$root_manifest"
  printf '%s\n' "$profile_off" >"$member_manifest"
  git -C "$rules_repo" add -- Cargo.toml crates/member/Cargo.toml || exit 2
  rules_case member-pin-without-root-pin FAIL "$need_profile"
  printf '%s\n' '[profile.release]' 'debug-assertions = true' >"$root_manifest"
  rules_case release-assertions-on FAIL "$need_profile"
  standins=$work/standins
  mkdir "$standins" || exit 2
  stand_in() {
    { printf '%s\n' '#!/bin/sh' && cat; } >"$standins/$1" && chmod +x "$standins/$1" || exit 2
  }
  forward_python() {
    stand_in "$1" <<'SH'
exec "$fixture_python" "$@"
SH
  }
  fixture_python=
  for program in python3.11 python3 python; do
    fixture_probe=$("$program" -I -c '
import sys, tomllib
assert sys.version_info >= (3, 11)
value = tomllib.loads("value = 6")["value"] ** 2
sys.stdout.write("%d:%d.%d.%d" % (value, *sys.version_info[:3]))' </dev/null 2>/dev/null) || continue
    fixture_version=$(expr "$fixture_probe" : '36:\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$') || continue
    fixture_python=$(command -v "$program") || exit 2
    break
  done
  [ -n "$fixture_python" ] || exit 2
  export fixture_python
  no_python='reading Cargo.toml requires Python 3.11 or later'
  for program in python3.11 python3 python; do
    stand_in "$program" <<'SH'
printf '%s\n' 'interpreter unavailable'
SH
  done
  PATH="$standins:$PATH" rules_case no-interpreter-behind-names FAIL "$no_python"
  forward_python python3
  expect_second="manifest reader: python3 $fixture_version"
  PATH="$standins:$PATH" expect_reader="$expect_second" rules_case skip-notice-interpreter FAIL "$need_profile"
  stand_in python3.11 <<'SH'
printf '%s\n' "$probe_reply"
SH
  bad_probe_case() {
    PATH="$standins:$PATH" probe_reply="$2" expect_reader="$expect_second" rules_case "$1" FAIL "$need_profile"
  }
  bad_probe_case probe-rejects-wrong-answer '0 3.11.0'
  bad_probe_case probe-rejects-version-suffix '42 3.11.0 extra'
  bad_probe_case probe-requires-three-version-parts '42 3.11'
  bad_probe_case probe-requires-major '42 .11.0'
  bad_probe_case probe-requires-minor '42 3..0'
  bad_probe_case probe-requires-patch '42 3.11.'
  bad_probe_case probe-requires-first-dot '42 3x11.0'
  bad_probe_case probe-requires-second-dot '42 3.11x0'
  stand_in python3.11 <<'SH'
if [ "${2:-}" = -c ]; then
  exec "$fixture_python" -I -c 'import sys, tomllib; sys.version_info = (12, 34, 56); exec(sys.argv[1])' "$3"
fi
exec "$fixture_python" "$@"
SH
  PATH="$standins:$PATH" expect_reader='manifest reader: python3.11 12.34.56' rules_case multi-digit-interpreter-version FAIL "$need_profile"
  stand_in python3.11 <<'SH'
if [ "${1:-}" = -c ] || [ "${2:-}" = -c ]; then
  printf 'candidate probe diagnostic\n' >&2
  exec "$fixture_python" "$@"
fi
{
  printf '%s\n' 'import os, sys' 'assert sys.flags.isolated and os.environ["PYTHONPATH"] not in sys.path'
  cat
} | "$fixture_python" "$@"
SH
  expect_first="manifest reader: python3.11 $fixture_version"
  mkdir "$work/empty-python-path" || exit 2
  PYTHONPATH="$work/empty-python-path" PATH="$standins:$PATH" expect_reader="$expect_first" rules_case manifest-reader-isolated FAIL "$need_profile"
  PYTHONIOENCODING=unknown-fixture-encoding rules_case python-ignores-caller-encoding FAIL "$need_profile"
  probe_input=$work/probe-input
  export probe_input
  stand_in python3.11 <<'SH'
IFS= read -r line && printf '%s\n' "$line" >"$probe_input"
exit 0
SH
  PATH="$standins:$PATH" expect_reader="$expect_second" rules_case probe-leaves-stdin-alone FAIL "$need_profile" <<'EOF'
a line belonging to the caller
EOF
  : >"$probe_input"
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then exec "$fixture_python" "$@"; fi
printf '%s\n' 'reader notice'
SH
  PATH="$standins:$PATH" rules_case reader-notice-with-success FAIL 'unexpected Cargo.toml reader output: reader notice'
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then exec "$fixture_python" "$@"; fi
exit 0
SH
  PATH="$standins:$PATH" rules_case reader-silent-with-success FAIL 'unexpected Cargo.toml reader output: '
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then exec "$fixture_python" "$@"; fi
printf 'profile-ok\n'
SH
  PATH="$standins:$PATH" rules_case reader-extra-newline FAIL 'unexpected Cargo.toml reader output: profile-ok'
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then exec "$fixture_python" "$@"; fi
printf 'profile-ok'
printf ' extra' >&2
SH
  PATH="$standins:$PATH" rules_case reader-extra-stderr FAIL 'unexpected Cargo.toml reader output: profile-ok extra'
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then exec "$fixture_python" "$@"; fi
printf 'profile-ok\000'
SH
  PATH="$standins:$PATH" rules_case reader-extra-nul FAIL 'unexpected Cargo.toml reader output: profile-ok?'
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then exec "$fixture_python" "$@"; fi
printf 'profile-ok'
exit 1
SH
  PATH="$standins:$PATH" rules_case reader-token-with-failure FAIL 'profile-ok)'
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then "$fixture_python" "$@"; fi
exit 1
SH
  PATH="$standins:$PATH" expect_reader="$expect_second" rules_case probe-answer-with-failure FAIL "$need_profile"
  stand_in python3.11 <<'SH'
if [ "$2" = -c ]; then
  exec "$fixture_python" -I -c 'import sys, tomllib; sys.version_info = (3, 10, 9); exec(sys.argv[1])' "$3"
fi
exit 1
SH
  PATH="$standins:$PATH" expect_reader="$expect_second" rules_case probe-requires-python-311 FAIL "$need_profile"
  for program in python3.11 python3 python; do
    stand_in "$program" <<'SH'
exit 1
SH
  done
  PATH="$standins:$PATH" rules_case no-working-interpreter FAIL "$no_python"
  forward_python python
  expect_third="manifest reader: python $fixture_version"
  PATH="$standins:$PATH" expect_reader="$expect_third" rules_case third-interpreter-bad-profile FAIL "$need_profile"
  printf '%s\n' "$profile_off" >"$root_manifest"
  PATH="$standins:$PATH" expect_reader="$expect_third" rules_case third-interpreter-good-profile PASS ''
  stand_in tr <<'SH'
cat
exit 1
SH
  PATH="$standins:$PATH" rules_case reader-output-filter-failed FAIL 'could not read the Cargo.toml reader output'
  mv "$standins/tr" "$work/used-reader-filter" || exit 2
  mkdir "$rules_repo/below" || exit 2
  rules_directory="$rules_repo/below" rules_case good-root-from-subdirectory PASS ''
  printf '%s\n' '[profile.release]' 'debug-assertions = true' >"$root_manifest"
  rules_directory="$rules_repo/below" rules_case bad-root-from-subdirectory FAIL "$need_profile"
  read_error='could not read the release profile in Cargo.toml'
  printf '%s\n' '[profile.release' >"$root_manifest"
  rules_case malformed-release-toml FAIL "$read_error"
  printf '%s\n' "$profile_off" '[profile.release.package]' 'foo = "bar"' >"$root_manifest"
  rules_case malformed-package-table FAIL "$read_error"
  printf '%s\n' '[profile.dev]' 'debug-assertions = false' >"$root_manifest"
  rules_case other-profile-off FAIL "$need_profile"
  printf '%s\n' '[workspace.metadata]' 'example = """' "$profile_off" '"""' >"$root_manifest"
  rules_case profile-inside-string FAIL "$need_profile"
  printf '%s\n' '[profile.release]' 'debug-assertions = "false"' >"$root_manifest"
  rules_case release-assertions-string FAIL "$need_profile"
  printf '%s\n' "$profile_off" '[profile.release.package.member]' 'debug-assertions = true' >"$root_manifest"
  rules_case package-assertions-on FAIL 'Cargo.toml enables release debug assertions for package member'
  printf '%s\n' "$profile_off" '[profile.release.package."*"]' 'debug-assertions = true' >"$root_manifest"
  rules_case wildcard-assertions-on FAIL 'Cargo.toml enables release debug assertions for package *'
  printf '%s\n' "$profile_off" '[profile.release.package.member]' 'debug-assertions = false' >"$root_manifest"
  rules_case release-assertions-off PASS ''
  mkdir "$rules_repo/.cargo" || exit 2
  : >"$rules_repo/.cargo/config"
  rules_case root-config-in-rules FAIL 'the cargo configuration .cargo/config is no tracked file'
  mv "$rules_repo/.cargo/config" "$rules_repo/.cargo/config.toml" || exit 2
  root_config_error='the cargo configuration .cargo/config.toml is no tracked file'
  rules_case root-config-toml-in-rules FAIL "$root_config_error"
  printf '%s\n' '.cargo/' >"$rules_repo/.gitignore"
  git -C "$rules_repo" check-ignore -q -- .cargo/config.toml || exit 2
  rules_case ignored-root-config-in-rules FAIL "$root_config_error"
  mv "$rules_repo/.cargo/config.toml" "$rules_repo/crates/member/.cargo/config.toml" || exit 2
  rules_case nested-config-toml-in-rules FAIL 'the cargo configuration crates/member/.cargo/config.toml is no tracked file'
  mv "$rules_repo/crates/member/.cargo/config.toml" "$work/used-config" || exit 2
  printf '%s\n' '[profile.release]' 'debug-assertions = true' >"$rules_repo/crates/member/.cargo/config"
  rules_case untracked-configuration-in-rules FAIL 'the cargo configuration crates/member/.cargo/config is no tracked file'
  git -C "$rules_repo" add -f -- crates/member/.cargo/config || exit 2
  rules_case tracked-configuration-in-rules FAIL 'the cargo configuration crates/member/.cargo/config is tracked'

  measure_repo=$work/measure
  git init -q "$measure_repo" || exit 2
  measure_manifest=$measure_repo/Cargo.toml
  printf '%s\n' '[profile.release]' 'debug-assertions = true' >"$measure_manifest"
  git -C "$measure_repo" add -- Cargo.toml || exit 2
  standins=$work/measure-tools
  mkdir "$standins" || exit 2
  measure_trace=$work/measure-started
  export measure_trace
  for program in mktemp cargo npm rustc; do
    stand_in "$program" <<'SH'
printf '%s\n' "${0##*/}" >>"$measure_trace"
exit 1
SH
  done
  measure_fixture() (
    for input in node_modules scripts/r1/ci.d/61-version.sh src-tauri; do
      if [ -e "$measure_repo/$input" ] || [ -L "$measure_repo/$input" ]; then
        printf 'unsafe measure fixture: %s\n' "$input"
        exit 2
      fi
    done
    unset TAURI_CONFIG
    for variable in $signing_names; do unset "$variable"; done
    CI=true CI_D_JOB=macos CI_D_REPO="$measure_repo" TMPDIR="$measure_manifest" PATH="$standins:$PATH" sh "$check_script"
  )
  measure_case() {
    output=$(TAURI_CONFIG=fixture APPLE_ID=fixture measure_fixture 2>"$work/measure.err")
    code=$?
    got=unexpected-exit-$code
    if [ "$code" = "$2" ] && [ "$output" = "$3" ] && [ ! -e "$measure_trace" ]; then
      got=FAIL
    else
      printf 'measure fixture %s exited %s with %s\n' "$1" "$code" "$output"
    fi
    verdict "$1" FAIL "$got"
  }
  measure_case plain-refuses-bad-profile 1 "FAIL $name ($need_profile)"
  printf '%s\n' "$profile_off" >"$measure_manifest"
  measure_case plain-stops-before-build 1 "FAIL $name (node_modules holds no @tauri-apps/cli; the Prerequisites step runs npm ci)"
  mkdir -p "$measure_repo/scripts/r1/ci.d" || exit 2
  for input in node_modules scripts/r1/ci.d/61-version.sh src-tauri; do
    : >"$measure_repo/$input"
    measure_case "guard-measure-fixture-${input##*/}" 2 "unsafe measure fixture: $input"
    mv "$measure_repo/$input" "$work/used-measure-input" || exit 2
  done
  if MSYS="${MSYS:+$MSYS }winsymlinks:nativestrict" ln -s missing-input "$measure_repo/node_modules" &&
     [ -L "$measure_repo/node_modules" ] && [ ! -e "$measure_repo/node_modules" ]; then
    measure_case guard-dangling-node-modules 2 'unsafe measure fixture: node_modules'
  else
    verdict guard-dangling-node-modules FAIL missing-dangling-symlink
  fi

  files_output=$(sh "$check_script" --files "$work/clean.bin" "$work/seam.bin" 2>"$work/files.err")
  code=$?
  got=unexpected-exit-$code
  [ "$code:$files_output" = "1:FAIL $name (seam.bin holds the marker halvo-capture:test-seam:this-build-has-it!)" ] && got=FAIL
  verdict files-form-reads-the-markers FAIL "$got"

  # A helper whose file of packaging/helpers.d says version=none is read and not started. The files
  # form runs in a directory with its own helper list, written with CRLF, and with a stand-in
  # 61-version.sh that records the files it gets; a stand-in xcrun gives each file a clean import
  # table on a Mac. With the test-seam marker in the helper the same call fails by the marker.
  unversioned_root=$work/unversioned
  mkdir -p "$unversioned_root/packaging/helpers.d" "$unversioned_root/scripts/r1/ci.d" "$unversioned_root/tools" "$unversioned_root/bin" || exit 2
  printf '%s\r\n' 'package=p' 'bin=b' 'manifest=m' >"$unversioned_root/packaging/helpers.d/with-version" || exit 2
  printf '%s\r\n' 'package=p' 'bin=b' 'manifest=m' 'version=none' >"$unversioned_root/packaging/helpers.d/without-version" || exit 2
  cat >"$unversioned_root/scripts/r1/ci.d/61-version.sh" <<'SH' || exit 2
[ "$1" = --check ] || exit 64
shift
printf '%s\n' "$@" >>handed-to-the-version-check
printf 'PASS 61-version\n'
SH
  cat >"$unversioned_root/tools/xcrun" <<'SH' || exit 2
#!/bin/sh
printf '%s\n' 'stand-in [arm64]:' '    -imports:' '      _free  (from libSystem)'
SH
  chmod +x "$unversioned_root/tools/xcrun" || exit 2
  printf 'clean\n' >"$unversioned_root/bin/with-version" || exit 2
  printf 'clean\n' >"$unversioned_root/bin/without-version.exe" || exit 2
  unversioned_run() (
    cd "$unversioned_root" || exit 2
    PATH="$unversioned_root/tools:$PATH" sh "$check_script" --files bin/with-version bin/without-version.exe 2>"$work/unversioned.err"
  )
  output=$(unversioned_run)
  code=$?
  got=unexpected-exit-$code
  case $code:$output in
    0:*"without-version.exe: read and not started, its file of packaging/helpers.d says version=none"*"files read: 2
PASS $name") got=not-started ;;
  esac
  handed=$(cat "$unversioned_root/handed-to-the-version-check" 2>/dev/null)
  [ "$got" = not-started ] && [ "$handed" != bin/with-version ] && got=the-version-check-got-other-files
  if [ "$got" = not-started ]; then
    printf '%s\n' 'halvo-capture:test-seam:this-build-has-it!' >>"$unversioned_root/bin/without-version.exe" || exit 2
    output=$(unversioned_run)
    code=$?
    got=unexpected-exit-$code
    [ "$code:$output" = "1:FAIL $name (without-version.exe holds the marker halvo-capture:test-seam:this-build-has-it!)" ] && got=FAIL
  fi
  verdict files-form-does-not-start-a-helper-without-a-version FAIL "$got"

  for ref in tag:fresh branch:cached :cached; do
    got=$(build_root_for "${ref%:*}")
    if [ "$got" = "${ref#*:}" ]; then got=PASS; else got=FAIL; fi
    verdict "build-root-on-${ref%:*}-ref" PASS "$got"
  done

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --rules) rules ;;
  --files) shift; files_form "$@" ;;
  '') measure ;;
  *) exit 64 ;;
esac
