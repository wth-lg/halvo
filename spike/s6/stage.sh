#!/bin/sh
# beforeBuildCommand of the S6 probe (copy used on the hosted runner too) (contracts.md C8 point 4): build the helper for the triple
# that Tauri names in TAURI_ENV_TARGET_TRIPLE and copy it to target/helpers/halvo-capture-<triple>.
# The workspace root is the first argument (Tauri's working directory for the command is not assumed).
set -u
export PATH="$HOME/.cargo/bin:$PATH"
root=$1
triple=${TAURI_ENV_TARGET_TRIPLE:-}
echo "stage: TAURI_ENV_TARGET_TRIPLE=${triple:-unset} TAURI_ENV_ARCH=${TAURI_ENV_ARCH:-unset} TAURI_ENV_PLATFORM=${TAURI_ENV_PLATFORM:-unset} pwd_is_root=$([ "$(pwd -P)" = "$(cd "$root" && pwd -P)" ] && echo yes || echo no)"
[ -n "$triple" ] || { echo "stage: no triple"; exit 1; }
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build --release --locked --manifest-path "$root/Cargo.toml" -p halvo-capture --bin halvo-capture --target "$triple" || exit 1
mkdir -p "$root/target/helpers" || exit 1
cp "$root/target/$triple/release/halvo-capture" "$root/target/helpers/halvo-capture-$triple" || exit 1
echo "stage: staged target/helpers/halvo-capture-$triple archs=$(lipo -archs "$root/target/helpers/halvo-capture-$triple")"
