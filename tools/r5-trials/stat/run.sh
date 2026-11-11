#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r5-trials/stat/run.sh [--help]

The gate statistics trial of the learning run (contracts.md L7.3). It builds
the crate r5-stat into target/r5-trials under the repository root, with cargo's
output in target/r5-trials/build.log, and prints its block: 20 critical lines,
the stub line, the targets line and the verdict line.

Arguments:
  (none)   print the block
  --help   print this text

Exit status:
  0  the block was printed and its verdict is stat=ok
  1  the block was printed and its verdict is stat=miss
  2  refused: one line "REFUSED <cause>: <detail>" on standard error and
     nothing on standard output; <cause> is usage, output (standard output
     is closed or cannot be written), build or input
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

true 2>/dev/null 3>&1 || refuse output "standard output cannot be written"

case $# in
0) ;;
1)
  [ "$1" = --help ] || refuse usage "the trial takes no argument but --help"
  usage
  exit 0
  ;;
*) refuse usage "the trial takes no argument but --help" ;;
esac

unset CDPATH
export PATH="${CARGO_HOME:-${HOME:-}/.cargo}/bin:$PATH"

here=$(dirname "$0")
cd -P "$here/../../.." 2>/dev/null || refuse input "the root of the work tree cannot be entered"

host=$(rustc -vV 2>/dev/null | sed -n 's/^host: \([^[:space:]]*\).*/\1/p')
[ -n "$host" ] || refuse build "rustc -vV names no host"

target=target/r5-trials
mkdir -p "$target" 2>/dev/null || refuse build "the target directory cannot be made"
cargo build --release --locked --manifest-path tools/r5-trials/Cargo.toml -p r5-stat --target "$host" --target-dir "$target" >"$target/build.log" 2>&1
code=$?
[ "$code" -eq 0 ] || refuse build "cargo exited $code; its output is in the build log"

bin=$target/$host/release/r5-stat
[ -f "$bin.exe" ] && bin=$bin.exe
exec "$bin"
