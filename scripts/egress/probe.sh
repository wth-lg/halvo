#!/bin/sh
# sh scripts/egress/probe.sh NAME: one probe of the egress check (contracts.md C26.6).
set -u
LC_ALL=C
export LC_ALL

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

case $# in
  1) ;;
  *) refuse usage "probe.sh takes one NAME" ;;
esac
case $1 in
  ipv4 | ipv6 | dns | loopback | udp | loopback2 | none | listen) name=$1 ;;
  *) refuse usage "$1" ;;
esac

dir=$(dirname "$0")
root=$(cd "$dir/../.." && pwd) || refuse output "$dir/../.."
exe=
case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*) exe=.exe ;;
esac
source=$root/scripts/egress/probe.rs
bin=$root/target/egress/bin
spawn=$bin/egress-spawn$exe
probe=$bin/egress-probe$exe

if [ ! -f "$spawn" ] || [ ! -f "$probe" ] || [ "$source" -nt "$spawn" ] || [ "$spawn" -nt "$probe" ]; then
  mkdir -p "$bin" || refuse output target/egress/bin
  part=$bin/build-$$
  rustc --edition 2021 -O -o "$part$exe" "$source" >"$part.log" 2>&1 || refuse output "target/egress/bin/egress-spawn$exe"
  cp "$part$exe" "$part-copy$exe" || refuse output "target/egress/bin/egress-probe$exe"
  mv -f "$part-copy$exe" "$probe" || refuse output "target/egress/bin/egress-probe$exe"
  mv -f "$part$exe" "$spawn" || refuse output "target/egress/bin/egress-spawn$exe"
fi

"$spawn" spawn "$name"
