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

built="sandbox wfp"

sandbox_entry() {
  case $2:$3 in
    v4:127.0.0.1 | 'v6:[::1]') return 0 ;;
  esac
  return 1
}

sandbox_rights() {
  if [ ! -x /usr/bin/sandbox-exec ] || [ ! -x /usr/bin/log ]; then
    rights="sandbox-exec or log is missing"
    return 1
  fi
  if ! /usr/bin/sandbox-exec -p '(version 1)(allow default)' /usr/bin/true 2>/dev/null; then
    rights="sandbox-exec cannot apply a profile here"
    return 1
  fi
}

sandbox_profile() {
  printf '(version 1)\n(allow default)\n'
  [ "$2" -eq 1 ] || return 0
  printf '(deny network-outbound (with message "%s other"))\n' "$1"
  printf '(allow network-outbound (remote unix-socket))\n'
  printf '(deny network-outbound (remote unix-socket (path-literal "/private/var/run/mDNSResponder")) (with message "%s dns"))\n' "$1"
  for family in tcp4 tcp6 udp4 udp6; do
    printf '(deny network-outbound (remote %s "*:*") (with message "%s %s any"))\n' "$family" "$1" "$family"
    printf '(deny network-outbound (remote %s "localhost:*") (with message "%s %s lo"))\n' "$family" "$1" "$family"
  done
  while read -r number part kind host port; do
    [ -n "$number" ] || continue
    case $kind:$part:$host in
      v4:*) printf '(allow network-outbound (remote ip4 "localhost:%s"))\n' "$port" ;;
      v6:*) printf '(allow network-outbound (remote ip6 "localhost:%s"))\n' "$port" ;;
    esac
  done <"$dir/entries"
}

sandbox_mark() {
  tries=0
  while [ "$tries" -lt 20 ]; do
    tries=$((tries + 1))
    /usr/bin/sandbox-exec -p "(version 1)(allow default)(deny network-outbound (with message \"$2 mark-$3\"))" \
      "$runner" connect loopback2 >/dev/null 2>&1
    waited=0
    while [ "$waited" -lt 10 ]; do
      if grep -q "$2 mark-$3" "$1/stream.ndjson" 2>/dev/null; then
        return 0
      fi
      sleep 0.05
      waited=$((waited + 1))
    done
  done
  return 1
}

sandbox_setup() {
  sandbox_profile "$2" "$3" >"$1/profile.sb" || refuse output "target/egress/$n/profile.sb"
  [ "$3" -eq 1 ] || return 0
  /usr/bin/log stream --style ndjson --predicate "eventMessage CONTAINS \"$2\"" >"$1/stream.ndjson" 2>"$1/stream.err" &
  stream=$!
  started=$(ps -o lstart= -p "$stream") || started=
  printf 'stream=%s\nstarted=%s\n' "$stream" "$started" >>"$1/state" || refuse output "target/egress/$n/state"
  if ! sandbox_mark "$1" "$2" start; then
    sandbox_teardown "$1" "$2"
    refuse mode "the log stream gave no start marker"
  fi
}

sandbox_launch() {
  run_dir=$1
  shift
  /usr/bin/sandbox-exec -f "$run_dir/profile.sb" "$@"
}

sandbox_collect() {
  [ "$3" -eq 1 ] || return 0
  if ! sandbox_mark "$1" "$2" end; then
    sandbox_teardown "$1" "$2"
    refuse mode "the log stream gave no end marker"
  fi
  sandbox_teardown "$1" "$2"
  awk -v token="$2" -f "$here/sandbox.awk" "$1/stream.ndjson" >"$1/denied.log" ||
    refuse output "target/egress/$n/denied.log"
}

sandbox_teardown() {
  if [ -n "${stream:-}" ]; then
    kill "$stream" 2>/dev/null
    wait "$stream" 2>/dev/null
    stream=
  fi
}

sandbox_reap() {
  for state in "$root"/target/egress/*/state; do
    [ -f "$state" ] || continue
    if grep -q '^done$' "$state"; then
      continue
    fi
    owner=$(sed -n 's/^pid=//p' "$state") || continue
    if [ -z "$owner" ] || kill -0 "$owner" 2>/dev/null; then
      continue
    fi
    left=$(sed -n 's/^stream=//p' "$state") || left=
    if [ -n "$left" ]; then
      recorded=$(sed -n 's/^started=//p' "$state") || recorded=
      now=$(ps -o lstart= -p "$left" 2>/dev/null) || now=
      command=$(ps -o command= -p "$left" 2>/dev/null) || command=
      case $command in
        "/usr/bin/log stream"*)
          if [ -n "$now" ] && [ "$now" = "$recorded" ]; then
            kill "$left" 2>/dev/null
          fi
          ;;
      esac
    fi
    printf 'done\n' >>"$state"
  done
}

wfp_ps() {
  MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass \
    -File "$wfp_script" "$@"
}

wfp_entry() {
  case $2 in
    v4 | v6) return 0 ;;
  esac
  return 1
}

wfp_rights() {
  wfp_script=$(cygpath -w "$here/wfp.ps1") || {
    rights="cygpath is missing"
    return 1
  }
  checker=
  read -r checker <"/proc/$$/winpid" || {
    rights="the Windows process id of the check cannot be read"
    return 1
  }
  rights=$(wfp_ps -Action rights 2>&1) || return 1
  rights=
}

wfp_program() {
  found=$(command -v "$1" 2>/dev/null) || return 0
  case $found in
    /*) ;;
    *) return 0 ;;
  esac
  [ -f "$found" ] || found=$found.exe
  [ -f "$found" ] || return 0
  cygpath -w "$found"
}

wfp_setup() {
  wfp_dir=$1
  wfp_token=$2
  wfp_deny=$3
  shift 3
  {
    cygpath -w "$runner"
    cygpath -w "$bin/egress-probe.exe"
    wfp_program "$1"
  } >"$wfp_dir/programs" || refuse output "target/egress/$n/programs"
  win_dir=$(cygpath -w "$wfp_dir") || refuse output "target/egress/$n"
  out=$(wfp_ps -Action setup -Dir "$win_dir" -Token "$wfp_token" -Deny "$wfp_deny" -CheckPid "$checker" 2>&1) ||
    refuse mode "${out:-the wfp set-up failed}"
}

wfp_launch() {
  shift
  "$@"
}

wfp_collect() {
  root_pid=0
  if [ -f "$1/runner.pid" ]; then
    read -r root_pid <"$1/runner.pid" || root_pid=0
  fi
  win_dir=$(cygpath -w "$1") || refuse output "target/egress/$n"
  out=$(wfp_ps -Action collect -Dir "$win_dir" -Token "$2" -Deny "$3" -Root "$root_pid" 2>&1) || {
    wfp_teardown "$1" "$2"
    refuse mode "${out:-the wfp audit could not be read}"
  }
}

wfp_teardown() {
  win_dir=$(cygpath -w "$1") || win_dir=$1
  wfp_ps -Action teardown -Dir "$win_dir" -Token "$2" -CheckPid "$checker" >/dev/null 2>&1
}

wfp_reap() {
  return 0
}

mode_entry() {
  case $mode in
    sandbox) sandbox_entry "$@" ;;
    wfp) wfp_entry "$@" ;;
  esac
}

mode_rights() {
  case $mode in
    sandbox) sandbox_rights ;;
    wfp) wfp_rights ;;
  esac
}

mode_setup() {
  case $mode in
    sandbox) sandbox_setup "$@" ;;
    wfp) wfp_setup "$@" ;;
  esac
}

mode_launch() {
  case $mode in
    sandbox) sandbox_launch "$@" ;;
    wfp) wfp_launch "$@" ;;
  esac
}

mode_collect() {
  case $mode in
    sandbox) sandbox_collect "$@" ;;
    wfp) wfp_collect "$@" ;;
  esac
}

mode_teardown() {
  case $mode in
    sandbox) sandbox_teardown "$@" ;;
    wfp) wfp_teardown "$@" ;;
  esac
}

mode_reap() {
  case $mode in
    sandbox) sandbox_reap ;;
    wfp) wfp_reap ;;
  esac
}

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
