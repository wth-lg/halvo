#!/bin/sh
# jobs: windows
# lane: alone
# rule: the built app on Windows, started with --background on a data directory that is not provisioned, runs after 60 s, and the check prints the summed peak working set of that halvo.exe and of every msedgewebview2.exe below it, with no bound; a run with no halvo.exe of that directory fails
set -u
name=35-app-windows-memory
settle_s=60
end_s=10

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

# Every halvo.exe and msedgewebview2.exe of the machine, one line each: the name, the process id,
# the id of its parent, its peak working set in KiB (Win32_Process gives PeakWorkingSetSize in
# kilobytes) and its command line.
list_processes() {
  script=$work/list.ps1
  [ -f "$script" ] || cat >"$script" <<'EOF' || return 1
$ErrorActionPreference = "Stop"
$found = Get-CimInstance -ClassName Win32_Process -Filter "Name = 'halvo.exe' OR Name = 'msedgewebview2.exe'"
foreach ($p in $found) {
  [Console]::Out.WriteLine(("{0} {1} {2} {3} {4}" -f $p.Name, $p.ProcessId, $p.ParentProcessId, $p.PeakWorkingSetSize, $p.CommandLine))
}
EOF
  native=$(native_path "$script") || return 1
  powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$native" >"$work/list.raw" || return 1
  tr -d '\r' <"$work/list.raw"
}

# The verdict on the list of processes in the file $1 for the data directory whose name is $2:
# the one halvo.exe whose command line holds that name, and every msedgewebview2.exe below it by
# the ids of the parents, with the sum of their peak working sets. It holds the sum against no
# bound.
verdict() {
  sums=$(awk -v token="$2" '
    {
      n++
      kind[n] = $1
      pid[n] = $2
      ppid[n] = $3
      kb[n] = $4
      line = $0
      for (f = 1; f <= 4; f++) sub(/^[^ ]* */, "", line)
      cmd[n] = line
    }
    END {
      apps = 0
      app_kb = 0
      views = 0
      view_kb = 0
      for (i = 1; i <= n; i++) {
        if (kind[i] == "halvo.exe" && index(cmd[i], token) > 0) {
          apps++
          app_kb += kb[i]
          below[pid[i]] = 1
        }
      }
      grew = 1
      while (grew) {
        grew = 0
        for (i = 1; i <= n; i++) {
          if (kind[i] == "msedgewebview2.exe" && (ppid[i] in below) && !(pid[i] in below)) {
            below[pid[i]] = 1
            views++
            view_kb += kb[i]
            grew = 1
          }
        }
      }
      printf "%d %d %d %d\n", apps, app_kb, views, view_kb
    }' "$1") || fail "the list of processes cannot be read"
  read -r apps app_kb views view_kb <<EOF
$sums
EOF
  case $apps in
    0) fail "no halvo.exe of the scratch directory runs after $settle_s s" ;;
    1) ;;
    *) fail "$apps halvo.exe of the scratch directory run after $settle_s s, not one" ;;
  esac
  sum_kb=$((app_kb + view_kb))
  printf 'app-memory: after %s s, the peak working set of halvo.exe %s KiB and of its %s msedgewebview2.exe %s KiB, together %s KiB (%s MiB), the debug build, no bound\n' "$settle_s" "$app_kb" "$views" "$view_kb" "$sum_kb" $((sum_kb / 1024))
}

# Ends every halvo.exe of the scratch directory with the processes below it, and waits up to
# end_s for them to go.
end_app() {
  trap - EXIT
  waited=0
  while :; do
    list_processes >"$work/snapshot.end" 2>/dev/null || return 0
    pids=$(awk -v token="$token" '$1 == "halvo.exe" && index($0, token) > 0 { print $2 }' "$work/snapshot.end")
    [ -n "$pids" ] || return 0
    if [ "$waited" -ge "$end_s" ]; then
      printf 'app-memory: still running at the end: %s\n' "$pids"
      return 0
    fi
    for pid in $pids; do
      taskkill //F //T //PID "$pid" >/dev/null 2>&1
    done
    sleep 1
    waited=$((waited + 1))
  done
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  cargo build --locked -p halvo >"$artifacts/build.txt" 2>&1 || fail "cargo build --locked -p halvo failed; see build.txt"
  app=target/debug/halvo.exe
  [ -f "$app" ] || fail "no app at $app"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  dir=$(mktemp -d "$work/mem.XXXXXX") || fail "mktemp failed"
  token=${dir##*/}
  native=$(native_path "$dir") || fail "the data directory has no native path"
  trap end_app EXIT
  "$app" --background --data-dir "$native" >"$work/app.out" 2>"$work/app.err" &
  sleep "$settle_s"
  list_processes >"$work/snapshot" || fail "the list of processes cannot be read"
  cp "$work/snapshot" "$artifacts/processes.txt" || fail "the list of processes cannot be kept"
  verdict "$work/snapshot" "$token"
  end_app
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the run"
  printf 'PASS %s\n' "$name"
}

check() {
  mode=$1
  want=$2
  out=$(cat "$work/$mode.out")
  last=$(printf '%s\n' "$out" | tail -n 1)
  code=$(cat "$work/$mode.code")
  case $code:$last in
    0:app-memory:*) got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$mode" "$want" "$got"
  if [ "$got" != "$want" ]; then
    printf '%s\n' "$out" | sed "s/^/  $mode: /"
    wrong="$wrong $mode"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

# The verdict on two lists: the app of the scratch directory with two processes of the web view
# below it, beside a web view process of another parent and an app of another directory with its
# own; and a list of no process, which must fail.
self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  token=mem.Ab12Cd
  cat >"$work/good" <<'EOF'
halvo.exe 100 4 51200 C:\t\halvo.exe --background --data-dir C:\t\mem.Ab12Cd
msedgewebview2.exe 200 100 122880 "C:\w\msedgewebview2.exe" --embedded-browser-webview=1
msedgewebview2.exe 300 200 81920 "C:\w\msedgewebview2.exe" --type=renderer
msedgewebview2.exe 400 999 70000 "C:\w\msedgewebview2.exe" --type=renderer
halvo.exe 500 4 40000 C:\t\halvo.exe --background --data-dir C:\t\mem.Other1
msedgewebview2.exe 600 500 90000 "C:\w\msedgewebview2.exe" --embedded-browser-webview=1
EOF
  : >"$work/none"
  for mode in good none; do
    (verdict "$work/$mode" "$token") >"$work/$mode.out" 2>&1
    printf '%s\n' "$?" >"$work/$mode.code"
  done
  wrong=
  rejected=0
  check good PASS 'halvo.exe 51200 KiB and of its 2 msedgewebview2.exe 204800 KiB, together 256000 KiB (250 MiB)'
  check none FAIL 'no halvo.exe of the scratch directory runs after 60 s'
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
