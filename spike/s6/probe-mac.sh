#!/bin/sh
# S6 probe for the hosted macos-15 runner (run 20261108-halvo-r1-capture, scratch branch only).
# The log is public: every line is a count, a size, a flag, a version or a file name inside the bundle.
#   step 1  cargo test --workspace --locked on the clean checkout, no helper staged
#   step 2  form A of contracts.md C8: the helper only through --config (externalBin) and a
#           beforeBuildCommand that stages it for TAURI_ENV_TARGET_TRIPLE; host build
#   step 3  the same with --target x86_64-apple-darwin: which architecture does the helper get
#   step 4  the dmg target with CI=true, then the disk image attached read-only
#   step 5  hdiutil create and attach of a 40 MB disk image
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root" || exit 1
work=$(mktemp -d)
echo "S6 probe: macOS $(sw_vers -productVersion) build $(sw_vers -buildVersion) arch $(uname -m) CI=${CI:-unset}"
export CARGO_BUILD_JOBS=4

echo "== step 1: clean checkout test"
echo "status_lines=$(git status --porcelain | wc -l | tr -d ' ')"
echo "staged_helper_dir_present=$([ -e target/helpers ] && echo true || echo false)"
echo "externalBin_in_tracked_config=$(grep -c externalBin src-tauri/tauri.conf.json)"
cargo test --workspace --locked > "$work/test.log" 2>&1
echo "cargo_test_exit=$?"
grep -E '^test result:' "$work/test.log" | sed 's/finished in [0-9.]*s//' | sort | uniq -c
grep -E '^test .* \.\.\. FAILED$' "$work/test.log" | sed 's/^/failed: /'

inspect() {
  tag=$1; a=$2
  echo "$tag bundle_present=$([ -d "$a" ] && echo true || echo false)"
  [ -d "$a" ] || return 0
  echo "$tag macos_dir=$(ls "$a/Contents/MacOS" | tr '\n' ' ')"
  for b in halvo halvo-capture; do
    f="$a/Contents/MacOS/$b"
    [ -e "$f" ] || { echo "$tag $b present=false"; continue; }
    echo "$tag $b identifier=$(codesign -dv "$f" 2>&1 | sed -n 's/^Identifier=//p') flags=$(codesign -dv "$f" 2>&1 | sed -n 's/^CodeDirectory.*flags=\([^ ]*\).*/\1/p') info_plist=$(codesign -dv "$f" 2>&1 | sed -n 's/^Info.plist=*//p' | tr ' ' '_')"
    echo "$tag $b archs=$(lipo -archs "$f") minos=$(vtool -show-build "$f" 2>/dev/null | awk '/minos/ {print $2}' | tr '\n' ' ')"
    if [ "$(uname -m)" = arm64 ] && lipo -archs "$f" | grep -qw arm64; then
      v=$("$f" --version 2>&1); echo "$tag $b version_exit=$? version=$v"
    fi
  done
  codesign --verify --deep --strict "$a" > /dev/null 2>&1; echo "$tag verify_deep_strict_exit=$?"
  echo "$tag LSMinimumSystemVersion=$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$a/Contents/Info.plist" 2>&1)"
}

echo "== step 2: form A, host"
npm ci --no-audit --no-fund > "$work/npm.log" 2>&1; echo "npm_ci_exit=$?"
npm run tauri build -- --config spike/s6/formA.json --bundles app > "$work/a-host.log" 2>&1
echo "formA_host_build_exit=$?"
grep -E '^stage: ' "$work/a-host.log" | sed 's/^/formA-host /'
inspect formA-host target/release/bundle/macos/Halvo.app

echo "== step 3: form A, --target x86_64-apple-darwin"
rustup target add x86_64-apple-darwin > "$work/rustup.log" 2>&1; echo "rustup_target_add_exit=$?"
npm run tauri build -- --config spike/s6/formA.json --bundles app --target x86_64-apple-darwin > "$work/a-x86.log" 2>&1
echo "formA_x86_build_exit=$?"
grep -E '^stage: ' "$work/a-x86.log" | sed 's/^/formA-x86 /'
inspect formA-x86 target/x86_64-apple-darwin/release/bundle/macos/Halvo.app

echo "== step 4: dmg target with CI=true"
CI=true npm run tauri build -- --config spike/s6/formA.json --bundles dmg > "$work/dmg.log" 2>&1
echo "dmg_build_exit=$?"
grep -iE 'error|bundle_dmg|Bundling|Finished' "$work/dmg.log" | sed -E 's#(/[^ ]*)/([^/ ]+)#\2#g' | head -8 | sed 's/^/dmg-log /'
dmg=$(ls target/release/bundle/dmg/*.dmg 2>/dev/null | head -1)
echo "dmg_present=$([ -n "$dmg" ] && echo true || echo false) dmg_name=$(basename "${dmg:-none}") dmg_bytes=$([ -n "$dmg" ] && stat -f %z "$dmg")"
if [ -n "$dmg" ]; then
  hdiutil verify "$dmg" > /dev/null 2>&1; echo "dmg_verify_exit=$?"
  mp=$(mktemp -d)
  hdiutil attach -nobrowse -readonly -mountpoint "$mp" "$dmg" > /dev/null 2>&1; echo "dmg_attach_exit=$?"
  echo "dmg_top_level=$(ls "$mp" | tr '\n' ' ')"
  inspect dmg-copy "$mp/Halvo.app"
  hdiutil detach "$mp" > /dev/null 2>&1; echo "dmg_detach_exit=$?"
fi

echo "== step 5: hdiutil 40 MB disk image"
img="$work/r1spike-40m.dmg"
hdiutil create -size 40m -fs APFS -volname r1spike "$img" > /dev/null 2>&1; echo "hdiutil_create_exit=$?"
echo "image_bytes=$(stat -f %z "$img" 2>/dev/null)"
mp2=$(mktemp -d)
hdiutil attach -nobrowse -mountpoint "$mp2" "$img" > /dev/null 2>&1; echo "hdiutil_attach_exit=$?"
df -k "$mp2" | awk 'NR == 2 {print "volume_kib_total=" $2 " volume_kib_avail=" $4}'
dd if=/dev/zero of="$mp2/fill" bs=1m count=8 > /dev/null 2>&1; echo "write_8mib_exit=$?"
dd if=/dev/zero of="$mp2/fill2" bs=1m count=64 > /dev/null 2>&1; echo "write_past_full_exit=$? (non-zero expected: the volume is full)"
df -k "$mp2" | awk 'NR == 2 {print "after_fill volume_kib_avail=" $4}'
hdiutil detach "$mp2" > /dev/null 2>&1; echo "hdiutil_detach_exit=$?"
echo "S6 probe done"
