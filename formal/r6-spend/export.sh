#!/bin/sh
set -u

help() {
    sed -n p <<'EOF'
usage: sh formal/r6-spend/export.sh --seed N --count N --out FILE

Writes COUNT traces of the machine of one hold to FILE in the line forms of
traces.jsonl, which README.md of this directory gives: first the directed traces
that Spend.tla lists, then traces that TLC draws in its simulation mode on
Export.cfg (the seed, one worker, a depth of 12). FILE must not exist. A person
commits the result as traces.jsonl.

  --seed N    1 to 999999999
  --count N   200 to 99999
  --out FILE  the file to write, in a directory that exists

The script fetches tla2tools.jar from the address of the line "tlc url=..." of
README.md into a directory from mktemp -d, which stays, and compares its size
and its SHA-256 with that line. Java is taken from JAVA_HOME when that is set
and else from the path; its major version is 17 or later.

Exit status: 0, FILE is written and holds every pair of a state and a move and
every cause. 1, the write of FILE failed after the script had made it; one line
says so, and the script, which calls no command that removes a file, leaves what
stands there. 2, a refusal: one line "REFUSED <cause>: <detail>" on standard
error and no file. The causes: usage, java, fetch, hash, file, and cover when the
traces lack a pair or a cause.
EOF
}

refuse() {
    printf 'REFUSED %s: %s\n' "$1" "$2" >&2
    exit 2
}

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | sed 's/ .*//'
    else
        shasum -a 256 "$1" | sed 's/ .*//'
    fi
}

case $0 in
    */*) here=${0%/*} ;;
    *) here=. ;;
esac

if test "$#" -eq 1 && test "$1" = --help; then
    help
    exit 0
fi

seed=
count=
out=
while test "$#" -gt 0; do
    test "$#" -ge 2 || refuse usage "an option has no value; see --help"
    case $1 in
        --seed)
            test -z "$seed" || refuse usage "--seed is given twice"
            seed=$2
            ;;
        --count)
            test -z "$count" || refuse usage "--count is given twice"
            count=$2
            ;;
        --out)
            test -z "$out" || refuse usage "--out is given twice"
            out=$2
            ;;
        *) refuse usage "the options are --seed N --count N --out FILE" ;;
    esac
    shift 2
done
test -n "$seed" || refuse usage "--seed is missing"
test -n "$count" || refuse usage "--count is missing"
test -n "$out" || refuse usage "--out is missing"
case $seed in
    *[!0-9]* | 0*) refuse usage "--seed is not a number from 1 to 999999999" ;;
esac
test "${#seed}" -le 9 || refuse usage "--seed is not a number from 1 to 999999999"
case $count in
    *[!0-9]* | 0*) refuse usage "--count is not a number from 200 to 99999" ;;
esac
test "${#count}" -le 5 || refuse usage "--count is not a number from 200 to 99999"
test "$count" -ge 200 || refuse usage "--count is not a number from 200 to 99999"

if test -e "$out" || test -L "$out"; then
    refuse file "--out names a path that exists"
fi
case $out in
    */*) outdir=${out%/*} ;;
    *) outdir=. ;;
esac
test -d "${outdir:-/}" || refuse file "--out names a file in a directory that does not exist"

if test -n "${JAVA_HOME:-}"; then
    java=$JAVA_HOME/bin/java
else
    java=java
fi
command -v "$java" >/dev/null 2>&1 || refuse java "no java at JAVA_HOME or on the path"
major=$("$java" -version 2>&1 | sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' | sed -n 1p)
test -n "$major" || refuse java "the version of java is not readable"
test "$major" -ge 17 || refuse java "java $major is older than 17"

for file in README.md Hold.tla Spend.tla Export.cfg; do
    test -f "$here/$file" || refuse usage "$file is not beside the script"
done
pin=$(sed -n 's/^tlc url=\([^ ]*\) bytes=\([1-9][0-9]*\) sha256=\([0-9a-f]*\)$/\1 \2 \3/p' "$here/README.md")
case $pin in
    "" | *"
"*) refuse hash "README.md holds no one line tlc url=<address> bytes=<n> sha256=<hex>" ;;
esac
url=${pin%% *}
sha=${pin##* }
bytes=${pin#* }
bytes=${bytes% *}
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 ||
    refuse hash "no sha256sum and no shasum to compare the jar with"
command -v curl >/dev/null 2>&1 || refuse fetch "no curl on the path"

work=$(mktemp -d) || refuse fetch "mktemp -d made no directory for the jar"
got=$(curl -fsSL --retry 2 -o "$work/tla2tools.jar" -w '%{size_download}' "$url" 2>/dev/null) ||
    refuse fetch "curl could not fetch the jar from the address of README.md"
test -f "$work/tla2tools.jar" || refuse fetch "curl wrote no file"
test "$got" = "$bytes" || refuse hash "the jar has $got bytes and README.md names $bytes"
test "$(digest "$work/tla2tools.jar")" = "$sha" ||
    refuse hash "the SHA-256 of the jar is not the one that README.md names"

for file in Hold.tla Spend.tla Export.cfg; do
    sed -n p "$here/$file" >"$work/$file" || refuse java "the model could not be copied for TLC"
done
(cd "$work" && "$java" -Xmx1g -XX:+UseParallelGC -Djava.io.tmpdir=. -cp tla2tools.jar tlc2.TLC \
    -simulate "num=$count" -seed "$seed" -depth 12 -workers 1 -metadir meta \
    -config Export.cfg Spend.tla >tlc.out 2>tlc.err)
code=$?
test "$code" -eq 0 || refuse java "TLC ended with the status $code"

facts=$(awk -v count="$count" -v body="$work/body.jsonl" '
    function put(line, drawn,    n, i, token, part, cap, active, spent, held, text, before) {
        n = split(line, token, " ")
        traces++
        if (n < 5 || n > 16 || (drawn && n != 16)) bad++
        split(token[1], cap, "=")
        split(token[2], active, "=")
        split(token[3], spent, "=")
        split(token[4], held, "=")
        if (cap[1] != "cap" || cap[2] !~ /^(-|[0-9]+)$/) bad++
        if (active[1] != "active" || (active[2] != "yes" && active[2] != "no")) bad++
        if (spent[1] != "spent" || spent[2] !~ /^[0-9]+$/) bad++
        if (held[1] != "held" || held[2] !~ /^[0-9]+$/) bad++
        text = "{\"cap\":" (cap[2] == "-" ? "null" : cap[2]) ",\"active\":\"" active[2] "\",\"spent\":" spent[2] ",\"held\":" held[2] ",\"moves\":["
        before = "new"
        for (i = 5; i <= n; i++) {
            if (split(token[i], part, ":") != 7) bad++
            if (!(part[1] in moves) || !(part[4] in states)) bad++
            if (part[2] !~ /^(-|[0-9]+)$/ || part[5] !~ /^[0-9]+$/ || part[6] !~ /^[0-9]+$/) bad++
            if (part[7] !~ /^[0-9]+$/) bad++
            if (part[3] != "ok" && !(part[3] in causes)) bad++
            if (!((before " " part[1]) in pair)) {
                pair[before " " part[1]] = 1
                pairs++
            }
            if (part[3] != "ok" && !(part[3] in cause)) {
                cause[part[3]] = 1
                found++
            }
            text = text (i > 5 ? "," : "") "{\"move\":\"" part[1] "\",\"arg\":" (part[2] == "-" ? "null" : part[2]) ",\"answer\":\"" part[3] "\",\"state\":\"" part[4] "\",\"spent\":" part[5] ",\"held\":" part[6] ",\"cost\":" part[7] "}"
            before = part[4]
        }
        print text "]}" >>body
    }
    BEGIN {
        split("reserve dispatch void settle expire", list, " ")
        for (i in list) moves[list[i]] = 1
        split("new held sent settled expired voided", list, " ")
        for (i in list) states[list[i]] = 1
        split("not_active no_cap over_cap no_hold used repeat other_arguments state", list, " ")
        for (i in list) causes[list[i]] = 1
        printf "" >body
    }
    { sub(/\r$/, "") }
    /^"directed [0-9]+ .*"$/ {
        line = substr($0, 11, length($0) - 11)
        at = index(line, " ")
        number = substr(line, 1, at - 1) + 0
        if (!(number in directed)) directed[number] = substr(line, at + 1)
    }
    /^"trace .*"$/ { drawn[++draws] = substr($0, 8, length($0) - 8) }
    END {
        while ((first + 1) in directed) first++
        for (i = 1; i <= first; i++) put(directed[i], 0)
        for (i = 1; i <= draws && traces < count; i++) put(drawn[i], 1)
        printf "%d %d %d %d %d\n", traces, bad, pairs, found, first
    }' "$work/tlc.out")
read -r traces bad pairs found directed more <<EOF
$facts
EOF
test -n "$directed" || refuse java "the output of TLC could not be read"
test -z "$more" || refuse java "the output of TLC could not be read"
test "$traces" -eq "$count" || refuse java "TLC gave $traces traces for a count of $count"
test "$bad" -eq 0 || refuse java "$bad parts of the traces of TLC are not of the form of the model"
if test "$pairs" -ne 30 || test "$found" -ne 8; then
    refuse cover "pairs=$pairs of 30 causes=$found of 8 with seed=$seed count=$count"
fi

{
    printf '{"seed":%s,"count":%s}\n' "$seed" "$count"
    sed -n p "$work/body.jsonl"
} >"$work/traces.jsonl" || refuse file "the traces could not be put together"

if ! (set -C && : >"$out") 2>/dev/null; then
    refuse file "--out could not be made, or another call made it first"
fi
if sed -n p "$work/traces.jsonl" >"$out" &&
    test "$(digest "$out")" = "$(digest "$work/traces.jsonl")"; then
    printf 'export seed=%s count=%s directed=%s pairs=%s causes=%s\n' \
        "$seed" "$count" "$directed" "$pairs" "$found"
    exit 0
fi
printf 'export written=no seed=%s count=%s\n' "$seed" "$count"
exit 1
