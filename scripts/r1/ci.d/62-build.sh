#!/bin/sh
# jobs: macos windows
# rule: The release build of halvo and halvo-capture, made by calls that name no other package and no feature, configuration or test target, takes every tauri package that cargo resolves for the host from the crates.io registry, with no untracked cargo configuration at the root of the checkout or one directory below it and no source replacement in a tracked one (none outside the checkout is read), prints the derived version, changes no file of the checkout, holds no stand-in or test-seam marker by a scan of every shipped file that stands as the backstop, and is what the artifacts directory holds, and no signing variable is set in its environment, because this run builds unsigned.
set -u
name=62-build
shipped_packages='halvo halvo-capture'
markers='halvo-standin-module-present
halvo-capture:test-seam:this-build-has-it!'
macos_calls='npm run tauri build -- -- --locked
cargo build --release --locked -p halvo-capture --bin halvo-capture'
windows_calls='npm run tauri build -- --no-bundle -- --locked
cargo build --release --locked -p halvo-capture --bin halvo-capture'

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
      --workspace|--all|--all-targets|--tests|--test|--test=*|--benches|--bench|--bench=*|--examples|--example|--example=*|--lib)
        printf 'the call has %s, which builds more than the shipped binaries\n' "$word"; return 1 ;;
      --features|--features=*|-F|-F?*|-f|-f?*|--all-features|--no-default-features)
        printf 'the call has %s, which changes the features of what ships\n' "$word"; return 1 ;;
      --config|--config=*|-c|-c?*)
        printf 'the call has %s, which gives the build a configuration that no tracked file holds\n' "$word"; return 1 ;;
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
    printf 'the cargo configuration %s is no tracked file\n' "$first"
    return 1
  fi
  for file in "$@"; do
    if grep -q -E '^[[:space:]]*\[source' "$file"; then
      printf '%s holds a source replacement\n' "$file"
      return 1
    fi
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
  configurations='.cargo/config .cargo/config.toml */.cargo/config */.cargo/config.toml'
  # shellcheck disable=SC2086
  untracked_configurations=$(git ls-files --others -- $configurations) || fail "git ls-files failed"
  # shellcheck disable=SC2086
  tracked_configurations=$(git ls-files -- $configurations) || fail "git ls-files failed"
  # shellcheck disable=SC2086
  found=$(cargo_config_defect "$untracked_configurations" $tracked_configurations) || fail "$found"
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
      ;;
    windows)
      set -- "$target/release/halvo.exe" "$target/release/halvo-capture.exe"
      ;;
  esac
  checked=$(sh scripts/r1/ci.d/61-version.sh --check "$@")
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
$target/release/halvo-capture"
      ;;
    windows)
      shipped="$1
$2"
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
      cp "$1" "$2" "$CI_D_ARTIFACTS/" || fail "cp could not copy the two executables"
      [ -f "$CI_D_ARTIFACTS/halvo.exe" ] && [ -f "$CI_D_ARTIFACTS/halvo-capture.exe" ] || fail "the artifacts directory lacks halvo.exe or halvo-capture.exe"
      bytes=$(wc -c <"$CI_D_ARTIFACTS/halvo-capture.exe" | tr -d ' ')
      printf 'halvo-capture.exe bytes=%s\n' "$bytes"
      ;;
  esac
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
  call_case runner-package FAIL 'cargo build --release --locked -p halvo-runner --bin halvo-runner'
  call_case whole-workspace FAIL 'cargo build --release --locked --workspace'
  call_case test-targets FAIL 'cargo build --release --locked -p halvo-capture --tests'
  call_case all-targets FAIL 'cargo build --release --locked -p halvo-capture --all-targets'
  call_case a-feature FAIL 'cargo build --release --locked -p halvo-capture --features test-seam'
  call_case a-feature-of-tauri FAIL 'npm run tauri build -- --features tauri/unstable'
  call_case a-configuration FAIL 'npm run tauri build -- --config src-tauri/other.json'
  call_case a-package-flag-at-the-end FAIL 'cargo build --release -p'
  got=FAIL
  TAURI_CONFIG='{"build":{}}' environment_defect >/dev/null && got=PASS
  verdict tauri-configuration-in-a-variable FAIL "$got"
  got=FAIL
  (unset TAURI_CONFIG; environment_defect >/dev/null) && got=PASS
  verdict environment-without-those-variables PASS "$got"

  printf 'head\000\001\002%s\000tail' 'halvo-standin-module-present' >"$work/standin.bin"
  printf 'head\000%s\377' 'halvo-capture:test-seam:this-build-has-it!' >"$work/seam.bin"
  printf 'head\000\377halvo-standin-module\000present' >"$work/clean.bin"
  for fixture in standin:FAIL seam:FAIL clean:PASS; do
    got=FAIL
    marker_defect "$work/${fixture%:*}.bin" >/dev/null && got=PASS
    verdict "marker-${fixture%:*}" "${fixture#*:}" "$got"
  done

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

  got=FAIL
  (APPLE_CERTIFICATE=stand-in-value signing_defect >/dev/null) && got=PASS
  verdict one-signing-name-set FAIL "$got"
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
  printf '[build]\njobs = 2\n' >"$work/plain-config.toml"
  printf '[source.crates-io]\nreplace-with = "vendored"\n' >"$work/replacing-config.toml"
  got=FAIL
  cargo_config_defect '' "$work/plain-config.toml" >/dev/null && got=PASS
  verdict tracked-configuration-without-a-source PASS "$got"
  got=FAIL
  cargo_config_defect '' "$work/replacing-config.toml" >/dev/null && got=PASS
  verdict tracked-configuration-with-a-source-replacement FAIL "$got"
  got=FAIL
  cargo_config_defect '.cargo/config.toml' >/dev/null && got=PASS
  verdict untracked-cargo-configuration-at-the-root FAIL "$got"
  got=FAIL
  cargo_config_defect 'src-tauri/.cargo/config' >/dev/null && got=PASS
  verdict untracked-cargo-configuration-below-the-root FAIL "$got"

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
  '') measure ;;
  *) exit 64 ;;
esac
