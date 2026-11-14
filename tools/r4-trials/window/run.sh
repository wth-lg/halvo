#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r4-trials/window/run.sh --arm main|a|a2|b --runs N
       sh tools/r4-trials/window/run.sh --sync get|channel|events --arm a2|b

The cells of the window trial and of the state-sync trial (contracts I11.3).
It refuses with usage for other arguments, and with not_hosted where neither
CI nor R4_GUEST is set, before it builds or starts anything. Then it builds
the trial app (cargo build --release --locked) and runs cells.mjs.

--arm A --runs N: N runs of the window arm A. Each run starts the app, shows
its window, waits 5 s, hides it (arms main, a, a2) or destroys it (arm b),
samples, waits 330 s (120 s after a destroy), samples again, shows the window
once more and quits. It prints one cell line per run and the figure line of
the arm over its counted runs.

--sync W --arm A: one cell of the sync way W in arm A: 200,000 changes of 256
bytes at 1,000 a second, a hide or destroy at the page's change 100,000 and a
show 60 s later.

Each answer of the app is compared with what the readers of the system show.
A difference, or a line without an answer inside its limit, prints a line
"miss trial=..." and that run gives no cell; a run that cannot be made for
another cause, such as an err answer, prints a line "nocell trial=...". One
err answer makes a cell: "err timeout" of the second show of a sync cell,
when the window reads on screen.

Exit: 0 when every cell was printed, 1 when one was not or the build failed,
2 on a refusal.
EOF
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac

here=$(cd "$(dirname "$0")" && pwd) || exit 1
node "$here/cells.mjs" --check "$@" || exit $?
trial=window
for word in "$@"; do
  [ "$word" = --sync ] && trial=sync
done
cargo build --release --locked --manifest-path "$here/../app/Cargo.toml" >&2
code=$?
if [ "$code" -ne 0 ]; then
  printf 'nocell trial=%s step=build cause=build exit=%s\n' "$trial" "$code"
  exit 1
fi
node "$here/cells.mjs" "$@"
