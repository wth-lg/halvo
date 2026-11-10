#!/bin/sh
set -eu

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo "usage: sh retime.sh ENGINE FRAMES OUT [FILES]" >&2
    echo "Times an engine again on the saved frames of a hosted macos-15 run, one process per frame, each under /usr/bin/time -l. ENGINE is apple-vision-accurate, apple-vision-fast or pp-ocrv6-tiny. FRAMES holds text-N.runM.png. OUT is a new directory for raw.jsonl and one time file per frame. FILES holds the runtime and the models that fetch-files.sh fetched; pp-ocrv6-tiny needs it." >&2
    exit 64
fi
engine=$1
frames=$2
out=$3
files=${4:-}
if [ -e "$out" ]; then
    echo "retime: $out exists. Fix: name a new directory." >&2
    exit 2
fi
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
runtime=1.28.0
case "$engine" in
pp-ocrv6-tiny)
    if [ -z "$files" ]; then
        echo "retime: pp-ocrv6-tiny needs FILES" >&2
        exit 64
    fi
    library=$files/onnxruntime-osx-arm64-$runtime/lib/libonnxruntime.dylib
    [ -f "$library" ] || tar -xzf "$files/onnxruntime-osx-arm64-$runtime.tgz" -C "$files"
    ;;
apple-vision-accurate | apple-vision-fast) ;;
*)
    echo "retime: no engine $engine" >&2
    exit 64
    ;;
esac
mkdir -p "$out"
count=0
for frame in "$frames"/text-*.run*.png; do
    [ -f "$frame" ] || continue
    name=$(basename "$frame" .png)
    page=${name%%.run*}
    count=$((count + 1))
    if [ "$engine" = pp-ocrv6-tiny ]; then
        /usr/bin/time -l "$here/target/release/r2-ocr" \
            --frame "$frame" --scale 1.0 --target "$page" \
            --dylib "$library" --det "$files/pp-ocrv6_tiny_det.onnx" \
            --rec "$files/pp-ocrv6_tiny_rec.onnx" --dict "$files/ppocrv6_tiny_dict.txt" \
            >>"$out/raw.jsonl" 2>"$out/time-$name.txt"
    else
        /usr/bin/time -l "$here/target/release/r2-probes" child extract \
            --engine "$engine" --scale 1.0 --target "$page" --frame-in "$frame" \
            >>"$out/raw.jsonl" 2>"$out/time-$name.txt"
    fi
done
echo "frames=$count"
