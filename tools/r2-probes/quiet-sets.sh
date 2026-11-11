#!/bin/sh
set -eu
if [ "$#" -ne 4 ]; then
    echo "usage: sh quiet-sets.sh WORKTREE ENGINE FRAMES OUT" >&2
    echo "Runs retime.sh three times in a row on the same frames, into OUT/set-1, OUT/set-2 and OUT/set-3. It is the one command of the helper call that the coordinator ruled on 2026-11-10 at 20:43Z (--max-load 4 --wait-seconds 5400)." >&2
    exit 64
fi
worktree=$1
engine=$2
frames=$3
out=$4
if [ -e "$out" ]; then
    echo "quiet-sets: $out exists. Fix: name a new directory." >&2
    exit 2
fi
mkdir -p "$out"
for set in 1 2 3; do
    sh "$worktree/tools/r2-probes/retime.sh" "$engine" "$frames" "$out/set-$set"
done
