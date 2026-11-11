#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r5-trials/drift/run.sh [--help]
       sh tools/r5-trials/drift/run.sh --case FILE --arm none|plain
       sh tools/r5-trials/drift/run.sh --count DIR

The drift harness and the admission trial of the learning run (contracts.md
L7.4 and L9). It builds the crate r5-drift for the host's target into
target/r5-trials under the repository root, with cargo's output in
target/r5-trials/build.log, and runs it.

Arguments:
  (none)                 print the trial's block: for each case file of
                         fixtures/r5/cases/dev, in byte order of the file
                         names, the lines of the arms none, plain, admit with
                         sort=yes and admit with sort=no; then the verdict line
  --case FILE --arm ARM  print the one line of the case file FILE, at any
                         path, for the arm ARM, which is none or plain
  --count DIR            print one spread line per form, in the order of
                         L9.3, for the case files of the directory DIR
  --help                 print this text

Exit status:
  0  the block or the lines were printed, whatever the verdict says
  2  refused: one line "REFUSED <cause>: <detail>" on standard error and
     nothing on standard output; <cause> is usage, output, build, case or
     input
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

absolute() {
  case $1 in
  /* | [A-Za-z]:[\\/]*) printf '%s' "$1" ;;
  *) printf '%s/%s' "$(pwd)" "$1" ;;
  esac
}

calls="the trial takes no argument, --help, --case FILE --arm none|plain, or --count DIR"
help=no
case $# in
0) ;;
1)
  [ "$1" = --help ] || refuse usage "$calls"
  help=yes
  ;;
2)
  [ "$1" = --count ] && [ -n "$2" ] || refuse usage "$calls"
  set -- --count "$(absolute "$2")"
  ;;
4)
  [ "$1" = --case ] && [ -n "$2" ] && [ "$3" = --arm ] || refuse usage "$calls"
  case $4 in
  none | plain) ;;
  *) refuse usage "the arm of --case is none or plain" ;;
  esac
  set -- --case "$(absolute "$2")" --arm "$4"
  ;;
*) refuse usage "$calls" ;;
esac

{ true 3>&1; } 2>/dev/null || refuse output "standard output cannot be written"

if [ "$help" = yes ]; then
  usage
  exit 0
fi

unset CDPATH
here=$(dirname "$0")
root=$(cd -P "$here/../../.." 2>/dev/null && pwd -P) ||
  refuse input "the root of the repository cannot be entered"
cd -P "$root" 2>/dev/null || refuse input "the root of the repository cannot be entered"

export PATH="${CARGO_HOME:-${HOME:-}/.cargo}/bin:$PATH"

host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
[ -n "$host" ] || refuse build "rustc -vV names no host"

target=target/r5-trials
mkdir -p "$target" 2>/dev/null || refuse build "the target directory cannot be made"
cargo build --release --locked --manifest-path tools/r5-trials/Cargo.toml -p r5-drift --target "$host" --target-dir "$target" >"$target/build.log" 2>&1
code=$?
[ "$code" -eq 0 ] || refuse build "cargo exited $code; its output is in the build log"

bin=$target/$host/release/r5-drift
[ -f "$bin.exe" ] && bin=$bin.exe
exec "$bin" ${1+"$@"}
