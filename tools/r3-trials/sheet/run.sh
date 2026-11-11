#!/bin/sh
set -u

usage() {
	cat <<'EOF'
Usage: sh tools/r3-trials/sheet/run.sh [--candidate umya|rewrite]

Runs the sheet trial of contracts A13.4 on this machine. For each candidate
(both when --candidate is not given) it copies each of the eight workbooks
of tools/r3-trials/sheet/corpus/ to target/r3-trials/sheet/out/<candidate>/
under the repository root, changes the one cell that docs/r3/trials/sheet.md
names for the workbook, saves, reads the original and the saved file with
calamine and as zip archives, and prints one line per workbook:

  sheet candidate=<c> file=<name> other_cells_same=<yes|no> lost=<kinds or -> cached_results=<fresh|stale|absent|none>

It prints no verdict line.

Exit status: 0 when every cell ran; 1 when a cell could not run, with one
"miss" line on standard error for each; 2 for a refusal, with one
"REFUSED usage: <detail>" line on standard error and no file written.
EOF
}

refuse() {
	echo "REFUSED usage: $1" >&2
	exit 2
}

candidate=""
while [ $# -gt 0 ]; do
	case "$1" in
	--help | -h)
		usage
		exit 0
		;;
	--candidate)
		[ $# -ge 2 ] || refuse "--candidate needs a value"
		case "$2" in
		umya | rewrite) candidate="$2" ;;
		*) refuse "--candidate is umya or rewrite" ;;
		esac
		shift 2
		;;
	*) refuse "unknown argument; see --help" ;;
	esac
done

here=$(cd "$(dirname "$0")" && pwd -P) || exit 1
root=$(cd "$here/../../.." && pwd -P) || exit 1

PATH="$HOME/.cargo/bin:$PATH"
export PATH

host=$(cargo -vV 2>/dev/null | sed -n 's/^host: //p')
bin="$here/target/$host/release/r3-sheet-trial"
if [ -z "$host" ] || ! cargo build --locked --release --quiet --manifest-path "$here/Cargo.toml" \
	--target-dir "$here/target" --target "$host" >&2 || [ ! -x "$bin" ]; then
	echo "miss cause=build" >&2
	exit 1
fi

set -- run --corpus "$here/corpus" --out "$root/target/r3-trials/sheet/out"
if [ -n "$candidate" ]; then
	set -- "$@" --candidate "$candidate"
fi
exec "$bin" "$@"
