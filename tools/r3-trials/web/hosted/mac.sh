# shellcheck shell=sh

LIMIT=${LIMIT:-420}
RUN_SH=${RUN_SH:-tools/r3-trials/web/run.sh}
recorded=""
ncall=0
code=0

alive() {
  s=$(ps -o stat= -p "$1" 2>/dev/null | tr -d ' ')
  [ -n "$s" ] || return 1
  case "$s" in Z*) return 1 ;; esac
  return 0
}

group_count() {
  ps -axo pgid= | awk -v g="$1" '$1 == g' | wc -l | tr -d ' '
}

listing() {
  if [ -d "$dir" ]; then
    echo "listing at=$1 entries=$(sudo find "$dir" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')"
    sudo find "$dir" -mindepth 1 -maxdepth 1 -exec stat -f '%Sp %Su:%Sg %z %N' {} + |
      awk -v at="$1" '{
        n = $NF; sub(/.*\//, "", n)
        if (n != "com.google.Chrome.plist" && n != "com.microsoft.Edge.plist") n = "other"
        print "listing at=" at " mode=" $1 " owner=" $2 " bytes=" $3 " name=" n }'
  else
    echo "listing at=$1 dir=absent"
  fi
}

page() {
  p=$(mktemp -d "$work/page.XXXXXX")
  perl -e 'setpgrp(0, 0); exec @ARGV or exit 127' "$1" --headless=new --use-mock-keychain \
    --user-data-dir="$p" --allow-chrome-scheme-url --no-pdf-header-footer --timeout=4000 \
    --print-to-pdf="$p.pdf" "$2" >/dev/null 2>&1 &
  bp=$!
  i=0
  while [ "$i" -lt "${PAGE_WAIT:-45}" ]; do
    [ -s "$p.pdf" ] && tail -c 64 "$p.pdf" | grep -q EOF && break
    sleep 1
    i=$((i + 1))
  done
  kill -KILL -- "-$bp" 2>/dev/null
  wait "$bp" 2>/dev/null
  sleep 1
  gl=$(group_count "$bp")
  pl=$(pgrep -f -- "$p" | wc -l | tr -d ' ')
  r=$("$work/reader" pdf "$p.pdf" "$3" "$4" "$p.txt" 2>/dev/null)
  [ -n "$r" ] || r="policy_page=no readable=no named=no named_by=none prefix_len=0 name_len=0 value_seen=no platform_count=0 near=-"
  fixed=no
  [ -s "$p.txt" ] && fixed=$("$bin" --policy-named "$3" "$p.txt" | sed -n 's/.*named=\([a-z]*\).*/\1/p')
  r=$(printf '%s\n' "$r" | sed "s/ named=\([a-z]*\) / swift_named=\1 /")
  echo "$r fixed_named=${fixed:-no} waited_s=$i pdf=$(basename "$p").pdf group_left=$gl profile_left=$pl"
}

readback() {
  t=1
  while :; do
    r=$(page "$1" chrome://policy/ "$3" "$4")
    u=chrome
    case "$r" in *"policy_page=yes"*) ;; *)
      r=$(page "$1" edge://policy/ "$3" "$4")
      u=edge
      ;;
    esac
    page_ok=no
    case "$r" in *"policy_page=yes"*) page_ok=yes ;; esac
    [ "$page_ok" = yes ] && break
    [ "$t" -ge 3 ] && break
    t=$((t + 1))
  done
  echo "policy read browser=$2 at=$5 name=$3 url=$u try=$t $r"
  pc=$(printf '%s\n' "$r" | sed -n 's/.* platform_count=\([0-9]*\).*/\1/p')
  named=$(printf '%s\n' "$r" | sed -n 's/.* fixed_named=\([a-z]*\).*/\1/p')
}

port_ready() {
  port=$1
  h=$(lsof -nP -t -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | head -1)
  if [ -z "$h" ]; then
    echo "port_check port=$port free=yes"
    return 0
  fi
  rec=no
  case " $recorded " in *" $h "*) rec=yes ;; esac
  if [ "$rec" = no ]; then
    g=$(ps -o pgid= -p "$h" 2>/dev/null | tr -d ' ')
    if [ -n "$g" ]; then
      case " $recorded " in *" $g "*) rec=yes ;; esac
    fi
  fi
  if [ "$rec" = yes ]; then
    kill -TERM "$h" 2>/dev/null
    sleep 2
    alive "$h" && kill -KILL "$h" 2>/dev/null
    sleep 1
    if [ -z "$(lsof -nP -t -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | head -1)" ]; then
      echo "port_check port=$port free=no holder_recorded=yes holder_ended=yes"
      return 0
    fi
  fi
  np=$((port + 1000))
  n=0
  while [ "$n" -lt 20 ] && lsof -nP -t -iTCP:"$np" -sTCP:LISTEN >/dev/null 2>&1; do
    np=$((np + 1000))
    n=$((n + 1))
  done
  echo "port_check port=$port free=no holder_recorded=$rec holder_ended=no moved_to=$np"
  port=$np
}

run_call() {
  cport=$1
  shift
  ncall=$((ncall + 1))
  out="$work/call-$ncall.log"
  s0=$(date +%s)
  perl -e 'setpgrp(0, 0); exec @ARGV or exit 127' sh "$RUN_SH" --candidate cdp --browser "$b" \
    --port "$cport" "$@" >"$out" 2>&1 &
  cp=$!
  recorded="$recorded $cp"
  stopped=no
  while alive "$cp"; do
    if [ $(($(date +%s) - s0)) -ge "$LIMIT" ]; then
      stopped=yes
      kill -TERM -- "-$cp" 2>/dev/null
      sleep 3
      kill -KILL -- "-$cp" 2>/dev/null
      break
    fi
    sleep 1
  done
  wait "$cp"
  e=$?
  while read -r fp; do
    recorded="$recorded $fp"
  done <<EOF
$(sed -n 's/.* fixture_pid=\([0-9][0-9]*\).*/\1/p' "$out")
EOF
  cat "$out"
  if [ "$stopped" = yes ]; then
    st=$(printf '%s\n' "$*" | sed -n 's/.*--step \([a-z_]*\).*/\1/p')
    ok=$(grep -c "^run candidate=.* step=$st .*result=ok" "$out")
    all=$(grep -c "^run candidate=.* step=$st " "$out")
    echo "call_stopped browser=${k:-} step=$st limit_s=$LIMIT runs_ok=$ok runs_done=$all group_left=$(group_count "$cp")"
  fi
  echo "call_end exit=$e seconds=$(($(date +%s) - s0)) port=$cport args=$(echo "$*" | tr ' ' '_')"
  [ "$e" -eq 0 ] || code=1
}

ctl() {
  c=$(perl -e 'alarm 300; exec @ARGV' "$bin" --org-control "$b" "$work" 2>/dev/null | tail -1)
  echo "org_control at=$1 browser=$k step=$2 ${c:-reply=no cause=none}"
}

cell() {
  b=$1
  k=$2
  dom=$3
  step=$4
  name=$5
  xml=$6
  readv=$7
  pagev=$8
  cell_port=$9
  before=$(ctl before "$step")
  echo "$before"
  case "$before" in *" reply=yes"*) cb=reply ;; *) cb=none ;; esac
  plist="$dir/$dom.plist"
  hadf=no
  [ -e "$plist" ] && hadf=yes
  sudo mkdir -p "$dir"
  printf '<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n\t<key>%s</key>\n\t%s\n</dict>\n</plist>\n' "$name" "$xml" >"$work/$dom.plist"
  lint=bad
  plutil -lint "$work/$dom.plist" >/dev/null 2>&1 && lint=ok
  sudo install -o root -g wheel -m 644 "$work/$dom.plist" "$plist"
  irc=$?
  sudo killall cfprefsd 2>/dev/null
  sleep 1
  listing plist_install
  back=$(defaults read "$dir/$dom" "$name" 2>/dev/null)
  matches=no
  [ "$back" = "$readv" ] && matches=yes
  echo "policy file browser=$k name=$name lint=$lint install_exit=$irc defaults_read_matches=$matches"
  seen=no
  readback "$b" "$k" "$name" "$pagev" before_cell
  why=policy_not_seen
  [ "$page_ok" = yes ] || why=page_unread
  [ "${ctl_ok:-no}" = yes ] || why=control_unread
  [ "$why" = policy_not_seen ] && [ "$named" = yes ] && [ -n "$pc" ] && [ "$pc" -eq $((ctl + 1)) ] && seen=yes
  echo "policy set browser=$k name=$name file_existed=$hadf owner=$(stat -f %Su:%Sg "$plist" 2>/dev/null) mode=$(stat -f %Sp "$plist" 2>/dev/null) value=$back page_named=$named page_platform=$pc control_platform=$ctl seen=$seen way=plist_install"
  if [ "$seen" = yes ]; then
    port_ready "$cell_port"
    export R3_ORG_CONTROL_BEFORE="$cb" R3_ORG_VALUE_READ="$back" R3_ORG_VALUE_WANT="$readv" R3_ORG_PAGE_NAMED="$named"
    run_call "$port" --runs 10 --rows org --round 3 --relay --step "$step"
    unset R3_ORG_CONTROL_BEFORE R3_ORG_VALUE_READ R3_ORG_VALUE_WANT R3_ORG_PAGE_NAMED
  else
    echo "notrun candidate=cdp machine=hosted-mac browser=$k step=$step cause=$why"
  fi
  sudo mv "$plist" "$work/removed-$k-$name.plist" 2>/dev/null
  sudo killall cfprefsd 2>/dev/null
  sleep 1
  if [ -e "$plist" ]; then
    echo "policy removed=no name=$name"
  else
    echo "policy removed=yes name=$name"
  fi
  ctl after "$step"
}

mac_main() {
  hdir=$1
  dir="/Library/Managed Preferences"
  [ -n "${RUNNER_TEMP:-}" ] || {
    echo "REFUSED usage: RUNNER_TEMP is not set" >&2
    exit 2
  }
  work="$RUNNER_TEMP/org"
  bin="$PWD/tools/r3-trials/web/target/$(rustc -vV | sed -n 's/^host: //p')/release/web-trial"
  mkdir -p "$work"
  echo "runner macos=$(sw_vers -productVersion)"
  if [ -d "$dir" ]; then echo "managed_dir existed=yes"; else echo "managed_dir existed=no"; fi
  s0=$(date +%s)
  swiftc -O "$hdir/reader.swift" -o "$work/reader" >"$work/swiftc.log" 2>&1
  echo "swiftc exit=$? seconds=$(($(date +%s) - s0)) error_lines=$(grep -c 'error:' "$work/swiftc.log")"
  echo "harness limit_s=$LIMIT"
  port0=18190
  for pair in "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome|chrome|com.google.Chrome" \
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge|edge|com.microsoft.Edge"; do
    b=${pair%%|*}
    rest=${pair#*|}
    k=${rest%%|*}
    dom=${rest#*|}
    [ -f "$b" ] || {
      echo "browser missing $k"
      code=1
      continue
    }
    echo "browser $k"
    readback "$b" "$k" RemoteDebuggingAllowed false control_no_policy
    ctl=${pc:-0}
    ctl_ok=$page_ok
    [ "$ctl_ok" = yes ] || {
      echo "policy control browser=$k page=no"
      code=1
    }
    [ "$named" = yes ] && {
      echo "policy control browser=$k named_with_no_file=yes"
      code=1
    }
    cell "$b" "$k" "$dom" org_debugging RemoteDebuggingAllowed '<false/>' 0 false "$port0"
    port0=$((port0 + 10))
    cell "$b" "$k" "$dom" org_proxy ProxyMode '<string>direct</string>' direct direct "$port0"
    port0=$((port0 + 10))
  done
  echo "scan pages_left=$(pgrep -f -- "$work/page." | wc -l | tr -d ' ') calls_recorded=$ncall"
  exit "$code"
}
