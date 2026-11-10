#!/bin/sh
set -eu

if [ "$#" -lt 1 ]; then
    echo "usage: sh run-hosted.sh OUT [arguments of 'r2-probes suite']..." >&2
    echo "Fetches the runtime, the models and LibreOffice, builds the probes from their lock and runs every cell into the new directory OUT." >&2
    exit 64
fi
out=$1
shift
if [ -e "$out" ]; then
    echo "run-hosted: $out exists. Fix: name a new directory." >&2
    exit 2
fi
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
files=${R2_FILES:-$out-files}
runtime=1.28.0
office=26.8.2

case "$(uname -s)" in
Darwin) system=macos ;;
*) system=windows ;;
esac

native() {
    if [ "$system" = windows ]; then
        cygpath -m "$1"
    else
        printf '%s\n' "$1"
    fi
}

sh "$here/fetch-files.sh" "$here/files.tsv" "ocr-$system" "$files"
sh "$here/fetch-files.sh" "$here/files.tsv" ocr-models "$files"
sh "$here/fetch-files.sh" "$here/files.tsv" "libreoffice-$system" "$files"

if [ "$system" = macos ]; then
    exe=
    library=$files/onnxruntime-osx-arm64-$runtime/lib/libonnxruntime.dylib
    [ -f "$library" ] || tar -xzf "$files/onnxruntime-osx-arm64-$runtime.tgz" -C "$files"
    soffice=$files/LibreOffice.app/Contents/MacOS/soffice
    if [ ! -x "$soffice" ]; then
        hdiutil attach -nobrowse -readonly -mountpoint "$files/office-volume" "$files/LibreOffice_${office}_MacOS_aarch64.dmg" >/dev/null
        cp -R "$files/office-volume/LibreOffice.app" "$files/LibreOffice.app"
        hdiutil detach "$files/office-volume" >/dev/null
    fi
else
    exe=.exe
    library=$files/onnxruntime-win-x64-$runtime/lib/onnxruntime.dll
    [ -f "$library" ] || unzip -q -o "$files/onnxruntime-win-x64-$runtime.zip" -d "$files"
    soffice="C:/Program Files/LibreOffice/program/soffice.exe"
    if [ ! -f "$soffice" ]; then
        package=$(cygpath -w "$files/LibreOffice_${office}_Win_x86-64.msi")
        powershell -NoProfile -Command "\$p = Start-Process msiexec -Wait -PassThru -ArgumentList '/i','$package','/qn','/norestart','ALLUSERS=1','CREATEDESKTOPLINK=0','REGISTER_NO_MSO_TYPES=1','ISCHECKFORPRODUCTUPDATES=0','QUICKSTART=0','UI_LANGS=en_US','RebootYesNo=No'; exit \$p.ExitCode"
    fi
    (cd "$here/input" && npm ci --no-audit --no-fund && npx playwright install chromium)
fi

(cd "$here" && cargo build --release --locked --features pp-ocrv6)

set -- suite --out "$(native "$out")" "$@" \
    --soffice "$(native "$soffice")" --soffice-version "$office" \
    --ocr-bin "$(native "$here/target/release/r2-ocr$exe")" --ocr-version "onnxruntime-$runtime" \
    --ocr-arg --dylib --ocr-arg "$(native "$library")" \
    --ocr-arg --det --ocr-arg "$(native "$files/pp-ocrv6_tiny_det.onnx")" \
    --ocr-arg --rec --ocr-arg "$(native "$files/pp-ocrv6_tiny_rec.onnx")" \
    --ocr-arg --dict --ocr-arg "$(native "$files/ppocrv6_tiny_dict.txt")"
if [ "$system" = windows ]; then
    set -- "$@" --node "$(native "$(command -v node)")" --input-dir "$(native "$here/input")"
fi
"$here/target/release/r2-probes$exe" "$@"
