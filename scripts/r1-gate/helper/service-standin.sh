#!/bin/sh
set -u
scenario=${STANDIN_SCENARIO:-good}
fixtures=$(cd "$(dirname "$0")/../fixtures" && pwd)
lease_s=${GATE_OVERRIDE_lease_bound_s:-10}
helper=${STANDIN_HELPER:-no-helper}

usage() {
  printf 'usage: halvo-capture --version | --timed-run [seconds] | --verify-store | --verify-deletion\n' >&2
  exit 64
}

[ "$scenario" = main ] && usage
mode=${1:-}
[ $# -gt 0 ] && shift
data=''; marker=''; span=13; count=''; from=''; to=''; verb=''
while [ $# -gt 0 ]; do
  case $1 in
    --data-dir) data=$2; shift ;;
    --marker) marker=$2; shift ;;
    --span-days) span=$2; shift ;;
    --from) from=$2; shift ;;
    --to) to=$2; shift ;;
    --lifeline) ;;
    *) if [ "$mode" = ctl ] && [ -z "$verb" ]; then verb=$1
       elif [ "$mode" = ctl ]; then count="$count $1"
       else count=$1; fi ;;
  esac
  shift
done
[ -n "$data" ] || usage
state=$data.standin
mkdir -p "$state"

put() {
  printf '%s\n' "$2" > "$1.new.$$" && mv "$1.new.$$" "$1"
}

new_id() {
  id_now=$(date +%s)
  printf '%s%s' "$id_now" "$$" | md5 -q
}

seg_of() {
  seg_hash=$(md5 -q -s "$1")
  printf '%s/store/segments/00/%s.seg\n' "$data" "$seg_hash"
}

read_id() {
  store_id=$(cat "$state/store_id")
}

layout() {
  mkdir -p "$data/store/segments/00" "$data/roots" "$data/run" "$data/logs"
  if [ ! -f "$state/store_id" ]; then
    fresh=$(new_id)
    printf '%s\n' "$fresh" > "$state/store_id"
  fi
  read_id
  [ -f "$data/store/store.key" ] || head -c 64 /dev/urandom > "$data/store/store.key"
  [ -f "$data/store/store.db" ] || head -c 4096 /dev/urandom > "$data/store/store.db"
  [ -f "$data/roots/root-$store_id" ] || head -c 32 /dev/urandom > "$data/roots/root-$store_id"
  touch "$state/events"
}

alive() {
  [ -f "$state/pid" ] || return 1
  alive_pid=$(cat "$state/pid")
  kill -0 "$alive_pid" 2>/dev/null
}

capture_state() {
  desired=$(cat "$state/desired" 2>/dev/null)
  lease=$(cat "$state/lease" 2>/dev/null)
  state_now=$(date +%s)
  if [ "$desired" != running ]; then printf 'paused\n'
  elif [ "$scenario" = lease-ignored ]; then printf 'running\n'
  elif [ "${lease:-0}" -eq 0 ] || [ "$((state_now - lease))" -ge "$lease_s" ]; then printf 'no_indicator\n'
  else printf 'running\n'; fi
}

helper_outside() {
  outside_pids=$(pgrep -f "$helper" 2>/dev/null)
  for hp in $outside_pids; do
    walk=$hp; inside=no
    while [ -n "$walk" ] && [ "$walk" -gt 1 ]; do
      [ "$walk" = "$1" ] && { inside=yes; break; }
      walk=$(ps -o ppid= -p "$walk" 2>/dev/null | tr -d ' ')
    done
    [ "$inside" = yes ] || return 0
  done
  return 1
}

expire() {
  expire_now=$(date +%s)
  cut=$(date -u -r "$((expire_now - 14 * 86400))" +%F)
  awk -v cut="$cut" '$2 < cut { print $2 }' "$state/events" | sort -u > "$state/expired-days"
  while read -r day; do
    [ "$scenario" = expiry-keeps-files ] && continue
    expired_file=$(seg_of "$day")
    rm -f "$expired_file"
  done < "$state/expired-days"
  awk -v cut="$cut" '$2 >= cut' "$state/events" > "$state/events.new" && mv "$state/events.new" "$state/events"
}

daemon() {
  [ -p /dev/stdin ] || { printf 'halvo-capture: standard input is no pipe\n' >&2; exit 64; }
  layout
  [ -f "$data/daemon-state.json" ] || printf '{"version":1,"desired":"paused","switches":{"screen_activity":false}}\n' > "$data/daemon-state.json"
  if grep -q '"desired":"running"' "$data/daemon-state.json"; then echo running; else echo paused; fi > "$state/desired"
  if grep -q '"screen_activity":true' "$data/daemon-state.json"; then screen=on; else screen=off; fi
  printf '%s\n' "$screen" > "$state/screen"
  [ "$scenario" = expiry-late ] || expire
  printf 'starting\n' > "$state/capture"
  printf '%s\n' "$$" > "$state/pid"
  started_now=$(date +%s)
  date -u -r "$started_now" +%Y-%m-%dT%H:%M:%SZ > "$state/started"
  echo 0 > "$state/lease"
  echo 0 > "$state/peak"
  : > "$data/logs/daemon.err"
  touch "$data/run/running"
  [ -S "$data/run/control.sock" ] || (cd "$data/run" && python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' control.sock)
  [ "$scenario" = no-mark ] || xattr -w com.apple.metadata:com_apple_backup_excludeItem com.apple.backupd "$data"
  root=$PPID
  leak_text=$(cat "$state/marker-text" 2>/dev/null)
  exec 3<&0
  cat <&3 > /dev/null &
  life=$!
  exec 3<&-
  peak=0
  while kill -0 "$life" 2>/dev/null && [ ! -f "$state/stop" ]; do
    capture=$(capture_state)
    late_now=$(date +%s)
    if [ "$scenario" = expiry-late ] && [ "$((late_now - started_now))" -ge "$((${GATE_OVERRIDE_expiry_bound_s:-6} + 3))" ]; then expire; fi
    if [ "$capture" = running ]; then
      t=$(date +%s); day=$(date -u +%F)
      day_file=$(seg_of "$day")
      [ -f "$day_file" ] || head -c 64 /dev/urandom > "$day_file"
      resumes=$(cat "$state/resumes" 2>/dev/null)
      if [ "$scenario" != dead-after-resume ] || [ "${resumes:-0}" -lt 2 ]; then
      printf '%s %s foreground_app 0\n%s %s window_title 0\n' "$t" "$day" "$t" "$day" >> "$state/events"
      fi
      case $scenario in
        live-leak) printf '%s\n' "$leak_text" > "$data/store/segments/live-leak.txt" ;;
        stderr-record) printf '{"kind":"keystroke","seq":7,"app":"com.example.mail"}\n' >&2 ;;
        tmp-leak) printf '%s\n' "$leak_text" > "${TMPDIR:-/tmp}/halvo-spill.txt" ;;
        name-leak) : > "$data/store/$leak_text.note" ;;
        camera-row) printf '%s %s camera 0\n' "$t" "$day" >> "$state/events" ;;
      esac
      if [ "$scenario" = dead-after-resume ] && [ "${resumes:-0}" -ge 2 ]; then :
      elif [ "$screen" = on ] && { [ "$scenario" = no-exclusion ] || helper_outside "$root"; }; then printf '%s %s screen_frame 0\n' "$t" "$day" >> "$state/events"; fi
      if [ "$scenario" = typed ]; then printf '%s %s keystroke 0\n%s %s pointer 0\n%s %s foreground_app 0\n' "$t" "$day" "$t" "$day" "$t" "$day" >> "$state/events"; fi
    fi
    printf '%s\n' "$capture" > "$state/capture.new" && mv "$state/capture.new" "$state/capture"
    rss=$(ps -o rss= -p "$$" | tr -d ' ')
    if [ "${rss:-0}" -gt "$peak" ]; then peak=$rss; put "$state/peak" "$peak"; fi
    sleep 0.2
  done
  kill "$life" 2>/dev/null
  rm -f "$data/run/running" "$state/stop"
  if [ "$scenario" = stop-leak ]; then printf '%s\n' "$leak_text" > "$data/logs/flush.txt"; printf 'flushed %s\n' "$leak_text" >&2; fi
  [ "$scenario" != exit-1-at-stop ] || exit 1
  exit 0
}

status() {
  capture=$(cat "$state/capture" 2>/dev/null)
  [ -n "$capture" ] || capture=starting
  store_word=open
  [ "$capture" != starting ] || store_word=opening
  lease=$(cat "$state/lease")
  status_now=$(date +%s)
  if [ "$lease" -eq 0 ]; then lease_word=none
  elif [ "$((status_now - lease))" -ge "$lease_s" ]; then lease_word=lapsed
  else lease_word=fresh; fi
  screen=$(cat "$state/screen")
  sources=foreground,input; screen_state=off; run_state=stopped
  [ "$capture" = running ] && run_state=running
  if [ "$screen" = on ]; then sources=foreground,screen,input; screen_state=$run_state; fi
  pid=$(cat "$state/pid"); started=$(cat "$state/started"); peak=$(cat "$state/peak"); desired=$(cat "$state/desired")
  read_id
  [ "$scenario" != wrong-pid ] || pid=$((pid + 1))
  sed -e "s/^pid=.*/pid=$pid/" \
    -e "s/^started_at=.*/started_at=$started/" \
    -e "s/^rss_peak_bytes=.*/rss_peak_bytes=$((peak * 1024 + 65536))/" \
    -e "s/^desired=.*/desired=$desired/" \
    -e "s/^capture=.*/capture=$capture/" \
    -e "s/^store=.*/store=$store_word/" \
    -e "s/^lease=.*/lease=$lease_word/" \
    -e "s/^store_id=.*/store_id=$store_id/" \
    -e "s/^enabled_sources=.*/enabled_sources=$sources/" \
    -e "s/^switch name=screen_activity value=.*/switch name=screen_activity value=$screen/" \
    -e "s/^source name=foreground state=.*/source name=foreground state=$run_state/" \
    -e "s/^source name=input state=.*/source name=input state=$run_state/" \
    -e "s/^source name=screen state=.*/source name=screen state=$screen_state/" \
    "$fixtures/c2-status-macos.txt"
}

inventory() {
  read_id
  printf 'store_id=%s\n' "$store_id"
  awk '
    BEGIN { split("screen_frame keystroke pointer foreground_app window_title system_audio microphone camera", order, " ") }
    { n[$2 " " $3]++; days[$2] = 1; total++ }
    END {
      count = 0
      for (d in days) list[++count] = d
      for (i = 1; i <= count; i++) for (j = i + 1; j <= count; j++) if (list[j] < list[i]) { x = list[i]; list[i] = list[j]; list[j] = x }
      for (i = 1; i <= count; i++) for (k = 1; k <= 8; k++) if (n[list[i] " " order[k]] > 0) printf "row day=%s modality=%s events=%d bytes=%d\n", list[i], order[k], n[list[i] " " order[k]], n[list[i] " " order[k]] * 40
      printf "total events=%d bytes=%d disk_bytes=%d\n", total, total * 40, 4096 + total * 64
    }' "$state/events"
}

segment_count() {
  find "$data/store/segments" -name '*.seg' > "$state/segments"
  wc -l < "$state/segments" | tr -d ' '
}

delete_all() {
  segments=$(segment_count)
  read_id
  rm -f "$data/roots/root-$store_id"
  rm -rf "$data/store/segments"
  mkdir -p "$data/store/segments"
  : > "$state/events"
  : > "$data/logs/daemon.err"
  fresh=$(new_id)
  put "$state/store_id" "$fresh"
  head -c 32 /dev/urandom > "$data/roots/root-$fresh"
  [ "$scenario" != delete-leaves-name ] || printf 'left\n' > "$data/store/left-behind.txt"
  printf 'deleted segments_shredded=%s keys_destroyed=%s root_destroyed=yes root_id=- files_left=0 database_left=no entries_removed=4 entries_left=0 unknown_entries=0 intent=written' "$segments" "$((segments + 1))"
}

ctl() {
  alive || { printf 'halvo-capture: capture service not running at %s/run/control.sock\n' "$data" >&2; exit 69; }
  case $verb in
    status)
      if [ "${count# }" = --indicator ]; then claimed=$(date +%s); put "$state/lease" "$claimed"; fi
      status ;;
    inventory) inventory ;;
    pause)
      put "$state/desired" paused
      perl -pi -e 's/"desired":"running"/"desired":"paused"/' "$data/daemon-state.json"
      printf 'capture=paused\n' ;;
    resume)
      put "$state/desired" running
      resumes=$(cat "$state/resumes" 2>/dev/null)
      put "$state/resumes" "$(( ${resumes:-0} + 1 ))"
      perl -pi -e 's/"desired":"paused"/"desired":"running"/' "$data/daemon-state.json"
      capture=$(capture_state)
      printf 'capture=%s\n' "$capture" ;;
    stop)
      touch "$state/stop"
      polls=0
      while alive && [ "$polls" -lt 50 ]; do polls=$((polls + 1)); sleep 0.1; done
      printf 'stopped flushed=yes events_lost=0\n' ;;
    delete-all)
      read_id
      if [ "${count# }" != "$store_id" ]; then printf 'already_deleted store_id=%s\n' "$store_id"; exit 0; fi
      delete_all
      read_id
      printf ' store_id=%s\n' "$store_id" ;;
    *) printf 'halvo-capture: unknown verb\n' >&2; exit 64 ;;
  esac
}

prefill() {
  layout
  if [ -n "$marker" ]; then md5 -q -s "$marker" > "$state/marker"; printf '%s\n' "$marker" > "$state/marker-text"; fi
  [ "$scenario" != raw-leak ] || printf 'title %s in plain text\n' "$marker" > "$data/store/raw.txt"
  t=$(date +%s); i=0
  while [ "$i" -lt "$count" ]; do
    if [ "$count" -gt 1 ]; then at=$((t - span * 86400 * (count - 1 - i) / (count - 1))); else at=$t; fi
    day=$(date -u -r "$at" +%F)
    case $((i % 5)) in 0) kind=screen_frame ;; 1) kind=keystroke ;; 2) kind=pointer ;; 3) kind=foreground_app ;; *) kind=window_title ;; esac
    printf '%s %s %s %s\n' "$at" "$day" "$kind" "${marker:+1}" >> "$state/events"
    day_file=$(seg_of "$day")
    [ -f "$day_file" ] || head -c 64 /dev/urandom > "$day_file"
    i=$((i + 1))
  done
  awk '{ print $2 }' "$state/events" | sort -u | while IFS= read -r seg_day; do
    seg_file=$(seg_of "$seg_day")
    printf 'segment day=%s file=%s\n' "$seg_day" "${seg_file#"$data"/}"
  done
  segments=$(segment_count)
  read_id
  printf 'prefilled events=%s segments=%s store_id=%s elapsed_ms=1\n' "$count" "$segments" "$store_id"
}

floor() {
  asked=''
  if [ -n "$marker" ]; then asked=$(md5 -q -s "$marker"); fi
  known=$(cat "$state/marker" 2>/dev/null)
  awk -v marker="$asked" -v known="$known" -v from="$1" -v to="$2" -v fromts="$from" -v tots="$to" '
    { n[$3]++; total++; if ($4 == "1") { app++; if ($3 == "window_title") title++ } ; if (from != "" && $1 >= from && $1 <= to) inside++ }
    END {
      printf "floor records=%d undecodable=0 forbidden_fields=0\n", total
      split("screen_frame keystroke pointer foreground_app window_title system_audio microphone camera", kinds, " ")
      for (k = 1; k <= 8; k++) printf "floor kind=%s records=%d\n", kinds[k], n[kinds[k]]
      split("printable editing navigation enter tab shortcut other", keys, " ")
      for (k = 1; k <= 7; k++) printf "floor key_class=%s records=%d\n", keys[k], (k == 1 ? n["keystroke"] : 0)
      split("click scroll drag move", points, " ")
      for (k = 1; k <= 4; k++) printf "floor pointer_class=%s records=%d folded=0\n", points[k], (k == 1 ? n["pointer"] : 0)
      printf "floor titles=%d max_title_bytes=%d sites=0\n", n["window_title"], (n["window_title"] > 0 ? 24 : 0)
      if (from != "") printf "floor interval from=%s to=%s records=%d\n", fromts, tots, inside
      if (marker == "") printf "floor marker=not_asked app=0 title=0 site=0 column=0\n"
      else if (marker == known && app > 0) printf "floor marker=found app=%d title=%d site=0 column=0\n", app, title
      else printf "floor marker=absent app=0 title=0 site=0 column=0\n"
    }' "$state/events"
}

case $mode in
  --service-run) daemon ;;
  ctl) ctl ;;
  --prefill-store) prefill ;;
  --floor-report)
    alive && { printf 'halvo-capture: the data directory is held\n' >&2; exit 75; }
    f=''; e=''
    [ -z "$from" ] || f=$(date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$from" +%s)
    [ -z "$to" ] || e=$(date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$to" +%s)
    floor "$f" "$e" ;;
  --delete-all)
    alive && { printf 'halvo-capture: the data directory is held\n' >&2; exit 75; }
    printf 'data_dir=%s\n' "$data"
    if [ ! -f "$data/store/store.key" ]; then printf 'no_store\n'; exit 0; fi
    delete_all
    printf '\n' ;;
  *) usage ;;
esac
