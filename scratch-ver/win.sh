#!/bin/sh
set -u
cd "$GITHUB_WORKSPACE" || exit 2
out=target/ver
mkdir -p "$out"
rustc --version
sh scripts/egress/probe.sh none
printf 'build exit=%s\n' "$?"
state() {
  printf '::group::machine state %s\n' "$1"
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scratch-ver/w-state.ps1
  printf '::endgroup::\n'
}
run_case() {
  label=$1
  shift
  printf '::group::case %s\n' "$label"
  printf 'start %s\n' "$(date -u +%H:%M:%S)"
  sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- "$@" >"$out/$label.out" 2>"$out/$label.err"
  code=$?
  cat "$out/$label.out"
  tail -n 3 "$out/$label.err"
  last=$(tail -n 1 "$out/$label.err")
  log=${last##*log=}
  [ -f "$log" ] && sed 's/^/  log: /' "$log"
  dir=${log%/denied.log}
  [ -f "$dir/counts" ] && sed 's/^/  counts: /' "$dir/counts"
  [ -f "$dir/rules" ] && sed 's/^/  rules: /' "$dir/rules"
  printf 'end %s\n' "$(date -u +%H:%M:%S)"
  printf '::endgroup::\n'
  printf 'case %s exit=%s | %s\n' "$label" "$code" "$last"
}
ps1() {
  label=$1
  shift
  run_case "$label" powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$@"
}
state before-any-run
ps1 w0-direct scratch-ver/connect.ps1 192.0.2.1 443
state after-one-run
ps1 w1-undo scratch-ver/w-undo.ps1
ps1 w2-child scratch-ver/w-child.ps1
ps1 w3-icmp scratch-ver/w-icmp.ps1
ps1 w4-wmi scratch-ver/w-wmi.ps1
printf 'w4 handed-off result:\n'
cat "$out/w4.txt" 2>/dev/null || printf 'none\n'
sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- powershell.exe -NoProfile -Command "Start-Sleep -Seconds 30" >"$out/w5.out" 2>"$out/w5.err" &
p=$!
sleep 15
kill -TERM "$p"
sleep 3
printf 'w5 check stopped by TERM at %s\n' "$(date -u +%H:%M:%S)"
state after-term
ps1 w6-after-term scratch-ver/connect.ps1 127.0.0.1 9
state end
