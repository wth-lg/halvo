#!/bin/sh
set -u
LC_ALL=C
export LC_ALL
unset CDPATH JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS

usage() {
    cat <<'EOF'
usage: sh formal/r5-admit/record.sh list
       sh formal/r5-admit/record.sh run DIR full [WORKERS [HOW]]
       sh formal/r5-admit/record.sh run DIR small
       sh formal/r5-admit/record.sh run DIR design FAULT
       sh formal/r5-admit/record.sh run DIR reach NAME RUNSMAX RUNSONBOTHSTEPS
       sh formal/r5-admit/record.sh write DIR

Writes record.txt of this directory from runs of TLC, one run for each call of run.

list prints the arguments of every run that the record needs, one run per line: full,
small, then one line for each line of runs.txt.

run makes one TLC run on a copy of this directory's model in a new temporary directory
under TMPDIR (or /tmp), which it removes when it ends, and writes three files into the
existing directory DIR: <key>.out (TLC's output), <key>.line (the line of the record) and
<key>.stamp (the stamp of the model's files when the run started). The key is full,
small, design-<FAULT> or reach-<NAME>-<RUNSMAX>-<RUNSONBOTHSTEPS>. full checks Admit.cfg
with WORKERS workers (default 4) and writes HOW (helper or nice19, default helper) and its
wall time into its line; small checks Small.cfg with 3 workers; a design checks Admit.cfg
with Fault = FAULT at the bound that runs.txt gives it, with one worker; a reach run
checks Reach.tla's one line INVARIANT NAME (a name that starts with Not) or PROPERTY NAME
(Never) at the bound given, with one worker.

write puts the lines of DIR together into record.txt: the stamp of the model's files now,
the full line, the small line, then the design and the reach lines in the order of
runs.txt.

TLA2TOOLS_JAR names the jar that README.md pins. Java is taken from JAVA_HOME when that
is set and else from the path, and its major version is 17 or later.

Seconds that one run took on a machine of 16 cores under the load of other work
(2026-11-12), in two rounds of all the runs: a design 1 to 524 and a reach run 1 to 62
with one worker (the longest, revert_compiled, at a 1-minute load of 82), small 44 with 3
workers, full 2,193 with 4 workers at the lowest priority.

Exit status: 0 when the run or the record is written. 1 when write finds a run's line
missing or of another stamp: one line says which, and record.txt is left as it was. 2 on
a refusal: one line "REFUSED <cause>: <detail>" on standard error, with the cause usage,
java or jar; nothing is written.
EOF
}

made=

tidy() {
    case $made in
        */r5-record.??????) rm -rf -- "$made" ;;
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

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | cut -d' ' -f1
    else
        shasum -a 256 | cut -d' ' -f1
    fi
}

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)

stamp() {
    (cd "$repo" && git -c core.quotepath=off ls-files -- 'formal/r5-admit/*.tla' 'formal/r5-admit/*.cfg') |
        sort | while IFS= read -r f; do
        printf '%s  %s\n' "$(digest <"$repo/$f")" "$f"
    done | digest
}

list() {
    printf 'full\nsmall\n'
    awk '$1 == "design" { print "design", $2 } $1 == "reach" { print "reach", $2, $3, $4 }' "$here/runs.txt"
}

java_of() {
    if test -n "${JAVA_HOME:-}"; then
        java=$JAVA_HOME/bin/java
    else
        java=java
    fi
    command -v "$java" >/dev/null 2>&1 || refuse java "no java at JAVA_HOME or on the path"
    major=$("$java" -version 2>&1 | sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' | head -n 1)
    test -n "$major" || refuse java "the version of java is not readable"
    test "$major" -ge 17 || refuse java "java $major is older than 17"
}

jar_of() {
    test -n "${TLA2TOOLS_JAR:-}" || refuse jar "TLA2TOOLS_JAR names no jar"
    test -f "$TLA2TOOLS_JAR" || refuse jar "TLA2TOOLS_JAR names no file"
    pin=$(sed -n 's/^jar url=[^ ]* bytes=[0-9][0-9]* sha256=\([0-9a-f]\{64\}\)$/\1/p' "$here/README.md")
    test "$(digest <"$TLA2TOOLS_JAR")" = "$pin" || refuse jar "the SHA-256 of the jar is not the one that README.md pins"
    jar=$(cd "$(dirname "$TLA2TOOLS_JAR")" && pwd)/$(basename "$TLA2TOOLS_JAR")
}

run() {
    out=$1
    kind=$2
    test -d "$out" || refuse usage "DIR names no directory"
    name=${3:-}
    workers=1
    module=Admit
    heap=2g
    how=helper
    case $kind:$# in
        full:2 | full:3 | full:4)
            key=full
            workers=${3:-4}
            how=${4:-helper}
            heap=4g
            case $workers in
                '' | 0* | *[!0-9]*) refuse usage "WORKERS is a number from 1" ;;
            esac
            case $how in
                helper | nice19) ;;
                *) refuse usage "HOW is helper or nice19" ;;
            esac
            ;;
        small:2)
            key=small
            workers=3
            ;;
        design:3)
            key=design-$3
            bound=$(awk -v f="$3" '$1 == "design" && $2 == f { print $3, $4, $5 }' "$here/runs.txt")
            case $bound in
                [1-9]' '[0-9]' '[1-9]) ;;
                *) refuse usage "runs.txt holds no one design $3" ;;
            esac
            ;;
        reach:5)
            key=reach-$3-$4-$5
            module=Reach
            case $3:$4:$5 in
                Not*:[1-9]:[0-9]) line=INVARIANT ;;
                Never*:[1-9]:[0-9]) line=PROPERTY ;;
                *) refuse usage "a reach run is NAME RUNSMAX RUNSONBOTHSTEPS, NAME of Not or Never" ;;
            esac
            ;;
        *) refuse usage "see sh formal/r5-admit/record.sh --help" ;;
    esac
    java_of
    jar_of
    stamp >"$out/$key.stamp"
    made=$(mktemp -d "${TMPDIR:-/tmp}/r5-record.XXXXXX") || refuse usage "no temporary directory"
    (cd "$repo" && git -c core.quotepath=off ls-files -- 'formal/r5-admit/*.tla') >"$made/tracked.txt" || exit 1
    while IFS= read -r f; do
        cp "$repo/$f" "$made/" || exit 1
    done <"$made/tracked.txt"
    mkdir "$made/jtmp" || exit 1
    case $kind in
        full) cp "$here/Admit.cfg" "$made/run.cfg" ;;
        small) cp "$here/Small.cfg" "$made/run.cfg" ;;
        design)
            runs=${bound%% *}
            rest=${bound#* }
            both=${rest%% *}
            edits=${rest#* }
            sed -e "s/^    Fault = \"none\"\$/    Fault = \"$name\"/" -e "s/^    RunsMax = 5\$/    RunsMax = $runs/" \
                -e "s/^    RunsOnBothSteps = 3\$/    RunsOnBothSteps = $both/" -e "s/^    EditMax = 3\$/    EditMax = $edits/" \
                "$here/Admit.cfg" >"$made/run.cfg"
            ;;
        reach)
            sed -e "s/^    RunsMax = 5\$/    RunsMax = $4/" -e "s/^    RunsOnBothSteps = 3\$/    RunsOnBothSteps = $5/" \
                -e '/^PROPERTIES$/,$d' "$here/Admit.cfg" >"$made/run.cfg"
            printf '%s %s\n' "$line" "$3" >>"$made/run.cfg"
            ;;
    esac
    start=$(date +%s)
    (cd "$made" && "$java" -Xmx"$heap" -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp "$jar" tlc2.TLC \
        -workers "$workers" -metadir meta -config run.cfg "$module" >tlc.out 2>&1 </dev/null)
    code=$?
    seconds=$(($(date +%s) - start))
    tr -d '\r' <"$made/tlc.out" >"$out/$key.out" || exit 1
    awk -v kind="$kind" -v name="$name" -v bound="${bound:-${4:-}-${5:-}}" -v code="$code" \
        -v workers="$workers" -v how="$how" -v seconds="$seconds" -v cfg="$made/run.cfg" '
        BEGIN {
            while ((getline l < cfg) > 0) {
                if (l == "PROPERTIES") on = 1
                else if (on && l ~ /^    Q[0-9]+[A-Za-z]*$/) { q = l; sub(/^    Q/, "", q); sub(/[A-Za-z]+$/, "", q); seen[q + 0] = 1 }
                else on = 0
            }
            for (q in seen) k++
            gsub(/ /, "-", bound)
        }
        /^Error: (Invariant|Action property) [A-Za-z0-9_]+ is violated/ {
            v = ($2 == "Invariant") ? $3 : $4
            if (first == "") first = v
            bad[v] = 1
        }
        /^[0-9]+ states generated, [0-9]+ distinct states found, [0-9]+ states left on queue\.$/ { distinct = $4; left = $8 }
        $0 == "Model checking completed. No error has been found." { complete = 1 }
        END {
            for (v in bad) nv++
            clean = complete && code == 0 && left == 0
            if (kind == "full") printf "full config=Admit.cfg states=%d properties=%d violated=%s workers=%s run=%s seconds=%s\n", distinct, k, (clean ? 0 : (nv ? nv : "unknown")), workers, how, seconds
            else if (kind == "small") printf "small config=Small.cfg states=%s\n", (clean ? distinct : "failed")
            else if (kind == "design") printf "design fault=%s bound=%s reports=%s states=%d\n", name, bound, (first != "" ? first : (clean ? "nothing" : "unknown")), distinct
            else printf "reach property=%s bound=%s reached=%s states=%d\n", name, bound, (first == name ? "yes" : "no"), distinct
        }' "$out/$key.out" >"$out/$key.line" || exit 1
    cat "$out/$key.line"
}

write() {
    out=$1
    test -d "$out" || refuse usage "DIR names no directory"
    now=$(stamp)
    made=$(mktemp -d "${TMPDIR:-/tmp}/r5-record.XXXXXX") || refuse usage "no temporary directory"
    printf 'stamp=%s\n' "$now" >"$made/record.txt"
    missed=0
    list >"$made/keys.txt"
    while read -r kind a b c; do
        case $kind in
            design) key=design-$a ;;
            reach) key=reach-$a-$b-$c ;;
            *) key=$kind ;;
        esac
        if ! test -s "$out/$key.line" || ! test -f "$out/$key.stamp" || test "$(cat "$out/$key.stamp")" != "$now"; then
            printf 'record missing or of another stamp: %s\n' "$key"
            missed=$((missed + 1))
            continue
        fi
        cat "$out/$key.line" >>"$made/record.txt" || exit 1
    done <"$made/keys.txt"
    test "$missed" -eq 0 || exit 1
    cp "$made/record.txt" "$here/record.txt" || exit 1
    printf 'record lines=%s stamp=%s\n' "$(grep -c '' "$here/record.txt")" "$now"
}

case ${1:-} in
    --help) usage ;;
    list)
        test $# -eq 1 || refuse usage "list takes no argument"
        list
        ;;
    run)
        test $# -ge 3 || refuse usage "run takes DIR and a run"
        shift
        run "$@"
        ;;
    write)
        test $# -eq 2 || refuse usage "write takes DIR"
        write "$2"
        ;;
    *) refuse usage "the first argument is list, run, write or --help" ;;
esac
