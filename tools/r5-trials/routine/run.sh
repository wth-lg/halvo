#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r5-trials/routine/run.sh [--help | --sample FILE]

The routine constants trial of the learning run (contracts.md L7.2). It builds
the crate r5-routine for the host's target into target/r5-trials under the
repository root, with cargo's output in target/r5-trials/build.log. It reads
the 27 cells and their seeds from tools/r5-trials/routine/seeds.txt and the
keys false_stale_max, detected_min and detect_days of [routine] from
docs/r5/targets.toml, whose every line must have the form of L8.1.

Arguments:
  (none)          print the block: 216 cell lines, 8 setting lines and the
                  verdict line
  --sample FILE   write the sample to FILE and print nothing: 135 lines, five
                  trains of each cell, for the verdict's setting, or for the
                  setting 0-0-0 after a verdict of rule=none; a relative FILE
                  is taken from the current directory
  --help          print this text

Every form of call needs an open standard output, also --sample, which
prints nothing to it.

Exit status:
  0  the block was printed, whatever the verdict says, the sample written,
     or this text printed
  2  refused: one line "REFUSED <cause>: <detail>" on standard error and
     nothing on standard output; <cause> is usage, build, seed, input or
     output (standard output is closed or cannot be written)
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

true 2>/dev/null 3>&1 || refuse output "standard output cannot be written"

wrong="the trial takes no argument, --help or --sample FILE"
sample=
case $# in
0) ;;
1)
  [ "$1" = --help ] || refuse usage "$wrong"
  usage
  exit 0
  ;;
2)
  [ "$1" = --sample ] && [ -n "$2" ] || refuse usage "$wrong"
  case $2 in
  /* | [A-Za-z]:[/\\]*) sample=$2 ;;
  *) sample=$(pwd)/$2 ;;
  esac
  folder=$(dirname -- "$sample")
  if ! { [ -d "$folder" ] && [ -w "$folder" ] && { [ ! -e "$sample" ] || { [ -f "$sample" ] && [ -w "$sample" ]; }; }; }; then
    refuse usage "the sample file cannot be written"
  fi
  ;;
*) refuse usage "$wrong" ;;
esac

unset CDPATH
export PATH="${CARGO_HOME:-${HOME:-}/.cargo}/bin:$PATH"

here=$(dirname -- "$0")
root=$(cd -P -- "$here/../../.." 2>/dev/null && pwd -P) ||
  refuse input "the root of the repository cannot be entered"
cd "$root" 2>/dev/null || refuse input "the root of the repository cannot be entered"

host=$(rustc -vV 2>/dev/null | LC_ALL=C tr -d '\r' | sed -n 's/^host: //p')
[ -n "$host" ] || refuse build "rustc -vV names no host"

target=target/r5-trials
mkdir -p "$target" 2>/dev/null || refuse build "the target directory cannot be made"
cargo build --release --locked --manifest-path tools/r5-trials/Cargo.toml -p r5-routine --target "$host" --target-dir "$target" >"$target/build.log" 2>&1
code=$?
[ "$code" -eq 0 ] || refuse build "cargo exited $code; its output is in the build log"

bin=$target/$host/release/r5-routine
[ -f "$bin.exe" ] && bin=$bin.exe
if [ -n "$sample" ]; then
  exec "$bin" --sample "$sample"
fi
exec "$bin"
