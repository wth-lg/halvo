#!/bin/sh
# sh scripts/egress/check.sh [--mode MODE] LISTFILE -- COMMAND [ARG...]
# sh scripts/egress/check.sh [--mode MODE] --self-test [--control-no-deny]
# Denies and logs every outbound connection and name lookup of COMMAND's process tree to a
# destination outside LISTFILE (contracts.md C26).
set -u
LC_ALL=C
export LC_ALL

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

# The constants of C26.3 and C26.7, set from docs/r2/trials/egress.md.
# default_mode PLATFORM: prints the default mode of a platform, a mode or none.
default_mode() {
  case $1 in
    macos-15) printf '%s\n' sandbox ;;
    this-mac) printf '%s\n' sandbox ;;
    windows-2025) printf '%s\n' wfp ;;
    *) printf '%s\n' none ;;
  esac
}

# not_observable PLATFORM MODE PROBE: exits 0 when the mode cannot observe the path of the probe.
not_observable() {
  case $1:$2:$3 in
    *) return 1 ;;
  esac
}

built=

mode_entry() { "${mode}_entry" "$@"; }
mode_rights() { "${mode}_rights"; }
mode_setup() { "${mode}_setup" "$@"; }
mode_launch() { "${mode}_launch" "$@"; }
mode_collect() { "${mode}_collect" "$@"; }
mode_teardown() { "${mode}_teardown" "$@"; }
mode_reap() { "${mode}_reap"; }
rights=

mode=
given=0
self_test=0
control=0
list=
while [ $# -gt 0 ]; do
  case $1 in
    --mode)
      [ $# -ge 2 ] || refuse usage "--mode needs a MODE"
      [ "$given" -eq 0 ] || refuse usage "--mode given twice"
      mode=$2
      given=1
      shift 2
      ;;
    --self-test)
      [ "$self_test" -eq 0 ] || refuse usage "--self-test given twice"
      self_test=1
      shift
      ;;
    --control-no-deny)
      [ "$control" -eq 0 ] || refuse usage "--control-no-deny given twice"
      control=1
      shift
      ;;
    --) refuse usage "-- without a LISTFILE before it" ;;
    -*) refuse usage "$1" ;;
    *)
      [ "$self_test" -eq 0 ] && [ "$control" -eq 0 ] || refuse usage "$1"
      list=$1
      shift
      [ "${1:-}" = -- ] || refuse usage "LISTFILE without -- after it"
      shift
      [ $# -gt 0 ] || refuse usage "no COMMAND after --"
      break
      ;;
  esac
done
if [ "$self_test" -eq 0 ] && [ -z "$list" ]; then
  refuse usage "no LISTFILE and no --self-test"
fi
if [ "$control" -eq 1 ] && [ "$self_test" -eq 0 ]; then
  refuse usage "--control-no-deny needs --self-test"
fi
if [ "$control" -eq 1 ] && [ "${GITHUB_ACTIONS:-}" != true ]; then
  refuse usage "--control-no-deny runs on a hosted runner only"
fi
if [ "$given" -eq 1 ]; then
  case $mode in
    sandbox | pf | wfp) ;;
    *) refuse usage "--mode $mode" ;;
  esac
fi

root=$(git rev-parse --show-toplevel 2>/dev/null) || refuse no-repository "$PWD"
[ -n "$root" ] || refuse no-repository "$PWD"
here=$root/scripts/egress

exe=
system=$(uname -s)
case $system in
  Darwin)
    if [ "${GITHUB_ACTIONS:-}" = true ]; then
      platform=macos-15
    else
      platform=this-mac
    fi
    ;;
  MINGW* | MSYS* | CYGWIN*)
    platform=windows-2025
    exe=.exe
    ;;
  *) platform=none ;;
esac

if [ -n "$list" ]; then
  [ -f "$list" ] && [ -r "$list" ] || refuse list "$list"
  size=$(wc -c <"$list") || refuse list "$list"
  entries=$(awk -v size="$((size))" -f "$here/list.awk" "$list") || {
    case $entries in
      "line "*) refuse list "$list $entries" ;;
      *) refuse list "$list" ;;
    esac
  }
fi

if [ "$given" -eq 0 ]; then
  mode=$(default_mode "$platform")
  [ "$mode" != none ] || refuse mode "no default mode on $platform"
fi
case $platform:$mode in
  this-mac:sandbox | macos-15:sandbox | windows-2025:wfp) ;;
  macos-15:pf) ;;
  this-mac:pf) refuse mode "pf runs where GITHUB_ACTIONS is true only" ;;
  *) refuse mode "$mode is no mode of $platform" ;;
esac
case " $built " in
  *" $mode "*) ;;
  *) refuse mode "$mode was not built" ;;
esac

if [ -n "$list" ]; then
  while read -r number part kind host port; do
    [ -n "$number" ] || continue
    [ "$kind" != name ] || refuse entry "$number"
    mode_entry "$part" "$kind" "$host" "$port" || refuse entry "$number"
  done <<EOF
$entries
EOF
fi

mode_rights || refuse mode "$rights"

bin=$root/target/egress/bin
runner=$bin/egress-spawn$exe
build_probe() {
  probe_out=$(sh "$here/probe.sh" none 2>&1)
  probe_code=$?
  case $probe_code:$probe_out in
    0:) ;;
    2:"REFUSED output: "*) refuse output "${probe_out#REFUSED output: }" ;;
    *) refuse output target/egress/bin ;;
  esac
}

new_run() {
  mkdir -p "$root/target/egress" || refuse output target/egress
  mode_reap
  n=1
  while ! mkdir "$root/target/egress/$n" 2>/dev/null; do
    [ -d "$root/target/egress/$n" ] || refuse output "target/egress/$n"
    n=$((n + 1))
  done
  dir=$root/target/egress/$n
  token=halvo-egress-$n-$$-$(date +%s) || refuse output "target/egress/$n"
  printf 'pid=%s\ntoken=%s\n' "$$" "$token" >"$dir/state" || refuse output "target/egress/$n/state"
  : >"$dir/denied.log" || refuse output "target/egress/$n/denied.log"
}

# first_form LISTFILE DENY COMMAND...: one run of the first form; sets run_status, run_denied and run_code.
first_form() {
  run_list=$1
  run_deny=$2
  shift 2
  new_run
  printf '%s\n' "$entries" >"$dir/entries" || refuse output "target/egress/$n/entries"
  mode_setup "$dir" "$token" "$run_deny" "$@"
  mode_launch "$dir" "$runner" run "$dir/status" "$dir/runner.pid" -- "$@"
  launched=$?
  run_status=
  if [ -f "$dir/status" ]; then
    read -r run_status <"$dir/status"
  fi
  if [ -z "$run_status" ]; then
    if [ "$launched" -gt 128 ]; then
      run_status=signal-$((launched - 128))
    else
      run_status=not-started
    fi
  fi
  mode_collect "$dir" "$token" "$run_deny"
  mode_teardown "$dir" "$token"
  printf 'done\n' >>"$dir/state"
  run_denied=$(awk 'END { print NR }' "$dir/denied.log") || refuse output "target/egress/$n/denied.log"
  printf 'egress mode=%s list=%s command=%s denied=%s log=target/egress/%s/denied.log\n' \
    "$mode" "$run_list" "$run_status" "$run_denied" "$n" >&2
  if [ "$run_status" = 0 ] && [ "$run_denied" -eq 0 ]; then
    run_code=0
  else
    run_code=1
  fi
}

if [ "$self_test" -eq 0 ]; then
  build_probe
  first_form "$list" 1 "$@"
  exit "$run_code"
fi

# want PROBE: prints the proto and the dest that a log line of the probe holds.
want() {
  case $1 in
    ipv4) printf '%s\n' 'tcp 192.0.2.1:443' ;;
    ipv6) printf '%s\n' 'tcp [2001:db8::1]:443' ;;
    dns) printf '%s\n' 'dns *' ;;
    loopback) printf '%s\n' 'tcp 127.0.0.1:47201' ;;
    udp) printf '%s\n' 'udp 192.0.2.1:9' ;;
    loopback2) printf '%s\n' 'tcp 127.0.0.1:47202' ;;
  esac
}

# classify PROBE TABLE-PROBE: reads the run that first_form just made and prints denied, not-observable or missed.
classify() {
  wanted=$(want "$1")
  if [ "$run_code" -eq 1 ] && awk -v want="$wanted" '
    function field(key,    i, n, parts) {
      n = split($0, parts, " ")
      for (i = 1; i <= n; i++) if (index(parts[i], key "=") == 1) return substr(parts[i], length(key) + 2)
      return ""
    }
    function port(dest) {
      sub(/^.*:/, "", dest)
      return dest
    }
    BEGIN { split(want, w, " ") }
    {
      proto = field("proto"); dest = field("dest"); image = field("image")
      if (proto != w[1]) next
      if (image != "-" && image != "egress-probe" && image != "egress-probe.exe") next
      if (w[2] == "*" || dest == w[2]) found = 1
      else if (index(dest, "-:") == 1 && port(dest) == port(w[2])) found = 1
    }
    END { exit found ? 0 : 1 }' "$dir/denied.log"; then
    printf '%s\n' denied
  elif [ ! -s "$dir/denied.log" ] && not_observable "$platform" "$mode" "$2"; then
    printf '%s\n' not-observable
  else
    printf '%s\n' missed
  fi
}

# empty_ok: prints ok when the run that first_form just made exited 0 with an empty log.
empty_ok() {
  if [ "$run_code" -eq 0 ] && [ ! -s "$dir/denied.log" ]; then
    printf '%s\n' ok
  else
    printf '%s\n' miss
  fi
}

listeners=
stop_listeners() {
  : >"$work/stop"
  for pid in $listeners; do
    wait "$pid"
  done
  listeners=
}

start_listener() {
  "$runner" serve "$1" "$work/stop" "$work/ready-$1" &
  listeners="$listeners $!"
  waited=0
  while [ ! -f "$work/ready-$1" ] && [ "$waited" -lt 100 ]; do
    kill -0 "$!" 2>/dev/null || break
    sleep 0.05
    waited=$((waited + 1))
  done
  if [ ! -f "$work/ready-$1" ]; then
    stop_listeners
    refuse port "127.0.0.1:$1"
  fi
}

deny=1
[ "$control" -eq 0 ] || deny=0
build_probe
work=$(mktemp -d "${TMPDIR:-/tmp}/egress-self-test.XXXXXX") || refuse output "${TMPDIR:-/tmp}"
start_listener 47201
start_listener 47202
empty=scripts/egress/lists/empty.txt
loopback=scripts/egress/lists/loopback.txt
cd "$root" || refuse output "$root"
entries=$(awk -f "$here/list.awk" "$empty") || refuse list "$empty"
if [ "$deny" -eq 1 ]; then
  lines="egress self-test mode=$mode deny=on"
else
  lines="egress self-test mode=$mode deny=off"
fi
probes=ok
for name in ipv4 ipv6 dns loopback udp; do
  first_form "$empty" "$deny" sh scripts/egress/probe.sh "$name" </dev/null >/dev/null
  result=$(classify "$name" "$name")
  [ "$result" != missed ] || probes=miss
  lines="$lines
probe name=$name result=$result"
done
first_form "$empty" "$deny" sh scripts/egress/probe.sh none </dev/null >/dev/null
quiet=$(empty_ok)
lines="$lines
quiet result=$quiet"
entries=$(awk -f "$here/list.awk" "$loopback") || refuse list "$loopback"
first_form "$loopback" "$deny" sh scripts/egress/probe.sh loopback </dev/null >/dev/null
listed=$(empty_ok)
first_form "$loopback" "$deny" sh scripts/egress/probe.sh loopback2 </dev/null >/dev/null
unlisted=$(classify loopback2 loopback)
stop_listeners
first_form "$loopback" "$deny" sh scripts/egress/probe.sh listen </dev/null >/dev/null
listen=$(empty_ok)
lines="$lines
listed result=$listed
unlisted result=$unlisted
listen result=$listen"
if [ "${GITHUB_ACTIONS:-}" != true ]; then
  lines="$lines
control result=left-out reason=not-a-hosted-runner"
fi
loop=miss
if [ "$listed" = ok ] && [ "$unlisted" != missed ] && [ "$listen" = ok ]; then
  loop=ok
fi
default=miss
trial=$root/docs/r2/trials/egress.md
constant=$(default_mode "$platform")
if [ -f "$trial" ]; then
  first=$(awk -v key="verdict egress-modes $platform: " '
    index($0, key) == 1 { rest = substr($0, length(key) + 1); sub(/[, ].*$/, "", rest); print rest; exit }' "$trial") || first=
  [ -z "$first" ] || [ "$first" != "$constant" ] || default=ok
fi
printf '%s\nchecks probes=%s quiet=%s loopback=%s default=%s\n' "$lines" "$probes" "$quiet" "$loop" "$default"
if [ "$probes" = ok ] && [ "$quiet" = ok ] && [ "$loop" = ok ] && [ "$default" = ok ]; then
  exit 0
fi
exit 1
