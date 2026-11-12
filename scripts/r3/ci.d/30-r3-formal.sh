#!/bin/sh
# jobs: macos windows
# rule: TLC finds none of the invariants P1 to P11 of formal/r3-confirm violated under Confirm.cfg and Rebinds.cfg, the model exports the committed policy table and traces byte for byte, and the control copy breaks P1.
set -u
LC_ALL=C
export LC_ALL
unset CDPATH JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
name=30-r3-formal
label=$name
workers=3
invariants='P1 P2 P3 P4 P5 P6 P7 P8 P9 P10 P11'

fail() {
  printf 'FAIL %s (%s)\n' "$label" "$1"
  exit 1
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum
  else
    shasum -a 256
  fi
}

locate() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    here=$(dirname "$0") || fail repo
    repo=$(cd "$here/../../.." && pwd) || fail repo
  fi
  dir=$repo/formal/r3-confirm
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$art" || fail artifacts
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r3-formal.XXXXXX") || fail scratch
  tmp=$(cd "$tmp" && pwd) || fail scratch
}

stamp() {
  git -C "$repo" diff --no-ext-diff --binary HEAD -- >"$tmp/tree.$1" 2>/dev/null || fail git
  sum=$(sha256 <"$tmp/tree.$1") || fail git
  printf '%s\n' "${sum%% *}" >"$tmp/stamp.$1" || fail git
}

present() {
  for f in Confirm.tla Confirm.cfg Rebinds.cfg Full.cfg Control.cfg Export.cfg export.sh README.md policy-table.tsv traces.jsonl; do
    [ -f "$dir/$f" ] || fail "missing formal/r3-confirm/$f"
  done
}

cfg_text() {
  printf 'CONSTANTS\n  Effects = %s\n  Approvals = {a1, a2}\n  Digests = {d1, d2}\n  MaxRebinds = %s\n' "$1" "$2"
  printf '  PaymentMayRun = %s\n  BeginUnguarded = %s\n  Seed = 0\n  Count = 0\n' "$3" "$3"
  printf 'SYMMETRY Symmetric\nINIT Init\nNEXT Next\nINVARIANTS\n'
  for p in $invariants; do
    printf '  %s\n' "$p"
  done
}

configs() {
  cfg_text '{e1, e2}' 1 FALSE >"$tmp/Confirm.cfg" || fail scratch
  cfg_text '{e1}' 2 FALSE >"$tmp/Rebinds.cfg" || fail scratch
  cfg_text '{e1, e2}' 2 FALSE >"$tmp/Full.cfg" || fail scratch
  cfg_text '{e1, e2}' 1 TRUE >"$tmp/Control.cfg" || fail scratch
  same=0
  for f in Confirm.cfg Rebinds.cfg Full.cfg Control.cfg; do
    if cmp -s "$tmp/$f" "$dir/$f"; then
      same=$((same + 1))
    else
      cp "$tmp/$f" "$art/want-$f" || fail artifacts
    fi
  done
  printf 'formal configs files=4 same=%s\n' "$same"
  [ "$same" -eq 4 ] || fail configs
  count=0
  for p in $invariants; do
    count=$((count + 1))
  done
}

pin() {
  grep '^tla2tools\.jar ' "$dir/README.md" >"$tmp/pin.txt"
  sed -n 's/^tla2tools\.jar release=[^ ]* url=\(https:[^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p' \
    "$tmp/pin.txt" >"$tmp/pin-read.txt" || fail pin
  lines=$(grep -c '' "$tmp/pin.txt")
  read_lines=$(grep -c '' "$tmp/pin-read.txt")
  [ "$lines:$read_lines" = 1:1 ] || fail pin
  read -r url bytes want <"$tmp/pin-read.txt" || fail pin
}

java_of() {
  if [ -n "${JAVA_HOME:-}" ]; then
    if command -v cygpath >/dev/null 2>&1; then
      JAVA_HOME=$(cygpath -u "$JAVA_HOME") || fail java
      export JAVA_HOME
    fi
    java=$JAVA_HOME/bin/java
  else
    java=java
  fi
  "$java" -version >"$tmp/java.raw" 2>&1 </dev/null
  tr -d '\r' <"$tmp/java.raw" >"$tmp/java.txt" || fail java
  sed -n 's/^.* version "\([0-9][0-9]*\).*$/\1/p' "$tmp/java.txt" >"$tmp/major.txt" || fail java
  major=none
  read -r major <"$tmp/major.txt" || major=none
  case $major in
    '' | *[!0-9]*) major=none ;;
  esac
  printf 'formal java major=%s\n' "$major"
  [ "$major" != none ] || fail java
  [ "$major" -ge 17 ] || fail java
}

jar() {
  if [ -n "${TLA2TOOLS_JAR:-}" ]; then
    source=given
    cp "$TLA2TOOLS_JAR" "$tmp/tla2tools.jar" 2>/dev/null || : >"$tmp/tla2tools.jar"
  else
    source=fetch
    (cd "$tmp" && curl -q -fsSL --retry 2 --max-time 300 -o tla2tools.jar "$url") || : >"$tmp/tla2tools.jar"
  fi
  size=$(wc -c <"$tmp/tla2tools.jar") || fail jar
  size=$((size))
  sum=$(sha256 <"$tmp/tla2tools.jar") || fail jar
  sum=${sum%% *}
  same=no
  if [ "$size" = "$bytes" ] && [ "$sum" = "$want" ]; then
    same=yes
  fi
  printf 'formal jar source=%s bytes=%s same=%s\n' "$source" "$size" "$same"
  [ "$same" = yes ] || fail jar
}

tlc() {
  run=$tmp/$1
  mkdir "$run" "$run/jtmp" || fail scratch
  cp "$dir/Confirm.tla" "$run/Confirm.tla" || fail scratch
  cp "$2" "$run/run.cfg" || fail scratch
  (
    cd "$run" || exit 99
    "$java" -XX:+UseParallelGC -Xmx2g -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC -workers "$workers" \
      -deadlock -metadir meta -config run.cfg Confirm.tla >out.raw 2>&1 </dev/null
  )
  status=$?
  tr -d '\r' <"$run/out.raw" >"$run/out.txt" || status=99
  cp "$run/out.txt" "$art/$1.txt" || status=99
  sed -n 's/^[0-9][0-9]* states generated, \([0-9][0-9]*\) distinct states found, 0 states left on queue\.$/\1/p' \
    "$run/out.txt" >"$run/distinct.txt" || status=99
  distinct=0
  read -r distinct <"$run/distinct.txt" || distinct=0
  sed -n 's/^Error: Invariant \([A-Za-z0-9_]*\) is violated.*$/\1/p' "$run/out.txt" >"$run/violated.txt" || status=99
  sort -u "$run/violated.txt" >"$run/violated-names.txt" || status=99
  violated=$(grep -c '' "$run/violated-names.txt")
  first=
  read -r first <"$run/violated.txt" || first=
  complete=$(grep -c -x 'Model checking completed. No error has been found.' "$run/out.txt")
}

holds() {
  tlc "$1" "$2"
  [ "$status" -eq 0 ] && [ "$complete" -eq 1 ] && [ "$violated" -eq 0 ] && [ "$distinct" -gt 0 ]
}

checked() {
  ok=yes
  holds confirm "$dir/Confirm.cfg" || ok=no
  states=$distinct
  broken=$violated
  holds rebinds "$dir/Rebinds.cfg" || ok=no
  states=$((states + distinct))
  broken=$((broken + violated))
  printf 'formal states=%s invariants=%s violated=%s\n' "$states" "$count" "$broken"
  [ "$ok" = yes ] || fail invariants
}

exported() {
  sed -n 's/^{"seed":\([0-9][0-9]*\),"count":\([0-9][0-9]*\)}$/\1 \2/p' "$dir/traces.jsonl" >"$tmp/head.txt" || fail traces
  seed=
  traces=
  read -r seed traces <"$tmp/head.txt" || fail traces
  mkdir -p "$art/export" || fail artifacts
  : >"$art/export/policy-table.tsv"
  : >"$art/export/traces.jsonl"
  mkdir "$tmp/export-tmp" || fail scratch
  TLA2TOOLS_JAR=$tmp/tla2tools.jar TMPDIR=$tmp/export-tmp sh "$dir/export.sh" --out "$art/export" --seed "$seed" \
    --count "$traces" >"$art/export.txt" 2>&1 </dev/null
  code=$?
  printf 'formal export status=%s\n' "$code"
}

table_same() {
  rows=$(grep -c '' "$1")
  table=no
  if cmp -s "$1" "$art/export/policy-table.tsv"; then
    table=yes
  fi
  printf 'formal table rows=%s same=%s\n' "$rows" "$table"
  [ "$table" = yes ]
}

traces_same() {
  same=no
  if cmp -s "$dir/traces.jsonl" "$art/export/traces.jsonl"; then
    same=yes
  fi
  printf 'formal traces count=%s same=%s\n' "$traces" "$same"
  [ "$same" = yes ]
}

control() {
  tlc control "$dir/Control.cfg"
  printf 'formal control violated=%s\n' "$violated"
  if [ "$status" -eq 0 ] || [ "$violated" -ne 1 ] || [ "$first" != P1 ]; then
    fail control
  fi
}

check() {
  locate
  stamp before
  present
  configs
  pin
  java_of
  jar
  checked
  exported
  table_same "$dir/policy-table.tsv" || fail table
  traces_same || fail traces
  control
  stamp after
  cmp -s "$tmp/stamp.before" "$tmp/stamp.after" || fail tracked
  printf 'PASS %s\n' "$name"
}

self_test() {
  label=self-test:$name
  locate
  present
  pin
  java_of
  jar
  rejected=0
  if holds fixture-control "$dir/Control.cfg"; then
    fail 'the control copy passed the check of the invariants'
  fi
  printf 'fixture control rejected=yes violated=%s\n' "$violated"
  rejected=$((rejected + 1))
  exported
  table_same "$dir/policy-table.tsv" || fail 'the export differs from the committed table'
  sed '1d' "$dir/policy-table.tsv" >"$tmp/short-table.tsv" || fail scratch
  if table_same "$tmp/short-table.tsv"; then
    fail 'the table with one row removed passed'
  fi
  printf 'fixture short-table rejected=yes\n'
  rejected=$((rejected + 1))
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $# in
  0) check ;;
  1)
    [ "$1" = --self-test ] || fail usage
    self_test
    ;;
  *) fail usage ;;
esac
