#!/bin/sh
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
TOOLS="$ROOT/.tools"
PINS="$ROOT/scripts/tools.sha256"
STRIPE_MOCK_VERSION=0.206.0
TLA_VERSION=1.7.4

say() { printf 'fetch-tools: %s\n' "$1"; }
die() {
  printf 'fetch-tools: error: %s\n' "$1" >&2
  exit 1
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    sha256sum "$1" | cut -d' ' -f1
  fi
}

fetch() {
  url=$1
  name=$2
  want=$(awk -v name="$name" '$2 == name { print $1 }' "$PINS")
  [ -n "$want" ] || die "scripts/tools.sha256 lists no sha256 for $name"
  file="$TOOLS/downloads/$name"
  if [ -f "$file" ]; then
    got=$(sha256_of "$file")
    [ "$got" = "$want" ] || die "$file has sha256 $got, not the pinned $want. Move it away and run again."
    say "$name present, sha256 matches"
    return 0
  fi
  say "downloading $url"
  curl -fsSL --retry 3 --retry-delay 2 -o "$file.part" "$url" || die "the download of $url failed"
  got=$(sha256_of "$file.part")
  [ "$got" = "$want" ] || die "$url has sha256 $got, not the pinned $want. The download is kept as $file.part."
  mv "$file.part" "$file"
  say "$name downloaded, sha256 matches"
}

mkdir -p "$TOOLS/downloads" "$TOOLS/stripe-mock"

case "$(uname -s)" in
  Darwin) os=darwin ;;
  Linux) os=linux ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  *) die "no stripe-mock build is pinned for $(uname -s)" ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch=arm64 ;;
  x86_64 | amd64) arch=amd64 ;;
  *) die "no stripe-mock build is pinned for $(uname -m)" ;;
esac
extension=tar.gz
binary=stripe-mock
if [ "$os" = windows ]; then
  extension=zip
  binary=stripe-mock.exe
fi
archive="stripe-mock_${STRIPE_MOCK_VERSION}_${os}_${arch}.$extension"
fetch "https://github.com/stripe/stripe-mock/releases/download/v$STRIPE_MOCK_VERSION/$archive" "$archive"
packed_binary() {
  if [ "$extension" = zip ]; then
    unzip -p "$TOOLS/downloads/$archive" "$binary"
  else
    tar -xzOf "$TOOLS/downloads/$archive" "$binary"
  fi
}
packed=$(packed_binary | if command -v shasum > /dev/null 2>&1; then shasum -a 256; else sha256sum; fi | cut -d' ' -f1)
if [ -x "$TOOLS/stripe-mock/$binary" ] && [ "$(sha256_of "$TOOLS/stripe-mock/$binary")" = "$packed" ]; then
  say "stripe-mock $STRIPE_MOCK_VERSION present: .tools/stripe-mock/$binary matches the archive"
else
  if [ "$extension" = zip ]; then
    unzip -o -q "$TOOLS/downloads/$archive" -d "$TOOLS/stripe-mock"
  else
    tar -xzf "$TOOLS/downloads/$archive" -C "$TOOLS/stripe-mock"
  fi
  [ "$(sha256_of "$TOOLS/stripe-mock/$binary")" = "$packed" ] || die "the unpacked $binary differs from the one in $archive"
  say "stripe-mock $STRIPE_MOCK_VERSION unpacked: .tools/stripe-mock/$binary"
fi

jar="tla2tools-$TLA_VERSION.jar"
fetch "https://github.com/tlaplus/tlaplus/releases/download/v$TLA_VERSION/tla2tools.jar" "$jar"
if cmp -s "$TOOLS/downloads/$jar" "$TOOLS/tla2tools.jar"; then
  say "tla2tools.jar $TLA_VERSION present: .tools/tla2tools.jar"
else
  cp "$TOOLS/downloads/$jar" "$TOOLS/tla2tools.jar"
  say "tla2tools.jar $TLA_VERSION copied: .tools/tla2tools.jar"
fi
