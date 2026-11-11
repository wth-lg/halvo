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
  head -n 20 "$out/$label.err"
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
ps1 w7-kill-runner scratch-ver/w-kill-runner.ps1
printf 'waiting 40 s outside the check, from %s\n' "$(date -u +%H:%M:%S)"
sleep 40
printf 'w7 surviving tree wrote at %s:\n' "$(date -u +%H:%M:%S)"
cat "$out/w7.txt" 2>/dev/null || printf 'none\n'
state end
