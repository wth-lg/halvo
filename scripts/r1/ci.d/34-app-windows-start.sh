#!/bin/sh
# jobs: windows
# rule: the built app on Windows starts its capture service as its own child and nothing before the Start step, a stop holds and a kill of the app ends the child: started with --background on a data directory that is not provisioned it runs with no halvo-capture.exe of that directory after 5 s and the directory holds no store; on a provisioned directory one halvo-capture.exe of that directory runs as a child of the app with no window of its own; after halvo-capture.exe ctl stop no halvo-capture.exe of that directory starts for 10 s while its app runs; and after a kill of the app no halvo-capture.exe of its directory runs inside 12 s
# lane: alone
set -u
name=34-app-windows-start
state_file='daemon-state.json'
before_start_s=5
child_start_s=60
answer_s=30
own_answer_s=4
stop_hold_s=10
kill_end_s=12
end_s=10
poll_pause_s=0

fail() {
  [ -z "${tokens:-}" ] || end_all
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

# Every halvo.exe and halvo-capture.exe of the machine, one line each: the name, the process
# id, the id of its parent, the handle of its main window (0 for none) and its command line.
list_processes() {
  script=$work/list.ps1
  [ -f "$script" ] || cat >"$script" <<'EOF' || return 1
$ErrorActionPreference = "Stop"
$found = Get-CimInstance -ClassName Win32_Process -Filter "Name = 'halvo.exe' OR Name = 'halvo-capture.exe'"
foreach ($p in $found) {
  $w = 0
  $live = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
  if ($live) { $w = [int64]$live.MainWindowHandle }
  [Console]::Out.WriteLine(("{0} {1} {2} {3} {4}" -f $p.Name, $p.ProcessId, $p.ParentProcessId, $w, $p.CommandLine))
}
EOF
  native=$(native_path "$script") || return 1
  powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$native" >"$work/list.raw" || return 1
  tr -d '\r' <"$work/list.raw"
}

end_process() {
  taskkill //F //PID "$1" >/dev/null 2>&1
}

snapshot() {
  list_processes >"$work/snapshot" || fail "the list of processes cannot be read"
}

# The processes named $1 of the last snapshot whose command line holds $2: the id, the id of
# the parent and the window handle of each, one line each.
of() {
  while read -r kind pid ppid window rest; do
    [ "$kind" = "$1" ] || continue
    case $rest in
      *"$2"*) printf '%s %s %s\n' "$pid" "$ppid" "$window" ;;
    esac
  done <"$work/snapshot"
}

count() {
  if [ -z "$1" ]; then
    printf '0\n'
  else
    printf '%s\n' "$1" | grep -c ''
  fi
}

start_app() {
  native=$(native_path "$1") || fail "the data directory has no native path"
  "$app" --background --data-dir "$native" >"$work/$2.out" 2>"$work/$2.err" &
}

seconds_since() {
  at=$(date +%s) || return 1
  printf '%s\n' $((at - $1))
}

# Ends every process of this run's data directories and waits up to end_s for them to go.
end_all() {
  trap - EXIT
  for round in 1 2; do
    list_processes >"$work/snapshot" 2>/dev/null || return 0
    left=
    for token in ${tokens:-}; do
      apps=$(of halvo.exe "$token") || apps=
      children=$(of halvo-capture.exe "$token") || children=
      left="$left$apps
$children"
    done
    pids=$(printf '%s\n' "$left" | while read -r pid rest; do [ -n "$pid" ] && printf '%s ' "$pid"; done)
    [ -n "$pids" ] || return 0
    [ "$round" = 1 ] || printf 'app-start: still running at the end: %s\n' "$pids"
    for pid in $pids; do
      end_process "$pid"
    done
    waited=0
    while [ "$waited" -lt "$end_s" ]; do
      list_processes >"$work/snapshot" 2>/dev/null || return 0
      still=
      for token in ${tokens:-}; do
        apps=$(of halvo.exe "$token") || apps=
        children=$(of halvo-capture.exe "$token") || children=
        still="$still$apps$children"
      done
      [ -n "$still" ] || return 0
      sleep 1
      waited=$((waited + 1))
    done
  done
}

show_logs() {
  for label in a b c; do
    [ -f "$work/$label.err" ] || continue
    tail -n 20 "$work/$label.err" | LC_ALL=C tr -c '[:print:]\n' '?' | sed "s/^/app-start err $label: /"
  done
}

ctl() {
  native=$(native_path "$1") || return 1
  "$daemon" ctl --data-dir "$native" "$2" >"$work/ctl-$3-$2.out" 2>&1
}

# Of the last snapshot: one app of the directory of token $1 and one halvo-capture.exe of that
# directory, its child with no window; the directory is named $2 in the lines. It sets the_app
# and the_child.
one_child_of_the_app() {
  apps=$(of halvo.exe "$1")
  n=$(count "$apps")
  [ "$n" = 1 ] || fail "$n apps of the $2 directory run, not one"
  the_app=${apps%% *}
  children=$(of halvo-capture.exe "$1")
  n=$(count "$children")
  [ "$n" = 1 ] || fail "$n halvo-capture.exe of the $2 directory run, not one"
  read -r the_child the_parent the_window <<EOF
$children
EOF
  printf 'case %s: halvo-capture.exe %s, its parent %s, the app %s, its window %s\n' "$2" "$the_child" "$the_parent" "$the_app" "$the_window"
  [ "$the_parent" = "$the_app" ] || fail "the halvo-capture.exe of the $2 directory is no child of the app: its parent is $the_parent, the app is $the_app"
  [ "$the_window" = 0 ] || fail "the halvo-capture.exe of the $2 directory has a window of its own"
}

# The cases on the app of `$1` and the capture service beside it, with the data directories
# under `$2`: A not provisioned, B provisioned and stopped by ctl, C provisioned and its app
# killed, the three apps at once.
cases() {
  bin=$1
  work=$2
  suffix=
  [ -f "$bin/halvo.exe" ] && suffix=.exe
  app=$bin/halvo$suffix
  daemon=$bin/halvo-capture$suffix
  [ -f "$app" ] || fail "no app at $app"
  [ -f "$daemon" ] || fail "no halvo-capture$suffix beside the app"
  tokens=
  trap end_all EXIT
  dir_a=$(mktemp -d "$work/a.XXXXXX") || fail "mktemp failed"
  dir_b=$(mktemp -d "$work/b.XXXXXX") || fail "mktemp failed"
  dir_c=$(mktemp -d "$work/c.XXXXXX") || fail "mktemp failed"
  for dir in "$dir_b" "$dir_c"; do
    printf '{}\n' >"$dir/$state_file" || fail "the state file of a provisioned directory cannot be written"
  done
  token_a=${dir_a##*/}
  token_b=${dir_b##*/}
  token_c=${dir_c##*/}
  tokens="$token_a $token_b $token_c"
  started=$(date +%s) || fail "the clock cannot be read"
  start_app "$dir_a" a
  start_app "$dir_b" b
  start_app "$dir_c" c

  while :; do
    snapshot
    children_b=$(of halvo-capture.exe "$token_b")
    children_c=$(of halvo-capture.exe "$token_c")
    [ -n "$children_b" ] && [ -n "$children_c" ] && break
    after_s=$(seconds_since "$started") || fail "the clock cannot be read"
    if [ "$after_s" -ge "$child_start_s" ]; then
      show_logs
      fail "no halvo-capture.exe of each provisioned directory ran within $child_start_s s"
    fi
    sleep "$poll_pause_s"
  done
  seen_s=$(seconds_since "$started") || fail "the clock cannot be read"
  printf 'case provisioned: a halvo-capture.exe of each provisioned directory after %s s\n' "$seen_s"
  served_b=no
  served_c=no
  while :; do
    [ "$served_b" = yes ] || { ctl "$dir_b" status b && served_b=yes; }
    [ "$served_c" = yes ] || { ctl "$dir_c" status c && served_c=yes; }
    [ "$served_b$served_c" = yesyes ] && break
    after_s=$(seconds_since "$started") || fail "the clock cannot be read"
    [ "$after_s" -lt $((seen_s + answer_s)) ] || fail "ctl status of a provisioned directory got no answer within $answer_s s of its halvo-capture.exe"
    sleep "$poll_pause_s"
  done
  served_s=$(seconds_since "$started") || fail "the clock cannot be read"
  printf 'case provisioned: ctl status of each provisioned directory answered after %s s\n' "$served_s"
  snapshot
  one_child_of_the_app "$token_b" provisioned
  child_b=$the_child
  one_child_of_the_app "$token_c" "second provisioned"
  end_process "$the_app" || fail "the app of the second provisioned directory cannot be ended"
  killed=$(date +%s) || fail "the clock cannot be read"
  printf 'case kill: the app %s of the second provisioned directory is ended\n' "$the_app"

  sleep "$own_answer_s"
  ctl "$dir_b" stop b || fail "ctl stop of the provisioned directory did not exit 0"
  stopped=$(date +%s) || fail "the clock cannot be read"
  printf 'case stop: ctl stop of the provisioned directory exited 0\n'

  after_s=$(seconds_since "$started") || fail "the clock cannot be read"
  while [ "$after_s" -lt "$before_start_s" ]; do
    sleep 1
    after_s=$(seconds_since "$started") || fail "the clock cannot be read"
  done
  snapshot
  after_s=$(seconds_since "$started") || fail "the clock cannot be read"
  apps_a=$(of halvo.exe "$token_a")
  children_a=$(of halvo-capture.exe "$token_a")
  stored=none
  [ -e "$dir_a/store" ] && stored=store
  [ -e "$dir_a/$state_file" ] && stored="$stored $state_file"
  n_apps=$(count "$apps_a")
  n_children=$(count "$children_a")
  printf 'case not provisioned: after %s s %s app, %s halvo-capture.exe, in the directory: %s\n' "$after_s" "$n_apps" "$n_children" "$stored"
  if [ "$n_apps" != 1 ]; then
    show_logs
    fail "the app on the directory that is not provisioned does not run after $after_s s"
  fi
  [ -z "$children_a" ] || fail "a halvo-capture.exe of the directory that is not provisioned runs after $after_s s, before the Start step"
  [ "$stored" = none ] || fail "the directory that is not provisioned holds $stored after $after_s s"

  gone_c=
  while :; do
    snapshot
    now=$(date +%s) || fail "the clock cannot be read"
    of halvo-capture.exe "$token_b" >"$work/children-b"
    while read -r pid rest; do
      [ "$pid" = "$child_b" ] || fail "a halvo-capture.exe of the provisioned directory started again $((now - stopped)) s after the stop"
    done <"$work/children-b"
    apps_b=$(of halvo.exe "$token_b")
    [ -n "$apps_b" ] || fail "the app of the provisioned directory does not run $((now - stopped)) s after the stop"
    if [ -z "$gone_c" ]; then
      children_c=$(of halvo-capture.exe "$token_c")
      if [ -z "$children_c" ]; then
        gone_c=$((now - killed))
      elif [ $((now - killed)) -ge "$kill_end_s" ]; then
        fail "the halvo-capture.exe of the killed app still runs $((now - killed)) s after the kill"
      fi
    fi
    [ -n "$gone_c" ] && [ $((now - stopped)) -ge "$stop_hold_s" ] && break
    sleep "$poll_pause_s"
  done
  [ ! -s "$work/children-b" ] || fail "the stopped halvo-capture.exe of the provisioned directory still runs $((now - stopped)) s after the stop"
  printf 'case kill: no halvo-capture.exe of the second provisioned directory %s s after the kill\n' "$gone_c"
  printf 'case stop: no halvo-capture.exe of the provisioned directory %s s after the stop, and its app runs\n' "$((now - stopped))"
  end_all
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  built=$(date +%s) || fail "the clock cannot be read"
  cargo build --locked -p halvo -p halvo-capture >"$artifacts/build.txt" 2>&1 || fail "cargo build --locked -p halvo -p halvo-capture failed; see build.txt"
  took=$(seconds_since "$built") || fail "the clock cannot be read"
  printf 'build seconds: %s\n' "$took"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  ran=$(date +%s) || fail "the clock cannot be read"
  cases target/debug "$work"
  took=$(seconds_since "$ran") || fail "the clock cannot be read"
  printf 'cases seconds: %s\n' "$took"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the build and the cases"
  printf 'PASS %s\n' "$name"
}

# The cases on the stand-ins of the self-test, which put a line of each process that they make
# in a file of STUB_PROCS named by its process id.
fixture() {
  before_start_s=1
  child_start_s=10
  answer_s=10
  own_answer_s=1
  stop_hold_s=3
  kill_end_s=4
  end_s=3
  poll_pause_s=0.5
  list_processes() {
    for file in "$STUB_PROCS"/*; do
      [ -f "$file" ] || continue
      kill -0 "${file##*/}" 2>/dev/null && cat "$file"
    done
    return 0
  }
  end_process() {
    kill -9 "$1" 2>/dev/null
    wait "$1" 2>/dev/null
    return 0
  }
  native_path() {
    printf '%s\n' "$1"
  }
  cases "$1" "$2"
  printf 'PASS %s\n' "$name"
}

check() {
  mode=$1
  want=$2
  out=$(cat "$work/$mode.out")
  last=$(printf '%s\n' "$out" | tail -n 1)
  code=$(cat "$work/$mode.code")
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  [ ! -s "$work/$mode.left" ] || got=$got-with-processes-left
  printf 'case %s/%s want=%s got=%s\n' "$name" "$mode" "$want" "$got"
  if [ "$got" != "$want" ]; then
    printf '%s\n' "$out" | sed "s/^/  $mode: /"
    wrong="$wrong $mode"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" || exit 2
  cat >"$work/bin/halvo" <<'EOF'
#!/bin/sh
[ "$1" = --background ] && [ "$2" = --data-dir ] && [ -n "$3" ] || exit 98
dir=$3
printf 'halvo.exe %s %s 0 halvo --background --data-dir %s\n' "$$" "$PPID" "$dir" >"$STUB_PROCS/$$"
last=
child() {
  sh "$STUB_BIN/halvo-capture" --service-run --lifeline --data-dir "$dir" &
  last=$!
}
if [ -f "$dir/daemon-state.json" ]; then
  child
  [ "$STUB_MODE" = two ] && child
else
  case $STUB_MODE in
    early) child ;;
    store) mkdir "$dir/store" ;;
    dead-app) exit 0 ;;
  esac
fi
again=yes
while :; do
  sleep 1
  if [ -n "$last" ] && ! kill -0 "$last" 2>/dev/null; then
    case $STUB_MODE:$again in
      restart:yes)
        again=no
        child
        ;;
      stop-ends-app:*) exit 0 ;;
    esac
  fi
done
EOF
  cat >"$work/bin/halvo-capture" <<'EOF'
#!/bin/sh
if [ "$1" = ctl ]; then
  [ "$2" = --data-dir ] && [ -n "$3" ] || exit 98
  pids=
  for file in "$STUB_PROCS"/*; do
    [ -f "$file" ] || continue
    kill -0 "${file##*/}" 2>/dev/null || continue
    case $(cat "$file") in
      "halvo-capture.exe "*" --data-dir $3") pids="$pids ${file##*/}" ;;
    esac
  done
  [ -n "$pids" ] || exit 69
  case $4 in
    status)
      printf 'store=open\n'
      if [ "$STUB_MODE" = no-fresh-lease ]; then printf 'lease=none\n'; else printf 'lease=fresh\n'; fi
      exit 0
      ;;
    stop)
      kill $pids
      exit 0
      ;;
  esac
  exit 64
fi
[ "$1" = --service-run ] && [ "$2" = --lifeline ] && [ "$3" = --data-dir ] && [ -n "$4" ] || exit 98
parent=$PPID
window=0
[ "$STUB_MODE" = parent ] && parent=1
[ "$STUB_MODE" = window ] && window=4242
[ "$STUB_MODE" = linger ] && trap '' TERM
printf 'halvo-capture.exe %s %s %s halvo-capture.exe --service-run --lifeline --data-dir %s\n' "$$" "$parent" "$window" "$4" >"$STUB_PROCS/$$"
while [ "$STUB_MODE" = orphan ] || kill -0 "$PPID" 2>/dev/null; do
  sleep 1
done
EOF
  chmod +x "$work/bin/halvo" "$work/bin/halvo-capture" || exit 2
  modes="good early two window parent store dead-app restart stop-ends-app linger orphan no-fresh-lease"
  fixture_limit_s=120
  for mode in $modes; do
    mkdir -p "$work/$mode/procs" || exit 2
    STUB_MODE=$mode STUB_BIN=$work/bin STUB_PROCS=$work/$mode/procs sh "$self" --fixture "$work/bin" "$work/$mode" >"$work/$mode.out" 2>&1 &
    fixture=$!
    began=$(date +%s) || exit 2
    waited=0
    while kill -0 "$fixture" 2>/dev/null && [ "$waited" -lt "$fixture_limit_s" ]; do
      sleep 0.2
      waited=$(seconds_since "$began") || exit 2
    done
    if kill -0 "$fixture" 2>/dev/null; then
      kill -9 "$fixture" 2>/dev/null
      printf 'the fixture ran past %s s and was ended\n' "$fixture_limit_s" >>"$work/$mode.out"
    fi
    wait "$fixture"
    printf '%s\n' "$?" >"$work/$mode.code"
    took=$(seconds_since "$began") || exit 2
    printf 'fixture %s: %s s\n' "$mode" "$took"
    : >"$work/$mode.left"
    for file in "$work/$mode/procs"/*; do
      if [ -f "$file" ] && kill -0 "${file##*/}" 2>/dev/null; then
        printf '%s ' "${file##*/}" >>"$work/$mode.left"
        kill -9 "${file##*/}" 2>/dev/null
      fi
    done
  done
  wrong=
  rejected=0
  check good PASS 'case stop: no halvo-capture.exe of the provisioned directory'
  check early FAIL 'a halvo-capture.exe of the directory that is not provisioned runs after'
  check two FAIL '2 halvo-capture.exe of the provisioned directory run, not one)'
  check window FAIL 'the halvo-capture.exe of the provisioned directory has a window of its own)'
  check parent FAIL 'the halvo-capture.exe of the provisioned directory is no child of the app: its parent is 1'
  check store FAIL 'the directory that is not provisioned holds store after'
  check dead-app FAIL 'the app on the directory that is not provisioned does not run after'
  check restart FAIL 'a halvo-capture.exe of the provisioned directory started again'
  check stop-ends-app FAIL 'the app of the provisioned directory does not run'
  check linger FAIL 'the stopped halvo-capture.exe of the provisioned directory still runs'
  check orphan FAIL 'the halvo-capture.exe of the killed app still runs'
  check no-fresh-lease FAIL 'ctl status of the provisioned directory did not read lease=fresh within 2 s'
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --fixture) fixture "$2" "$3" ;;
  '') measure ;;
  *) exit 64 ;;
esac
