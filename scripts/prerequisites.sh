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

sha512_of() {
  if command -v sha512sum >/dev/null 2>&1; then
    sha512sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 512 "$1" | cut -d ' ' -f 1
  fi
}

pnpm_platform() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo darwin-arm64 ;;
    Darwin-x86_64) echo darwin-x64 ;;
    MINGW*|MSYS*|CYGWIN*) echo win32-x64 ;;
    Linux-x86_64) echo linux-x64 ;;
    Linux-aarch64) echo linux-arm64 ;;
    *) return 1 ;;
  esac
}

pnpm_lock_integrity() {
  if [ ! -e "$1" ]; then
    echo no-file
    return 1
  fi
  if [ ! -f "$1" ]; then
    echo not-a-file
    return 1
  fi
  if [ ! -r "$1" ]; then
    echo unreadable
    return 1
  fi
  LC_ALL=C tr '\000' '\001' <"$1" 2>/dev/null | LC_ALL=C awk -v want="$2" -v q="'" '
    function refuse(why) { answer = why " " NR; exit }
    function starts(s, head) { return substr(s, 1, length(head)) == head }
    function framed(s, head, tail) { return length(s) > length(head) + length(tail) && starts(s, head) && substr(s, length(s) - length(tail) + 1) == tail }
    function inner(s, head, tail) { return substr(s, length(head) + 1, length(s) - length(head) - length(tail)) }
    function version(s) { return s ~ "^[0-9]+[.][0-9]+[.][0-9]+$" }
    function scoped(s) { return s ~ "^@[a-z0-9-]+/[a-z0-9._-]+$" }
    function plain(s) { return s ~ "^[a-z0-9][a-z0-9._-]*$" }
    function named(k, quoted,   at) {
      at = match(k, "@[^@]*$")
      if (at < 2 || !version(substr(k, at + 1))) return 0
      return quoted ? scoped(substr(k, 1, at - 1)) : plain(substr(k, 1, at - 1))
    }
    function entry_key(s,   k) {
      if (framed(s, "  " q, q ":")) { k = inner(s, "  " q, q ":"); return named(k, 1) ? k : "" }
      if (framed(s, "  ", ":")) { k = inner(s, "  ", ":"); return named(k, 0) ? k : "" }
      return ""
    }
    function resolution(s,   v) {
      if (!framed(s, "    resolution: {integrity: sha512-", "==}")) return ""
      v = inner(s, "    resolution: {integrity: sha512-", "==}")
      return length(v) == 86 && v ~ "^[A-Za-z0-9+/]+$" ? v "==" : ""
    }
    function listed(s, key) { return framed(s, "    " key ": [", "]") && inner(s, "    " key ": [", "]") ~ "^[a-z0-9]+$" }
    {
      sub(/\r$/, "")
      line = $0
      if (length(line) > 1024) refuse("long")
      if (NR == 1) { if (line != "---") refuse("form"); next }
      if (line == "---") exit
      if (expect) {
        expect = 0
        if (top != "packages" || resolution(line) == "") { answer = "no-resolution " entry; exit }
        value = resolution(line)
      }
      if (line == "") next
      match(line, "^ *")
      indent = RLENGTH
      if (indent == 0) {
        if (line == "importers:" || line == "packages:" || line == "snapshots:") k = substr(line, 1, length(line) - 1)
        else if (framed(line, "lockfileVersion: " q, q) && inner(line, "lockfileVersion: " q, q) ~ "^[0-9]+[.][0-9]+$") k = "lockfileVersion"
        else refuse("form")
        if (k in top_seen) refuse("repeat")
        top_seen[k] = 1
        top = k
        l2 = ""
        l4 = ""
        l6 = ""
        next
      }
      if (indent == 2) {
        if (top == "importers" && line == "  .:") k = "."
        else if (top == "packages" || top == "snapshots") k = entry_key(line)
        else k = ""
        if (k == "") refuse("form")
        if ((top, k) in seen) {
          if (top == "packages" && k == want) refuse("twice")
          refuse("repeat")
        }
        seen[top, k] = 1
        l2 = k
        l4 = ""
        l6 = ""
        if (top == "packages" && k == want) { entries++; expect = 1; entry = NR }
        next
      }
      if (indent == 4 && l2 != "") {
        k = ""
        l4 = ""
        if (top == "importers" && line == "    configDependencies: {}") k = "configDependencies"
        if (top == "importers" && line == "    packageManagerDependencies:") { k = "packageManagerDependencies"; l4 = "managers" }
        if (top == "packages" && resolution(line) != "") k = "resolution"
        if (top == "packages" && listed(line, "cpu")) k = "cpu"
        if (top == "packages" && listed(line, "os")) k = "os"
        if (top == "packages" && listed(line, "libc")) k = "libc"
        if (top == "packages" && framed(line, "    engines: {node: " q, q "}") && inner(line, "    engines: {node: " q, q "}") ~ "^[0-9A-Za-z.*<>=^~| -]+$") k = "engines"
        if (top == "packages" && line == "    hasBin: true") k = "hasBin"
        if (top == "snapshots" && line == "    optional: true") k = "optional"
        if (top == "snapshots" && line == "    optionalDependencies:") { k = "optionalDependencies"; l4 = "optional" }
        if (k == "") refuse("form")
        if ((top, l2, k) in seen) refuse("repeat")
        seen[top, l2, k] = 1
        l6 = ""
        next
      }
      if (indent == 6 && l4 == "managers" && line == "      pnpm:") {
        if ((top, l2, l4, "pnpm") in seen) refuse("repeat")
        seen[top, l2, l4, "pnpm"] = 1
        l6 = "pnpm"
        next
      }
      if (indent == 6 && l4 == "optional" && framed(line, "      " q, "") && index(line, q ": ") > 7) {
        k = substr(line, 8, index(line, q ": ") - 8)
        if (!scoped(k) || !version(substr(line, index(line, q ": ") + 3))) refuse("form")
        if ((top, l2, l4, k) in seen) refuse("repeat")
        seen[top, l2, l4, k] = 1
        next
      }
      if (indent == 8 && l6 == "pnpm" && (starts(line, "        specifier: ") || starts(line, "        version: "))) {
        k = substr(line, 9, index(line, ":") - 9)
        if (!version(substr(line, index(line, ":") + 2))) refuse("form")
        if ((top, l2, l4, l6, k) in seen) refuse("repeat")
        seen[top, l2, l4, l6, k] = 1
        next
      }
      refuse("form")
    }
    END {
      if (answer == "" && expect) answer = "no-resolution " entry
      if (answer == "" && NR == 0) answer = "empty"
      if (answer == "" && entries == 0) answer = "no-entry"
      if (answer != "") { print answer; exit 1 }
      print "value " value
    }
  '
}

sha512_checked() {
  sum=$(sha512_of "$1") || sum=
  if [ -z "$sum" ]; then
    echo "prerequisites: error: the sha512 of $1 came out empty; check that shasum or sha512sum can read it, then run sh scripts/prerequisites.sh again" >&2
    return 1
  fi
}

hex_of_base64() {
  printf '%s' "$1" | base64 -d | od -A n -v -t x1 | tr -d ' \n'
}

download_once() {
  if [ -e "$2" ] && [ ! -f "$2" ]; then
    echo "prerequisites: error: $2 is not a regular file; move it away, then run sh scripts/prerequisites.sh again" >&2
    return 1
  fi
  if [ -f "$2" ]; then
    return
  fi
  echo "prerequisites: downloading $1"
  if ! curl -fsSL --retry 3 --retry-delay 2 -o "$2.part" "$1"; then
    echo "prerequisites: error: the download of $1 failed; check the network or its proxy, then run sh scripts/prerequisites.sh again" >&2
    return 1
  fi
  mv "$2.part" "$2"
}

ensure_pnpm() {
  prefix=$1
  field=$(sed -n 's/.*"packageManager": "pnpm@\([^"]*\)".*/\1/p' backend/package.json)
  pinned=$(printf '%s\n' "$field" | sed -n 's/^\([0-9][0-9.]*\)+sha512\.[0-9a-f]\{128\}$/\1/p')
  if [ -z "$pinned" ]; then
    echo "prerequisites: error: backend/package.json pins no pnpm version with its sha512 under packageManager, as pnpm@<version>+sha512.<128 hex digits>" >&2
    return 1
  fi
  pinned_sum=${field#"$pinned"+sha512.}
  user_path=$PATH
  PATH="$prefix/bin:$prefix:$PATH"
  export PATH
  found=$(pnpm --version 2>/dev/null) || found=
  if [ "$found" = "$pinned" ]; then
    echo "prerequisites: pnpm $pinned present"
  else
    if [ -n "$found" ]; then
      echo "prerequisites: the pnpm on the path is $found, not the $pinned that backend/package.json pins"
    fi
    if ! platform=$(pnpm_platform); then
      echo "prerequisites: error: no platform package of pnpm matches $(uname -s) $(uname -m), so this script cannot check one against backend/pnpm-lock.yaml; install pnpm $pinned yourself and put it on your PATH, then run sh scripts/prerequisites.sh again" >&2
      return 1
    fi
    package=@pnpm/exe.$platform@$pinned
    if ! answer=$(pnpm_lock_integrity backend/pnpm-lock.yaml "$package"); then
      fix="take backend/pnpm-lock.yaml as main holds it"
      case $answer in
        no-file) cause="there is no backend/pnpm-lock.yaml" ;;
        not-a-file)
          cause="backend/pnpm-lock.yaml is not a regular file"
          fix="move it away and take backend/pnpm-lock.yaml as main holds it"
          ;;
        unreadable)
          cause="this user may not read backend/pnpm-lock.yaml"
          fix="give yourself the right to read it, for example with chmod u+r backend/pnpm-lock.yaml"
          ;;
        empty) cause="backend/pnpm-lock.yaml is empty" ;;
        "long "*) cause="line ${answer#long } of backend/pnpm-lock.yaml has more than 1,024 characters, more than a YAML key may have and more than any line that pnpm writes there" ;;
        "form "*) cause="line ${answer#form } of backend/pnpm-lock.yaml is in no form that this script reads in the lock's first document" ;;
        "repeat "*) cause="line ${answer#repeat } of backend/pnpm-lock.yaml repeats a key of the lock's first document" ;;
        no-entry)
          cause="the first document of backend/pnpm-lock.yaml holds no entry for $package, so that lock does not belong to the pnpm $pinned that backend/package.json pins"
          fix="take backend/package.json and backend/pnpm-lock.yaml as main holds them"
          ;;
        "twice "*) cause="the first document of backend/pnpm-lock.yaml holds the entry for $package more than once, again at line ${answer#twice }" ;;
        "no-resolution "*) cause="the entry for $package at line ${answer#no-resolution } of backend/pnpm-lock.yaml is not followed by its line    resolution: {integrity: sha512-<86 base64 characters>==}" ;;
        *) cause="the reader of backend/pnpm-lock.yaml gave no answer" ;;
      esac
      echo "prerequisites: error: $cause, so the platform package cannot be checked; $fix, then run sh scripts/prerequisites.sh again" >&2
      return 1
    fi
    integrity=${answer#value }
    tools=$(pwd -P)/backend/.tools
    downloads=$tools/downloads
    archive=$downloads/pnpm-$pinned.tgz
    platform_archive=$downloads/pnpm-exe.$platform-$pinned.tgz
    mkdir -p backend/.tools/downloads || return
    download_once "https://registry.npmjs.org/pnpm/-/pnpm-$pinned.tgz" "$archive" || return
    sha512_checked "$archive" || return
    if [ "$sum" != "$pinned_sum" ]; then
      echo "prerequisites: error: $archive has the sha512 $sum, not the one that backend/package.json pins for pnpm $pinned; move it away, then run sh scripts/prerequisites.sh again" >&2
      return 1
    fi
    download_once "https://registry.npmjs.org/@pnpm/exe.$platform/-/exe.$platform-$pinned.tgz" "$platform_archive" || return
    sha512_checked "$platform_archive" || return
    if [ "$sum" != "$(hex_of_base64 "$integrity")" ]; then
      echo "prerequisites: error: $platform_archive has the sha512 $sum, not the one that backend/pnpm-lock.yaml holds for $package; move it away, then run sh scripts/prerequisites.sh again" >&2
      return 1
    fi
    if ! npm_cache=$(mktemp -d "$tools/npm-cache.XXXXXX"); then
      echo "prerequisites: error: no new directory for npm's cache could be made under $tools; check that it is writable, then run sh scripts/prerequisites.sh again" >&2
      return 1
    fi
    echo "prerequisites: installing pnpm $pinned for the user into $prefix with npm offline and the new empty cache $npm_cache, from $archive and $platform_archive, whose sha512 values are the pinned ones"
    if ! (
      trap 'rm -rf "$npm_cache"' EXIT
      trap 'exit 1' HUP INT TERM
      npm install --global --prefix "$prefix" --cache "$npm_cache" --offline --omit=optional --no-update-notifier "$archive" "$platform_archive"
    ); then
      echo "prerequisites: error: npm could not install pnpm $pinned from $archive and $platform_archive; read npm's lines above, then run sh scripts/prerequisites.sh again" >&2
      return 1
    fi
    found=$(pnpm --version 2>/dev/null) || found=
    if [ "$found" != "$pinned" ]; then
      echo "prerequisites: error: after npm installed pnpm $pinned into $prefix, the pnpm on the path is ${found:-missing}" >&2
      return 1
    fi
    echo "prerequisites: pnpm $found"
  fi
  directory=$(dirname "$(command -v pnpm)")
  case ":$user_path:" in
    *":$directory:"*) ;;
    *) echo "prerequisites: pnpm is in $directory, which is not on your PATH; add it, for example with export PATH=\"$directory:\$PATH\" in your shell's profile" ;;
  esac
}

ensure_backend() {
  ensure_pnpm "$1" || return
  echo "prerequisites: installing the packages of backend/pnpm-lock.yaml with install scripts off"
  if ! (cd backend && pnpm install --frozen-lockfile --ignore-scripts); then
    echo "prerequisites: error: pnpm could not install the packages of backend/pnpm-lock.yaml; read its lines above, check the network or its proxy, then run sh scripts/prerequisites.sh again" >&2
    return 1
  fi
  if ! sh backend/scripts/fetch-tools.sh; then
    echo "prerequisites: error: backend/scripts/fetch-tools.sh could not fetch a pinned tool; its line above names the file, check the network or its proxy or move a file whose sha256 differs away, then run sh scripts/prerequisites.sh again" >&2
    return 1
  fi
  echo "prerequisites: fetching the licence of each package of backend/pnpm-lock.yaml that this platform does not install, for the backend's check 85"
  if ! node backend/scripts/fetch-licenses.mjs; then
    echo "prerequisites: error: backend/scripts/fetch-licenses.mjs could not fetch a licence from the package registry; its lines above name the package, check the network or its proxy, then run sh scripts/prerequisites.sh again" >&2
    return 1
  fi
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

  integrity_of() {
    node -e "process.stdout.write(require('node:crypto').createHash('sha512').update(require('node:fs').readFileSync(0)).digest('base64'))" <"$1"
  }

  write_lock() {
    {
      printf -- "---\nlockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    packageManagerDependencies:\n      pnpm:\n        specifier: 12.14.0\n        version: 12.14.0\n\npackages:\n\n"
      while [ "$#" -gt 1 ]; do
        printf "  '%s':\n    resolution: {integrity: sha512-%s}\n    cpu: [arm64]\n\n" "$1" "$2"
        shift 2
      done
      printf -- "---\nlockfileVersion: '9.0'\n\npackages:\n\n"
    } >"$case_dir/repo/backend/pnpm-lock.yaml"
  }

  new_backend_case() {
    new_case "$1"
    mkdir -p "$case_dir/repo/backend/scripts" "$case_dir/home"
    printf 'the pnpm 12.14.0 archive of a case\n' >"$case_dir/assets/pnpm-12.14.0.tgz"
    printf '{\n  "packageManager": "pnpm@12.14.0+sha512.%s"\n}\n' "$(sha512_of "$case_dir/assets/pnpm-12.14.0.tgz")" >"$case_dir/repo/backend/package.json"
    case_platform=$(pnpm_platform)
    platform_asset=$case_dir/assets/exe.$case_platform-12.14.0.tgz
    printf 'the pnpm 12.14.0 binary of a case for %s\n' "$case_platform" >"$platform_asset"
    write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")"
    cat >"$case_dir/repo/backend/scripts/fetch-tools.sh" <<EOF
echo "fetch-tools \$(pwd -P)" >>'$case_dir/calls'
EOF
    cat >"$case_dir/pnpm" <<EOF
#!/bin/sh
case "\$1" in
  --version) echo 12.14.0 ;;
  *)
    echo "pnpm \$* in \$(pwd -P)" >>'$case_dir/calls'
    exit "\$(cat '$case_dir/pnpm.exit' 2>/dev/null || echo 0)"
    ;;
esac
EOF
    cat >"$case_dir/stubs/npm" <<EOF
#!/bin/sh
line=npm
while [ "\$#" -gt 0 ]; do
  if [ "\$1" = --cache ]; then
    echo "\$2 \$(ls -A "\$2" 2>/dev/null | wc -l | tr -d ' ') \$([ -d "\$2" ] && echo directory)" >>'$case_dir/cache'
    line="\$line --cache <cache>"
    shift 2
    continue
  fi
  line="\$line \$1"
  shift
done
echo "\$line" >>'$case_dir/calls'
mkdir -p '$case_dir/home/.local/bin' && cp '$case_dir/pnpm' '$case_dir/home/.local/bin/pnpm'
EOF
    mv "$case_dir/stubs/curl" "$case_dir/curl-stub"
    cat >"$case_dir/stubs/curl" <<EOF
#!/bin/sh
echo "\$*" >>'$case_dir/curl-args'
exec '$case_dir/curl-stub' "\$@"
EOF
    chmod +x "$case_dir/stubs/curl"
    cat >"$case_dir/stubs/node" <<EOF
#!/bin/sh
echo "node \$* in \$(pwd -P)" >>'$case_dir/calls'
exit "\$(cat '$case_dir/node.exit' 2>/dev/null || echo 0)"
EOF
    chmod +x "$case_dir/pnpm" "$case_dir/stubs/npm" "$case_dir/stubs/node"
    backend_dir=$(cd "$case_dir/repo/backend" && pwd -P)
    repo_dir=$(cd "$case_dir/repo" && pwd -P)
    archive_path=$repo_dir/backend/.tools/downloads/pnpm-12.14.0.tgz
    platform_archive_path=$repo_dir/backend/.tools/downloads/pnpm-exe.$case_platform-12.14.0.tgz
    npm_line="npm install --global --prefix $case_dir/home/.local --cache <cache> --offline --omit=optional --no-update-notifier $archive_path $platform_archive_path"
    platform_url=https://registry.npmjs.org/@pnpm/exe.$case_platform/-/exe.$case_platform-12.14.0.tgz
  }

  expect_nothing_installed() {
    [ "$(grep -c '^npm ' "$case_dir/calls" || :)" = 0 ] || problems="$problems; npm ran"
    [ ! -e "$case_dir/home/.local" ] || problems="$problems; the prefix $case_dir/home/.local exists"
    [ "$(count 'pnpm install' "$case_dir/calls")" = 0 ] || problems="$problems; pnpm install ran"
    [ "$(count 'prerequisites: error:' "$case_dir/output")" = 1 ] || problems="$problems; the step did not stop at its first error line"
  }

  run_ensure_backend() {
    status=0
    (
      cd "$case_dir/repo" || exit 2
      PATH="$case_dir/stubs:/usr/bin:/bin"
      ensure_backend "$case_dir/home/.local"
    ) >>"$case_dir/output" 2>&1 || status=$?
    case "$1:$status" in
      ok:0 | fails:[1-9]*) ;;
      *) problems="$problems; ensure_backend exited $status, wanted $1" ;;
    esac
  }

  name='with no pnpm on the path, npm installs the pinned pnpm for the user, the backend packages and tools are installed, and a second run installs no pnpm'
  new_backend_case backend-no-pnpm
  run_ensure_backend ok
  run_ensure_backend ok
  [ "$(count "$npm_line" "$case_dir/calls")" = 1 ] || problems="$problems; npm did not install pnpm offline under the user's .local from the archive and the platform package once"
  [ "$(grep -c '^npm ' "$case_dir/calls" || :)" = 1 ] || problems="$problems; npm ran more than once"
  [ "$(count 'curl ' "$case_dir/calls")" = 2 ] || problems="$problems; the archive and the platform package were not downloaded once each"
  [ "$(count "curl $platform_url" "$case_dir/calls")" = 1 ] || problems="$problems; the platform package was not downloaded once from $platform_url"
  cmp -s "$platform_asset" "$platform_archive_path" || problems="$problems; the platform package that npm installs is not the downloaded one"
  [ "$(count "pnpm install --frozen-lockfile --ignore-scripts in $backend_dir" "$case_dir/calls")" = 2 ] || problems="$problems; pnpm install did not run in backend/ with the frozen lockfile and no install scripts on each run"
  [ "$(count "fetch-tools $repo_dir" "$case_dir/calls")" = 2 ] || problems="$problems; backend/scripts/fetch-tools.sh did not run on each run"
  [ "$(count 'prerequisites: pnpm 12.14.0 present' "$case_dir/output")" = 1 ] || problems="$problems; the second run did not report pnpm present"

  [ "$(count "prerequisites: pnpm is in $case_dir/home/.local/bin, which is not on your PATH" "$case_dir/output")" = 2 ] || problems="$problems; no line told the person on each run to put the user's pnpm directory on the PATH"
  verdict "$name"

  name='a pnpm on the path of another version is not used: npm installs the pinned one for the user, and that one runs the install'
  new_backend_case backend-other-pnpm
  cat >"$case_dir/stubs/pnpm" <<EOF
#!/bin/sh
case "\$1" in
  --version) echo 11.0.0 ;;
  *) echo "other-pnpm \$*" >>'$case_dir/calls' ;;
esac
EOF
  chmod +x "$case_dir/stubs/pnpm"
  run_ensure_backend ok
  [ "$(count "$npm_line" "$case_dir/calls")" = 1 ] || problems="$problems; npm did not install pnpm from the archive and the platform package once"
  [ "$(count 'other-pnpm ' "$case_dir/calls")" = 0 ] || problems="$problems; the pnpm of another version ran"
  [ "$(count "pnpm install --frozen-lockfile --ignore-scripts in $backend_dir" "$case_dir/calls")" = 1 ] || problems="$problems; the pinned pnpm did not run the install once"
  verdict "$name"

  name='where npm puts pnpm in the prefix itself, as on Windows, that entry of the path finds it'
  new_backend_case backend-pnpm-in-the-prefix
  cat >"$case_dir/stubs/npm" <<EOF
#!/bin/sh
echo "npm \$*" >>'$case_dir/calls'
mkdir -p '$case_dir/home/.local' && cp '$case_dir/pnpm' '$case_dir/home/.local/pnpm'
EOF
  chmod +x "$case_dir/stubs/npm"
  run_ensure_backend ok
  [ "$(count "pnpm install --frozen-lockfile --ignore-scripts in $backend_dir" "$case_dir/calls")" = 1 ] || problems="$problems; the pnpm in the prefix did not run the install"
  [ "$(count "prerequisites: pnpm is in $case_dir/home/.local, which is not on your PATH" "$case_dir/output")" = 1 ] || problems="$problems; no line named the prefix for the PATH"
  verdict "$name"

  name='a failed npm install of pnpm fails the step, and no install or fetch runs'
  new_backend_case backend-npm-fails
  cat >"$case_dir/stubs/npm" <<EOF
#!/bin/sh
echo "npm \$*" >>'$case_dir/calls'
exit 1
EOF
  chmod +x "$case_dir/stubs/npm"
  run_ensure_backend fails
  [ "$(count 'pnpm install' "$case_dir/calls")" = 0 ] || problems="$problems; pnpm install ran after the failed npm"
  [ "$(count 'fetch-tools ' "$case_dir/calls")" = 0 ] || problems="$problems; the tools were fetched after the failed npm"
  [ "$(count 'after npm installed pnpm' "$case_dir/output")" = 0 ] || problems="$problems; the step went on after npm failed"
  [ "$(count 'then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line said what to do after npm failed"
  [ "$(count "npm could not install pnpm 12.14.0 from $archive_path and $platform_archive_path; read npm's lines above" "$case_dir/output")" = 1 ] || problems="$problems; the line after the failed npm did not name both files"
  [ "$(find "$repo_dir/backend/.tools" -name 'npm-cache.*' | wc -l | tr -d ' ')" = 0 ] || problems="$problems; the cache of npm was left after npm failed"
  verdict "$name"

  name='npm gets a cache directory that the step made new and empty under backend/.tools, and the step removes it'
  new_backend_case backend-npm-cache
  run_ensure_backend ok
  [ "$(count "$npm_line" "$case_dir/calls")" = 1 ] || problems="$problems; npm did not get the cache flag once"
  cache_seen=$(cat "$case_dir/cache" 2>/dev/null) || cache_seen=none
  case "$cache_seen" in
    "$repo_dir/backend/.tools/npm-cache."*" 0 directory") ;;
    *) problems="$problems; npm's cache was not a new empty directory under backend/.tools: $cache_seen" ;;
  esac
  [ "$(find "$repo_dir/backend/.tools" -name 'npm-cache.*' | wc -l | tr -d ' ')" = 0 ] || problems="$problems; the cache of npm was left after the install"
  [ -f "$archive_path" ] && [ -f "$platform_archive_path" ] || problems="$problems; a downloaded file was removed with the cache"
  verdict "$name"

  name='a directory under the kept name of the platform package is refused with a line that says so'
  new_backend_case backend-platform-kept-directory
  mkdir -p "$platform_archive_path"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "prerequisites: error: $platform_archive_path is not a regular file; move it away, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line said that the kept name is no regular file"
  verdict "$name"

  name='a sum that comes out empty is refused with a line that says so, and no line prints an empty value'
  new_backend_case backend-empty-sum
  printf '#!/bin/sh\nexit 0\n' >"$case_dir/stubs/sha512sum"
  chmod +x "$case_dir/stubs/sha512sum"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "prerequisites: error: the sha512 of $archive_path came out empty; check that shasum or sha512sum can read it, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line said that the sum came out empty"
  [ "$(count 'has the sha512 ,' "$case_dir/output")" = 0 ] || problems="$problems; a line printed an empty sum"
  verdict "$name"

  name='each download runs curl with -f to a .part name, then moves it, and the pnpm archive comes from its pinned address'
  new_backend_case backend-download-form
  run_ensure_backend ok
  first_call=$(sed -n 1p "$case_dir/curl-args" 2>/dev/null) || first_call=none
  case "${first_call%% *}" in
    -*f*) ;;
    *) problems="$problems; curl did not get -f: $first_call" ;;
  esac
  case "$first_call" in
    *" -o $archive_path.part "*) ;;
    *) problems="$problems; the download did not go to the .part name: $first_call" ;;
  esac
  [ -f "$archive_path" ] && [ ! -e "$archive_path.part" ] || problems="$problems; the .part file was not moved to the kept name"
  [ "${first_call##* }" = https://registry.npmjs.org/pnpm/-/pnpm-12.14.0.tgz ] || problems="$problems; the pnpm archive did not come from its pinned address: ${first_call##* }"
  verdict "$name"

  name='an archive of other bytes than the pinned sha512 is refused: npm installs nothing, and one line says what to do'
  new_backend_case backend-archive-other-bytes
  printf 'other bytes\n' >"$case_dir/assets/pnpm-12.14.0.tgz"
  run_ensure_backend fails
  [ "$(count 'npm install' "$case_dir/calls")" = 0 ] || problems="$problems; npm ran"
  [ "$(count 'not the one that backend/package.json pins for pnpm 12.14.0; move it away, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line refused the archive and said what to do"
  [ -f "$archive_path" ] || problems="$problems; the refused archive was not kept for the person to read"
  verdict "$name"

  name='a failed download of the archive fails the step: npm installs nothing, and one line says what to do'
  new_backend_case backend-archive-download-fails
  mv "$case_dir/assets/pnpm-12.14.0.tgz" "$case_dir/assets/pnpm-12.14.0.tgz.absent"
  run_ensure_backend fails
  [ "$(count 'npm install' "$case_dir/calls")" = 0 ] || problems="$problems; npm ran"
  [ "$(count 'failed; check the network or its proxy, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line said what to do after the failed download"
  verdict "$name"

  expect_platform_refused() {
    expect_nothing_installed
    [ "$(count "has the sha512 $(sha512_of "$platform_archive_path"), $1" "$case_dir/output")" = 1 ] || problems="$problems; no line refused the platform package and said what to do"
    [ -f "$platform_archive_path" ] || problems="$problems; the refused platform package was not kept for the person to read"
  }

  name='a platform package whose bytes differ from the value of the first document of backend/pnpm-lock.yaml is refused: npm installs nothing, and one line says what to do'
  new_backend_case backend-platform-other-bytes
  printf 'other bytes\n' >"$platform_asset"
  run_ensure_backend fails
  [ "$(count "curl $platform_url" "$case_dir/calls")" = 1 ] || problems="$problems; the platform package was not downloaded once"
  expect_platform_refused "not the one that backend/pnpm-lock.yaml holds for @pnpm/exe.$case_platform@12.14.0; move it away, then run sh scripts/prerequisites.sh again"
  verdict "$name"

  name='a platform package of another version is refused, although the lock holds its bytes under that version'
  new_backend_case backend-platform-other-version
  printf 'the pnpm 12.13.0 binary of a case\n' >"$platform_asset"
  write_lock "@pnpm/exe.$case_platform@12.13.0" "$(integrity_of "$platform_asset")" "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$case_dir/assets/pnpm-12.14.0.tgz")"
  run_ensure_backend fails
  expect_platform_refused "not the one that backend/pnpm-lock.yaml holds for @pnpm/exe.$case_platform@12.14.0; move it away, then run sh scripts/prerequisites.sh again"
  verdict "$name"

  name='the value of another platform package in the lock does not admit the package of this platform'
  new_backend_case backend-platform-value-of-another
  write_lock "@pnpm/exe.another-platform@12.14.0" "$(integrity_of "$platform_asset")" "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$case_dir/assets/pnpm-12.14.0.tgz")"
  run_ensure_backend fails
  expect_platform_refused "not the one that backend/pnpm-lock.yaml holds for @pnpm/exe.$case_platform@12.14.0; move it away, then run sh scripts/prerequisites.sh again"
  verdict "$name"

  name='a platform package already in the downloads directory is checked again, not downloaded again, and refused when its bytes differ'
  new_backend_case backend-platform-kept-other-bytes
  mkdir -p "$case_dir/repo/backend/.tools/downloads"
  printf 'bytes written after the download\n' >"$platform_archive_path"
  run_ensure_backend fails
  [ "$(count "curl $platform_url" "$case_dir/calls")" = 0 ] || problems="$problems; the kept platform package was downloaded again"
  expect_platform_refused "not the one that backend/pnpm-lock.yaml holds for @pnpm/exe.$case_platform@12.14.0; move it away, then run sh scripts/prerequisites.sh again"
  verdict "$name"

  name='a platform package already in the downloads directory with the bytes of the lock is installed without a download'
  new_backend_case backend-platform-kept
  mkdir -p "$case_dir/repo/backend/.tools/downloads"
  cp "$platform_asset" "$platform_archive_path"
  run_ensure_backend ok
  [ "$(count "curl $platform_url" "$case_dir/calls")" = 0 ] || problems="$problems; the kept platform package was downloaded again"
  [ "$(count "$npm_line" "$case_dir/calls")" = 1 ] || problems="$problems; npm did not install the kept platform package once"
  verdict "$name"

  name='a failed download of the platform package fails the step: npm installs nothing, and one line says what to do'
  new_backend_case backend-platform-download-fails
  mv "$platform_asset" "$platform_asset.absent"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "the download of $platform_url failed; check the network or its proxy, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line said what to do after the failed download of the platform package"
  verdict "$name"

  name='a lock whose first document holds no value for the platform package stops the step before any download'
  new_backend_case backend-lock-without-platform
  write_lock
  printf "  '@pnpm/exe.%s@12.14.0':\n    resolution: {integrity: sha512-%s}\n\n" "$case_platform" "$(integrity_of "$platform_asset")" >>"$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'curl ' "$case_dir/calls")" = 0 ] || problems="$problems; a download ran"
  [ "$(count "the first document of backend/pnpm-lock.yaml holds no entry for @pnpm/exe.$case_platform@12.14.0, so that lock does not belong to the pnpm 12.14.0 that backend/package.json pins, so the platform package cannot be checked; take backend/package.json and backend/pnpm-lock.yaml as main holds them, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line named the missing entry and what to do"
  verdict "$name"

  name='with no lock file the step names that cause and what to do, before any download'
  new_backend_case backend-lock-absent
  mv "$case_dir/repo/backend/pnpm-lock.yaml" "$case_dir/repo/backend/pnpm-lock.yaml.absent"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'prerequisites: error: there is no backend/pnpm-lock.yaml, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line named the missing lock and what to do"
  verdict "$name"

  name='a directory in the place of the lock is named as no regular file, with what to do, before any download'
  new_backend_case backend-lock-directory
  mv "$case_dir/repo/backend/pnpm-lock.yaml" "$case_dir/repo/backend/pnpm-lock.yaml.moved"
  mkdir "$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'prerequisites: error: backend/pnpm-lock.yaml is not a regular file, so the platform package cannot be checked; move it away and take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line named the directory and what to do"
  verdict "$name"

  name='a lock that this user may not read is named so, with what to do, and the shell prints no line of its own'
  new_backend_case backend-lock-unreadable
  chmod 000 "$case_dir/repo/backend/pnpm-lock.yaml"
  if [ -r "$case_dir/repo/backend/pnpm-lock.yaml" ]; then
    echo "  this platform or user reads a file of mode 000, so the case holds nothing here"
  else
    run_ensure_backend fails
    expect_nothing_installed
    [ "$(count 'prerequisites: error: this user may not read backend/pnpm-lock.yaml, so the platform package cannot be checked; give yourself the right to read it, for example with chmod u+r backend/pnpm-lock.yaml, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line named the unreadable lock and what to do"
    [ "$(count 'Permission denied' "$case_dir/output")" = 0 ] || problems="$problems; the shell printed a line of its own"
  fi
  chmod 644 "$case_dir/repo/backend/pnpm-lock.yaml"
  verdict "$name"

  name='an empty lock is named so, with what to do, before any download'
  new_backend_case backend-lock-empty
  : >"$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'prerequisites: error: backend/pnpm-lock.yaml is empty, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line named the empty lock and what to do"
  verdict "$name"

  name='a key line of more than 1,024 characters is named by its number, before any download'
  new_backend_case backend-lock-long-key
  long_name=$(printf '%01100d' 0 | tr 0 a)
  write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")" "@pnpm/exe.$long_name@12.14.0" "$(integrity_of "$platform_asset")"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'prerequisites: error: line 18 of backend/pnpm-lock.yaml has more than 1,024 characters, more than a YAML key may have and more than any line that pnpm writes there, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; no line named the long key and what to do"
  verdict "$name"

  name='a line of the lock in no form that the script reads is named by its number, before any download'
  new_backend_case backend-lock-line-in-no-form
  write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")"
  sed '3s/^$/# a note/' "$case_dir/repo/backend/pnpm-lock.yaml" >"$case_dir/lock"
  mv "$case_dir/lock" "$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "prerequisites: error: line 3 of backend/pnpm-lock.yaml is in no form that this script reads in the lock's first document, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line named line 3 and what to do"
  verdict "$name"

  name='a key that the first document of the lock repeats is named by its line, before any download'
  new_backend_case backend-lock-repeated-key
  write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")"
  sed "3s/^\$/lockfileVersion: '9.0'/" "$case_dir/repo/backend/pnpm-lock.yaml" >"$case_dir/lock"
  mv "$case_dir/lock" "$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "prerequisites: error: line 3 of backend/pnpm-lock.yaml repeats a key of the lock's first document, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line named the repeated key and what to do"
  verdict "$name"

  name='an entry of the package that stands twice is named by the line of its second one, before any download'
  new_backend_case backend-lock-entry-twice
  write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")" "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "prerequisites: error: the first document of backend/pnpm-lock.yaml holds the entry for @pnpm/exe.$case_platform@12.14.0 more than once, again at line 18, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line named the second entry and what to do"
  verdict "$name"

  name='an entry whose next line is not its resolution line is named by its line, before any download'
  new_backend_case backend-lock-entry-without-resolution
  write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")"
  awk '/^    resolution: /{ print "    cpu: [arm64]"; next } { print }' "$case_dir/repo/backend/pnpm-lock.yaml" >"$case_dir/lock"
  mv "$case_dir/lock" "$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count "prerequisites: error: the entry for @pnpm/exe.$case_platform@12.14.0 at line 14 of backend/pnpm-lock.yaml is not followed by its line    resolution: {integrity: sha512-<86 base64 characters>==}, so the platform package cannot be checked; take backend/pnpm-lock.yaml as main holds it, then run sh scripts/prerequisites.sh again" "$case_dir/output")" = 1 ] || problems="$problems; no line named the entry without its resolution and what to do"
  verdict "$name"

  name='a lock that names the served bytes in a scalar above its quoted packages key is refused before any download: npm installs nothing'
  new_backend_case backend-lock-scalar-names-served
  printf -- "---\nlockfileVersion: '9.0'\n\nnotes: \"\npackages:\n\n  '@pnpm/exe.%s@12.14.0':\n    resolution: {integrity: sha512-%s}\n\nend\"\n'packages':\n\n  '@pnpm/exe.%s@12.14.0':\n    resolution: {integrity: sha512-%s}\n\n---\n" "$case_platform" "$(integrity_of "$platform_asset")" "$case_platform" "$(integrity_of "$case_dir/assets/pnpm-12.14.0.tgz")" >"$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'curl ' "$case_dir/calls")" = 0 ] || problems="$problems; a download ran"
  verdict "$name"

  name='a lock whose quoted packages key names the served bytes below a scalar that names others is refused before any download'
  new_backend_case backend-lock-quoted-key-names-served
  printf -- "---\nlockfileVersion: '9.0'\n\nnotes: \"\npackages:\n\n  '@pnpm/exe.%s@12.14.0':\n    resolution: {integrity: sha512-%s}\n\nend\"\n'packages':\n\n  '@pnpm/exe.%s@12.14.0':\n    resolution: {integrity: sha512-%s}\n\n---\n" "$case_platform" "$(integrity_of "$case_dir/assets/pnpm-12.14.0.tgz")" "$case_platform" "$(integrity_of "$platform_asset")" >"$case_dir/repo/backend/pnpm-lock.yaml"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'curl ' "$case_dir/calls")" = 0 ] || problems="$problems; a download ran"
  verdict "$name"

  name='the pnpm archive served at the address of the platform package is refused: npm installs nothing'
  new_backend_case backend-platform-is-the-archive
  cp "$case_dir/assets/pnpm-12.14.0.tgz" "$platform_asset"
  run_ensure_backend fails
  [ "$(count "curl $platform_url" "$case_dir/calls")" = 1 ] || problems="$problems; the platform package was not downloaded once"
  expect_platform_refused "not the one that backend/pnpm-lock.yaml holds for @pnpm/exe.$case_platform@12.14.0; move it away, then run sh scripts/prerequisites.sh again"
  verdict "$name"

  name='a pin of another version than 12.14.0 reads the entry of that version and installs its two files'
  new_backend_case backend-another-pinned-version
  printf 'the pnpm 12.15.0 archive of a case\n' >"$case_dir/assets/pnpm-12.15.0.tgz"
  printf 'the pnpm 12.15.0 binary of a case\n' >"$case_dir/assets/exe.$case_platform-12.15.0.tgz"
  printf '{\n  "packageManager": "pnpm@12.15.0+sha512.%s"\n}\n' "$(sha512_of "$case_dir/assets/pnpm-12.15.0.tgz")" >"$case_dir/repo/backend/package.json"
  write_lock "@pnpm/exe.$case_platform@12.14.0" "$(integrity_of "$platform_asset")" "@pnpm/exe.$case_platform@12.15.0" "$(integrity_of "$case_dir/assets/exe.$case_platform-12.15.0.tgz")"
  sed 's/echo 12.14.0/echo 12.15.0/' "$case_dir/pnpm" >"$case_dir/pnpm.new"
  mv "$case_dir/pnpm.new" "$case_dir/pnpm"
  chmod +x "$case_dir/pnpm"
  run_ensure_backend ok
  [ "$(count "--offline --omit=optional --no-update-notifier $repo_dir/backend/.tools/downloads/pnpm-12.15.0.tgz $repo_dir/backend/.tools/downloads/pnpm-exe.$case_platform-12.15.0.tgz" "$case_dir/calls")" = 1 ] || problems="$problems; npm did not install the two files of 12.15.0"
  [ "$(count "curl https://registry.npmjs.org/@pnpm/exe.$case_platform/-/exe.$case_platform-12.15.0.tgz" "$case_dir/calls")" = 1 ] || problems="$problems; the platform package of 12.15.0 was not downloaded"
  verdict "$name"

  name='a write error of tr, as when the reader of the lock stops at the end of its first document, prints no line'
  new_backend_case backend-lock-reader-stops-early
  cat >"$case_dir/stubs/tr" <<'EOF'
#!/bin/sh
if [ "$1" = '\000' ]; then
  echo "tr: stdout: Broken pipe" >&2
fi
exec /usr/bin/tr "$@"
EOF
  chmod +x "$case_dir/stubs/tr"
  run_ensure_backend ok
  [ "$(count 'Broken pipe' "$case_dir/output")" = 0 ] || problems="$problems; the write error of tr reached the output"
  [ "$(count "$npm_line" "$case_dir/calls")" = 1 ] || problems="$problems; npm did not install pnpm once"
  verdict "$name"

  name='a host for which this script names no platform package of pnpm stops the step before any download'
  new_backend_case backend-no-platform
  cat >"$case_dir/stubs/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo FreeBSD ;;
  *) echo amd64 ;;
esac
EOF
  chmod +x "$case_dir/stubs/uname"
  run_ensure_backend fails
  expect_nothing_installed
  [ "$(count 'curl ' "$case_dir/calls")" = 0 ] || problems="$problems; a download ran"
  [ "$(count 'no platform package of pnpm matches FreeBSD amd64' "$case_dir/output")" = 1 ] || problems="$problems; no line named the host"
  [ "$(count 'install pnpm 12.14.0 yourself and put it on your PATH, then run sh scripts/prerequisites.sh again' "$case_dir/output")" = 1 ] || problems="$problems; the line did not say what to do"
  verdict "$name"

  name='each host that the script knows gets the name of its platform package, and another host gets none'
  new_case pnpm-platform
  while read -r system machine wanted; do
    cat >"$case_dir/stubs/uname" <<STUB
#!/bin/sh
case "\$1" in
  -s) echo $system ;;
  *) echo $machine ;;
esac
STUB
    chmod +x "$case_dir/stubs/uname"
    got=$(PATH="$case_dir/stubs:/usr/bin:/bin" pnpm_platform) || got=none
    [ "$got" = "$wanted" ] || problems="$problems; $system $machine gave $got, not $wanted"
  done <<'EOF'
Darwin arm64 darwin-arm64
Darwin x86_64 darwin-x64
MINGW64_NT-10.0-26100 x86_64 win32-x64
MSYS_NT-10.0-26100 x86_64 win32-x64
CYGWIN_NT-10.0-26100 x86_64 win32-x64
Linux x86_64 linux-x64
Linux aarch64 linux-arm64
Linux riscv64 none
Darwin ppc none
FreeBSD amd64 none
EOF
  verdict "$name"

  name='the lock is read for the sha512 on the line under the entry of its first document, and for nothing else'
  new_case pnpm-lock
  mkdir -p "$case_dir/repo/backend"
  printf 'one\n' >"$case_dir/one"
  printf 'two\n' >"$case_dir/two"
  good=$(integrity_of "$case_dir/one")
  other=$(integrity_of "$case_dir/two")
  lock_file=$case_dir/repo/backend/pnpm-lock.yaml
  lock_reads() {
    if answer=$(pnpm_lock_integrity "$lock_file" '@pnpm/exe.p@1.0.0'); then
      got="value ${answer#value }"
    else
      got="no value"
    fi
    wanted="no value"
    [ -z "$1" ] || wanted="value $1"
    [ "$got" = "$wanted" ] || problems="$problems; $2 gave '$got' ($answer)"
  }
  write_lock '@pnpm/exe.p@1.0.0' "$good"
  lock_reads "$good" 'the entry of the first document'
  write_lock '@pnpm/exe.q@1.0.0' "$other" '@pnpm/exe.p@1.0.0' "$good" '@pnpm/exe.r@1.0.0' "$other"
  lock_reads "$good" 'the entry between two others'
  write_lock '@pnpm/exe.p@1.0.1' "$good" '@pnpm/exe.p@1.0.0.1' "$good"
  lock_reads '' 'entries of other versions'
  write_lock
  printf "  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >>"$lock_file"
  lock_reads '' 'an entry of the second document'
  write_lock '@pnpm/exe.p@1.0.0' "${good%==}"
  lock_reads '' 'a value without its padding'
  write_lock '@pnpm/exe.p@1.0.0' "A$good"
  lock_reads '' 'a value one character longer'
  write_lock
  printf "  '@pnpm/exe.p@1.0.0':\n    cpu: [arm64]\n    resolution: {integrity: sha512-%s}\n\n" "$good" >"$case_dir/entry"
  sed -n '1,/^packages:/p' "$lock_file" >"$case_dir/lock"
  printf '\n' >>"$case_dir/lock"
  cat "$case_dir/entry" >>"$case_dir/lock"
  cp "$case_dir/lock" "$lock_file"
  lock_reads '' 'a line between the entry and its resolution'
  write_lock
  sed -n '1,/^packages:/p' "$lock_file" >"$case_dir/lock"
  printf "\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha1-%s}\n\n" "$good" >>"$case_dir/lock"
  cp "$case_dir/lock" "$lock_file"
  lock_reads '' 'a value of another algorithm'
  write_lock
  sed -n '1,/^packages:/p' "$lock_file" >"$case_dir/lock"
  printf "\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s, tarball: https://example.invalid/p.tgz}\n\n" "$good" >>"$case_dir/lock"
  cp "$case_dir/lock" "$lock_file"
  lock_reads '' 'a resolution with a second field'
  write_lock '@pnpm/exe.p@1.0.0' "$other" '@pnpm/exe.p@1.0.0' "$good"
  lock_reads '' 'two entries of the package'
  printf -- "---\npackages:\n\n  \"@pnpm/exe.p@1.0.0\":\n    resolution: {integrity: sha512-%s}\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$other" "$good" >"$lock_file"
  lock_reads '' 'two entries of the package, one of them in double quotes'
  printf -- "---\nnotes: |\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$other" "$good" >"$lock_file"
  lock_reads '' 'the entry under packages: after the text of an entry in a block scalar'
  printf -- "---\nnotes: |\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\npackages:\n" "$good" >"$lock_file"
  lock_reads '' 'the text of an entry in a block scalar alone'
  printf -- "---\nimporters:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\npackages:\n" "$good" >"$lock_file"
  lock_reads '' 'an entry under importers:'
  write_lock '@pnpm/exe.p@1.0.0' "$good"
  awk '{ printf "%s\r\n", $0 }' "$lock_file" >"$case_dir/lock"
  cp "$case_dir/lock" "$lock_file"
  lock_reads "$good" 'a first document with CRLF lines, read as YAML reads them'
  printf -- "---\npackages:\n# a note\r---\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'an entry after a document marker that a lone carriage return starts'
  printf -- "---\npackages:\n\n  \"@pnpm/exe.p@1.0.0\":\n    resolution: {integrity: sha512-%s}\n--- \npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" "$other" >"$lock_file"
  lock_reads '' 'an entry after a document marker with a trailing space'
  printf -- "---\npackages:\n...\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'an entry after the end marker of the first document'
  printf -- "---\npackages:\n--- \npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'the only entry, after a document marker with a trailing space'
  printf -- "---\npackages:\n...\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'the only entry, under a packages: key after the end marker of the first document'
  printf -- "---\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n    resolution: {integrity: sha512-%s}\n" "$good" "$other" >"$lock_file"
  lock_reads '' 'an entry with two resolution lines'
  printf -- "---\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n    cpu: [arm64]\n\nsnapshots:\n\n  '@pnpm/exe.p@1.0.0':\n    optional: true\n\n---\nlockfileVersion: '9.0'\n" "$good" >"$lock_file"
  lock_reads "$good" 'the entry under packages: with its snapshot under snapshots:'
  printf -- "---\r\npackages:\r\n\r\n---\r\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'an entry after a first document with CRLF lines'
  printf -- "--- \npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'the entry of a first document whose marker has a trailing space'
  printf -- "---\npackages:\n# a comment\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n" "$good" >"$lock_file"
  lock_reads '' 'the entry after a comment under packages:'
  printf -- "---\nimporters:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\npackages:\n\n  \"@pnpm/exe.p@1.0.0\":\n    resolution: {integrity: sha512-%s}\n" "$good" "$other" >"$lock_file"
  lock_reads '' 'an entry under importers: with the package in double quotes under packages:'
  mv "$lock_file" "$lock_file.absent"
  lock_reads '' 'no lock file'
  verdict "$name"

  name='a lock gives a value only when every line of its first document is in a form that pnpm writes there'
  new_case pnpm-lock-forms
  mkdir -p "$case_dir/repo/backend"
  printf 'one\n' >"$case_dir/one"
  printf 'two\n' >"$case_dir/two"
  good=$(integrity_of "$case_dir/one")
  other=$(integrity_of "$case_dir/two")
  lock_file=$case_dir/repo/backend/pnpm-lock.yaml
  base_lock() {
    printf -- "---\nlockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    configDependencies: {}\n    packageManagerDependencies:\n      pnpm:\n        specifier: 1.0.0\n        version: 1.0.0\n\n"
    printf "packages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n    cpu: [arm64]\n    os: [darwin]\n    libc: [glibc]\n\n" "$good"
    printf "  pnpm@1.0.0:\n    resolution: {integrity: sha512-%s}\n    engines: {node: '>=18.*'}\n    hasBin: true\n\n" "$other"
    printf "snapshots:\n\n  '@pnpm/exe.p@1.0.0':\n    optional: true\n\n  pnpm@1.0.0:\n    optionalDependencies:\n      '@pnpm/exe.p': 1.0.0\n\n---\nlockfileVersion: '9.0'\n"
  }
  lock_variant() {
    base_lock >"$case_dir/base"
    at=$(grep -n -x -F -- "$2" "$case_dir/base" | sed -n '1s/:.*//p')
    if [ -z "$at" ]; then
      problems="$problems; no line '$2' in the base lock"
      return
    fi
    keep=$at
    [ "$1" = replace ] && keep=$((at - 1))
    {
      awk -v last="$keep" 'NR <= last' "$case_dir/base"
      cat "$case_dir/insert"
      awk -v first="$at" 'NR > first' "$case_dir/base"
    } >"$lock_file"
  }
  key_line="  '@pnpm/exe.p@1.0.0':"
  resolution_line="    resolution: {integrity: sha512-$good}"
  base_lock >"$lock_file"
  lock_reads "$good" 'the base lock, which holds every form'
  printf '    os: ["darwin"]\n' >"$case_dir/insert"
  lock_variant replace '    os: [darwin]'
  lock_reads '' 'a double quote'
  printf "notes: 'a\n  b'\n" >"$case_dir/insert"
  lock_variant after "lockfileVersion: '9.0'"
  lock_reads '' 'a single-quoted text that does not close on its line'
  printf '# a note\n' >"$case_dir/insert"
  lock_variant after "$resolution_line"
  lock_reads '' 'a comment at the start of a line'
  printf '    # a note\n' >"$case_dir/insert"
  lock_variant after "$resolution_line"
  lock_reads '' 'an indented comment'
  printf "  '@pnpm/exe.p@1.0.0': # a note\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'a comment after the entry key'
  printf '  "@pnpm/exe.p@1.0.0":\n' >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'the entry key in double quotes'
  printf '  "\\x40pnpm/exe.p@1.0.0":\n    resolution: {integrity: sha512-%s}\n\n' "$other" >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'a second entry whose key holds an escape'
  printf "  '@pnpm/exe.p@1.0.0' :\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'a blank before the colon of the entry key'
  printf "  '@pnpm/exe.p@1.0.0'\t:\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'a tab before the colon of the entry key'
  printf "  '@pnpm/exe.p@1.0.0':\t\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'a tab after the colon of the entry key'
  printf '  @pnpm/exe.p@1.0.0:\n' >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'the entry key plain where pnpm quotes it'
  printf "  'pnpm@1.0.0':\n" >"$case_dir/insert"
  lock_variant replace '  pnpm@1.0.0:'
  lock_reads '' 'a key quoted where pnpm writes it plain'
  printf "    'resolution': {integrity: sha512-%s}\n" "$other" >"$case_dir/insert"
  lock_variant after "$resolution_line"
  lock_reads '' 'a quoted resolution key'
  printf '    resolution : {integrity: sha512-%s}\n' "$other" >"$case_dir/insert"
  lock_variant after "$resolution_line"
  lock_reads '' 'a blank before the colon of the resolution key'
  printf 'packages: # a note\n' >"$case_dir/insert"
  lock_variant replace 'packages:'
  lock_reads '' 'a comment after packages:'
  printf 'packages: \n' >"$case_dir/insert"
  lock_variant replace 'packages:'
  lock_reads '' 'a blank after packages:'
  printf "'packages':\n" >"$case_dir/insert"
  lock_variant replace 'packages:'
  lock_reads '' 'packages: in quotes'
  printf "  ? '@pnpm/exe.p@1.0.0'\n  : resolution: {integrity: sha512-%s}\n\n" "$other" >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'an explicit key'
  printf "  &k '@pnpm/exe.p@1.0.0':\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'an anchor'
  printf '    hasBin: *k\n' >"$case_dir/insert"
  lock_variant replace '    hasBin: true'
  lock_reads '' 'an alias'
  printf '    hasBin: !!bool true\n' >"$case_dir/insert"
  lock_variant replace '    hasBin: true'
  lock_reads '' 'a tag'
  printf '  <<: *k\n' >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'a merge key'
  printf 'notes: |\n  text\n' >"$case_dir/insert"
  lock_variant after "lockfileVersion: '9.0'"
  lock_reads '' 'a block scalar'
  printf '    cpu: [arm64,\n      x64]\n' >"$case_dir/insert"
  lock_variant replace '    cpu: [arm64]'
  lock_reads '' 'a flow list that does not close on its line'
  printf '    cpu: [{a: b}]\n' >"$case_dir/insert"
  lock_variant replace '    cpu: [arm64]'
  lock_reads '' 'a flow list that holds a mapping'
  printf "  '@pnpm/exe.p@1.0.0': {resolution: {integrity: sha512-%s}}\n\n" "$other" >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'an entry written as one flow mapping'
  printf "notes: \"\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\nend\"\n'packages':\n" "$other" >"$case_dir/insert"
  lock_variant replace 'packages:'
  lock_reads '' 'a scalar over several lines that holds packages:, with the key quoted below it'
  printf "notes: \"\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\nend\"\n\"packages\":\n" "$other" >"$case_dir/insert"
  lock_variant replace 'packages:'
  lock_reads '' 'a scalar over several lines that holds packages:, with the key in double quotes below it'
  printf "notes: \"\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\nend\"\npackages:\n" "$other" >"$case_dir/insert"
  lock_variant replace 'packages:'
  lock_reads '' 'a scalar over several lines that holds packages:, with the plain key below it'
  printf "    notes: \"\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\n    end\"\n" "$other" >"$case_dir/insert"
  lock_variant after '    configDependencies: {}'
  lock_reads '' 'a scalar over several lines under importers:'
  printf "  pnpm@1.0.0:\n    deprecated: \"a\n  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n    b\"\n" "$other" >"$case_dir/insert"
  lock_variant replace '  pnpm@1.0.0:'
  lock_reads '' 'the text of the entry inside a scalar of another entry'
  printf '    cpu:\t[arm64]\n' >"$case_dir/insert"
  lock_variant replace '    cpu: [arm64]'
  lock_reads '' 'a tab'
  printf '    os: [darwin]\r    cpu: [x64]\n' >"$case_dir/insert"
  lock_variant replace '    os: [darwin]'
  lock_reads '' 'a carriage return inside a line'
  printf '\302\205\n' >"$case_dir/insert"
  lock_variant after "lockfileVersion: '9.0'"
  lock_reads '' 'a NEL line'
  printf '\342\200\250\n' >"$case_dir/insert"
  lock_variant after "lockfileVersion: '9.0'"
  lock_reads '' 'a line separator'
  printf '    hasBin:\302\240true\n' >"$case_dir/insert"
  lock_variant replace '    hasBin: true'
  lock_reads '' 'a no-break space'
  printf "  '@pnpm/exe.p@1.0.0':\000\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'a NUL after the entry key'
  printf '\357\273\277---\n' >"$case_dir/insert"
  lock_variant replace '---'
  lock_reads '' 'a byte-order mark'
  printf "   '@pnpm/exe.p@1.0.0':\n" >"$case_dir/insert"
  lock_variant replace "$key_line"
  lock_reads '' 'the entry key at three spaces'
  printf "  pnpm@2.0.0:\n      resolution: {integrity: sha512-%s}\n\n" "$other" >"$case_dir/insert"
  lock_variant after '    hasBin: true'
  lock_reads '' 'an entry whose lines stand at another indentation'
  printf -- '---\t\n' >"$case_dir/insert"
  lock_variant replace '---'
  lock_reads '' 'a first marker with a tab after it'
  printf -- '--- \n' >"$case_dir/insert"
  lock_variant replace '---'
  lock_reads '' 'a first marker with a blank after it'
  printf -- '--- |\n' >"$case_dir/insert"
  lock_variant replace '---'
  lock_reads '' 'a first document that is one scalar'
  printf -- '%%YAML 1.2\n---\n' >"$case_dir/insert"
  lock_variant replace '---'
  lock_reads '' 'a directive before the first marker'
  printf '...\n' >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'an end marker inside the first document'
  printf 'packages:\n\n' >"$case_dir/insert"
  lock_variant replace 'snapshots:'
  lock_reads '' 'a second packages: without the entry'
  printf 'packages: {}\n' >"$case_dir/insert"
  lock_variant after '    hasBin: true'
  lock_reads '' 'a second packages key written as an empty flow mapping'
  printf "  '@pnpm/exe.p@1.0.0':\n    resolution: {integrity: sha512-%s}\n\n" "$other" >"$case_dir/insert"
  lock_variant after '    hasBin: true'
  lock_reads '' 'the entry twice'
  printf '  bad: [\n' >"$case_dir/insert"
  lock_variant after '    optional: true'
  lock_reads '' 'a line that no YAML reader reads'
  printf "      '@pnpm/exe.p': 1.0.0\n" >"$case_dir/insert"
  lock_variant after "      '@pnpm/exe.p': 1.0.0"
  lock_reads '' 'a name twice under optionalDependencies:'
  printf '        specifier: 1.0.0\n' >"$case_dir/insert"
  lock_variant after '        specifier: 1.0.0'
  lock_reads '' 'specifier twice under the pnpm manager'
  printf '        version: 1.0.0\n' >"$case_dir/insert"
  lock_variant after '        version: 1.0.0'
  lock_reads '' 'version twice under the pnpm manager'
  long_name=$(printf '%01100d' 0 | tr 0 a)
  printf "  '@pnpm/exe.%s@1.0.0':\n    resolution: {integrity: sha512-%s}\n\n" "$long_name" "$other" >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'another entry whose key line has more than 1,024 characters'
  printf "      '@pnpm/exe.%s': 1.0.0\n" "$long_name" >"$case_dir/insert"
  lock_variant after "      '@pnpm/exe.p': 1.0.0"
  lock_reads '' 'a name of more than 1,024 characters under optionalDependencies:'
  printf -- "---\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n" >"$lock_file"
  lock_reads '' 'the entry as the last line of the lock'
  printf -- "---\npackages:\n\n  '@pnpm/exe.p@1.0.0':\n---\n" >"$lock_file"
  lock_reads '' 'the entry as the last line of the first document'
  printf "\r  '@pnpm/exe.q@1.0.0':\n    resolution: {integrity: sha512-%s}\n\n" "$other" >"$case_dir/insert"
  lock_variant after '    libc: [glibc]'
  lock_reads '' 'a lone carriage return after the entry'
  printf -- "---\t\npackages:\n\n  '@pnpm/exe.q@1.0.0':\n    resolution: {integrity: sha512-%s}\n\n" "$other" >"$case_dir/insert"
  lock_variant after '      '"'"'@pnpm/exe.p'"'"': 1.0.0'
  lock_reads '' 'a marker with a tab after it inside the first document'
  printf '    cpu: [arm64] # resolution: {integrity: sha512-%s}\n    resolution: {integrity: sha512-%s}\n' "$good" "$other" >"$case_dir/insert"
  lock_variant replace "$resolution_line"
  lock_reads '' 'a resolution in a comment of the line under the entry'
  printf '    resolution: {integrity: sha512-%s==}\n' "$(printf '%s' "$good" | cut -c1-85)" >"$case_dir/insert"
  lock_variant replace "$resolution_line"
  lock_reads '' 'a value of 85 characters'
  for spaces in '   ' '     ' '      '; do
    printf '%sresolution: {integrity: sha512-%s}\n' "$spaces" "$good" >"$case_dir/insert"
    lock_variant replace "$resolution_line"
    lock_reads '' "a resolution line at ${#spaces} spaces"
  done
  verdict "$name"

  name='a failed fetch of the pinned tools fails the step, and one line says what to do'
  new_backend_case backend-tools-fail
  cp "$case_dir/pnpm" "$case_dir/stubs/pnpm"
  printf 'exit 1\n' >>"$case_dir/repo/backend/scripts/fetch-tools.sh"
  run_ensure_backend fails
  [ "$(count 'backend/scripts/fetch-tools.sh could not fetch a pinned tool' "$case_dir/output")" = 1 ] || problems="$problems; no line said what to do after the failed fetch"
  verdict "$name"

  name='a pnpm of the pinned version on the path is used, npm installs nothing, and no line asks for the PATH'
  new_backend_case backend-pnpm-present
  cp "$case_dir/pnpm" "$case_dir/stubs/pnpm"
  run_ensure_backend ok
  [ "$(count 'npm install --global' "$case_dir/calls")" = 0 ] || problems="$problems; npm installed pnpm"
  [ "$(count 'which is not on your PATH' "$case_dir/output")" = 0 ] || problems="$problems; a line asked for the PATH although pnpm is on it"
  [ "$(count "pnpm install --frozen-lockfile --ignore-scripts in $backend_dir" "$case_dir/calls")" = 1 ] || problems="$problems; pnpm install did not run once in backend/"
  [ "$(count "fetch-tools $repo_dir" "$case_dir/calls")" = 1 ] || problems="$problems; backend/scripts/fetch-tools.sh did not run once"
  verdict "$name"

  name='a backend/package.json that pins no pnpm stops the step before anything is installed'
  new_backend_case backend-no-pin
  printf '{\n  "name": "halvo-backend"\n}\n' >"$case_dir/repo/backend/package.json"
  run_ensure_backend fails
  [ ! -s "$case_dir/calls" ] || problems="$problems; a command ran: $(tr '\n' ';' <"$case_dir/calls")"
  [ "$(count 'backend/package.json pins no pnpm version with its sha512' "$case_dir/output")" = 1 ] || problems="$problems; no line names the missing pin"
  verdict "$name"

  name='a backend/package.json that pins a pnpm version without its sha512 stops the step before anything is installed'
  new_backend_case backend-pin-without-hash
  printf '{\n  "packageManager": "pnpm@12.14.0"\n}\n' >"$case_dir/repo/backend/package.json"
  run_ensure_backend fails
  [ ! -s "$case_dir/calls" ] || problems="$problems; a command ran: $(tr '\n' ';' <"$case_dir/calls")"
  [ "$(count 'backend/package.json pins no pnpm version with its sha512' "$case_dir/output")" = 1 ] || problems="$problems; no line names the missing pin"
  verdict "$name"

  name='a failed package install fails the step, and no tool is fetched'
  new_backend_case backend-install-fails
  cp "$case_dir/pnpm" "$case_dir/stubs/pnpm"
  echo 1 >"$case_dir/pnpm.exit"
  run_ensure_backend fails
  [ "$(count 'fetch-tools ' "$case_dir/calls")" = 0 ] || problems="$problems; the tools were fetched after the failed install"
  [ "$(count 'pnpm could not install the packages of backend/pnpm-lock.yaml' "$case_dir/output")" = 1 ] || problems="$problems; no line said what to do after the failed install"
  verdict "$name"

  name='the licences of the packages that this platform does not install are fetched at the repository root after the tools, on each run'
  new_backend_case backend-licences
  cp "$case_dir/pnpm" "$case_dir/stubs/pnpm"
  run_ensure_backend ok
  run_ensure_backend ok
  fetched=$(count "node backend/scripts/fetch-licenses.mjs in $repo_dir" "$case_dir/calls")
  [ "$fetched" = 2 ] || problems="$problems; backend/scripts/fetch-licenses.mjs did not run at the repository root on each run"
  second=$(sed -n 2p "$case_dir/calls")
  third=$(sed -n 3p "$case_dir/calls")
  [ "$second;$third" = "fetch-tools $repo_dir;node backend/scripts/fetch-licenses.mjs in $repo_dir" ] || problems="$problems; the licences were not fetched right after the tools"
  verdict "$name"

  name='a failed fetch of the licences fails the step, and one line says what to do'
  new_backend_case backend-licences-fail
  cp "$case_dir/pnpm" "$case_dir/stubs/pnpm"
  echo 1 >"$case_dir/node.exit"
  run_ensure_backend fails
  told=$(count 'backend/scripts/fetch-licenses.mjs could not fetch a licence from the package registry; its lines above name the package, check the network or its proxy, then run sh scripts/prerequisites.sh again' "$case_dir/output")
  [ "$told" = 1 ] || problems="$problems; no line said what to do after the failed fetch of the licences"
  verdict "$name"

  name='a failed fetch of the pinned tools fetches no licence'
  new_backend_case backend-tools-fail-no-licences
  cp "$case_dir/pnpm" "$case_dir/stubs/pnpm"
  printf 'exit 1\n' >>"$case_dir/repo/backend/scripts/fetch-tools.sh"
  run_ensure_backend fails
  fetched=$(count 'node ' "$case_dir/calls")
  [ "$fetched" = 0 ] || problems="$problems; the licences were fetched after the failed fetch of the tools"
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
ensure_backend "$HOME/.local"

echo "prerequisites: done"
