#!/bin/sh
# jobs: macos windows
# rule: In a tree whose formal/r3-confirm and this script are the commit's, TLC finds none of P1 to P11 violated under Confirm.cfg and Rebinds.cfg and reaches 637933 and 61728 distinct states, the five configuration files are the texts this script holds, the model exports the committed table and traces byte for byte, and each of twelve controls reports its invariant; what it does not see is in formal/r3-confirm/README.md, section The check in CI.
# lane: free
set -u
LC_ALL=C
export LC_ALL
unset CDPATH JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
name=30-r3-formal
label=$name
self_path=scripts/r3/ci.d/30-r3-formal.sh
workers=3
invariants='P1 P2 P3 P4 P5 P6 P7 P8 P9 P10 P11'
reach_want=637933+61728
seed_want=20261113
count_want=400
fetch_tries=2
fetch_connect=10
fetch_time=25
fetch_pause=5
fetch_bound=60
controls_list='P2 P3 P4 P5 P6 P7 P8 P9 P10 P11 P10-live'
control_slots=3

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

committed() {
  git -C "$repo" diff --quiet --no-ext-diff HEAD -- formal/r3-confirm "$self_path" 2>/dev/null
  code=$?
  [ "$code" -ne 1 ] || fail tracked
  [ "$code" -eq 0 ] || fail git
}

subject() {
  head_id=$(git -C "$repo" rev-parse HEAD) || fail git
  printf 'formal subject commit=%s files=formal/r3-confirm,%s same-as-commit=yes fetch-bound-seconds=%s\n' \
    "$head_id" "$self_path" "$fetch_bound"
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
  for p in $4; do
    printf '  %s\n' "$p"
  done
}

export_text() {
  printf 'CONSTANTS\n  Effects = {e1}\n  Approvals = {a1}\n  Digests = {d1}\n  MaxRebinds = 0\n'
  printf '  PaymentMayRun = FALSE\n  BeginUnguarded = FALSE\n  Seed = %s\n  Count = %s\n' "$seed_want" "$count_want"
  printf 'INIT ExportInit\nNEXT ExportNext\n'
}

configs() {
  cfg_text '{e1, e2}' 1 FALSE "$invariants" >"$tmp/Confirm.cfg" || fail scratch
  cfg_text '{e1}' 2 FALSE "$invariants" >"$tmp/Rebinds.cfg" || fail scratch
  cfg_text '{e1, e2}' 2 FALSE "$invariants" >"$tmp/Full.cfg" || fail scratch
  cfg_text '{e1, e2}' 1 TRUE "$invariants" >"$tmp/Control.cfg" || fail scratch
  export_text >"$tmp/Export.cfg" || fail scratch
  same=0
  for f in Confirm.cfg Rebinds.cfg Full.cfg Control.cfg Export.cfg; do
    if cmp -s "$tmp/$f" "$dir/$f"; then
      same=$((same + 1))
    else
      cp "$tmp/$f" "$art/want-$f" || fail artifacts
    fi
  done
  printf 'formal configs files=5 same=%s\n' "$same"
  [ "$same" -eq 5 ] || fail configs
  count=0
  for p in $invariants; do
    count=$((count + 1))
  done
  first_line=
  IFS= read -r first_line <"$dir/traces.jsonl" || fail traces
  [ "$first_line" = "{\"seed\":$seed_want,\"count\":$count_want}" ] || fail traces
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

limits_ok() {
  [ $((fetch_tries * fetch_time + (fetch_tries - 1) * fetch_pause)) -le "$fetch_bound" ] &&
    [ "$fetch_connect" -le "$fetch_time" ]
}

fetch() {
  try=1
  while [ "$try" -le "$1" ]; do
    if (cd "$tmp" && curl -q -fsS --globoff --proto =https --proto-redir =https -L --connect-timeout "$2" \
      --max-time "$3" -o tla2tools.jar "$url") 2>/dev/null; then
      return 0
    fi
    try=$((try + 1))
    [ "$try" -gt "$1" ] || sleep "$4"
  done
  return 1
}

jar() {
  limits_ok || fail limits
  if [ -n "${TLA2TOOLS_JAR:-}" ]; then
    source=given
    cp "$TLA2TOOLS_JAR" "$tmp/tla2tools.jar" 2>/dev/null || printf '' >"$tmp/tla2tools.jar"
  else
    source=fetch
    fetch "${1:-$fetch_tries}" "${2:-$fetch_connect}" "${3:-$fetch_time}" "${4:-$fetch_pause}" || printf '' >"$tmp/tla2tools.jar"
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

tlc_start() {
  run=$tmp/$1
  mkdir "$run" "$run/jtmp" || fail scratch
  cp "${3:-$dir/Confirm.tla}" "$run/Confirm.tla" || fail scratch
  cp "$2" "$run/run.cfg" || fail scratch
  date +%s >"$run/started.txt" || fail scratch
  (
    cd "$run" || exit 99
    "$java" -XX:+UseParallelGC -Xmx2g -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC -workers "${4:-$workers}" \
      -deadlock -metadir meta -config run.cfg Confirm.tla >out.raw 2>&1 </dev/null
    printf '%s\n' "$?" >status.txt
    date +%s >ended.txt
  ) &
}

tlc_read() {
  run=$tmp/$1
  status=99
  read -r status <"$run/status.txt" || status=99
  awk '
    { sub(/\r$/, "") }
    /^[0-9]+ states generated, [0-9]+ distinct states found, 0 states left on queue\.$/ { distinct = $4 }
    /^[0-9]+ states generated, [0-9]+ distinct states found, [0-9]+ states left on queue\.$/ { found = $4 }
    /^Error: Invariant [A-Za-z0-9_]+ is violated/ { if (first == "") first = $3; if (!($3 in seen)) { seen[$3] = 1; n++ } }
    $0 == "Model checking completed. No error has been found." { complete++ }
    END { printf "%d %d %d %s %d\n", distinct + 0, n + 0, complete + 0, (first == "" ? "-" : first), found + 0 }
  ' "$run/out.raw" >"$run/facts.txt" || status=99
  cp "$run/out.raw" "$art/$1.txt" || status=99
  distinct=0
  violated=0
  complete=0
  first=-
  found=0
  read -r distinct violated complete first found <"$run/facts.txt" || status=99
  began=0
  read -r began <"$run/started.txt" || began=0
  ended=0
  read -r ended <"$run/ended.txt" || ended=0
  seconds=$((ended - began))
}

tlc() {
  tlc_start "$@"
  wait "$!"
  tlc_read "$1"
}

control_prepare() {
  k=$1
  case $k in
    P10-live) invariant=P10 ;;
    *) invariant=$k ;;
  esac
  model=$tmp/model-$k.tla
  cp "${2:-$dir/Confirm.tla}" "$model" || fail scratch
  step=1
  while block "$k" "${step}old" >"$tmp/old-$k-$step.txt"; do
    block "$k" "${step}new" >"$tmp/new-$k-$step.txt" || fail control
    replaced "$model" "$tmp/old-$k-$step.txt" "$tmp/new-$k-$step.txt" || return 1
    step=$((step + 1))
  done
  [ "$step" -gt 1 ] || return 1
  cfg_text '{e1}' "${3:-2}" FALSE "$invariant" >"$tmp/control-$k.cfg" || fail scratch
}

control_report() {
  k=$1
  case $k in
    P10-live) invariant=P10 ;;
    *) invariant=$k ;;
  esac
  tlc_read "control-$k"
  printf 'formal control name=%s invariant=%s reported=%s status=%s states=%s seconds=%s\n' \
    "$k" "$invariant" "$first" "$status" "$found" "$seconds"
  [ "$status" -ne 0 ] && [ "$violated" -eq 1 ] && [ "$first" = "$invariant" ]
}

one_control() {
  control_prepare "$@" || return 1
  tlc_start "control-$1" "$tmp/control-$1.cfg" "$tmp/model-$1.tla" 1
  wait "$!"
  control_report "$1"
}

controls_wait() {
  wait
  for k in $batch; do
    if control_report "$k"; then
      reported=$((reported + 1))
    else
      wrong=yes
    fi
  done
  batch=
  slot=0
}

controls() {
  reported=1
  wrong=no
  batch=
  slot=0
  started=$(date +%s)
  for k in $controls_list; do
    if ! control_prepare "$k"; then
      printf 'formal control name=%s replaced=no\n' "$k"
      wrong=yes
      continue
    fi
    tlc_start "control-$k" "$tmp/control-$k.cfg" "$tmp/model-$k.tla" 1
    batch="$batch $k"
    slot=$((slot + 1))
    [ "$slot" -lt "$control_slots" ] || controls_wait
  done
  controls_wait
  ended=$(date +%s)
  printf 'formal controls count=12 reported=%s seconds=%s\n' "$reported" "$((ended - started))"
  [ "$wrong" = no ] || fail control
}

holds() {
  tlc "$1" "$2"
  [ "$status" -eq 0 ] && [ "$complete" -eq 1 ] && [ "$violated" -eq 0 ] && [ "$distinct" -gt 0 ]
}

checked() {
  ok=yes
  holds confirm "$dir/Confirm.cfg" || ok=no
  states=$distinct
  reach=$distinct
  broken=$violated
  holds rebinds "$dir/Rebinds.cfg" || ok=no
  states=$((states + distinct))
  reach=$reach+$distinct
  broken=$((broken + violated))
  printf 'formal states=%s invariants=%s violated=%s\n' "$states" "$count" "$broken"
  [ "$ok" = yes ] || fail invariants
  same=no
  [ "$reach" != "$reach_want" ] || same=yes
  printf 'formal reach confirm+rebinds=%s same=%s\n' "$reach" "$same"
  [ "$same" = yes ] || fail reach
}

exported() {
  mkdir -p "$art/export" || fail artifacts
  printf '' >"$art/export/policy-table.tsv" || fail artifacts
  printf '' >"$art/export/traces.jsonl" || fail artifacts
  mkdir "$tmp/export-tmp" || fail scratch
  TLA2TOOLS_JAR=$tmp/tla2tools.jar TMPDIR=$tmp/export-tmp sh "$dir/export.sh" --out "$art/export" --seed "$seed_want" \
    --count "$count_want" >"$art/export.txt" 2>&1 </dev/null
  code=$?
  printf 'formal export status=%s\n' "$code"
  [ "$code" -eq 0 ] || fail export
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
  printf 'formal traces count=%s same=%s\n' "$count_want" "$same"
  [ "$same" = yes ]
}

control() {
  tlc control "$dir/Control.cfg"
  printf 'formal control violated=%s\n' "$violated"
  if [ "$status" -eq 0 ] || [ "$violated" -ne 1 ] || [ "$first" != P1 ]; then
    fail control
  fi
}

block() {
  case $1:$2 in
    P2:1old) printf '%s\n' '         ELSE IF m.eff # x.id THEN Refuse(x, "approval")' '         ELSE IF m.dig # x.dig THEN Refuse(x, "digest")' ;;
    P2:1new) printf '%s\n' '         ELSE IF m.eff # x.id THEN Refuse(x, "approval")' ;;
    P3:1old) printf '%s\n' 'Used(a) == IF \E x \in Effects : a \in eff[x].was THEN "yes" ELSE "no"' ;;
    P3:1new) printf '%s\n' 'Used(a) == "no"' ;;
    P4:1old) printf '%s\n' '  /\ mem[e] = "begun"' '  /\ Len(wire[e]) < 2' ;;
    P4:1new) printf '%s\n' '  /\ mem[e] \in {"begun", "sent"}' '  /\ Len(wire[e]) < 2' ;;
    P5:1old) printf '%s\n' '         ELSE IF x.cls \in NoEffect THEN Refuse(x, "effect")' ;;
    P5:1new) ;;
    P6:1old | P8:1old) printf '%s\n' '  IF PaymentMayRun /\ c = "payment" THEN {Table(c, ck), "run"} ELSE {Table(c, ck)}' ;;
    P6:1new) printf '%s\n' '  IF PaymentMayRun /\ c = "payment" THEN {Table(c, ck), "run"} ELSE {Table(c, "yes")}' ;;
    P7:1old) printf '%s\n' "  /\\ cfg' = [cfg EXCEPT !.st = IF cfg.allow \\subseteq Members(Allowable) THEN \"ok\" ELSE \"malformed\"]" ;;
    P7:1new) printf '%s\n' "  /\\ cfg' = [cfg EXCEPT !.st = IF TRUE THEN \"ok\" ELSE \"malformed\"]" ;;
    P8:1new) printf '%s\n' '  IF PaymentMayRun /\ c = "payment" THEN {Table(c, ck), "run"} ELSE IF Table(c, ck) = "confirm" /\ c \notin ApprovalClasses THEN {"run"} ELSE {Table(c, ck)}' ;;
    P9:1old) printf '%s\n' '  /\ mem[e] = "replied"' '  /\ Plain(e, Bare("commit"))' ;;
    P9:1new) printf '%s\n' '  /\ Plain(e, Bare("commit"))' ;;
    P10:1old) printf '%s\n' 'Lose(e) ==' '  /\ alive = "no"' ;;
    P10:1new) printf '%s\n' 'Lose(e) ==' ;;
    P11:1old) printf '%s\n' '  /\ UNCHANGED <<cfg, eff, val, given, wire, seen>>' ;;
    P11:1new)
      printf '%s\n' "  /\\ eff' = [e \\in Effects |-> IF eff[e].st = \"releasing\" THEN [eff[e] EXCEPT !.st = IF eff[e].apr \\in Approvals THEN \"approved\" ELSE \"cleared\"] ELSE eff[e]]" \
        '  /\ UNCHANGED <<cfg, val, given, wire, seen>>'
      ;;
    P10-live:1old) printf '%s\n' '  /\ alive = "yes"' '  /\ mem[e] = "begun"' '  /\ Len(wire[e]) < 2' ;;
    P10-live:1new) printf '%s\n' '  /\ mem[e] = "begun"' '  /\ (alive = "yes" \/ eff[e].st = "releasing")' '  /\ Len(wire[e]) < 2' ;;
    P10-live:2old) printf '%s\n' "  /\\ mem' = [e \\in Effects |-> \"idle\"]" ;;
    P10-live:2new) printf '%s\n' "  /\\ mem' = [e \\in Effects |-> IF mem[e] = \"begun\" THEN \"begun\" ELSE \"idle\"]" ;;
    *) return 1 ;;
  esac
}

replaced() {
  awk -v oldf="$2" -v newf="$3" '
    BEGIN {
      while ((getline l < oldf) > 0) o[++n] = l
      while ((getline l < newf) > 0) w[++m] = l
    }
    { t[++k] = $0 }
    END {
      hits = 0
      for (i = 1; i + n - 1 <= k; i++) {
        ok = 1
        for (j = 1; j <= n; j++) if (t[i + j - 1] != o[j]) { ok = 0; break }
        if (ok) { hits++; at = i }
      }
      if (n == 0 || hits != 1) exit 3
      for (i = 1; i < at; i++) print t[i]
      for (j = 1; j <= m; j++) print w[j]
      for (i = at + n; i <= k; i++) print t[i]
    }' "$1" >"$1.next" && mv "$1.next" "$1"
}

finish() {
  committed
  stamp after
  cmp -s "$tmp/stamp.before" "$tmp/stamp.after" || fail tracked
}

check() {
  locate
  committed
  subject
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
  controls
  finish
  printf 'PASS %s\n' "$name"
}

stub_text() {
  cat <<'EOF'
#!/bin/sh
if [ "${1:-}" = -version ]; then
  major=${STUB_MAJOR:-17}
  if [ "${STUB_MODE:-}" = tool-options ]; then
    major=11
    [ -z "${JAVA_TOOL_OPTIONS:-}" ] || major=17
  fi
  printf 'openjdk version "%s.0.1" 2026-01-20\n' "$major" >&2
  exit 0
fi
cfg=
prev=
for a in "$@"; do
  [ "$prev" != -config ] || cfg=$a
  prev=$a
done
mode=${STUB_MODE:-ok}
if [ "$cfg" = Export.cfg ]; then
  [ "$mode" != export-fails ] || exit 1
  awk 'BEGIN { FS = "\t"; OFS = " " } { $1 = $1; printf "\"T %s\"\n", $0 }' "$STUB_DIR/policy-table.tsv"
  printf '"D 304"\n'
  awk '{ gsub(/"/, "\\\""); printf "\"J %04d %s\"\n", NR - 1, $0 }' "$STUB_DIR/traces.jsonl"
  exit 0
fi
if [ "$mode" = writes ] && [ -n "${STUB_WRITE:-}" ]; then
  printf 'written by a step\n' >>"$STUB_WRITE"
fi
names=$(sed -n '/^INVARIANTS$/,$p' "$cfg" | sed -n 's/^  //p' | tr '\n' ' ')
violation() {
  printf 'Error: Invariant %s is violated.\n12 states generated, 9 distinct states found, 3 states left on queue.\n' "$1"
  exit 12
}
case $names in
  'P1 P2 P3 P4 P5 P6 P7 P8 P9 P10 P11 ')
    if grep -q 'BeginUnguarded = TRUE' "$cfg"; then
      [ "$mode" != control-other ] || violation P5
      if [ "$mode" = control-status0 ]; then
        printf 'Error: Invariant P1 is violated.\n'
        exit 0
      fi
      if [ "$mode" = control-two ]; then
        printf 'Error: Invariant P1 is violated.\nError: Invariant P5 is violated.\n'
        exit 12
      fi
      violation P1
    fi
    ;;
  *)
    one=${names% }
    [ "$mode" != controls-other ] || violation P8
    violation "$one"
    ;;
esac
distinct=61728
grep -q 'Effects = {e1, e2}' "$cfg" && distinct=637933
case $mode:$distinct in
  rebinds-violated:61728) violation P3 ;;
  rebinds-status:61728)
    printf 'Model checking completed. No error has been found.\n'
    printf '1 states generated, 61728 distinct states found, 0 states left on queue.\n'
    exit 1
    ;;
  rebinds-incomplete:61728)
    printf '1 states generated, 1 distinct states found, 0 states left on queue.\n'
    exit 0
    ;;
  rebinds-silent:61728)
    printf 'Error: Invariant P3 is violated.\nModel checking completed. No error has been found.\n'
    printf '1 states generated, 61728 distinct states found, 0 states left on queue.\n'
    exit 0
    ;;
  confirm-no-states:637933)
    printf 'Model checking completed. No error has been found.\n'
    exit 0
    ;;
  reach:61728) distinct=61727 ;;
esac
printf 'Model checking completed. No error has been found.\n'
printf '%s states generated, %s distinct states found, 0 states left on queue.\n' "$distinct" "$distinct"
exit 0
EOF
}

listener_text() {
  cat <<'EOF'
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.SocketTimeoutException;
import java.util.ArrayList;
import java.util.List;

public class Silent {
  public static void main(String[] args) throws Exception {
    ServerSocket server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
    System.out.println(server.getLocalPort());
    System.out.flush();
    long end = System.currentTimeMillis() + Long.parseLong(args[0]) * 1000;
    List<Socket> held = new ArrayList<>();
    server.setSoTimeout(200);
    while (System.currentTimeMillis() < end) {
      try {
        held.add(server.accept());
      } catch (SocketTimeoutException e) {
      }
    }
  }
}
EOF
}

fx_commit() {
  git -C "$fx" add -A || fail selftest
  tree=$(git -C "$fx" write-tree) || fail selftest
  printf 'tree %s\n%sauthor fixture <fixture@example.invalid> 1 +0000\ncommitter fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' \
    "$tree" "${1:+parent $1
}" >"$tmp/commit.txt" || fail selftest
  commit=$(git -C "$fx" hash-object -t commit -w --stdin <"$tmp/commit.txt") || fail selftest
  git -C "$fx" update-ref HEAD "$commit" || fail selftest
}

fx_reset() {
  git -C "$fx" update-ref HEAD "$base" || fail selftest
  git -C "$fx" reset -q --hard || fail selftest
}

fx_make() {
  fx=$tmp/fixture-repository
  mkdir -p "$fx/formal/r3-confirm" "$fx/${self_path%/*}" || fail selftest
  git -C "$fx" init -q || fail selftest
  git -C "$fx" config core.autocrlf false || fail selftest
  for f in .gitattributes Confirm.tla Confirm.cfg Rebinds.cfg Full.cfg Control.cfg Export.cfg export.sh README.md policy-table.tsv traces.jsonl; do
    cp "$dir/$f" "$fx/formal/r3-confirm/$f" || fail selftest
  done
  cp "$repo/$self_path" "$fx/$self_path" || fail selftest
  printf 'a tracked file outside formal/r3-confirm\n' >"$fx/outside.txt" || fail selftest
  fx_commit ''
  base=$commit
  mkdir "$tmp/stub" "$tmp/stub/bin" "$tmp/nojava" || fail selftest
  stub_text >"$tmp/stub/bin/java" || fail selftest
  chmod +x "$tmp/stub/bin/java" || fail selftest
}

expect() {
  want=$1
  shift
  (
    label=$name
    CI_D_REPO=$fx
    CI_D_ARTIFACTS=$tmp/fixture-artifacts
    JAVA_HOME=$tmp/stub
    STUB_DIR=$dir
    TLA2TOOLS_JAR=${fixture_jar:-$tmp/tla2tools.jar}
    export CI_D_REPO CI_D_ARTIFACTS JAVA_HOME STUB_DIR TLA2TOOLS_JAR
    for assignment in "$@"; do
      export "${assignment?}"
    done
    check
  ) >"$tmp/fixture.out" 2>&1
  last=
  java_ran=no
  while IFS= read -r line; do
    last=$line
    case $line in
      'formal java '*) java_ran=yes ;;
    esac
  done <"$tmp/fixture.out"
  if [ "$want:$java_ran" = tracked:yes ] && [ "${fixture%-outside}" != step-writes ]; then
    cp "$tmp/fixture.out" "$art/fixture-$fixture.txt" || fail selftest
    fail "fixture $fixture looked for java before the cause tracked"
  fi
  if [ "$last" = "FAIL $name ($want)" ]; then
    printf 'fixture %s cause=%s rejected=yes\n' "$fixture" "$want"
    rejected=$((rejected + 1))
  else
    cp "$tmp/fixture.out" "$art/fixture-$fixture.txt" || fail selftest
    fail "fixture $fixture ended with: $last"
  fi
}

fixture_file() {
  fixture=$1
  case $1 in
    no-rebinds-cfg) mv "$fx/formal/r3-confirm/Rebinds.cfg" "$tmp/Rebinds.cfg.out" ;;
    cfg-one-invariant-fewer) sed '/^  P11$/d' "$dir/Confirm.cfg" >"$fx/formal/r3-confirm/Confirm.cfg" ;;
    export-begin-unguarded) sed 's/^  BeginUnguarded = FALSE$/  BeginUnguarded = TRUE/' "$dir/Export.cfg" >"$fx/formal/r3-confirm/Export.cfg" ;;
    export-override) printf 'CONSTANT Decide <- AltDecide\n' >>"$fx/formal/r3-confirm/Export.cfg" ;;
    export-seed) sed "s/^  Seed = $seed_want\$/  Seed = 1/" "$dir/Export.cfg" >"$fx/formal/r3-confirm/Export.cfg" ;;
    traces-count-399) sed "1s/.*/{\"seed\":$seed_want,\"count\":399}/" "$dir/traces.jsonl" >"$fx/formal/r3-confirm/traces.jsonl" ;;
    pin-http) sed 's|^\(tla2tools\.jar release=[^ ]* url=\)https:|\1http:|' "$dir/README.md" >"$fx/formal/r3-confirm/README.md" ;;
    pin-second-line) printf 'tla2tools.jar of another form\n' >>"$fx/formal/r3-confirm/README.md" ;;
    pin-size-no-number) sed 's|^\(tla2tools\.jar .*\) bytes=[0-9]* |\1 bytes=size |' "$dir/README.md" >"$fx/formal/r3-confirm/README.md" ;;
    table-one-row-fewer) sed '1d' "$dir/policy-table.tsv" >"$fx/formal/r3-confirm/policy-table.tsv" ;;
    traces-one-byte) sed '2s/"bound":"d1"/"bound":"d2"/' "$dir/traces.jsonl" >"$fx/formal/r3-confirm/traces.jsonl" ;;
    control-no-match) sed 's/^Used(a) == IF /Used(a) ==  IF /' "$dir/Confirm.tla" >"$fx/formal/r3-confirm/Confirm.tla" ;;
    *) return 1 ;;
  esac
}

committed_fixture() {
  fixture_file "$1" || fail selftest
  fx_commit "$base"
  shift
  expect "$@"
  fx_reset
  [ ! -f "$tmp/Rebinds.cfg.out" ] || mv "$tmp/Rebinds.cfg.out" "$fx/formal/r3-confirm/Rebinds.cfg" || fail selftest
  fx_reset
}

silent_fetch() {
  fixture=silent-address
  listener_text >"$tmp/Silent.java" || fail selftest
  "$java" "$tmp/Silent.java" 30 >"$tmp/port.raw" 2>"$tmp/listener.err" </dev/null &
  listener=$!
  port=
  waited=0
  while [ -z "$port" ] && [ "$waited" -lt 40 ]; do
    sleep 1
    waited=$((waited + 1))
    tr -d '\r' <"$tmp/port.raw" >"$tmp/port.txt" || fail selftest
    port=
    read -r port <"$tmp/port.txt" || port=
  done
  cp "$tmp/tla2tools.jar" "$tmp/tla2tools.keep" || fail scratch
  started=$(date +%s)
  (
    label=$name
    unset TLA2TOOLS_JAR
    url=https://127.0.0.1:$port/tla2tools.jar
    jar 2 1 2 1
  ) >"$tmp/fixture.out" 2>&1
  ended=$(date +%s)
  cp "$tmp/tla2tools.keep" "$tmp/tla2tools.jar" || fail scratch
  kill "$listener" 2>/dev/null
  wait "$listener" 2>/dev/null
  last=
  while IFS= read -r line; do
    last=$line
  done <"$tmp/fixture.out"
  [ -n "$port" ] || fail 'the silent listener gave no port'
  [ "$last" = "FAIL $name (jar)" ] || fail "fixture silent-address ended with: $last"
  [ $((ended - started)) -le 8 ] || fail "fixture silent-address took $((ended - started)) s"
  printf 'fixture %s cause=jar seconds=%s bound=5 rejected=yes\n' "$fixture" "$((ended - started))"
  rejected=$((rejected + 1))
}

self_test() {
  label=self-test:$name
  locate
  committed
  subject
  present
  pin
  java_of
  jar
  real_java=$java
  limits_ok || fail "the limits of the fetch sum to more than $fetch_bound s"
  printf 'self-test limits seconds=%s bound=%s\n' "$((fetch_tries * fetch_time + (fetch_tries - 1) * fetch_pause))" "$fetch_bound"
  rejected=0
  fixture=control-copy
  if holds fixture-control "$dir/Control.cfg"; then
    fail 'the control copy passed the check of the invariants'
  fi
  printf 'fixture %s cause=invariants violated=%s rejected=yes\n' "$fixture" "$violated"
  rejected=$((rejected + 1))
  exported
  table_same "$dir/policy-table.tsv" || fail 'the export differs from the committed table'
  sed '1d' "$dir/policy-table.tsv" >"$tmp/short-table.tsv" || fail scratch
  fixture=short-table
  if table_same "$tmp/short-table.tsv"; then
    fail 'the table with one row removed passed'
  fi
  printf 'fixture %s cause=table rejected=yes\n' "$fixture"
  rejected=$((rejected + 1))
  fixture=p5-true
  sed '/^P5 ==$/,/^$/c\
P5 == TRUE\
' "$dir/Confirm.tla" >"$tmp/p5-true.tla" || fail scratch
  if one_control P5 "$tmp/p5-true.tla" 0; then
    fail 'the control of P5 reported P5 with P5 replaced by TRUE'
  fi
  printf 'fixture %s cause=control rejected=yes\n' "$fixture"
  rejected=$((rejected + 1))
  silent_fetch
  fx_make
  fixture=unchanged
  (
    label=$name
    CI_D_REPO=$fx
    CI_D_ARTIFACTS=$tmp/fixture-artifacts
    JAVA_HOME=$tmp/stub
    STUB_DIR=$dir
    TLA2TOOLS_JAR=$tmp/tla2tools.jar
    export CI_D_REPO CI_D_ARTIFACTS JAVA_HOME STUB_DIR TLA2TOOLS_JAR
    check
  ) >"$tmp/fixture.out" 2>&1 || {
    cp "$tmp/fixture.out" "$art/fixture-unchanged.txt"
    fail 'the unchanged fixture repository did not pass with the stand-in java'
  }
  printf 'fixture unchanged passed=yes\n'
  fixture=tracked-unstaged
  printf '\\* changed\n' >>"$fx/formal/r3-confirm/Confirm.tla" || fail selftest
  expect tracked
  fx_reset
  fixture=tracked-staged
  printf '\\* changed\n' >>"$fx/formal/r3-confirm/Export.cfg" || fail selftest
  git -C "$fx" add formal/r3-confirm/Export.cfg || fail selftest
  expect tracked
  fx_reset
  fixture=tracked-script
  printf '# changed\n' >>"$fx/$self_path" || fail selftest
  expect tracked
  fx_reset
  committed_fixture no-rebinds-cfg 'missing formal/r3-confirm/Rebinds.cfg'
  committed_fixture cfg-one-invariant-fewer configs
  committed_fixture export-begin-unguarded configs
  committed_fixture export-override configs
  committed_fixture export-seed configs
  committed_fixture traces-count-399 traces
  committed_fixture pin-http pin
  committed_fixture pin-second-line pin
  committed_fixture pin-size-no-number pin
  fixture='java-11'
  expect java STUB_MAJOR=11
  fixture='java-none'
  expect java "JAVA_HOME=$tmp/nojava"
  fixture='java-tool-options'
  (
    CI_D_REPO=$fx
    CI_D_ARTIFACTS=$tmp/fixture-artifacts
    JAVA_HOME=$tmp/stub
    STUB_MODE=tool-options
    JAVA_TOOL_OPTIONS=-Dfixture=yes
    export CI_D_REPO CI_D_ARTIFACTS JAVA_HOME STUB_MODE JAVA_TOOL_OPTIONS
    sh "$fx/$self_path"
  ) >"$tmp/fixture.out" 2>&1
  last=
  while IFS= read -r line; do
    last=$line
  done <"$tmp/fixture.out"
  [ "$last" = "FAIL $name (java)" ] || fail "fixture java-tool-options ended with: $last"
  printf 'fixture %s cause=java rejected=yes\n' "$fixture"
  rejected=$((rejected + 1))
  fixture=jar-one-bit
  od -An -v -tu1 -N1 "$tmp/tla2tools.jar" >"$tmp/first-byte.txt" || fail selftest
  read -r byte <"$tmp/first-byte.txt" || fail selftest
  octal=$(printf '%03o' "$((byte ^ 1))") || fail selftest
  printf '%b' "\\0$octal" >"$tmp/bit.jar" || fail selftest
  tail -c +2 "$tmp/tla2tools.jar" >>"$tmp/bit.jar" || fail selftest
  fixture_jar=$tmp/bit.jar
  expect jar
  fixture_jar=
  fixture=rebinds-violated
  expect invariants STUB_MODE=rebinds-violated
  fixture=rebinds-incomplete
  expect invariants STUB_MODE=rebinds-incomplete
  fixture=rebinds-status
  expect invariants STUB_MODE=rebinds-status
  fixture=rebinds-silent-violation
  expect invariants STUB_MODE=rebinds-silent
  fixture=confirm-no-states
  expect invariants STUB_MODE=confirm-no-states
  fixture=reach
  expect reach STUB_MODE=reach
  fixture=export-fails
  expect export STUB_MODE=export-fails
  fixture=export-truncation
  mkdir -p "$tmp/truncation/export/policy-table.tsv" || fail selftest
  (
    art=$tmp/truncation
    label=$name
    exported
  ) >"$tmp/fixture.out" 2>&1
  last=
  while IFS= read -r line; do
    last=$line
  done <"$tmp/fixture.out"
  [ "$last" = "FAIL $name (artifacts)" ] || fail "fixture export-truncation ended with: $last"
  printf 'fixture %s cause=artifacts rejected=yes\n' "$fixture"
  rejected=$((rejected + 1))
  committed_fixture table-one-row-fewer table
  committed_fixture traces-one-byte traces
  fixture=control-other
  expect control STUB_MODE=control-other
  fixture=control-status0
  expect control STUB_MODE=control-status0
  fixture=control-two
  expect control STUB_MODE=control-two
  fixture=controls-other
  expect control STUB_MODE=controls-other
  committed_fixture control-no-match control
  fixture='step-writes'
  expect tracked STUB_MODE=writes "STUB_WRITE=$fx/formal/r3-confirm/README.md"
  fx_reset
  fixture='step-writes-outside'
  expect tracked STUB_MODE=writes "STUB_WRITE=$fx/outside.txt"
  fx_reset
  java=$real_java
  committed
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
