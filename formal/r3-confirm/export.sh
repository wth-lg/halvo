#!/bin/sh
set -u

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

fail() {
  printf 'export: %s\n' "$1" >&2
  exit 1
}

usage() {
  cat <<'EOF'
usage: sh formal/r3-confirm/export.sh [--out DIR] [--seed N] [--count N]

Writes policy-table.tsv and traces.jsonl from the model Confirm.tla.

  --out DIR    the directory that gets the two files; it must exist
               (default: the directory of this script)
  --seed N     the seed of the drawn traces, 0 to 999999999
               (default: the Seed of Export.cfg)
  --count N    the number of traces, 200 to 9999; the model needs at least its
               directed traces (default: the Count of Export.cfg)

TLA2TOOLS_JAR names the tla2tools.jar of README.md; the SHA-256 of that file is
compared with the line in README.md. java is taken from JAVA_HOME when that is
set and else from the path, and it must be of major version 17 or later.

Exit status: 0 both files are written. 1 the export failed; a line on standard
error says why, and no file is written. 2 a refusal: one line
"REFUSED <cause>: <detail>" on standard error, and nothing is written.
Causes: usage, java, jar.
EOF
}

number() {
  case $1 in
    '' | *[!0-9]* | 0[0-9]*) return 1 ;;
  esac
  [ "${#1}" -le 9 ]
}

here=$(cd "$(dirname "$0")" && pwd) || fail "the directory of the script cannot be read"
out=$here
seed=
count=
while [ "$#" -gt 0 ]; do
  case $1 in
    --help)
      usage
      exit 0
      ;;
    --out | --seed | --count)
      [ "$#" -ge 2 ] || refuse usage "$1 needs a value"
      case $1 in
        --out) out=$2 ;;
        --seed) seed=$2 ;;
        *) count=$2 ;;
      esac
      [ -n "$2" ] || refuse usage "$1 needs a value"
      shift 2
      ;;
    *)
      refuse usage "unknown argument; --help prints the usage"
      ;;
  esac
done
if [ -n "$seed" ]; then
  number "$seed" || refuse usage "--seed takes a number from 0 to 999999999"
fi
if [ -n "$count" ]; then
  number "$count" || refuse usage "--count takes a number from 200 to 9999"
  if [ "$count" -lt 200 ] || [ "$count" -gt 9999 ]; then
    refuse usage "--count takes a number from 200 to 9999"
  fi
fi
[ -d "$out" ] || refuse usage "--out names no directory"
[ -w "$out" ] || refuse usage "the directory of --out cannot be written"

if [ -n "${JAVA_HOME:-}" ]; then
  java=$JAVA_HOME/bin/java
else
  java=java
fi
version=$("$java" -version 2>&1) || refuse java "no java runs; a java of major version 17 or later is needed"
major=$(printf '%s\n' "$version" | sed -n 's/^.* version "\([0-9][0-9]*\).*$/\1/p' | head -n 1)
case $major in
  '' | *[!0-9]*) refuse java "the version of java cannot be read; major version 17 or later is needed" ;;
esac
[ "$major" -ge 17 ] || refuse java "java of major version $major found; 17 or later is needed"

jar=${TLA2TOOLS_JAR:-}
[ -n "$jar" ] || refuse jar "TLA2TOOLS_JAR is not set; it names the tla2tools.jar of README.md"
[ -f "$jar" ] || refuse jar "TLA2TOOLS_JAR names no file"
case $jar in
  /* | [A-Za-z]:*) ;;
  *) jar=$(pwd)/$jar ;;
esac
want=$(sed -n 's/^tla2tools\.jar release=.* sha256=\([0-9a-f]\{64\}\)$/\1/p' "$here/README.md")
[ "${#want}" -eq 64 ] || fail "README.md holds no line with the SHA-256 of tla2tools.jar"
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum <"$jar") || fail "the SHA-256 of the jar cannot be computed"
else
  got=$(shasum -a 256 <"$jar") || fail "the SHA-256 of the jar cannot be computed"
fi
got=${got%% *}
[ "$got" = "$want" ] || refuse jar "the SHA-256 of the file that TLA2TOOLS_JAR names differs from the line in README.md"

if [ -z "$seed" ]; then
  seed=$(sed -n 's/^  Seed = \([0-9][0-9]*\)$/\1/p' "$here/Export.cfg")
fi
if [ -z "$count" ]; then
  count=$(sed -n 's/^  Count = \([0-9][0-9]*\)$/\1/p' "$here/Export.cfg")
fi
number "$seed" || fail "Export.cfg holds no Seed"
number "$count" || fail "Export.cfg holds no Count"

work=$(mktemp -d "${TMPDIR:-/tmp}/r3-confirm-export.XXXXXX") || fail "mktemp made no directory"
mkdir "$work/java" || fail "the scratch directory cannot be written"
cp "$here/Confirm.tla" "$work/Confirm.tla" || fail "Confirm.tla cannot be copied"
sed -e "s/^  Seed = .*\$/  Seed = $seed/" -e "s/^  Count = .*\$/  Count = $count/" "$here/Export.cfg" >"$work/Export.cfg" ||
  fail "Export.cfg cannot be copied"

(
  cd "$work" || exit 1
  "$java" -XX:+UseParallelGC -Xmx1g -Djava.io.tmpdir=java -cp "$jar" tlc2.TLC -workers 1 -deadlock -metadir states \
    -config Export.cfg Confirm.tla >tlc.out 2>tlc.err
)
status=$?
[ "$status" -eq 0 ] || fail "TLC ended with status $status; its output is tlc.out in $work"

tr -d '\r' <"$work/tlc.out" >"$work/lines" || fail "the output of TLC cannot be read"
sed -n 's/^"T \(.*\)"$/\1/p' "$work/lines" >"$work/table.raw" || fail "the table lines cannot be read"
tr ' ' '\t' <"$work/table.raw" >"$work/table.tabs" || fail "the table lines cannot be read"
LC_ALL=C sort -u "$work/table.tabs" >"$work/policy-table.tsv" || fail "the table cannot be sorted"
sed -n '/^"J [0-9][0-9][0-9][0-9] /p' "$work/lines" >"$work/traces.raw" || fail "the trace lines cannot be read"
LC_ALL=C sort -u "$work/traces.raw" >"$work/traces.sorted" || fail "the traces cannot be sorted"
sed -e 's/^"J [0-9][0-9][0-9][0-9] //' -e 's/"$//' -e 's/\\"/"/g' "$work/traces.sorted" >"$work/traces.jsonl" ||
  fail "the trace lines cannot be read"

rows=$(wc -l <"$work/policy-table.tsv") || fail "the table cannot be counted"
rows=$((rows))
[ "$rows" -eq 3840 ] || fail "the table has $rows rows, not 3840; the output of TLC is tlc.out in $work"
lines=$(wc -l <"$work/traces.jsonl") || fail "the traces cannot be counted"
lines=$((lines))
[ "$lines" -eq $((count + 1)) ] || fail "the traces file has $lines lines, not $((count + 1)); the output of TLC is tlc.out in $work"
first=$(head -n 1 "$work/traces.jsonl") || fail "the traces cannot be read"
[ "$first" = "{\"seed\":$seed,\"count\":$count}" ] || fail "the first line of the traces is not the seed and the count"
directed=$(sed -n 's/^"D \([0-9][0-9]*\)"$/\1/p' "$work/lines" | head -n 1)
number "$directed" || fail "the output of TLC holds no count of the directed traces"

cp "$work/policy-table.tsv" "$out/policy-table.tsv" || fail "policy-table.tsv cannot be written"
cp "$work/traces.jsonl" "$out/traces.jsonl" || fail "traces.jsonl cannot be written"
printf 'export rows=%s traces=%s directed=%s seed=%s\n' "$rows" "$count" "$directed" "$seed"
