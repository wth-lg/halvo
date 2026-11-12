#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export PATH="$CARGO_HOME/bin:$PATH"

cargo_release_version=1.1.7

cargo_release_asset() {
  prefix="cargo-release-v$cargo_release_version"
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "$prefix-aarch64-apple-darwin.tar.gz" ;;
    Darwin-x86_64) echo "$prefix-x86_64-apple-darwin.tar.gz" ;;
    MINGW*|MSYS*|CYGWIN*) echo "$prefix-x86_64-pc-windows-msvc.zip" ;;
    *) return 1 ;;
  esac
}

cargo_release_executable() {
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) echo cargo-release.exe ;;
    *) echo cargo-release ;;
  esac
}

unpack_cargo_release() {
  case "$1" in
    *.tar.gz) tar -xzf "$1" -C "$2" ;;
    *.zip) powershell.exe -NoProfile -Command "\$ErrorActionPreference = 'Stop'; Expand-Archive -LiteralPath '$(cygpath -w "$1")' -DestinationPath '$(cygpath -w "$2")' -Force" ;;
  esac
}

install_cargo_release_prebuilt() {
  if ! asset=$(cargo_release_asset); then
    echo "prerequisites: no prebuilt cargo-release matches $(uname -s) $(uname -m)"
    return 1
  fi
  executable=$(cargo_release_executable)
  url="https://github.com/crate-ci/cargo-release/releases/download/v$cargo_release_version/$asset"
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/prerequisites.XXXXXX") || return 1
  failed=
  if ! curl -sSfL --retry 5 --retry-delay 3 --retry-all-errors -o "$tmp/$asset" "$url"; then
    failed="downloading the prebuilt cargo-release from $url"
  elif ! unpack_cargo_release "$tmp/$asset" "$tmp"; then
    failed="unpacking the prebuilt cargo-release $asset"
  elif ! { mkdir -p "$CARGO_HOME/bin" && cp "$tmp/$executable" "$CARGO_HOME/bin/$executable"; }; then
    failed="copying the prebuilt cargo-release into $CARGO_HOME/bin"
  fi
  rm -rf "$tmp"
  if [ -n "$failed" ]; then
    echo "prerequisites: $failed failed"
    return 1
  fi
}

install_cargo_release() {
  echo "prerequisites: installing cargo-release $cargo_release_version into $CARGO_HOME/bin"
  install_cargo_release_prebuilt && return
  echo "prerequisites: building cargo-release $cargo_release_version with cargo install instead"
  cargo install cargo-release --version "$cargo_release_version" --locked
}

ensure_cargo_release() {
  if cargo release --version 2>/dev/null | grep -q " $cargo_release_version\$"; then
    echo "prerequisites: cargo-release $cargo_release_version present"
    return
  fi
  install_cargo_release || return
  version=$(cargo release --version) || return
  echo "prerequisites: $version"
}

cargo_deny_version=0.20.2
cargo_deny_sha256_aarch64_apple_darwin=fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3
cargo_deny_sha256_x86_64_apple_darwin=248da7f581724e470071990c088ffc55c811981715f4cbdb258621fb79f8b7a6
cargo_deny_sha256_x86_64_pc_windows_msvc=975a22143262fd27476d19ee00c7af67978426e40e1dee94eed6bbade1cf87dc

cargo_deny_target() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo aarch64-apple-darwin ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    MINGW*|MSYS*|CYGWIN*) echo x86_64-pc-windows-msvc ;;
    *) return 1 ;;
  esac
}

cargo_deny_sha256() {
  case "$1" in
    aarch64-apple-darwin) echo "$cargo_deny_sha256_aarch64_apple_darwin" ;;
    x86_64-apple-darwin) echo "$cargo_deny_sha256_x86_64_apple_darwin" ;;
    x86_64-pc-windows-msvc) echo "$cargo_deny_sha256_x86_64_pc_windows_msvc" ;;
    *) return 1 ;;
  esac
}

cargo_deny_executable() {
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) echo cargo-deny.exe ;;
    *) echo cargo-deny ;;
  esac
}

cargo_deny_sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum <"$1") || return 1
  else
    sum=$(shasum -a 256 <"$1") || return 1
  fi
  echo "${sum%% *}"
}

install_cargo_deny_prebuilt() {
  if ! target=$(cargo_deny_target); then
    echo "prerequisites: no prebuilt cargo-deny matches $(uname -s) $(uname -m)"
    return 1
  fi
  asset="cargo-deny-$cargo_deny_version-$target.tar.gz"
  executable=$(cargo_deny_executable)
  url="https://github.com/EmbarkStudios/cargo-deny/releases/download/$cargo_deny_version/$asset"
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/prerequisites-cargo-deny.XXXXXX") || return 1
  failed=
  if ! curl -sSfL --retry 5 --retry-delay 3 --retry-all-errors -o "$tmp/$asset" "$url"; then
    failed="downloading the prebuilt cargo-deny from $url"
  elif ! sum=$(cargo_deny_sha256_of "$tmp/$asset") || [ "$sum" != "$(cargo_deny_sha256 "$target")" ]; then
    failed="checking the sha256 of the prebuilt cargo-deny $asset"
  elif ! tar -xzf "$tmp/$asset" -C "$tmp"; then
    failed="unpacking the prebuilt cargo-deny $asset"
  elif ! { mkdir -p "$CARGO_HOME/bin" && cp "$tmp/cargo-deny-$cargo_deny_version-$target/$executable" "$CARGO_HOME/bin/$executable"; }; then
    failed="copying the prebuilt cargo-deny into $CARGO_HOME/bin"
  fi
  rm -rf "$tmp"
  if [ -n "$failed" ]; then
    echo "prerequisites: $failed failed"
    return 1
  fi
}

install_cargo_deny() {
  echo "prerequisites: installing cargo-deny $cargo_deny_version into $CARGO_HOME/bin"
  if cargo_deny_target >/dev/null; then
    install_cargo_deny_prebuilt
    return
  fi
  echo "prerequisites: no prebuilt cargo-deny matches $(uname -s) $(uname -m)"
  echo "prerequisites: building cargo-deny $cargo_deny_version with cargo install instead"
  cargo install cargo-deny --version "$cargo_deny_version" --locked
}

ensure_cargo_deny() {
  if cargo deny --version 2>/dev/null | grep -q " $cargo_deny_version\$"; then
    echo "prerequisites: cargo-deny $cargo_deny_version present"
    return
  fi
  install_cargo_deny || return
  version=$(cargo deny --version) || return
  echo "prerequisites: $version"
}

self_test() {
  asset=$(cargo_release_asset) || {
    echo "prerequisites: self-test FAIL: no prebuilt cargo-release matches $(uname -s) $(uname -m), so this platform has no case"
    return 1
  }
  executable=$(cargo_release_executable)
  work=$(mktemp -d "${TMPDIR:-/tmp}/prerequisites-self-test.XXXXXX")
  failures=0
  mkdir "$work/binary"
  echo "cargo-release $cargo_release_version" >"$work/binary/$executable"

  make_archive() {
    case "$3" in
      *.tar.gz) tar -czf "$3" -C "$1" "$2" ;;
      *.zip) powershell.exe -NoProfile -Command "\$ErrorActionPreference = 'Stop'; Compress-Archive -LiteralPath '$(cygpath -w "$1/$2")' -DestinationPath '$(cygpath -w "$3")'" ;;
    esac
  }

  new_case() {
    case_dir="$work/$1"
    mkdir "$case_dir" "$case_dir/stubs" "$case_dir/assets" "$case_dir/cargo-home"
    CARGO_HOME="$case_dir/cargo-home"
    : >"$case_dir/calls"
    : >"$case_dir/output"
    cat >"$case_dir/stubs/curl" <<EOF
#!/bin/sh
out=
while [ "\$#" -gt 1 ]; do
  if [ "\$1" = -o ]; then out=\$2; shift; fi
  shift
done
echo "curl \$1" >>'$case_dir/calls'
if [ -f '$case_dir/assets/'"\${1##*/}" ]; then
  cp '$case_dir/assets/'"\${1##*/}" "\$out"
else
  echo "curl: (22) The requested URL returned error: 404" >&2
  exit 22
fi
EOF
    cat >"$case_dir/stubs/cargo" <<EOF
#!/bin/sh
echo "cargo \$*" >>'$case_dir/calls'
case "\$1" in
  release) cat '$CARGO_HOME/bin/$executable' ;;
  install) mkdir -p '$CARGO_HOME/bin' && echo "cargo-release $cargo_release_version" >'$CARGO_HOME/bin/$executable' ;;
  *) exit 2 ;;
esac
EOF
    chmod +x "$case_dir/stubs/curl" "$case_dir/stubs/cargo"
    problems=
  }

  run_ensure() {
    status=0
    (
      PATH="$case_dir/stubs:$PATH"
      ensure_cargo_release
    ) >>"$case_dir/output" 2>&1 || status=$?
    [ "$status" = 0 ] || problems="$problems; ensure_cargo_release exited $status"
  }

  count() {
    grep -c -F -- "$1" "$2" || :
  }

  verdict() {
    if [ -z "$problems" ]; then
      echo "PASS $1"
    else
      echo "FAIL $1 (${problems#; })"
      sed 's/^/  | /' "$case_dir/output"
      failures=$((failures + 1))
    fi
  }

  name='a cargo home with no bin directory gets the prebuilt cargo-release, and a second run finds it and installs nothing'
  new_case prebuilt
  make_archive "$work/binary" "$executable" "$case_dir/assets/$asset"
  run_ensure
  run_ensure
  cmp -s "$work/binary/$executable" "$case_dir/cargo-home/bin/$executable" || problems="$problems; bin/$executable is not the binary of the archive"
  [ "$(count 'curl ' "$case_dir/calls")" = 1 ] || problems="$problems; the download ran $(count 'curl ' "$case_dir/calls") times, not once"
  [ "$(count 'cargo install' "$case_dir/calls")" = 0 ] || problems="$problems; cargo install ran"
  [ "$(count "prerequisites: cargo-release $cargo_release_version present" "$case_dir/output")" = 1 ] || problems="$problems; the second run did not report the tool present"
  [ "$(count ' failed' "$case_dir/output")" = 0 ] || problems="$problems; a line reports a failure"
  verdict "$name"

  expect_fallback() {
    run_ensure
    [ "$(count "prerequisites: $1" "$case_dir/output")" = 1 ] || problems="$problems; no line says \"prerequisites: $1\""
    for other in downloading unpacking copying 'no prebuilt'; do
      case "$1" in "$other"*) continue ;; esac
      [ "$(count "prerequisites: $other" "$case_dir/output")" = 0 ] || problems="$problems; a line names $other"
    done
    [ "$(count "cargo install cargo-release --version $cargo_release_version --locked" "$case_dir/calls")" = 1 ] || problems="$problems; cargo install did not run once with the pinned version"
    [ -f "$CARGO_HOME/bin/$executable" ] || problems="$problems; cargo install left no bin/$executable"
    [ "$(tail -n 1 "$case_dir/output")" = "prerequisites: cargo-release $cargo_release_version" ] || problems="$problems; the last line does not give the installed version"
  }

  name='a failed download is named, and cargo install still installs the tool'
  new_case download
  expect_fallback 'downloading the prebuilt cargo-release'
  verdict "$name"

  name='a failed unpacking is named, and cargo install still installs the tool'
  new_case unpack
  echo 'not an archive' >"$case_dir/assets/$asset"
  expect_fallback 'unpacking the prebuilt cargo-release'
  verdict "$name"

  name='a failed copy is named, and cargo install still installs the tool'
  new_case copy
  mkdir "$case_dir/other"
  echo 'not cargo-release' >"$case_dir/other/README.md"
  make_archive "$case_dir/other" README.md "$case_dir/assets/$asset"
  expect_fallback 'copying the prebuilt cargo-release'
  verdict "$name"

  name='a platform with no prebuilt is named, and cargo install still installs the tool'
  new_case platform
  cat >"$case_dir/stubs/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo Linux ;;
  *) echo x86_64 ;;
esac
EOF
  chmod +x "$case_dir/stubs/uname"
  expect_fallback 'no prebuilt cargo-release matches Linux x86_64'
  verdict "$name"

  deny_target=$(cargo_deny_target) || {
    echo "prerequisites: self-test FAIL: no prebuilt cargo-deny matches $(uname -s) $(uname -m), so this platform has no case"
    return 1
  }
  deny_asset="cargo-deny-$cargo_deny_version-$deny_target.tar.gz"
  deny_executable=$(cargo_deny_executable)
  deny_built="cargo-deny $cargo_deny_version built by cargo install"
  real_tar=$(command -v tar)
  mkdir "$work/deny" "$work/deny/cargo-deny-$cargo_deny_version-$deny_target"
  echo "cargo-deny $cargo_deny_version" >"$work/deny/cargo-deny-$cargo_deny_version-$deny_target/$deny_executable"
  tar -czf "$work/deny/$deny_asset" -C "$work/deny" "cargo-deny-$cargo_deny_version-$deny_target"
  deny_archive_sum=$(cargo_deny_sha256_of "$work/deny/$deny_asset")

  new_deny_case() {
    new_case "$1"
    mkdir "$case_dir/tmp"
    cat >"$case_dir/stubs/cargo" <<EOF
#!/bin/sh
echo "cargo \$*" >>'$case_dir/calls'
case "\$1" in
  deny) cat '$CARGO_HOME/bin/$deny_executable' ;;
  install) mkdir -p '$CARGO_HOME/bin' && echo "$deny_built" >'$CARGO_HOME/bin/$deny_executable' ;;
  *) exit 2 ;;
esac
EOF
    cat >"$case_dir/stubs/tar" <<EOF
#!/bin/sh
echo "tar \$*" >>'$case_dir/calls'
exec '$real_tar' "\$@"
EOF
    chmod +x "$case_dir/stubs/cargo" "$case_dir/stubs/tar"
    cp "$work/deny/$deny_asset" "$case_dir/assets/$deny_asset"
  }

  run_ensure_deny() {
    status=0
    stub_path=$(printenv PATH)
    (
      PATH="$case_dir/stubs:$stub_path"
      TMPDIR="$case_dir/tmp"
      cargo_deny_sha256() { echo "$deny_sum"; }
      ensure_cargo_deny
    ) >>"$case_dir/output" 2>&1 || status=$?
    case "$1:$status" in
      ok:0 | fails:[1-9]*) ;;
      *) problems="$problems; ensure_cargo_deny exited $status, wanted $1" ;;
    esac
    [ -z "$(ls -A "$case_dir/tmp")" ] || problems="$problems; a temporary directory was left in place"
  }

  expect_no_deny() {
    [ ! -e "$CARGO_HOME/bin/$deny_executable" ] || problems="$problems; bin/$deny_executable exists"
    [ "$(count 'cargo install' "$case_dir/calls")" = 0 ] || problems="$problems; cargo install ran"
    [ "$(count "prerequisites: $1 failed" "$case_dir/output")" = 1 ] || problems="$problems; no line says that $1 failed"
    [ "$(count 'prerequisites: building' "$case_dir/output")" = 0 ] || problems="$problems; a line names building"
  }

  name='a cargo home with no bin directory gets the prebuilt cargo-deny whose sha256 is the pinned one, and a second run finds it and installs nothing'
  new_deny_case deny-prebuilt
  deny_sum=$deny_archive_sum
  run_ensure_deny ok
  run_ensure_deny ok
  cmp -s "$work/deny/cargo-deny-$cargo_deny_version-$deny_target/$deny_executable" "$case_dir/cargo-home/bin/$deny_executable" || problems="$problems; bin/$deny_executable is not the binary of the archive"
  [ "$(count 'curl ' "$case_dir/calls")" = 1 ] || problems="$problems; the download ran $(count 'curl ' "$case_dir/calls") times, not once"
  [ "$(count 'tar ' "$case_dir/calls")" = 1 ] || problems="$problems; tar ran $(count 'tar ' "$case_dir/calls") times, not once"
  [ "$(count 'cargo install' "$case_dir/calls")" = 0 ] || problems="$problems; cargo install ran"
  [ "$(count "prerequisites: cargo-deny $cargo_deny_version present" "$case_dir/output")" = 1 ] || problems="$problems; the second run did not report the tool present"
  [ "$(count ' failed' "$case_dir/output")" = 0 ] || problems="$problems; a line reports a failure"
  verdict "$name"

  name='an archive whose sha256 is not the pinned one is never unpacked or copied, and the script fails and builds nothing'
  new_deny_case deny-checksum
  deny_sum=0000000000000000000000000000000000000000000000000000000000000000
  run_ensure_deny fails
  expect_no_deny "checking the sha256 of the prebuilt cargo-deny $deny_asset"
  [ "$(count 'tar ' "$case_dir/calls")" = 0 ] || problems="$problems; tar ran"
  verdict "$name"

  name='a failed download of the prebuilt cargo-deny fails the script, which builds nothing'
  new_deny_case deny-download
  mv "$case_dir/assets/$deny_asset" "$case_dir/assets/$deny_asset.absent"
  deny_sum=$deny_archive_sum
  run_ensure_deny fails
  expect_no_deny "downloading the prebuilt cargo-deny from https://github.com/EmbarkStudios/cargo-deny/releases/download/$cargo_deny_version/$deny_asset"
  verdict "$name"

  name='a platform with no prebuilt cargo-deny is named, and cargo install builds the tool'
  new_deny_case deny-platform
  cat >"$case_dir/stubs/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo Linux ;;
  *) echo x86_64 ;;
esac
EOF
  chmod +x "$case_dir/stubs/uname"
  deny_sum=$deny_archive_sum
  run_ensure_deny ok
  [ "$(count 'prerequisites: no prebuilt cargo-deny matches Linux x86_64' "$case_dir/output")" = 1 ] || problems="$problems; no line names the platform"
  [ "$(count 'curl ' "$case_dir/calls")" = 0 ] || problems="$problems; a download ran"
  [ "$(count "cargo install cargo-deny --version $cargo_deny_version --locked" "$case_dir/calls")" = 1 ] || problems="$problems; cargo install did not run once with the pinned version"
  [ "$(cat "$CARGO_HOME/bin/$deny_executable" 2>/dev/null)" = "$deny_built" ] || problems="$problems; bin/$deny_executable is not the one that cargo install wrote"
  verdict "$name"

  if [ "$failures" -gt 0 ]; then
    echo "prerequisites: self-test FAIL: cases failed: $failures"
    return 1
  fi
  echo "prerequisites: self-test PASS"
}

if [ "${1:-}" = --self-test ]; then
  self_test
  exit
fi

current_directory_is_git_work_tree_root() {
  [ "$(git rev-parse --is-inside-work-tree 2>/dev/null)" = true ] &&
    [ -z "$(git rev-parse --show-cdup)" ]
}

if current_directory_is_git_work_tree_root; then
  if [ "$(git config --local --get core.hooksPath || true)" != .githooks ]; then
    echo "prerequisites: setting git core.hooksPath to .githooks"
    git config --local --replace-all core.hooksPath .githooks
  fi
  if [ "$(git config --get core.hooksPath || true)" != .githooks ]; then
    echo "prerequisites: error: git does not read core.hooksPath=.githooks although the config of this clone holds it, so git runs no hook of .githooks. The setting below has a higher priority. Remove it, then run this script again." >&2
    git config --show-origin --get-all core.hooksPath | sed -n '$p' >&2
    exit 1
  fi
  echo "prerequisites: git hooks path present: .githooks"
fi

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

ensure_cargo_release
ensure_cargo_deny
cargo fetch --locked --manifest-path account/Cargo.toml

echo "prerequisites: done"
