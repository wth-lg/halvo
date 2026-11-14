#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r4-trials/window/sample.sh <pid>

Reads, on macOS, the figures of the process <pid> and of its tree as the
reader of the trial app finds it (tools/r4-trials/app/system/system.mjs tree:
the processes below <pid> and the WebKit processes that started after it on
its behalf), and prints one line

  sample pid=<pid> key_bytes=<n> second_bytes=<n> helpers=<n> load_x100=<n> cpu_pct=- ncpu=<n>

and one line per process of the tree

  proc pid=<n> start=<n> cpu_ms=<n>

key_bytes: the summed resident size (ps rss). second_bytes: the summed
physical footprint (proc_pid_rusage, through footprint.swift, which the
script builds once with swiftc into a directory under TMPDIR). helpers: the processes of the tree other than
<pid>. cpu_ms: the processor time of the process so far (ps time).
load_x100: the 1-minute load average times 100 (sysctl vm.loadavg); ncpu:
the processor count (sysctl hw.ncpu).

Exit: 0 with the lines; 1 with "sample pid=<pid> found=no" when no such
process exists, or "sample pid=<pid> found=partial" when the tree changed
under three reads; 2 on a refusal.
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac
[ "$#" -eq 1 ] || refuse usage 'sample.sh takes one process id'
case "$1" in
  '' | *[!0-9]*) refuse usage 'the process id is a number' ;;
esac
system=$(uname -s) || system=
[ "$system" = Darwin ] || refuse usage 'sample.sh reads macOS; sample.ps1 reads Windows'
pid=$1
here=$(cd "$(dirname "$0")" && pwd) || exit 1
reader="$here/../app/system/system.mjs"

footprint_bin() {
  digest=$(shasum -a 256 "$here/footprint.swift" | cut -c1-16) || return 1
  dir="${TMPDIR:-/tmp}/r4-trial-footprint-$digest"
  if [ ! -x "$dir/footprint" ]; then
    mkdir -p "$dir" || return 1
    swiftc -O -o "$dir/footprint-$$" "$here/footprint.swift" >&2 || return 1
    mv "$dir/footprint-$$" "$dir/footprint" || return 1
  fi
  printf '%s\n' "$dir/footprint"
}

fp=$(footprint_bin) || {
  printf 'sample pid=%s found=partial\n' "$pid"
  exit 1
}

read_once() {
  tree=$(node "$reader" tree "$pid") || return 2
  count=$(printf '%s\n' "$tree" | sed -n 's/^tree count=\([0-9]*\) webview=[0-9]*$/\1/p')
  [ -n "$count" ] || return 2
  [ "$count" -gt 0 ] || return 3
  starts=$(printf '%s\n' "$tree" | sed -n 's/^pid=\([0-9]*\) start=\([0-9]*\)$/\1 \2/p')
  pids=$(printf '%s\n' "$starts" | awk '{ printf "%s%s", (NR > 1 ? "," : ""), $1 }')
  rows=$(ps -o pid=,rss=,time= -p "$pids") || rows=
  set --
  for p in $(printf '%s\n' "$starts" | awk '{ print $1 }'); do
    set -- "$@" "$p"
  done
  marks=$("$fp" "$@" | awk 'NF == 2 { print $2 }')
  got=$(printf '%s\n' "$rows" | awk '/[0-9]/ { n += 1 } END { print n + 0 }')
  [ "$got" -eq "$count" ] || return 4
  got=$(printf '%s\n' "$marks" | awk '/[0-9]/ { n += 1 } END { print n + 0 }')
  [ "$got" -eq "$count" ] || return 4
  return 0
}

tries=0
while :; do
  tries=$((tries + 1))
  read_once
  code=$?
  [ "$code" -eq 0 ] && break
  if [ "$code" -eq 3 ]; then
    printf 'sample pid=%s found=no\n' "$pid"
    exit 1
  fi
  if [ "$tries" -ge 3 ]; then
    printf 'sample pid=%s found=partial\n' "$pid"
    exit 1
  fi
  sleep 1
done

key=$(printf '%s\n' "$rows" | awk '{ s += $2 } END { printf "%.0f", s * 1024 }')
second=$(printf '%s\n' "$marks" | awk '{ s += $1 } END { printf "%.0f", s }')
load=$(sysctl -n vm.loadavg | awk '{ printf "%.0f", $2 * 100 }')
ncpu=$(sysctl -n hw.ncpu)
printf 'sample pid=%s key_bytes=%s second_bytes=%s helpers=%s load_x100=%s cpu_pct=- ncpu=%s\n' \
  "$pid" "$key" "$second" "$((count - 1))" "$load" "$ncpu"
printf '%s\n' "$rows" | STARTS=$starts awk '
  BEGIN {
    n = split(ENVIRON["STARTS"], list, "\n")
    for (i = 1; i <= n; i += 1) {
      split(list[i], pair, " ")
      start[pair[1]] = pair[2]
    }
  }
  NF == 3 {
    k = split($3, part, ":")
    seconds = 0
    for (i = 1; i <= k; i += 1) seconds = seconds * 60 + part[i]
    printf "proc pid=%s start=%s cpu_ms=%.0f\n", $1, start[$1], seconds * 1000
  }'
