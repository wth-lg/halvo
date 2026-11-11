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
                         L9.3, for the case files of the directory DIR;
                         DIR holds files <name>.jsonl alone, and another
                         entry refuses the call with case
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

if ! true 2>/dev/null 3>&1; then
  refuse output "standard output cannot be written"
fi

calls="the trial takes no argument, --help, --case FILE --arm none|plain, or --count DIR"
help=no
case $# in
0) ;;
1)
  if [ "$1" != --help ]; then
    refuse usage "$calls"
  fi
  help=yes
  ;;
2)
  if [ "$1" != --count ] || [ -z "$2" ]; then
    refuse usage "$calls"
  fi
  set -- --count "$(absolute "$2")"
  ;;
4)
  if [ "$1" != --case ] || [ -z "$2" ] || [ "$3" != --arm ]; then
    refuse usage "$calls"
  fi
  case $4 in
  none | plain) ;;
  *) refuse usage "the arm of --case is none or plain" ;;
  esac
  set -- --case "$(absolute "$2")" --arm "$4"
  ;;
*) refuse usage "$calls" ;;
esac

if [ "$help" = yes ]; then
  usage
  exit 0
fi

unset CDPATH
export PATH="${CARGO_HOME:-${HOME:-}/.cargo}/bin:$PATH"

here=$(dirname "$0")
if ! cd -P "$here/../../.." 2>/dev/null; then
  refuse input "the root of the repository cannot be entered"
fi

host=$(rustc -vV 2>/dev/null | sed -n 's/^host: \([^[:space:]]*\).*/\1/p')
if [ -z "$host" ]; then
  refuse build "rustc -vV names no host"
fi

target=target/r5-trials
if ! mkdir -p "$target" 2>/dev/null; then
  refuse build "the target directory cannot be made"
fi
cargo build --release --locked --manifest-path tools/r5-trials/Cargo.toml -p r5-drift --target "$host" --target-dir "$target" >"$target/build.log" 2>&1
code=$?
if [ "$code" -ne 0 ]; then
  refuse build "cargo exited $code; its output is in the build log"
fi

bin=$target/$host/release/r5-drift
if [ -f "$bin.exe" ]; then
  bin=$bin.exe
fi
exec "$bin" ${1+"$@"}
