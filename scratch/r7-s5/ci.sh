#!/usr/bin/env bash
# Runs the S5 and S6 measurements on a hosted runner. The workflow calls one step at a time.
# Safe to run twice: builds reuse their directories, stored items are replaced, and S6 makes
# a fresh test CA on every run.
set -euo pipefail

step=${1:?step: facts, s5, s6-build, s6 or s6-selftest}
runner=${2:?runner label}

if [ "${GITHUB_ACTIONS:-}" != true ]; then
  echo "ci.sh writes to the key store and the trust store, so it runs only in a hosted runner job" >&2
  exit 2
fi

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cd "$(dirname "$0")"

case "$(uname -s)" in
  Darwin)
    os=macos
    exe=
    work="$RUNNER_TEMP/r7-s5"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    os=windows
    exe=.exe
    work="$(cygpath -m "$RUNNER_TEMP")/r7-s5"
    ;;
  *)
    echo "unsupported system $(uname -s)" >&2
    exit 2
    ;;
esac
mkdir -p "$work"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

facts() {
  channel=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' ../../rust-toolchain.toml)
  rustup toolchain install "$channel" --profile minimal --no-self-update
  rustc -vV
  cargo -V
  echo "FACT runner=$runner system=$(uname -srm)"
  if [ "$os" = macos ]; then
    sw_vers
    echo "FACT cpu=$(sysctl -n machdep.cpu.brand_string) cores=$(sysctl -n hw.ncpu) memory_bytes=$(sysctl -n hw.memsize)"
    echo "FACT default keychain: $(security default-keychain)"
    echo "FACT keychain search list: $(security list-keychains | tr -d '\n')"
    echo "FACT $(csrutil status 2>&1)"
  else
    cmd //c ver
    echo "FACT cores=$NUMBER_OF_PROCESSORS"
    powershell -NoProfile -Command '"FACT cpu=" + (Get-CimInstance Win32_Processor | Select-Object -First 1).Name + " memory_bytes=" + (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory'
  fi
}

custody="$work/installed/custody$exe"

# Puts build <stamp> at the one path the program runs from, as an update does. The copy
# goes in under a new name and is renamed over the old file, so the new build is a new file.
install_build() {
  stamp=$1
  mkdir -p "$work/installed"
  cp "$work/build-$stamp/release/custody$exe" "$custody.new"
  if [ "$os" = macos ]; then
    codesign --force --sign - --identifier dev.halvo.spike.r7-s5.custody "$custody.new"
  fi
  mv -f "$custody.new" "$custody"
  echo "S5 $runner build $stamp installed: stamp=$("$custody" stamp) sha256=$(sha256 "$custody")"
  cdhash=
  if [ "$os" = macos ]; then
    cdhash=$(codesign -dvvv "$custody" 2>&1 | sed -n 's/^CDHash=//p')
    {
      codesign -dvvv "$custody" 2>&1 | grep -E '^(Identifier|CodeDirectory|CDHash|Signature|TeamIdentifier)'
      codesign -d -r- "$custody" 2>&1 | grep designated
    } | sed "s/^/S5 $runner build $stamp codesign: /" || true
  fi
}

build_custody() {
  stamp=$1
  S5_BUILD_STAMP=$stamp cargo build --release --locked --no-default-features --bin custody \
    --target-dir "$work/build-$stamp"
}

does_not_count() {
  echo "S5 RULE $runner: DOES NOT COUNT ($1)"
  exit 1
}

s5() {
  value="synthetic-s5-item-$(date +%s)-$RANDOM"

  build_custody 1
  install_build 1
  first=$(sha256 "$custody")
  first_cdhash=$cdhash
  "$custody" store "$value" || does_not_count "build 1 could not store the item"
  "$custody" read "$value" ||
    does_not_count "the build that stored the item cannot read it back, so this key store is unusable here"
  echo "S5 $runner control: build 1 read its own item with user interaction disabled"

  build_custody 2
  install_build 2
  if [ "$(sha256 "$custody")" = "$first" ]; then
    does_not_count "the rebuilt binary is identical to the first, so it does not stand for an update"
  fi
  if [ "$os" = macos ] && { [ -z "$cdhash" ] || [ "$cdhash" = "$first_cdhash" ]; }; then
    does_not_count "the two builds do not have two different code directory hashes ('$first_cdhash', '$cdhash')"
  fi

  set +e
  "$custody" read "$value"
  rc=$?
  set -e
  if [ "$os" = macos ] &&
    security find-generic-password -s dev.halvo.spike.r7-s5 -a custody >/dev/null 2>&1; then
    echo "S5 $runner: the item is still in the keychain after build 2's read (its attributes are readable)"
  fi

  # Back to build 1: if it reads the item again, the item is intact and build 2's result
  # comes from the change of build, not from the passing of time or from the earlier reads.
  # If it cannot, the leg shows nothing about a rebuild.
  install_build 1
  if "$custody" read "$value"; then
    echo "S5 $runner control after: build 1, installed again, read the item"
  else
    does_not_count "build 1, installed again, could not read the item, so build 2's result cannot be tied to the rebuild"
  fi

  case $rc in
    0) echo "S5 RULE $runner: MET (rebuilt build 2 read the item that build 1 stored, with user interaction disabled)" ;;
    10 | 12) echo "S5 RULE $runner: MISSED (rebuilt build 2 could not read the item that build 1 stored, with user interaction disabled; read exit $rc)" ;;
    *) does_not_count "build 2's read ended with exit $rc" ;;
  esac

  "$custody" delete || echo "S5 $runner: the item was left in the key store of this runner"
}

s6_build() {
  cargo build --release --locked --bins --target-dir "$work/s6"
}

s6() {
  if [ "$os" = macos ]; then
    trust=(/bin/bash "$PWD/trust-ca-macos.sh" "{ca}" "{leaf}")
  else
    trust=(certutil -addstore -f Root "{ca}")
  fi
  "$work/s6/release/bench$exe" --helper "$work/s6/release/helper$exe" --workdir "$work" \
    --runner "$runner" --runs 30 --limit-ms 1000 -- "${trust[@]}"
}

s6_selftest() {
  cargo test --release --locked --target-dir "$work/s6"
}

case "$step" in
  facts) facts ;;
  s5) s5 ;;
  s6-build) s6_build ;;
  s6) s6 ;;
  s6-selftest) s6_selftest ;;
  *)
    echo "unknown step $step" >&2
    exit 2
    ;;
esac
