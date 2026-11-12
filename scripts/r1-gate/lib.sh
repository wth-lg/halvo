# shellcheck shell=sh
export LC_ALL=C
gate_root_named=${GATE_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
GATE_ROOT=$(cd "$gate_root_named" 2> /dev/null && pwd -P) || GATE_ROOT=$gate_root_named
GATE_REPO=${GATE_REPO:-wth-lg/halvo}
GATE_DURATION=${GATE_DURATION:-600}

predicate_duration() {
  case $1 in ''|*[!0-9]*|0*) return 1 ;; esac
}

if ! predicate_duration "$GATE_DURATION"; then
  printf '%s: GATE_DURATION=%s is no positive integer without a leading zero\n' "${0##*/}" "$GATE_DURATION" >&2
  exit 64
fi
GATE_TARGET=$GATE_ROOT/target/r1-gate
GATE_BIN=$GATE_TARGET/debug/halvo-capture

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

gate_bound() {
  bounds_file=${2:-$GATE_ROOT/docs/baseline/r1-gate-bounds.txt}
  bound_lines=$(grep -c -e "^$1=" -- "$bounds_file" 2>/dev/null)
  [ "$bound_lines" = 1 ] || return 1
  bound_digits=$(sed -n "s/^$1=\\([0-9][0-9]*\\)\$/\\1/p" "$bounds_file")
  [ -n "$bound_digits" ] || return 1
  printf '%s\n' "$bound_digits"
}

gate_bounds_used=''
gate_overrides=$(env | sed -n 's/^GATE_OVERRIDE_\([a-z0-9_]*\)=.*/\1/p' | sort | tr '\n' ' ')

use_bound() {
  bound_override=''
  eval "bound_override=\${GATE_OVERRIDE_$1-}"
  if [ -n "$bound_override" ]; then
    case $bound_override in *[!0-9]*) return 1 ;; esac
    bound=$bound_override
  else
    bound=$(gate_bound "$1") || return 1
  fi
  gate_bounds_used="$gate_bounds_used$1=$bound "
}

need_bound() {
  use_bound "$1" && return 0
  emit_fail "$name" "bound $1 is not readable: docs/baseline/r1-gate-bounds.txt must hold exactly one $1=digits line"
  return 1
}

emit_line() {
  line="$1 $2"
  detail=${3:-}
  if [ -n "$gate_bounds_used" ]; then detail="${detail:+$detail; }bounds: ${gate_bounds_used% }"; fi
  [ -z "$detail" ] || line="$line ($detail)"
  [ -z "$gate_overrides" ] || line="$line [override: ${gate_overrides% }; not a done-when run]"
  [ "${GATE_TREE_STATE:-clean}" = clean ] || line="$line [dirty tree; not a done-when run]"
  printf '%s\n' "$line"
}
emit_pass() { emit_line PASS "$@"; }
emit_no_input() { emit_line NO-INPUT "$@"; }

verdict_under_input() {
  if [ "$gate_input" = none ]; then
    emit_no_input "$name" "input=none, $1: $2"
    return 3
  fi
  emit_pass "$name" "input=$gate_input: $2"
}

worst_status() {
  case $2 in
    0) printf '%s\n' "$1" ;;
    3) if [ "$1" -eq 0 ]; then printf '3\n'; else printf '%s\n' "$1"; fi ;;
    *) printf '1\n' ;;
  esac
}
emit_fail() { emit_line FAIL "$@"; }

name=${name:-gate}
gate_scratch=''
gate_remove_scratch=no
gate_verbose=''
gate_input=none
gate_sha=''
gate_strict=no
gate_case=''

scratch_root() {
  [ -z "$gate_scratch" ] || return 0
  scratch_base=${TMPDIR:-/tmp}
  gate_scratch=$(mktemp -d "${scratch_base%/}/r1-gate-$name.XXXXXX") && return 0
  emit_fail "$name" "mktemp could not make a scratch directory"
  exit 1
}

child_wrapper=''
child_pid=''
helper_pid=''
watched_pid=''

watched() {
  watched_defect=''
  if ! use_bound child_deadline_extra_s; then
    watched_defect="did not start: bound child_deadline_extra_s is not readable: docs/baseline/r1-gate-bounds.txt must hold exactly one child_deadline_extra_s=digits line"
    return 1
  fi
  watched_deadline=$bound
  watched_start=$(date +%s)
  "$@" &
  watched_pid=$!
  while kill -0 "$watched_pid" 2> /dev/null; do
    watched_now=$(date +%s)
    if [ "$((watched_now - watched_start))" -ge "$watched_deadline" ]; then
      watched_defect="ran past its deadline of ${watched_deadline}s and was ended"
      kill -TERM "$watched_pid" 2> /dev/null
      watched_polls=0
      while kill -0 "$watched_pid" 2> /dev/null; do
        watched_polls=$((watched_polls + 1))
        if [ "$watched_polls" -gt 50 ]; then kill -KILL "$watched_pid" 2> /dev/null; break; fi
        sleep 0.1
      done
      break
    fi
    sleep 0.1
  done
  wait "$watched_pid"
  watched_status=$?
  watched_pid=''
  [ -z "$watched_defect" ] || return 1
  return "$watched_status"
}

stop_children() {
  if [ -n "$child_wrapper" ]; then
    if [ -n "$child_pid" ]; then kill -TERM "$child_pid" 2>/dev/null; else pkill -TERM -P "$child_wrapper" 2>/dev/null; fi
    wait "$child_wrapper" 2>/dev/null
    child_wrapper=''
  fi
  if [ -n "$watched_pid" ]; then
    kill -TERM "$watched_pid" 2>/dev/null
    wait "$watched_pid" 2>/dev/null
    watched_pid=''
  fi
  helper_stop
  service_teardown
}

gate_traps() {
  trap gate_exit EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  trap 'exit 129' HUP
}

gate_exit() {
  stop_children
  [ -n "$gate_scratch" ] || return 0
  if [ "$gate_remove_scratch" = yes ]; then
    rm -rf "$gate_scratch"
  else
    printf 'scratch %s: %s\n' "$name" "$gate_scratch" >&2
  fi
}

t() {
  st_case=$1; st_want=$2; shift 2
  if [ -n "$gate_case" ] && [ "$gate_case" != "$st_case" ]; then return 0; fi
  st_out=/dev/null
  [ -z "$gate_scratch" ] || st_out=$gate_scratch/case-$name-$st_case.out
  ( "$@" ) > "$st_out" 2>&1
  case $? in 0) st_got=PASS ;; 3) st_got=NO-INPUT ;; *) st_got=FAIL ;; esac
  st_cases=$((st_cases + 1))
  st_note=''
  if [ "$st_got" != "$st_want" ]; then
    st_wrong=$((st_wrong + 1))
    st_tail=$(awk '/^scratch / { next } { last = $0 } /share=/ { share = $0 } END { print (share != "" ? share : last) }' "$st_out" 2>/dev/null)
    st_load=$(sysctl -n vm.loadavg 2>/dev/null | awk '{ print $2 }')
    st_note=": ${st_tail:-no output}; 1-minute load=${st_load:-unread}"
  fi
  [ "$st_want" = PASS ] || st_rejected=$((st_rejected + 1))
  if [ -n "$gate_verbose" ] || [ -n "$st_note" ]; then printf 'case %s/%s want=%s got=%s%s\n' "$name" "$st_case" "$st_want" "$st_got" "$st_note"; fi
}

selftest_run() {
  st_cases=0; st_wrong=0; st_rejected=0
  cases
  if [ "$st_cases" -eq 0 ]; then emit_fail "self-test:$name" "zero cases ran"; return 1; fi
  if [ "$st_wrong" -ne 0 ]; then emit_fail "self-test:$name" "$st_wrong of $st_cases cases got another verdict than they want"; return 1; fi
  emit_pass "self-test:$name" "$st_cases cases, $st_rejected failing fixtures rejected"
}

check_main() {
  mode=measure; seconds=$GATE_DURATION
  while [ $# -gt 0 ]; do
    case $1 in
      --self-test) mode=self-test ;;
      --measure) mode=measure ;;
      -v) gate_verbose=yes ;;
      --case) shift; gate_case=${1:-} ;;
      --remove-scratch) gate_remove_scratch=yes ;;
      --strict) gate_strict=yes ;;
      --sha) shift; gate_sha=${1:-} ;;
      --input)
        shift
        case ${1:-} in
          none|operator) gate_input=$1 ;;
          *) printf 'check-%s.sh: --input takes none or operator\n' "$name" >&2; exit 64 ;;
        esac ;;
      ''|*[!0-9]*) printf 'check-%s.sh: unknown argument: %s\n' "$name" "$1" >&2; exit 64 ;;
      *) seconds=$1 ;;
    esac
    shift
  done
  gate_traps
  if [ "$mode" = self-test ]; then selftest_run; else measure "$seconds"; fi
}

dir_disk_usage_bytes() {
  [ -d "$1" ] || return 1
  usage=$(du -sk "$1" 2>/dev/null) || return 1
  usage_kib=${usage%%[!0-9]*}
  printf '%s\n' "$((usage_kib * 1024))"
}

modality_count() {
  sed -n "s/^modality=$2 count=\\([0-9][0-9]*\\)\$/\\1/p" "$1" 2>/dev/null
}

report_counts() {
  for kind in screen_frame keystroke pointer foreground_app window_title; do
    count=$(modality_count "$1" "$kind")
    printf '%s=%s ' "$kind" "${count:-absent}"
  done
}

predicate_modalities_present() {
  for required in screen_frame keystroke pointer foreground_app window_title; do
    count=$(modality_count "$1" "$required")
    case $count in ''|*[!0-9]*) return 1 ;; esac
    [ "$count" -gt 0 ] || return 1
  done
  return 0
}

predicate_timed_run_report() {
  predicate_modalities_present "$1" || return 1
  predicate_foreground_switches "$1" || return 1
  report_failed=$(sed -n 's/^failed_sources=\([0-9][0-9]*\)$/\1/p' "$1" 2>/dev/null)
  [ "$report_failed" = 0 ] || return 1
  ! grep -q -e "^failed_source=" "$1" 2>/dev/null
}

marker_scan_dir() {
  [ -n "$2" ] || return 2
  [ -f "$1/store.db" ] || return 2
  [ -f "$1/store.key" ] || return 2
  scan_links=$(find "$1" -type l 2>/dev/null) || return 2
  [ -z "$scan_links" ] || return 3
  grep -rqF -e "$2" -- "$1" 2>/dev/null
  case $? in 1) return 0 ;; 0) return 1 ;; *) return 2 ;; esac
}

marker_scan_files() {
  marker=$1; shift
  [ -n "$marker" ] || return 2
  [ $# -gt 0 ] || return 2
  for capture in "$@"; do
    grep -qF -e "$marker" -- "$capture" 2>/dev/null
    case $? in 1) ;; 0) return 1 ;; *) return 2 ;; esac
  done
  return 0
}

predicate_marker_absent() {
  marker_scan_dir "$1" "$2" || return 1
}

predicate_marker_not_printed() {
  marker_scan_files "$@"
}

marker_defect() {
  marker_scan_dir "$1" "$2"
  case $? in
    0) ;;
    1) printf 'leak: the marker is in the store directory\n'; return 1 ;;
    3) printf 'a symbolic link below the store directory: the scan does not follow links, so it cannot read what the link holds\n'; return 1 ;;
    *) printf 'marker scan did not run on the store directory: it needs a marker, store.db, store.key and files that grep can read\n'; return 1 ;;
  esac
  shift
  marker_scan_files "$@"
  case $? in
    0) return 0 ;;
    1) printf 'leak: the marker is in what the binary printed\n'; return 1 ;;
    *) printf 'marker scan did not run on the captured output: it needs a marker and both capture files\n'; return 1 ;;
  esac
}

report_has_key() {
  printf '%s\n' "$1" | tr '\t\r' '  ' | tr ' ' '\n' | grep -q -e "^$2="
}

report_values() {
  printf '%s\n' "$1" | tr '\t\r' '  ' | tr ' ' '\n' | sed -n "s/^$2=//p"
}

report_defects() {
  line=$1; shift
  for want in "$@"; do
    keys=${want%%=*}; expected=${want#*=}
    case $keys in
      \?*)
        key=${keys#\?}
        report_has_key "$line" "$key" || continue ;;
      *\|*)
        key=${keys%%|*}; other=${keys#*|}
        if report_has_key "$line" "$other"; then
          if report_has_key "$line" "$key"; then printf '%s %s ' "$key" "$other"; continue; fi
          key=$other
        fi ;;
      *) key=$keys ;;
    esac
    value=$(report_values "$line" "$key")
    [ "$value" = "$expected" ] || printf '%s ' "$key"
  done
}

store_report_defects() {
  report_defects "$1" 'rows|events=2000' '?records=2000' secure_delete=1 temp_store=2 '?fts_secure_delete=1' cipher_provider=libtomcrypt segment_roundtrip=ok unwrap=ok prompt=none
}

deletion_report_defects() {
  report_defects "$1" decryptable=no file_gone=yes sibling_readable=yes wal_bytes=0 '?expired_events_left=0' root_destroyed=yes store_sealed=yes
}

predicate_store_report() {
  defects=$(store_report_defects "$1")
  [ -z "$defects" ]
}

predicate_deletion_report() {
  defects=$(deletion_report_defects "$1")
  [ -z "$defects" ]
}

sources_defect() {
  source_line=$(grep -m 1 -E '^halvo-capture: source [^ ]+ did not start' "$2" 2>/dev/null)
  case $? in
    1) ;;
    0) source_line=${source_line#halvo-capture: }
       case $source_line in *'is not granted'*) source_line="$source_line (needs a human grant)" ;; esac
       printf '%s\n' "$source_line"; return 1 ;;
    *) printf 'the stderr of the run could not be read\n'; return 1 ;;
  esac
  source_line=$(grep -m 1 -E '^failed_source=' "$1" 2>/dev/null)
  if [ -n "$source_line" ]; then
    case $source_line in *'is not granted'*) source_line="$source_line (needs a human grant)" ;; esac
    printf '%s\n' "$source_line"; return 1
  fi
  for kind in screen_frame foreground_app; do
    count=$(modality_count "$1" "$kind")
    case $count in ''|*[!0-9]*) printf 'the report holds no one line modality=%s count\n' "$kind"; return 1 ;; esac
    [ "$count" -gt 0 ] || { printf 'modality=%s count=0: that source recorded nothing\n' "$kind"; return 1; }
  done
}

predicate_sources_ran() {
  sources_defect "$1" "$2" > /dev/null
}

input_defect() {
  input_report=$1; shift
  input_keys=$(modality_count "$input_report" keystroke)
  input_points=$(modality_count "$input_report" pointer)
  input_counts="keystroke=${input_keys:-absent} pointer=${input_points:-absent}"
  case $input_keys:$input_points in
    *[!0-9:]*|:*|*:)
      printf 'the report holds no one line modality=keystroke count and one line modality=pointer count, so it does not show the input of the run: %s\n' "$input_counts"
      return 1 ;;
  esac
  if [ "$gate_input" = none ]; then
    [ "$input_keys" -eq 0 ] && [ "$input_points" -eq 0 ] && return 0
    printf 'input reached a run that was started with --input none: %s\n' "$input_counts"
    return 1
  fi
  for input_kind in "$@"; do
    case $input_kind in keystroke) input_count=$input_keys ;; *) input_count=$input_points ;; esac
    [ "$input_count" -eq 0 ] || continue
    printf 'the operator gave no input: the report of this run holds %s under --input operator\n' "$input_counts"
    return 1
  done
}

signal_leaves_no_child() {
  command -v "$1" > /dev/null 2>&1 || return 1
  signal_root=$gate_scratch/signal-$1-$2
  mkdir -p "$signal_root" || return 1
  TMPDIR=$signal_root "$1" -c "set -- --self-test --case no-such-case; . \"\$0\" > /dev/null 2>&1; against_the_standin hang none" "$DIR/check-$name.sh" > "$signal_root.out" 2>&1 &
  signal_pid=$!
  signal_polls=0
  until pgrep -f "$signal_root" > /dev/null; do
    signal_polls=$((signal_polls + 1))
    [ "$signal_polls" -lt 100 ] || { kill "$signal_pid" 2> /dev/null; printf 'the child of the check did not start within %s polls of 0.1 s\n' "$signal_polls"; return 1; }
    sleep 0.1
  done
  kill "-$2" "$signal_pid"
  signal_polls=0
  while kill -0 "$signal_pid" 2> /dev/null; do
    signal_polls=$((signal_polls + 1))
    [ "$signal_polls" -lt 100 ] || { printf 'the check did not end within %s polls of 0.1 s after the signal\n' "$signal_polls"; return 1; }
    sleep 0.1
  done
  signal_left=$(pgrep -f "$signal_root")
  [ -z "$signal_left" ] && return 0
  printf 'a child of the check is left after the signal, after %s polls of 0.1 s\n' "$signal_polls"
  return 1
}

fails_saying() {
  said_text=$1; shift
  said=$("$@" 2> /dev/null)
  case $said in *"FAIL $name ("*"$said_text"*) return 1 ;; esac
  return 0
}

predicate_zero_secure_keystrokes() {
  count=$(sed -n 's/^secure_input_keystrokes=\([0-9][0-9]*\)$/\1/p' "$1" 2>/dev/null)
  [ "$count" = 0 ]
}

gate_build() {
  mkdir -p "$GATE_TARGET" || { gate_build_error="cannot make $GATE_TARGET"; return 1; }
  ( cd "$GATE_ROOT" && CARGO_TARGET_DIR="$GATE_TARGET" cargo build -q --locked -p halvo-capture --bin halvo-capture ) > "$GATE_TARGET/build.log" 2>&1 && return 0
  gate_build_error=$(grep -m 1 -E '^error' "$GATE_TARGET/build.log")
  [ -n "$gate_build_error" ] || gate_build_error=$(head -1 "$GATE_TARGET/build.log")
  return 1
}

gate_defect=''

predicate_sha() {
  case $1 in ''|*[!0-9a-f]*) return 1 ;; esac
  [ "${#1}" -eq 40 ]
}

measured_sha() {
  if [ -n "$gate_sha" ]; then printf '%s\n' "$gate_sha"; return 0; fi
  git -C "$GATE_ROOT" rev-parse HEAD 2>/dev/null
}

tree_id() {
  tree_listing=$(git -C "$1" ls-tree -r "$2" 2>/dev/null) || return 1
  printf '%s\n' "$tree_listing" | awk -F '\t' 'index($2, "docs/baseline/") != 1 && index($2, "\"docs/baseline/") != 1' | git hash-object --stdin
}

head_read() {
  head_sha=$(git -C "$GATE_ROOT" rev-parse HEAD 2>/dev/null) || return 1
  head_tree=$(tree_id "$GATE_ROOT" HEAD) || return 1
  head_porcelain=$(git -C "$GATE_ROOT" -c core.fileMode=true status --porcelain --untracked-files=no --ignore-submodules=none 2>/dev/null) || return 1
  head_untracked=$(git -C "$GATE_ROOT" ls-files --others --exclude-per-directory=.gitignore 2>/dev/null) || return 1
  head_index=$(git -C "$GATE_ROOT" ls-files -v 2>/dev/null) || return 1
  head_flagged=$(printf '%s\n' "$head_index" | awk 'NF > 0 && $1 != "H" { print; exit }')
  head_hidden=$(hidden_untracked "$GATE_ROOT") || return 1
  head_state=clean
  predicate_tree_clean "$head_porcelain" || head_state=dirty
  predicate_tree_clean "$head_untracked" || head_state=dirty
  predicate_tree_clean "$head_flagged" || head_state=dirty
  predicate_tree_clean "$head_hidden" || head_state=dirty
  printf '%s %s %s\n' "$head_sha" "$head_tree" "$head_state"
}

hidden_untracked() {
  hidden_list=$(git -C "$1" status --porcelain --ignored --untracked-files=normal 2>/dev/null) || return 1
  hidden_ifs=$IFS
  IFS='
'
  for hidden_line in $hidden_list; do
    IFS=$hidden_ifs
    case $hidden_line in '!! '*) ;; *) continue ;; esac
    hidden_entry=${hidden_line#'!! '}
    hidden_source=$(git -C "$1" check-ignore -v --no-index -- "$hidden_entry" 2>/dev/null) || { printf '%s\n' "$hidden_entry"; return 0; }
    hidden_file=${hidden_source%%:*}
    git -C "$1" ls-files --error-unmatch -- "$hidden_file" > /dev/null 2>&1 || { printf '%s\n' "$hidden_entry"; return 0; }
  done
  IFS=$hidden_ifs
}

predicate_tree_clean() {
  [ -z "$1" ]
}

predicate_run_unchanged() {
  predicate_sha "$1" || return 1
  [ "$1" = "$3" ] || return 1
  [ "$2" = "$4" ]
}

machine_kind() {
  machine_vmm=$(sysctl -n kern.hv_vmm_present 2>/dev/null)
  if [ "$machine_vmm" = 1 ]; then printf 'guest\n'; else printf 'host\n'; fi
}

fail_defect() {
  emit_fail "$name" "$gate_defect"
  return 1
}

need_build() {
  gate_build && return 0
  gate_defect="build failed: $gate_build_error"
  return 1
}

capture_bin=$GATE_BIN
capture_helper_bin=$GATE_TARGET/window-helper
capture_builds=yes
capture_label=''

build_for_measure() {
  [ "$capture_builds" = yes ] || return 0
  need_build
}

standin_as_the_binary() {
  export STANDIN_SCENARIO="$1"
  capture_bin=$standin
  capture_helper_bin=$gate_scratch/standin-helper
  capture_builds=no
  capture_label="$st_case-"
  gate_input=$2
  printf '#!/bin/sh\nprintf "helper visible pid=1\\n"\nexec sleep 30\n' > "$capture_helper_bin"
  chmod 755 "$capture_helper_bin"
}

standin_build() {
  standin=$GATE_TARGET/load-standin
  mkdir -p "$GATE_TARGET" || return 1
  cc -O2 -o "$standin" "$GATE_ROOT/scripts/r1-gate/helper/load-standin.c" > "$GATE_TARGET/load-standin.log" 2>&1
}

child_defect=''
child_status=''

timed_child() {
  child_deadline=$1; child_dir=$2; child_bin=$3; shift 3
  child_defect=''; child_pid=''
  case $child_deadline in ''|*[!0-9]*|0) gate_defect="the deadline of the child is no positive number of seconds"; return 1 ;; esac
  child_start=$(date +%s)
  /usr/bin/time -l -o "$child_dir/time" "$child_bin" "$@" > "$child_dir/stdout" 2> "$child_dir/stderr" &
  child_wrapper=$!
  child_polls=0
  while :; do
    child_pid=$(pgrep -P "$child_wrapper" 2>/dev/null | head -1)
    [ -z "$child_pid" ] || break
    if ! kill -0 "$child_wrapper" 2>/dev/null; then
      child_defect="the process ended before its pid was read"; break
    fi
    child_polls=$((child_polls + 1))
    if [ "$child_polls" -ge 100 ]; then
      child_defect="the time wrapper had no child after 10 s"
      kill -TERM "$child_wrapper" 2>/dev/null
      break
    fi
    sleep 0.1
  done
  child_polls=0
  while [ -n "$child_pid" ]; do
    child_comm=$(ps -o comm= -p "$child_pid" 2>/dev/null)
    [ "$child_comm" != "$child_bin" ] || break
    child_polls=$((child_polls + 1))
    if [ "$child_polls" -ge 10 ]; then
      child_defect="the child of the time wrapper is ${child_comm:-gone}, not $child_bin"
      kill -TERM "$child_pid" 2>/dev/null
      break
    fi
    sleep 0.1
  done
  child_samples=0; child_sum_kib=0; child_peak_kib=0; child_others=no
  while [ -z "$child_defect" ] && kill -0 "$child_wrapper" 2>/dev/null; do
    child_rss=$(ps -o rss= -p "$child_pid" 2>/dev/null)
    child_rss=${child_rss##* }
    case $child_rss in
      ''|*[!0-9]*) ;;
      *) child_samples=$((child_samples + 1)); child_sum_kib=$((child_sum_kib + child_rss))
         [ "$child_rss" -le "$child_peak_kib" ] || child_peak_kib=$child_rss ;;
    esac
    child_other=$(pgrep -x halvo-capture 2>/dev/null | grep -v -x -e "$child_pid" | head -1)
    [ -z "$child_other" ] || child_others=yes
    child_now=$(date +%s)
    if [ "$((child_now - child_start))" -ge "$child_deadline" ]; then
      child_defect="the child ran past its deadline of ${child_deadline}s and was ended"
      kill -TERM "$child_pid" 2>/dev/null
      child_polls=0
      while kill -0 "$child_wrapper" 2>/dev/null; do
        child_polls=$((child_polls + 1))
        if [ "$child_polls" -gt 50 ]; then kill -KILL "$child_pid" 2>/dev/null; break; fi
        sleep 0.1
      done
      break
    fi
    sleep 1
  done
  child_mean_kib=0
  [ "$child_samples" -eq 0 ] || child_mean_kib=$((child_sum_kib / child_samples))
  printf '%s %s %s\n' "$child_peak_kib" "$child_mean_kib" "$child_samples" > "$child_dir/samples"
  printf '%s\n' "$child_others" > "$child_dir/others"
  wait "$child_wrapper"
  child_status=$?
  child_wrapper=''
  child_pid=''
  gate_defect=$child_defect
  [ -z "$child_defect" ]
}

child_report_defect() {
  case $child_status in
    0|3) return 0 ;;
    2) gate_defect="the timed run ended on a signal before its time (exit 2)" ;;
    *) child_first=$(head -1 "$child_dir/stderr" 2>/dev/null)
       gate_defect="timed-run unavailable (exit $child_status): $child_first" ;;
  esac
  return 1
}

helper_build() {
  helper_source=$GATE_ROOT/scripts/r1-gate/helper/window-helper.swift
  helper_fresh=$(find "$GATE_TARGET/window-helper" -newer "$helper_source" 2>/dev/null)
  [ -z "$helper_fresh" ] || return 0
  mkdir -p "$GATE_TARGET" || { gate_defect="cannot make $GATE_TARGET"; return 1; }
  swiftc -O -o "$GATE_TARGET/window-helper" "$helper_source" > "$GATE_TARGET/window-helper.log" 2>&1 && return 0
  helper_first=$(head -1 "$GATE_TARGET/window-helper.log")
  gate_defect="the helper window does not build: $helper_first"
  return 1
}

helper_start() {
  "$2" "$1" > "$gate_scratch/helper.out" 2>&1 &
  helper_pid=$!
  helper_polls=0
  until grep -q '^helper visible' "$gate_scratch/helper.out" 2>/dev/null; do
    if ! kill -0 "$helper_pid" 2>/dev/null; then
      helper_first=$(head -1 "$gate_scratch/helper.out")
      gate_defect="the helper window ended before it was on screen: $helper_first"
      helper_pid=''
      return 1
    fi
    helper_polls=$((helper_polls + 1))
    if [ "$helper_polls" -ge "${3:-100}" ]; then
      gate_defect="the helper window was not on screen in time"
      helper_stop
      return 1
    fi
    sleep 0.1
  done
}

helper_stop() {
  [ -n "$helper_pid" ] || return 0
  kill -TERM "$helper_pid" 2>/dev/null
  wait "$helper_pid" 2>/dev/null
  helper_pid=''
}

capture_helper=no

capture_prepare() {
  build_for_measure || return 1
  scratch_root
  run_store=$gate_scratch/${capture_label}store; run_dir=$gate_scratch/${capture_label}run
  mkdir "$run_store" "$run_dir" || { gate_defect="cannot make the scratch store"; return 1; }
  export HALVO_STORE_DIR="$run_store"
}

capture_run() {
  capture_seconds=$1; shift
  use_bound child_deadline_extra_s || { gate_defect="bound child_deadline_extra_s is not readable: docs/baseline/r1-gate-bounds.txt must hold exactly one child_deadline_extra_s=digits line"; return 1; }
  if [ "$capture_helper" = yes ]; then
    [ "$capture_builds" = no ] || helper_build || return 1
    helper_start "$((capture_seconds + 60))" "$capture_helper_bin" || return 1
  fi
  run_before=$(machine_state)
  timed_child "$((capture_seconds + bound))" "$run_dir" "$capture_bin" --timed-run "$capture_seconds" --store-dir "$run_store" "$@"
  capture_status=$?
  helper_stop
  [ "$capture_status" -eq 0 ] || return 1
  run_after=$(machine_state)
  child_report_defect || return 1
  run_real=$(awk -v asked="$capture_seconds" 'NR == 1 && $2 == "real" { if ($1 + 0 < asked + 0) print $1 }' "$run_dir/time" 2>/dev/null)
  [ -z "$run_real" ] || { gate_defect="the timed run ended after ${run_real}s of ${capture_seconds}s"; return 1; }
}

run_machine_note() {
  run_others=$(cat "$run_dir/others" 2>/dev/null)
  printf 'start %s, end %s, other halvo-capture=%s, binary=%s\n' "$run_before" "$run_after" "${run_others:-unread}" "${capture_bin#"$GATE_ROOT"/}"
}

strict_run_ok() {
  [ "$gate_strict" = yes ] || return 0
  run_others=$(cat "$run_dir/others" 2>/dev/null)
  predicate_machine_quiet "${run_before#* }" "${run_after#* }" "$run_others"
}

time_file_cpu_pct() {
  awk 'NR == 1 {
    decimal = "^[0-9]+(\\.[0-9]+)?$"
    if (NF != 6 || $2 != "real" || $4 != "user" || $6 != "sys") exit 1
    if ($1 !~ decimal || $3 !~ decimal || $5 !~ decimal) exit 1
    printf "%.2f\n", ($3 + $5) / $1 * 100
    found = 1
  }
  END { if (!found) exit 1 }' "$1" 2>/dev/null
}

time_file_number() {
  awk -v label="$2" '{
    value = $1
    $1 = ""
    sub(/^ +/, "")
    if ($0 == label && value ~ /^[0-9]+$/) { print value; found = 1 }
  }
  END { if (!found) exit 1 }' "$1" 2>/dev/null
}

memory_figures() {
  mem_maxrss=$(time_file_number "$1/time" "maximum resident set size") || { printf 'the time file holds no maximum resident set size\n'; return 1; }
  mem_footprint=$(time_file_number "$1/time" "peak memory footprint") || { printf 'the time file holds no peak memory footprint\n'; return 1; }
  read -r mem_ps_kib mem_mean_kib mem_samples < "$1/samples" || { printf 'the sampler wrote no samples file\n'; return 1; }
  awk -v a="$mem_maxrss" -v b="$mem_ps_kib" -v c="$mem_footprint" -v mean="$mem_mean_kib" -v n="$mem_samples" 'BEGIN {
    whole = "^[0-9]+$"
    if (a !~ whole || b !~ whole || c !~ whole || mean !~ whole || n !~ whole) { print "a memory figure is not a number"; exit 1 }
    peak = a + 0
    if (b * 1024 > peak) peak = b * 1024
    if (c + 0 > peak) peak = c + 0
    printf "%d %d %d %d %d %d\n", peak, mean, n, a, b, c
  }'
}

predicate_memory_within_bound() {
  awk -v peak="$1" -v mean_kib="$2" -v samples="$3" -v max="$4" 'BEGIN {
    whole = "^[0-9]+$"
    if (peak !~ whole || mean_kib !~ whole || samples !~ whole || max !~ whole) exit 2
    if (max + 0 <= 0) exit 1
    if (samples + 0 == 0) exit 1
    if (peak + 0 > max + 0) exit 1
    if (mean_kib * 1024 > max + 0) exit 1
    exit 0
  }'
}

machine_state() {
  state_load=$(sysctl -n vm.loadavg 2>/dev/null | awk '{print $2}')
  state_level=$(sysctl -n kern.memorystatus_vm_pressure_level 2>/dev/null)
  printf 'load1=%s pressure=%s\n' "${state_load:-unread}" "${state_level:-unread}"
}

predicate_machine_quiet() {
  [ "$1" = "pressure=1" ] || return 1
  [ "$2" = "pressure=1" ] || return 1
  [ "$3" = no ]
}

predicate_cost_within_bound() {
  awk -v cpu="$1" -v growth="$2" -v seconds="$3" -v cpu_max="$4" -v per_day="$5" 'BEGIN {
    decimal = "^[0-9]+(\\.[0-9]+)?$"; whole = "^[0-9]+$"
    if (cpu !~ decimal || cpu_max !~ decimal || growth !~ /^-?[0-9]+$/) exit 2
    if (seconds !~ whole || per_day !~ whole || seconds + 0 == 0) exit 2
    if (cpu_max + 0 <= 0 || per_day + 0 <= 0) exit 1
    if (cpu + 0 > cpu_max + 0) exit 1
    if (growth + 0 > seconds * per_day / 86400) exit 1
    exit 0
  }'
}

ci_facts() {
  gh run list -R "$GATE_REPO" -c "$1" -L 100 --json databaseId,event,headSha,status,conclusion \
    --jq '.[] | "run \(.databaseId) \(.event) \(.headSha) \(.status) \(if .conclusion == null or .conclusion == "" then "none" else .conclusion end)"' > "$2.runs" || return 1
  : > "$2"
  while read -r fact_kind fact_id fact_event fact_rest; do
    printf '%s %s %s %s\n' "$fact_kind" "$fact_id" "$fact_event" "$fact_rest" >> "$2"
    [ "$fact_event" = push ] || continue
    gh run view -R "$GATE_REPO" "$fact_id" --json jobs \
      --jq '.jobs[] | "job \(.name | gsub(" "; "-")) \(if .conclusion == null or .conclusion == "" then "none" else .conclusion end) \(.steps | length)"' >> "$2" || return 1
  done < "$2.runs"
}

ci_defect() {
  awk -v sha="$1" '
    function close_run() {
      if (!in_push || defect != "") return
      if (windows == "absent") defect = "push run " run_id " has no job named windows"
      else if (windows == "did not start") defect = "push run " run_id ": the windows job did not start (zero steps)"
      else if (windows != "success") defect = "push run " run_id ": the windows job concluded " windows
    }
    BEGIN {
      if (sha !~ /^[0-9a-f]+$/ || length(sha) != 40) { print "the commit to read is no 40 hex digits: [" sha "]"; bad = 1; exit 1 }
    }
    $1 == "run" {
      close_run()
      in_push = ($3 == "push")
      if (!in_push) next
      pushes++; run_id = $2; windows = "absent"
      if ($4 != sha && defect == "") defect = "push run " $2 " is for " $4 ", not for the measured commit"
    }
    $1 == "job" && in_push && $2 == "windows" {
      if ($4 + 0 < 1) windows = "did not start"
      else windows = $3
    }
    END {
      if (bad) exit 1
      close_run()
      if (defect == "" && pushes == 0) defect = "no push run for " sha
      if (defect != "") { print defect; exit 1 }
    }' "$2"
}

predicate_ci() {
  ci_defect "$1" "$2" > /dev/null
}

GATE_LIMIT=$GATE_ROOT/scripts/r1-gate/helper/limit.pl

limited() {
  perl "$GATE_LIMIT" "$@"
}

cargo_test_outcome() {
  awk '
    /^test result: / {
      for (i = 1; i < NF; i++) {
        if ($(i + 1) == "passed;") passed += $i
        if ($(i + 1) == "failed;") failed += $i
      }
    }
    END {
      if (passed + failed != 1) print "ran " passed + failed " tests"
      else if (failed == 1) print "failed"
      else print "passed"
    }' "$1"
}

mutant_verdict() {
  verdict_kind=${1%% *}
  verdict_rest=${1#"$verdict_kind"}
  verdict_rest=${verdict_rest# }
  case $verdict_kind in
    measured)
      awk -v keys="$verdict_rest" '
        /^FAIL [a-z-]+ \(.*measured: / {
          text = $0
          sub(/^.*measured: /, "", text)
          sub(/[;)].*$/, "", text)
          have_n = split(text, have, " ")
          want_n = split(keys, want, " ")
          named = (want_n > 0)
          for (i = 1; i <= want_n; i++) {
            found = 0
            for (j = 1; j <= have_n; j++) if (have[j] == want[i]) found = 1
            if (!found) named = 0
          }
          if (named) { print; hit = 1; exit }
        }
        END { exit hit ? 0 : 1 }' "$2" ;;
    leak) grep -m 1 -E '^FAIL [a-z-]+ \(leak: ' "$2" ;;
    refused)
      [ -n "$verdict_rest" ] || return 1
      grep -E '^FAIL [a-z-]+ \(verify-[a-z]+ unavailable' "$2" | grep -m 1 -F -e "$verdict_rest" ;;
    *) return 1 ;;
  esac
}

predicate_tree_lines() {
  for tree_name in $2; do
    tree_hit=$(printf '%s\n' "$1" | grep -m 1 -e "^$tree_name v")
    [ -n "$tree_hit" ] || { printf 'required line missing: %s v\n' "$tree_name"; return 1; }
  done
  for tree_name in $3; do
    tree_hit=$(printf '%s\n' "$1" | grep -m 1 -e "^$tree_name v")
    [ -z "$tree_hit" ] || { printf 'forbidden crate line: %s\n' "$tree_hit"; return 1; }
  done
  return 0
}

code_view() {
  view_blob=$(git -C "$1" cat-file -p ":$2" 2>/dev/null) || return 2
  printf '%s\n' "$view_blob" | awk -v path="$2" '
    {
      line = $0; out = ""; i = 1; n = length(line)
      while (i <= n) {
        rest = substr(line, i)
        if (depth > 0) {
          e = index(rest, "*/"); o = index(rest, "/*")
          if (o > 0 && (e == 0 || o < e)) { depth++; i += o + 1; continue }
          if (e == 0) break
          depth--; i += e + 1; continue
        }
        q = index(rest, "\""); c = index(rest, "//"); b = index(rest, "/*")
        if (q > 0 && (c == 0 || q < c) && (b == 0 || q < b)) { out = out rest; break }
        if (c > 0 && (b == 0 || c < b)) { out = out substr(rest, 1, c - 1); break }
        if (b > 0) { out = out substr(rest, 1, b - 1) " "; depth = 1; i += b + 1; continue }
        out = out rest; break
      }
      print path ":" NR ":" out
    }'
}

source_rule() {
  rule=$1; rule_pattern=$2; rule_allowed=$3; rule_repo=$4
  rule_files=$(git -C "$rule_repo" -c core.quotepath=off grep --cached -a -l -E -e "$rule_pattern" -- '*.rs' 2>/dev/null)
  case $? in 1) return 0 ;; 0) ;; *) printf 'not scanned: git grep could not read the index for rule %s\n' "$rule"; return 2 ;; esac
  rule_ifs=$IFS
  IFS='
'
  for rule_file in $rule_files; do
    IFS=$rule_ifs
    rule_view=$(code_view "$rule_repo" "$rule_file") || { printf 'not scanned: git could not read %s for rule %s\n' "$rule_file" "$rule"; return 2; }
    left=$(printf '%s\n' "$rule_view" | sed -e "$rule_allowed" | grep -E -e "$rule_pattern")
    case $? in 1) continue ;; 0) ;; *) printf 'not scanned: the second read failed for rule %s\n' "$rule"; return 2 ;; esac
    where=$(printf '%s\n' "$left" | sed -n '1s/^\([^:]*:[0-9]*\):.*/\1/p')
    printf '%s at %s\n' "$rule" "$where"
    return 1
  done
  IFS=$rule_ifs
  return 0
}

listen_only_defect() {
  listen_files=$(git -C "$1" -c core.quotepath=off grep --cached -a -l -F -e 'CGEventTapOptions::ListenOnly' -- 'crates/halvo-capture/src/macos/*.rs' 2>/dev/null)
  case $? in 1) printf 'listen-only-absent under crates/halvo-capture/src/macos\n'; return 1 ;; 0) ;; *) printf 'not scanned: git grep could not read crates/halvo-capture/src/macos\n'; return 2 ;; esac
  listen_ifs=$IFS
  IFS='
'
  for listen_file in $listen_files; do
    IFS=$listen_ifs
    listen_view=$(code_view "$1" "$listen_file") || { printf 'not scanned: git could not read %s\n' "$listen_file"; return 2; }
    case $listen_view in *CGEventTapOptions::ListenOnly*) return 0 ;; esac
  done
  IFS=$listen_ifs
  printf 'listen-only-absent under crates/halvo-capture/src/macos: every CGEventTapOptions::ListenOnly there is inside a comment\n'
  return 1
}

source_scan_defect() {
  [ -n "$1" ] || { printf 'not scanned: the root to scan is an empty path\n'; return 2; }
  scan_repo=$(cd "$1" 2>/dev/null && pwd -P) || { printf 'not scanned: %s is missing\n' "$1"; return 2; }
  scan_top=$(git -C "$scan_repo" rev-parse --show-toplevel 2>/dev/null)
  [ "$scan_top" = "$scan_repo" ] || { printf 'not scanned: %s is not the top of a git work tree\n' "$1"; return 2; }
  scan_index=$(git -C "$scan_repo" -c core.quotepath=off ls-files -s -- '*.rs' 2>/dev/null) || { printf 'not scanned: git could not read the index\n'; return 2; }
  [ -n "$scan_index" ] || { printf 'not scanned: no .rs file\n'; return 2; }
  scan_link=$(printf '%s\n' "$scan_index" | awk -F '\t' 'substr($1, 1, 7) == "120000 " { print $2; exit }')
  [ -z "$scan_link" ] || { printf 'tracked symbolic link: %s\n' "$scan_link"; return 1; }
  for scan_root in crates/ src-tauri/src/ crates/halvo-capture/src/macos/; do
    printf '%s\n' "$scan_index" | awk -F '\t' -v root="$scan_root" 'index($2, root) == 1 { found = 1 } END { exit !found }' ||
      { printf 'not scanned: no tracked .rs file under %s\n' "${scan_root%/}"; return 2; }
  done
  source_rule windows-hook 'SetWindowsHookEx|WH_KEYBOARD_LL|WH_MOUSE_LL' 's/^//' "$scan_repo" || return $?
  source_rule active-tap 'CGEventTapOptions::|kCGEventTapOptionDefault|CGEventTapCreate|CGEventPost|post\([^)]*CGEventTapLocation|post_to_pid|post_from_tap|new_keyboard_event|new_mouse_event|new_scroll_event' 's/CGEventTapOptions::ListenOnly\([^A-Za-z0-9_]\)/\1/g; s/CGEventTapOptions::ListenOnly$//' "$scan_repo" || return $?
  source_rule audio-capture 'setCapturesAudio|setCaptureMicrophone|SCStreamOutputType::' 's/SCStreamOutputType::Screen\([^A-Za-z0-9_]\)/\1/g; s/SCStreamOutputType::Screen$//' "$scan_repo" || return $?
  source_rule local-machine-key 'CRYPTPROTECT_LOCAL_MACHINE' 's/^//' "$scan_repo" || return $?
  source_rule untied-post '\.post[[:space:]]*\(' 's/^//' "$scan_repo" || return $?
  listen_only_defect "$scan_repo"
}

predicate_no_ll_hook() {
  source_scan_defect "$1" > /dev/null
}

gate_slack_s=${gate_slack_s:-2}
gate_claim_every_s=${gate_claim_every_s:-3}
gate_poll_s=${gate_poll_s:-0.5}
GATE_FIXTURES=$GATE_ROOT/scripts/r1-gate/fixtures
svc_dir=''
svc_data=''
svc_pid=''
svc_wrapper=''
lease_s=''
batch_s=''
stop_s=''
svc_fd=no
svc_label=''
svc_claimed=0
svc_ctl_n=0
svc_helper_pid=''
svc_proc_tmp=''

gate_marker() {
  gate_marker_now=$(date +%s)
  printf 'r1gate%s%s\n' "$$" "$gate_marker_now"
}

status_value() {
  awk -v key="$2" 'index($0, key "=") == 1 && index($0, " ") == 0 { print substr($0, length(key) + 2); exit }' "$1" 2>/dev/null
}

source_state() {
  sed -n "s/^source name=$2 state=\\([a-z_]*\\).*/\\1/p" "$1" 2>/dev/null | head -1
}

switch_value() {
  sed -n "s/^switch name=$2 value=\\([a-z]*\\)\$/\\1/p" "$1" 2>/dev/null | head -1
}

inventory_total() {
  sed -n 's/^total events=\([0-9][0-9]*\) .*/\1/p' "$1" 2>/dev/null | head -1
}

inventory_events() {
  awk -v kind="$2" '$1 == "row" && $3 == "modality=" kind { sub(/^events=/, "", $4); n += $4 } END { print n + 0 }' "$1" 2>/dev/null
}

inventory_days_before() {
  awk -v cut="$2" '$1 == "row" { day = ""; for (i = 2; i <= NF; i++) if (index($i, "day=") == 1) day = substr($i, 5); if (day == "" || day < cut) seen[day] = 1 } END { for (d in seen) n++; print n + 0 }' "$1" 2>/dev/null
}

inventory_days_from() {
  awk -v cut="$2" '$1 == "row" { day = ""; for (i = 2; i <= NF; i++) if (index($i, "day=") == 1) day = substr($i, 5); if (day != "" && day >= cut) seen[day] = 1 } END { for (d in seen) n++; print n + 0 }' "$1" 2>/dev/null
}

report_token() {
  token_text=$(tr '\n\r\t' '   ' < "$1" 2>/dev/null)
  printf ' %s ' "$token_text" | sed -n "s/.* $2=\\([^ ]*\\) .*/\\1/p"
}

mode_missing() {
  [ "$1" = 64 ] || return 1
  mode_first=$(head -1 "$2" 2>/dev/null)
  gate_defect="no $3 mode: the binary answered $3 with exit 64${mode_first:+ ($mode_first)}"
}

ctl_defect() {
  ctl_err=$(head -1 "$2" 2>/dev/null)
  case $1 in
    0) return 0 ;;
    69) printf 'ctl %s cannot reach the daemon (exit 69): %s\n' "$3" "$ctl_err" ;;
    64) printf 'no ctl mode: the binary answered ctl %s with exit 64 (%s)\n' "$3" "$ctl_err" ;;
    *) printf 'ctl %s exited %s: %s\n' "$3" "$1" "$ctl_err" ;;
  esac
  return 1
}

predicate_ctl_answered() {
  ctl_defect "$1" "$2" "$3" > /dev/null
}

switch_defect() {
  switch_seen=$(switch_value "$1" "$2")
  [ "$switch_seen" = on ] && return 0
  printf 'the first status answer holds switch name=%s value=%s, and the check wrote the switch as on into daemon-state.json before the start\n' "$2" "${switch_seen:-absent}"
  return 1
}

predicate_switch_on() {
  switch_defect "$1" "$2" > /dev/null
}

store_defect() {
  store_seen=$(status_value "$1" store)
  [ "$store_seen" = open ] && return 0
  store_error_seen=$(status_value "$1" store_error)
  printf 'store=%s store_error=%s\n' "${store_seen:-absent}" "$store_error_seen"
  return 1
}

predicate_store_open() {
  store_defect "$1" > /dev/null
}

sources_running_defect() {
  running_file=$1; shift
  for running_name in "$@"; do
    running_state=$(source_state "$running_file" "$running_name")
    [ "$running_state" = running ] && continue
    running_line=$(grep -m 1 -e "^source name=$running_name " "$running_file" 2>/dev/null)
    printf 'source %s is not running: %s\n' "$running_name" "${running_line:-no source line}"
    return 1
  done
  running_capture=$(status_value "$running_file" capture)
  [ "$running_capture" = running ] && return 0
  printf 'capture=%s with every required source running\n' "${running_capture:-absent}"
  return 1
}

predicate_sources_running() {
  sources_running_defect "$@" > /dev/null
}

growth_defect() {
  growth_fa=$1; growth_fb=$2
  growth_a=$(inventory_total "$1"); growth_b=$(inventory_total "$2")
  case $growth_a:$growth_b in *[!0-9:]*|:*|*:) printf 'an inventory holds no total line\n'; return 1 ;; esac
  [ "$growth_b" -gt "$growth_a" ] || { printf 'the event total did not grow: %s to %s\n' "$growth_a" "$growth_b"; return 1; }
  shift 2
  for growth_kind in "$@"; do
    growth_ka=$(inventory_events "$growth_fa" "$growth_kind"); growth_kb=$(inventory_events "$growth_fb" "$growth_kind")
    [ "$growth_kb" -gt "$growth_ka" ] || { printf 'no %s row among the growth: %s to %s\n' "$growth_kind" "$growth_ka" "$growth_kb"; return 1; }
  done
}

predicate_growth() {
  growth_defect "$@" > /dev/null
}

off_kinds_defect() {
  off_row=$(grep -m 1 -E '^row day=[^ ]+ modality=(system_audio|microphone|camera) ' "$1" 2>/dev/null)
  [ -z "$off_row" ] && return 0
  off_kind=$(printf '%s\n' "$off_row" | cut -d ' ' -f 3)
  printf 'an inventory row of a kind that R1 never captures: %s\n' "$off_kind"
  return 1
}

predicate_no_off_kind_rows() {
  off_kinds_defect "$1" > /dev/null
}

own_child_defect() {
  own_a=$(inventory_total "$1"); own_b=$(inventory_total "$2")
  case $own_a:$own_b in *[!0-9:]*|:*|*:) printf 'an inventory of the control run holds no total line\n'; return 1 ;; esac
  [ "$own_b" -gt "$own_a" ] || { printf 'the control run recorded nothing, so its zero proves nothing: %s to %s\n' "$own_a" "$own_b"; return 1; }
  own_sa=$(inventory_events "$1" screen_frame); own_sb=$(inventory_events "$2" screen_frame)
  [ "$own_sb" -eq "$own_sa" ] && return 0
  printf 'screen activity rows grew from %s to %s while the helper window was the check'"'"'s own child, so the exclusion rule did not drop them\n' "$own_sa" "$own_sb"
  return 1
}

predicate_own_child_excluded() {
  own_child_defect "$@" > /dev/null
}

lease_defect() {
  lease_capture=$(status_value "$1" capture)
  [ "$lease_capture" = no_indicator ] || { printf 'capture=%s after the claims stopped, not capture=no_indicator\n' "${lease_capture:-absent}"; return 1; }
  lease_a=$(inventory_total "$2"); lease_b=$(inventory_total "$3")
  case $lease_a:$lease_b in *[!0-9:]*|:*|*:) printf 'an inventory after the lapse holds no total line\n'; return 1 ;; esac
  [ "$lease_b" -eq "$lease_a" ] && return 0
  printf 'events still grew after the lease lapsed: %s to %s\n' "$lease_a" "$lease_b"
  return 1
}

predicate_lease_lapsed() {
  lease_defect "$@" > /dev/null
}

stop_defect() {
  stop_line=$(head -1 "$2" 2>/dev/null)
  case $1:$stop_line in
    '0:stopped flushed=yes events_lost=0') return 0 ;;
  esac
  printf 'ctl stop exited %s and printed: %s\n' "$1" "${stop_line:-nothing}"
  return 1
}

predicate_stopped() {
  stop_defect "$@" > /dev/null
}

backup_mark_defect() {
  [ "$1" = 0 ] || { printf 'xattr -p com.apple.metadata:com_apple_backup_excludeItem exited %s: the data directory carries no Time Machine mark\n' "$1"; return 1; }
  [ -s "$2" ] && return 0
  printf 'the Time Machine mark of the data directory is empty\n'
  return 1
}

predicate_backup_mark() {
  backup_mark_defect "$@" > /dev/null
}

foreground_switches_defect() {
  case $1 in ''|*[!0-9]*) printf 'no foreground_app count\n'; return 1 ;; esac
  [ "$1" -ge 2 ] && return 0
  printf 'foreground_app count=%s: the input of the run switches apps, so at least 2 are required\n' "$1"
  return 1
}

predicate_foreground_switches() {
  [ "$gate_input" = none ] && return 0
  foreground_seen=$(modality_count "$1" foreground_app)
  foreground_switches_defect "$foreground_seen" > /dev/null
}

take_bound() {
  use_bound "$1" && return 0
  gate_defect="bound $1 is not readable: docs/baseline/r1-gate-bounds.txt must hold exactly one $1=digits line"
  return 1
}

lease_control() {
  lapse_end=$((svc_claimed + lease_s + gate_slack_s))
  while :; do
    ctl_or_defect lapse status || return 1
    lapse_capture=$(status_value "$ctl_out" capture)
    [ "$lapse_capture" != no_indicator ] || break
    lapse_now=$(date +%s)
    [ "$lapse_now" -lt "$lapse_end" ] || break
    sleep "$gate_poll_s"
  done
  lapse_status=$ctl_out
  ctl_or_defect lapse-a inventory || return 1
  lapse_a=$ctl_out
  hold_for "$((batch_s + gate_slack_s))" no-claims || return 1
  ctl_or_defect lapse-b inventory || return 1
  lapse_now=$(date +%s)
  lapse_found=$(lease_defect "$lapse_status" "$lapse_a" "$ctl_out") || { gate_defect="lease control, ${lease_s}s plus ${gate_slack_s}s after the last claim, read $((lapse_now - svc_claimed))s after it: $lapse_found"; return 1; }
}

window_steps() {
  ctl_or_defect "$1-a" inventory || return 1
  cp "$ctl_out" "$svc_dir/$1-a.inventory" || return 1
  hold_for "${2:-$((batch_s + gate_slack_s))}" || return 1
  claim || { gate_defect=$(ctl_defect "$ctl_status" "$ctl_out.err" status); return 1; }
  cp "$ctl_out" "$svc_dir/$1.status" || return 1
  ctl_or_defect "$1-b" inventory || return 1
  cp "$ctl_out" "$svc_dir/$1-b.inventory"
}

stop_steps() {
  service_stop "$stop_s"
  stop_found=$(stop_defect "$svc_stop_status" "$svc_stop_out") || { gate_defect=$stop_found; return 1; }
  [ "$svc_exit" = 0 ] || { gate_defect="the daemon did not exit 0 within stop_deadline_s=${stop_s}s of ctl stop: exit $svc_exit"; return 1; }
}

utc_of() {
  date -u -r "$1" +%Y-%m-%dT%H:%M:%SZ
}

floor_run() {
  floor_label=$1; shift
  TMPDIR=$svc_proc_tmp "$capture_bin" --floor-report --data-dir "$svc_data" "$@" > "$svc_dir/$floor_label.out" 2> "$svc_dir/$floor_label.err" 7>&-
  floor_status=$?
  [ "$floor_status" -eq 0 ] && return 0
  mode_missing "$floor_status" "$svc_dir/$floor_label.err" --floor-report && return 1
  gate_defect="--floor-report exited $floor_status: $(head -1 "$svc_dir/$floor_label.err" 2>/dev/null)"
  return 1
}

floor_value() {
  sed -n "s/^floor $2 \\(.*\\)\$/ \\1 /p" "$1" 2>/dev/null | head -1 | sed -n "s/.* $3=\\([^ ]*\\) .*/\\1/p"
}

registered_names_defect() {
  [ -d "$1" ] || { printf 'the data directory is missing\n'; return 1; }
  names_roots=0
  names_list=$(cd "$1" && find . -mindepth 1 2>/dev/null) || { printf 'the data directory could not be listed\n'; return 1; }
  while IFS= read -r names_entry; do
    [ -n "$names_entry" ] || continue
    names_entry=${names_entry#./}
    case $names_entry in
      daemon-state.json|store|store/store.key|store/store.db|store/store.db-wal|store/segments|roots|run|run/control.sock|run/running|logs) ;;
      logs/daemon.err) [ ! -s "$1/$names_entry" ] || { printf 'logs/daemon.err is not empty after the delete-all\n'; return 1; } ;;
      roots/*/*) printf 'a name outside the registered names of C17: %s\n' "$names_entry"; return 1 ;;
      roots/*) names_roots=$((names_roots + 1)) ;;
      *) printf 'a name outside the registered names of C17: %s\n' "$names_entry"; return 1 ;;
    esac
  done <<NAMES
$names_list
NAMES
  [ "$names_roots" -eq 1 ] && return 0
  printf 'roots/ holds %s root files, and the registry allows exactly one after a delete-all\n' "$names_roots"
  return 1
}

predicate_registered_names() {
  registered_names_defect "$1" > /dev/null
}

data_dir_scan_defect() {
  scan_data=$1; scan_marker=$2; shift 2
  [ -n "$scan_marker" ] || { printf 'not scanned: no marker\n'; return 1; }
  [ -d "$scan_data" ] || { printf 'not scanned: the data directory is missing\n'; return 1; }
  tree_scan_defect "the data directory" "$scan_marker" "$scan_data" || return 1
  [ $# -gt 0 ] || { printf 'not scanned: no capture file\n'; return 1; }
  for scan_file in "$@"; do
    [ -f "$scan_file" ] || { printf 'not scanned: the capture file %s is missing\n' "${scan_file##*/}"; return 1; }
  done
  tree_scan_defect "the captured output" "$scan_marker" "$@"
}

tree_scan_defect() {
  scan_what=$1; scan_marker=$2; shift 2
  [ -n "$scan_marker" ] || { printf 'not scanned: no marker\n'; return 1; }
  scan_out=$(perl "$GATE_ROOT/scripts/r1-gate/helper/scan-tree.pl" "$scan_marker" "$@") || { printf 'not scanned: the scan of %s could not run\n' "$scan_what"; return 1; }
  [ -n "$scan_out" ] || return 0
  scan_first=$(printf '%s\n' "$scan_out" | sed -n 1p)
  scan_tab=$(printf '\t')
  scan_kind=${scan_first%%"$scan_tab"*}
  scan_rel=${scan_first#*"$scan_tab"}
  scan_rel=${scan_rel#*"$scan_tab"}
  case $scan_kind in
    link) printf 'a symbolic link under %s: %s\n' "$scan_what" "$scan_rel" ;;
    name) printf 'leak: the marker or a record field name is in a name under %s: %s\n' "$scan_what" "$scan_rel" ;;
    hit) printf 'leak: the marker or a record field name is in %s: %s\n' "$scan_what" "$scan_rel" ;;
    unreadable) printf 'not scanned: %s under %s cannot be read\n' "$scan_rel" "$scan_what" ;;
    missing) printf 'not scanned: %s is missing\n' "$scan_what" ;;
    *) printf 'not scanned: the scan of %s printed %s\n' "$scan_what" "$scan_kind" ;;
  esac
  return 1
}

predicate_data_dir_clean() {
  data_dir_scan_defect "$@" > /dev/null
}

service_prepare() {
  scratch_root
  svc_dir=$(mktemp -d "$gate_scratch/${capture_label}service.XXXXXX") || { gate_defect="mktemp could not make the run directory"; return 1; }
  svc_data=$(mktemp -d "$gate_scratch/${capture_label}data.XXXXXX") || { gate_defect="mktemp could not make the data directory"; return 1; }
  svc_proc_tmp=$(mktemp -d "$gate_scratch/${capture_label}proc-tmp.XXXXXX") || { gate_defect="mktemp could not make the TMPDIR of the processes under test"; return 1; }
}

state_file_write() {
  printf '{"version":1,"desired":"%s","storage_cap_bytes":26843545600,"permissions_once_granted":[],"permissions_asked":[],"switches":{"title_text":true,"key_class":true,"pointer_class":true,"screen_activity":%s,"site_read":false}}\n' "$3" "$2" > "$1/daemon-state.json" &&
    chmod 600 "$1/daemon-state.json"
}

ctl_call() {
  ctl_name=$1; shift
  svc_ctl_n=$((svc_ctl_n + 1))
  ctl_out=$svc_dir/ctl-$svc_ctl_n-$ctl_name.out
  TMPDIR=$svc_proc_tmp "$capture_bin" ctl --data-dir "$svc_data" "$@" > "$ctl_out" 2> "$ctl_out.err" 7>&-
  ctl_status=$?
  return "$ctl_status"
}

ctl_or_defect() {
  ctl_call "$@" && return 0
  gate_defect=$(ctl_defect "$ctl_status" "$ctl_out.err" "$2")
  return 1
}

claim() {
  ctl_call claim status --indicator || return 1
  svc_claimed=$(date +%s)
}

hold_for() {
  hold_end=$(( $(date +%s) + $1 ))
  while hold_now=$(date +%s) && [ "$hold_now" -lt "$hold_end" ]; do
    if [ "${2:-claims}" = claims ] && [ "$((hold_now - svc_claimed))" -ge "$gate_claim_every_s" ]; then
      claim || { gate_defect=$(ctl_defect "$ctl_status" "$ctl_out.err" status); return 1; }
    fi
    sleep "$gate_poll_s"
  done
}

prefill_run() {
  prefill_label=$1; shift
  TMPDIR=$svc_proc_tmp /usr/bin/time -l -o "$svc_dir/$prefill_label.time" "$capture_bin" --prefill-store "$@" --data-dir "$svc_data" > "$svc_dir/$prefill_label.stdout" 2> "$svc_dir/$prefill_label.stderr" 7>&-
  prefill_status=$?
  [ "$prefill_status" -eq 0 ] && return 0
  mode_missing "$prefill_status" "$svc_dir/$prefill_label.stderr" --prefill-store && return 1
  gate_defect="--prefill-store exited $prefill_status: $(head -1 "$svc_dir/$prefill_label.stderr" 2>/dev/null)"
  return 1
}

service_start() {
  svc_label=$1
  svc_fifo=$svc_dir/$1.lifeline
  mkfifo "$svc_fifo" || { gate_defect="mkfifo could not make the lifeline"; return 1; }
  if [ "${2:-}" = time ]; then
    TMPDIR=$svc_proc_tmp /usr/bin/time -l -o "$svc_dir/$1.time" "$capture_bin" --service-run --lifeline --data-dir "$svc_data" < "$svc_fifo" > "$svc_dir/$1.stdout" 2> "$svc_dir/$1.stderr" 7>&- &
    svc_wrapper=$!
  else
    TMPDIR=$svc_proc_tmp "$capture_bin" --service-run --lifeline --data-dir "$svc_data" < "$svc_fifo" > "$svc_dir/$1.stdout" 2> "$svc_dir/$1.stderr" 7>&- &
    svc_wrapper=''
  fi
  svc_pid=$!
  exec 7> "$svc_fifo"
  svc_fd=yes
  service_open
}

service_open() {
  open_deadline=$(( $(date +%s) + 30 ))
  svc_first=''
  while :; do
    if ! kill -0 "$svc_pid" 2>/dev/null; then
      wait "$svc_pid"
      open_status=$?
      svc_pid=''; svc_wrapper=''
      mode_missing "$open_status" "$svc_dir/$svc_label.stderr" --service-run && return 1
      gate_defect="the daemon exited $open_status before store=open: $(head -1 "$svc_dir/$svc_label.stderr" 2>/dev/null)"
      return 1
    fi
    if claim; then
      if [ -z "$svc_first" ]; then svc_first=$svc_dir/$svc_label.first-status; cp "$ctl_out" "$svc_first"; fi
      open_store=$(status_value "$ctl_out" store)
      [ "$open_store" != open ] || return 0
    fi
    open_now=$(date +%s)
    if [ "$open_now" -ge "$open_deadline" ]; then
      if [ -n "$svc_first" ]; then gate_defect="the store did not open within 30 s: $(store_defect "$ctl_out")"
      else gate_defect="no status answer within 30 s: $(ctl_defect "$ctl_status" "$ctl_out.err" status)"; fi
      return 1
    fi
    sleep "$gate_poll_s"
  done
}

daemon_of_wrapper() {
  [ -n "$svc_wrapper" ] || { printf '%s\n' "$svc_pid"; return 0; }
  pgrep -P "$svc_wrapper" 2>/dev/null | head -1
}

service_stop() {
  ctl_call stop stop
  svc_stop_status=$ctl_status
  svc_stop_out=$ctl_out
  service_exit_within "$1"
}

service_exit_within() {
  [ "$svc_fd" = no ] || { exec 7>&-; svc_fd=no; }
  exit_end=$(( $(date +%s) + $1 ))
  while [ -n "$svc_pid" ] && kill -0 "$svc_pid" 2>/dev/null; do
    exit_now=$(date +%s)
    [ "$exit_now" -lt "$exit_end" ] || { svc_exit=late; return 1; }
    sleep "$gate_poll_s"
  done
  if [ -n "$svc_pid" ]; then wait "$svc_pid"; svc_exit=$?; else svc_exit=gone; fi
  svc_pid=''; svc_wrapper=''
}

service_teardown() {
  [ -n "$svc_data" ] || return 0
  if [ -n "$svc_pid" ] && kill -0 "$svc_pid" 2>/dev/null; then
    ctl_call teardown-stop stop
    service_exit_within 5 || {
      teardown_daemon=$(daemon_of_wrapper)
      kill -TERM ${teardown_daemon:+"$teardown_daemon"} "$svc_pid" 2>/dev/null
      sleep 1
      kill -KILL ${teardown_daemon:+"$teardown_daemon"} "$svc_pid" 2>/dev/null
      wait "$svc_pid" 2>/dev/null
      svc_pid=''
    }
  fi
  [ "$svc_fd" = no ] || { exec 7>&-; svc_fd=no; }
  TMPDIR=$svc_proc_tmp "$capture_bin" --delete-all --data-dir "$svc_data" > "$svc_dir/teardown-delete.out" 2>&1 7>&-
  helper_outside_stop
}

helper_bundle() {
  bundle=$GATE_TARGET/window-helper.app
  mkdir -p "$bundle/Contents/MacOS" || return 1
  cp "$GATE_TARGET/window-helper" "$bundle/Contents/MacOS/window-helper" || return 1
  printf '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>CFBundleExecutable</key><string>window-helper</string><key>CFBundleIdentifier</key><string>com.halvo.gate.window-helper</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/></dict></plist>\n' > "$bundle/Contents/Info.plist"
}

helper_outside_start() {
  outside_out=$svc_dir/helper-outside.out
  : > "$outside_out"
  if [ "$capture_builds" = yes ]; then
    outside_path=$GATE_TARGET/window-helper.app/Contents/MacOS/window-helper
    helper_build || return 1
    helper_bundle || { gate_defect="the helper bundle could not be made"; return 1; }
    open -n -g -a "$GATE_TARGET/window-helper.app" --stdout "$outside_out" --stderr "$outside_out.err" --args "$1" 7>&- ||
      { gate_defect="open could not start the helper window outside the check's tree"; return 1; }
  else
    outside_path=$capture_helper_bin
    ( "$capture_helper_bin" "$1" > "$outside_out" 2>&1 7>&- & )
  fi
  outside_polls=0
  until svc_helper_pid=$(sed -n 's/^helper visible pid=\([0-9][0-9]*\)$/\1/p' "$outside_out" 2>/dev/null) && [ -n "$svc_helper_pid" ]; do
    outside_polls=$((outside_polls + 1))
    [ "$outside_polls" -lt "${gate_helper_polls:-100}" ] || {
      pkill -f "$outside_path" 2>/dev/null
      gate_defect="the helper window outside the check's tree was not on screen after $outside_polls polls of 0.1 s, and the check ended it"
      return 1
    }
    sleep 0.1
  done
}

helper_outside_stop() {
  [ -n "$svc_helper_pid" ] || return 0
  kill -TERM "$svc_helper_pid" 2>/dev/null
  svc_helper_pid=''
}

privacy_rows_line() {
  printf 'note privacy rows: a run of this check starts %s, the one gate-built path, which macOS will list in Privacy & Security, once that binary asks for each grant, under Screen & System Audio Recording, Accessibility and Input Monitoring; the helper window reads nothing and adds no row\n' "${capture_bin#"$GATE_ROOT"/}"
}

privacy_line_first() {
  service_standin "$1" "$2"
  first_out=$(measure 3 2>/dev/null)
  first_line=$(printf '%s\n' "$first_out" | sed -n 1p)
  first_want=$(privacy_rows_line)
  [ "$first_line" = "$first_want" ]
}

privacy_no_capture_line() {
  printf 'note privacy rows: the unattended half of this check starts no capture source and adds no row\n'
}

service_line() {
  printf 'with screen_activity on in daemon-state.json; binary=%s' "${capture_bin#"$GATE_ROOT"/}"
}

service_standin() {
  export STANDIN_SCENARIO="$1"
  export STANDIN_HELPER="$gate_scratch/standin-helper"
  capture_bin=$GATE_ROOT/scripts/r1-gate/helper/service-standin.sh
  capture_helper_bin=$STANDIN_HELPER
  capture_builds=no
  capture_label="$st_case-"
  gate_input=$2
  printf '#!/bin/sh\nprintf "helper visible pid=%%s\\n" "%s"\nend=%s((%s(date +%%s) + %s))\nwhile [ "%s(date +%%s)" -lt "%s" ]; do sleep 0.2; done\n' "\$\$" "\$" "\$" "\$1" "\$" "\$end" > "$capture_helper_bin"
  chmod 755 "$capture_helper_bin"
  export GATE_OVERRIDE_lease_bound_s=3 GATE_OVERRIDE_batch_age_s=1 GATE_OVERRIDE_stop_deadline_s=6 GATE_OVERRIDE_expiry_bound_s=6 GATE_OVERRIDE_prefill_events=60
  gate_slack_s=1; gate_claim_every_s=1; gate_poll_s=0.2
}

c2_fixture() {
  sed -e "$3" "$GATE_FIXTURES/$1" > "$gate_scratch/$2"
  printf '%s\n' "$gate_scratch/$2"
}
