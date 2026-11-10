#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: sh retime.sh FRAMES FILES OUT" >&2
    echo "Times the PP-OCRv6 probe again on the saved frames of a hosted macos-15 run: FRAMES holds text-N.runM.png, FILES holds the runtime and the models that fetch-files.sh fetched, OUT is a new directory for raw.jsonl and one time file per run." >&2
    exit 64
fi
frames=$1
files=$2
out=$3
if [ -e "$out" ]; then
    echo "retime: $out exists. Fix: name a new directory." >&2
    exit 2
fi
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
runtime=1.28.0
library=$files/onnxruntime-osx-arm64-$runtime/lib/libonnxruntime.dylib
[ -f "$library" ] || tar -xzf "$files/onnxruntime-osx-arm64-$runtime.tgz" -C "$files"
mkdir -p "$out"
for page in 1 2 3; do
    for run in 1 2 3 4 5; do
        /usr/bin/time -l "$here/target/release/r2-ocr" \
            --frame "$frames/text-$page.run$run.png" --scale 1.0 --target "text-$page" \
            --dylib "$library" --det "$files/pp-ocrv6_tiny_det.onnx" \
            --rec "$files/pp-ocrv6_tiny_rec.onnx" --dict "$files/ppocrv6_tiny_dict.txt" \
            >>"$out/raw.jsonl" 2>"$out/time-$page-$run.txt"
    done
done
echo "runs=$(wc -l <"$out/raw.jsonl" | tr -d ' ')"
