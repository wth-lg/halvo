#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: sh fetch-files.sh LIST SET DIRECTORY" >&2
    echo "Fetches each file of SET that LIST names into DIRECTORY, once, and checks its size and SHA-256." >&2
    exit 64
fi
list=$1
wanted=$2
directory=$3
mkdir -p "$directory"

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum <"$1" | cut -d ' ' -f 1
    else
        shasum -a 256 <"$1" | cut -d ' ' -f 1
    fi
}

holds() {
    [ -f "$1" ] && [ "$(wc -c <"$1" | tr -d ' ')" = "$2" ] && [ "$(digest "$1")" = "$3" ]
}

tab=$(printf '\t')
found=0
while IFS=$tab read -r set name address size sha256 _source; do
    [ "$set" = "$wanted" ] || continue
    sha256=$(printf '%s' "$sha256" | tr -d '\r')
    found=$((found + 1))
    target=$directory/$name
    if holds "$target" "$size" "$sha256"; then
        echo "kept $name size=$size"
        continue
    fi
    curl --fail --silent --show-error --location --connect-timeout 30 --speed-limit 50000 --speed-time 60 --retry 3 --user-agent r2-probes --output "$target.part" "$address"
    if ! holds "$target.part" "$size" "$sha256"; then
        echo "fetch-files: $name has size $(wc -c <"$target.part" | tr -d ' ') and SHA-256 $(digest "$target.part"), and the list says $size and $sha256. Fix: correct the line of $name in $list, or the address." >&2
        exit 1
    fi
    mv "$target.part" "$target"
    echo "fetched $name size=$size"
done <"$list"
if [ "$found" -eq 0 ]; then
    echo "fetch-files: the list names no file of the set $wanted." >&2
    exit 1
fi
