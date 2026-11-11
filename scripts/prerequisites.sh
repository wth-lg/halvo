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

echo "prerequisites: done"
