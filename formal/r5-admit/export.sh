#!/bin/sh
set -u

usage() {
    cat <<'EOF'
usage: sh formal/r5-admit/export.sh [--out DIR]

Writes edit-table.tsv and traces.jsonl, which TLC prints from the model of this
directory under Export.cfg. DIR is an existing directory; without --out the two
files go into this directory. The seed and the count of the traces are those of
the first line of traces.jsonl in this directory, or 1 and 200 when that file is
not there. The seed has at most 9 digits and the count is 200 to 1000.

TLC is tla2tools.jar of the release that README.md names. The variable
TLA2TOOLS_JAR gives its path; without it the script fetches the jar into a new
temporary directory. Java is taken from JAVA_HOME when that is set and else from
the path, and its major version is 17 or later.

Exit status: 0 when both files are written. 1 when the export ran and a check
missed: one line says which, and no file is written. 2 on a refusal: one line
"REFUSED <cause>: <detail>" on standard error, with the cause usage, java or jar.
A refusal writes no file. The script makes its temporary directories under TMPDIR,
or /tmp when TMPDIR is not set, and removes them when it ends, on every path out.
EOF
}

fetched=
work=

tidy() {
    case $fetched in
        */r5-export.??????) rm -rf -- "$fetched" ;;
    esac
    case $work in
        */r5-export.??????) rm -rf -- "$work" ;;
    esac
}
trap tidy EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

refuse() {
    printf 'REFUSED %s: %s\n' "$1" "$2" >&2
    exit 2
}

miss() {
    printf 'export %s\n' "$1"
    exit 1
}

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

here=$(cd "$(dirname "$0")" && pwd)
out=$here
case $# in
    0) ;;
    1)
        test "$1" = --help || refuse usage "the arguments are --out DIR or none"
        usage
        exit 0
        ;;
    2)
        test "$1" = --out || refuse usage "the arguments are --out DIR or none"
        out=$2
        ;;
    *) refuse usage "the arguments are --out DIR or none" ;;
esac
test -d "$out" || refuse usage "--out names no directory"

seed=1
count=200
if test -f "$here/traces.jsonl"; then
    IFS= read -r line <"$here/traces.jsonl" || line=
    first=$(printf '%s\n' "$line" | tr -d '\r' |
        sed -n 's/^{"seed":\([0-9][0-9]*\),"count":\([0-9][0-9]*\)}$/\1 \2/p')
    test -n "$first" || refuse usage "the first line of traces.jsonl is not {\"seed\":<n>,\"count\":<n>}"
    seed=${first% *}
    count=${first#* }
    case $seed in
        0 | [1-9]*) ;;
        *) refuse usage "the seed of traces.jsonl starts with 0" ;;
    esac
    case $count in
        [1-9]*) ;;
        *) refuse usage "the count of traces.jsonl starts with 0" ;;
    esac
    test "${#seed}" -le 9 || refuse usage "the seed of traces.jsonl has more than 9 digits"
    test "${#count}" -le 4 || refuse usage "the count of traces.jsonl is above 1000"
    test "$count" -ge 200 || refuse usage "the count of traces.jsonl is below 200"
    test "$count" -le 1000 || refuse usage "the count of traces.jsonl is above 1000"
fi

if test -n "${JAVA_HOME:-}"; then
    java=$JAVA_HOME/bin/java
else
    java=java
fi
command -v "$java" >/dev/null 2>&1 || refuse java "no java at JAVA_HOME or on the path"
major=$("$java" -version 2>&1 | sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' | head -n 1)
test -n "$major" || refuse java "the version of java is not readable"
test "$major" -ge 17 || refuse java "java $major is older than 17"

pin=$(sed -n 's/^jar url=\([^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]*\)$/\1 \2 \3/p' "$here/README.md")
pins=$(printf '%s\n' "$pin" | wc -l | tr -d ' ')
test -n "$pin" || refuse jar "README.md holds no one line jar url=<address> bytes=<n> sha256=<hex>"
test "$pins" -eq 1 || refuse jar "README.md holds no one line jar url=<address> bytes=<n> sha256=<hex>"
url=${pin%% *}
sha=${pin##* }
bytes=${pin#* }
bytes=${bytes% *}
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 ||
    refuse jar "no sha256sum and no shasum to check the jar"

if test -n "${TLA2TOOLS_JAR:-}"; then
    jar=$TLA2TOOLS_JAR
    test -f "$jar" || refuse jar "TLA2TOOLS_JAR names no file"
else
    command -v curl >/dev/null 2>&1 || refuse jar "no TLA2TOOLS_JAR and no curl to fetch the jar"
    fetched=$(mktemp -d "${TMPDIR:-/tmp}/r5-export.XXXXXX") || refuse jar "no temporary directory for the jar"
    jar=$fetched/tla2tools.jar
    curl -fsSL --retry 2 -o "$jar" "$url" 2>/dev/null || refuse jar "the fetch of the jar failed"
fi
size=$(wc -c <"$jar" | tr -d ' ')
test "$size" = "$bytes" || refuse jar "the jar has $size bytes and README.md pins $bytes"
test "$(digest "$jar")" = "$sha" || refuse jar "the SHA-256 of the jar is not the one that README.md pins"
jar=$(cd "$(dirname "$jar")" && pwd)/$(basename "$jar")

work=$(mktemp -d "${TMPDIR:-/tmp}/r5-export.XXXXXX") || miss "check=scratch"
cp "$here/Admit.tla" "$here/Export.tla" "$work/" || miss "check=scratch"
sed -e "s/^    Seed = .*/    Seed = $seed/" -e "s/^    Count = .*/    Count = $count/" \
    "$here/Export.cfg" >"$work/Export.cfg" || miss "check=scratch"
(cd "$work" && "$java" -Xmx1g -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers 1 \
    -metadir meta -config Export.cfg Export >tlc.out 2>&1)
code=$?
test "$code" -eq 0 || miss "check=tlc exit=$code"

tr -d '\r' <"$work/tlc.out" | sed -n 's/^"row \(.*\)"$/\1/p' | tr ' ' '\t' |
    LC_ALL=C sort -u >"$work/edit-table.tsv"
tr -d '\r' <"$work/tlc.out" | sed -n 's/^"trace \(.*\)"$/\1/p' | LC_ALL=C sort -u |
    LC_ALL=C sort -n >"$work/traces.txt"

rows=$(wc -l <"$work/edit-table.tsv" | tr -d ' ')
test "$rows" -eq 57 || miss "check=rows rows=$rows"
kinds=$(awk -F '\t' '
    NF != 5 { bad++ }
    $5 == "same" { same++ }
    $5 == "-" { none++ }
    END { printf "%d-%d-%d\n", bad, same, none }' "$work/edit-table.tsv")
test "$kinds" = 0-11-43 || miss "check=clean fields-same-none=$kinds"
traces=$(wc -l <"$work/traces.txt" | tr -d ' ')
test "$traces" -eq "$count" || miss "check=traces traces=$traces count=$count"

facts=$(awk -v table="$work/edit-table.tsv" '
    BEGIN {
        while ((getline row < table) > 0) {
            split(row, cell, "\t")
            answer[cell[1] "\t" cell[2] "\t" cell[3]] = cell[4]
            after[cell[1] "\t" cell[2] "\t" cell[3]] = cell[5]
        }
    }
    {
        lines++
        if ($1 != lines || NF < 2 || NF > 17) bad++
        state = "none"
        clean = 0
        hit = 0
        for (i = 2; i <= NF; i++) {
            split($i, told, ":")
            guard = "-"
            if (state == "provisional" && told[1] == "use_ok" && clean == 0) guard = "clean_below"
            if (state == "provisional" && told[1] == "use_ok" && clean == 1) guard = "clean_reaches"
            key = state "\t" told[1] "\t" told[2]
            want = after[key]
            if (want == "same") want = clean
            if (want == "-") want = "null"
            if (told[2] != guard || !(key in answer) || answer[key] != told[3]) bad++
            else if ((told[4] "") != (want "")) bad++
            seen[key] = 1
            if (told[3] == "edit") hit = 1
            else {
                state = told[3]
                clean = told[4]
            }
        }
        refused += hit
    }
    END {
        for (key in seen) if (key in answer) covered++
        printf "%d-%d-%d\n", bad, covered, refused
    }' "$work/traces.txt")
refused=${facts##*-}
test "$facts" = "0-$rows-$refused" || miss "check=replay bad-covered-refused=$facts"

{
    printf '{"seed":%s,"count":%s}\n' "$seed" "$count"
    awk '{
        printf "{\"moves\":["
        for (i = 2; i <= NF; i++) {
            split($i, told, ":")
            printf "%s{\"move\":\"%s\",\"guard\":\"%s\",\"answer\":\"%s\",\"clean\":%s}", (i > 2 ? "," : ""), told[1], told[2], told[3], told[4]
        }
        printf "]}\n"
    }' "$work/traces.txt"
} >"$work/traces.jsonl"

cp "$work/edit-table.tsv" "$out/edit-table.tsv" || miss "check=write"
cp "$work/traces.jsonl" "$out/traces.jsonl" || miss "check=write"
printf 'export seed=%s count=%s rows=%s traces=%s refused=%s\n' "$seed" "$count" "$rows" "$traces" "$refused"
