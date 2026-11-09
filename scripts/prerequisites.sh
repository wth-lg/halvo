#!/bin/sh
# Installs what the Build and Release commands need, as the current user, with no sudo.
# Safe to run twice: every step first checks whether its result is already present.
# Installs into: $HOME/.rustup and $HOME/.cargo (rustup, the pinned Rust toolchain,
# cargo-release) and node_modules/ in this repository (the Tauri CLI).
# Expects curl, node and npm on PATH, plus the Xcode Command Line Tools on macOS or
# Visual Studio C++ build tools on Windows.
set -eu
cd "$(dirname "$0")/.."

CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export PATH="$CARGO_HOME/bin:$PATH"

if ! command -v rustup >/dev/null 2>&1; then
  echo "prerequisites: installing rustup into $HOME/.rustup and $CARGO_HOME"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none
else
  echo "prerequisites: rustup present: $(rustup --version 2>/dev/null | head -1)"
fi

channel=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' rust-toolchain.toml)
rustup toolchain install "$channel" --profile minimal --no-self-update
echo "prerequisites: rust toolchain $channel present: $(rustup run "$channel" cargo --version)"

echo "prerequisites: installing the Tauri CLI into node_modules/"
npm ci --no-audit --no-fund

cargo_release_version=1.1.7

# Downloads the prebuilt cargo-release binary for this platform into $CARGO_HOME/bin.
# Returns 1 when there is no prebuilt binary for the platform or the download fails.
install_cargo_release_prebuilt() {
  base="https://github.com/crate-ci/cargo-release/releases/download/v$cargo_release_version"
  prefix="cargo-release-v$cargo_release_version"
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) asset="$prefix-aarch64-apple-darwin.tar.gz" ;;
    Darwin-x86_64) asset="$prefix-x86_64-apple-darwin.tar.gz" ;;
    MINGW*|MSYS*|CYGWIN*) asset="$prefix-x86_64-pc-windows-msvc.zip" ;;
    *) return 1 ;;
  esac
  tmp=$(mktemp -d)
  ok=0
  if curl -sSfL --retry 5 --retry-delay 3 --retry-all-errors -o "$tmp/$asset" "$base/$asset"; then
    case "$asset" in
      *.tar.gz)
        tar -xzf "$tmp/$asset" -C "$tmp" && cp "$tmp/cargo-release" "$CARGO_HOME/bin/cargo-release" && ok=1 ;;
      *.zip)
        powershell.exe -NoProfile -Command "Expand-Archive -LiteralPath '$(cygpath -w "$tmp/$asset")' -DestinationPath '$(cygpath -w "$tmp")' -Force" \
          && cp "$tmp/cargo-release.exe" "$CARGO_HOME/bin/cargo-release.exe" && ok=1 ;;
    esac
  fi
  rm -rf "$tmp"
  [ "$ok" = 1 ]
}

if cargo release --version 2>/dev/null | grep -q " $cargo_release_version\$"; then
  echo "prerequisites: cargo-release $cargo_release_version present"
else
  echo "prerequisites: installing cargo-release $cargo_release_version into $CARGO_HOME/bin"
  if ! install_cargo_release_prebuilt; then
    echo "prerequisites: no prebuilt cargo-release for this platform, or its download failed; building it with cargo install"
    cargo install cargo-release --version "$cargo_release_version" --locked
  fi
  echo "prerequisites: $(cargo release --version)"
fi

echo "prerequisites: done"
