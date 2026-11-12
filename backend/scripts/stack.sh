#!/bin/sh
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
. "$ROOT/scripts/env.sh"
STACK_STATE=${STACK_STATE:-$ROOT/.stack}
STACK_READY_SECONDS=${STACK_READY_SECONDS:-60}
export STACK_STATE

usage() {
  cat >&2 <<'EOF'
The local stack: one member per file under stack.d/, each a server on a loopback port.

  sh scripts/stack.sh up          start the members that are not running; wait until each answers
  sh scripts/stack.sh down        stop every member and its child processes
  sh scripts/stack.sh status      one line per member; exit 0 when every member answers
  sh scripts/stack.sh url NAME    print a member's base URL

A member file sets `port` and defines `run`, which execs the server in the foreground on
127.0.0.1:$port. It can use ROOT, STACK_STATE and STACK_PORT_OFFSET.
STACK_STATE (default .stack) holds each member's pid file and log. STACK_PORT_OFFSET is added to
every port; scripts/env.sh takes it from the checkout's path, or from .stack/port-offset.
Every command is safe to run twice. Exit codes: 0 done, 1 a member failed, 64 bad usage.
EOF
  exit 64
}

name_of() { basename "$1" .sh | sed 's/^[0-9]*-//'; }
# shellcheck source=stack.d/20-worker.sh
port_of() { (. "$1" && printf '%s\n' "$port"); }
answers() { curl -s -o /dev/null --max-time 2 "http://127.0.0.1:$1/"; }
listeners_on() {
  lsof -nP -a -iTCP:"$1" -sTCP:LISTEN -Fpn 2> /dev/null | awk -v port="$1" '
    /^p/ { pid = substr($0, 2) }
    $0 == "n127.0.0.1:" port { at_loopback[pid] = 1; any_at_loopback = 1 }
    $0 == "n*:" port { at_every_address[pid] = 1 }
    END {
      if (any_at_loopback) { for (pid in at_loopback) print pid } else { for (pid in at_every_address) print pid }
    }'
}
started_at() { LC_ALL=C TZ=UTC0 ps -o lstart= -p "$1" 2> /dev/null || true; }

leader_pid_was_reused() {
  start_time_now=$1
  start_time_in_pid_file=$2
  [ "$start_time_now" != "$start_time_in_pid_file" ]
}

group_runs_from_this_checkout() {
  ps -axo pgid=,command= |
    awk -v group="$1" -v root="$ROOT" '$1 == group && index($0, root) { found = 1 } END { exit !found }'
}

group_pids() {
  [ -s "$STACK_STATE/$1.pid" ] || return 0
  leader=$(sed -n 1p "$STACK_STATE/$1.pid")
  started=$(sed -n 2p "$STACK_STATE/$1.pid")
  now=$(started_at "$leader")
  if [ -n "$now" ]; then
    ! leader_pid_was_reused "$now" "$started" || return 0
  else
    group_runs_from_this_checkout "$leader" || return 0
  fi
  ps -axo pid=,pgid= | awk -v group="$leader" '$2 == group { print $1 }'
}

member_listens_on() {
  {
    group_pids "$1"
    echo listeners
    listeners_on "$2"
  } | awk '$0 == "listeners" { listed = 1; next } !listed { own[$0] = 1; next } $0 in own { found = 1 } END { exit !found }'
}

start() {
  leader=$(node "$ROOT/scripts/spawn-detached.mjs" "$STACK_STATE/$2.log" sh "$ROOT/scripts/stack.sh" run "$1")
  printf '%s\n%s\n' "$leader" "$(started_at "$leader")" > "$STACK_STATE/$2.pid"
}

up() {
  mkdir -p "$STACK_STATE"
  for file in "$ROOT"/stack.d/*.sh; do
    name=$(name_of "$file")
    port=$(port_of "$file")
    if [ -n "$(group_pids "$name")" ]; then
      continue
    fi
    if answers "$port"; then
      echo "stack: $name cannot start: another process already answers on 127.0.0.1:$port. Write another multiple of 30 into .stack/port-offset (now $STACK_PORT_OFFSET)." >&2
      exit 1
    fi
    start "$file" "$name"
  done
  deadline=$(($(date +%s) + STACK_READY_SECONDS))
  for file in "$ROOT"/stack.d/*.sh; do
    name=$(name_of "$file")
    port=$(port_of "$file")
    until answers "$port"; do
      gave_up=
      if [ -z "$(group_pids "$name")" ]; then
        gave_up="ended before it answered on 127.0.0.1:$port"
      elif [ "$(date +%s)" -ge "$deadline" ]; then
        gave_up="did not answer on 127.0.0.1:$port within $STACK_READY_SECONDS s"
      fi
      if [ -n "$gave_up" ]; then
        echo "stack: $name $gave_up; the end of $STACK_STATE/$name.log:" >&2
        tail -20 "$STACK_STATE/$name.log" >&2 || true
        exit 1
      fi
      sleep 0.1
    done
    if ! member_listens_on "$name" "$port"; then
      others=$(listeners_on "$port" | tr '\n' ' ' | sed 's/ $//')
      echo "stack: $name is not up: another process answers on 127.0.0.1:$port (listening: pid ${others:-unknown}), and no process of $name listens there. Stop that process, or write another multiple of 30 into .stack/port-offset (now $STACK_PORT_OFFSET)." >&2
      exit 1
    fi
    echo "stack: $name up on 127.0.0.1:$port (pid $(sed -n 1p "$STACK_STATE/$name.pid"))"
  done
}

down() {
  failed=0
  for file in "$ROOT"/stack.d/*.sh; do
    name=$(name_of "$file")
    if [ -z "$(group_pids "$name")" ]; then
      echo "stack: $name is down"
      continue
    fi
    leader=$(sed -n 1p "$STACK_STATE/$name.pid")
    kill -s TERM -- "-$leader" 2> /dev/null || true
    tries=100
    while [ -n "$(group_pids "$name")" ] && [ "$tries" -gt 0 ]; do
      sleep 0.1
      tries=$((tries - 1))
    done
    if [ -n "$(group_pids "$name")" ]; then
      kill -s KILL -- "-$leader" 2> /dev/null || true
      sleep 1
    fi
    if [ -n "$(group_pids "$name")" ]; then
      echo "stack: $name did not stop; still running: $(group_pids "$name" | tr '\n' ' ')" >&2
      failed=1
    else
      : > "$STACK_STATE/$name.pid"
      echo "stack: $name stopped"
    fi
  done
  return "$failed"
}

status() {
  all_up=0
  for file in "$ROOT"/stack.d/*.sh; do
    name=$(name_of "$file")
    port=$(port_of "$file")
    count=$(group_pids "$name" | wc -l | tr -d ' ')
    if [ "$count" -gt 0 ] && answers "$port" && member_listens_on "$name" "$port"; then
      echo "stack: $name up on 127.0.0.1:$port (pid $(sed -n 1p "$STACK_STATE/$name.pid"), $count processes)"
    elif [ "$count" -gt 0 ] && answers "$port"; then
      echo "stack: $name running, but another process answers on 127.0.0.1:$port ($count processes)"
      all_up=1
    elif [ "$count" -gt 0 ]; then
      echo "stack: $name running but not answering on 127.0.0.1:$port ($count processes)"
      all_up=1
    else
      echo "stack: $name down (127.0.0.1:$port)"
      all_up=1
    fi
  done
  return "$all_up"
}

if ! ps -o pid= -p $$ > /dev/null 2>&1; then
  echo "stack: ps cannot run here, so the stack cannot tell which processes are its own" >&2
  exit 1
fi
if ! command -v lsof > /dev/null 2>&1; then
  echo "stack: lsof is missing, so the stack cannot tell whose process listens on a port" >&2
  exit 1
fi

case "${1:-}" in
  up) [ $# -eq 1 ] || usage; up ;;
  down) [ $# -eq 1 ] || usage; down ;;
  status) [ $# -eq 1 ] || usage; status ;;
  url)
    [ $# -eq 2 ] || usage
    for file in "$ROOT"/stack.d/*.sh; do
      if [ "$(name_of "$file")" = "$2" ]; then
        echo "http://127.0.0.1:$(port_of "$file")"
        exit 0
      fi
    done
    echo "stack: no member named $2" >&2
    exit 64
    ;;
  run)
    [ $# -eq 2 ] || usage
    # shellcheck source=stack.d/20-worker.sh
    . "$2"
    run
    ;;
  *) usage ;;
esac
