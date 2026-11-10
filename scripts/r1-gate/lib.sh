# shellcheck shell=sh
export LC_ALL=C
GATE_ROOT=${GATE_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
GATE_ROOT=$(cd "$GATE_ROOT" && pwd -P)
GATE_REPO=${GATE_REPO:-wth-lg/halvo}
GATE_DURATION=${GATE_DURATION:-600}
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
emit_fail() { emit_line FAIL "$@"; }

name=${name:-gate}
gate_scratch=''
gate_remove_scratch=no
gate_verbose=''
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

gate_exit() {
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
  ( "$@" ) > /dev/null 2>&1
  case $? in 0) st_got=PASS ;; 3) st_got=NO-INPUT ;; *) st_got=FAIL ;; esac
  st_cases=$((st_cases + 1))
  [ "$st_got" = "$st_want" ] || st_wrong=$((st_wrong + 1))
  [ "$st_want" = PASS ] || st_rejected=$((st_rejected + 1))
  [ -z "$gate_verbose" ] || printf 'case %s/%s want=%s got=%s\n' "$name" "$st_case" "$st_want" "$st_got"
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
      ''|*[!0-9]*) printf 'check-%s.sh: unknown argument: %s\n' "$name" "$1" >&2; exit 64 ;;
      *) seconds=$1 ;;
    esac
    shift
  done
  trap gate_exit EXIT
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
  for kind in screen_frame keystroke pointer foreground_app; do
    count=$(modality_count "$1" "$kind")
    printf '%s=%s ' "$kind" "${count:-absent}"
  done
}

predicate_modalities_present() {
  for kind in screen_frame keystroke pointer foreground_app; do
    count=$(modality_count "$1" "$kind")
    case $count in ''|*[!0-9]*) return 1 ;; esac
    [ "$count" -gt 0 ] || return 1
  done
  return 0
}

marker_scan_dir() {
  [ -n "$2" ] || return 2
  [ -f "$1/store.db" ] || return 2
  [ -f "$1/store.key" ] || return 2
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
  marker_scan_dir "$1" "$2"
}

predicate_marker_not_printed() {
  marker_scan_files "$@"
}

marker_defect() {
  marker_scan_dir "$1" "$2"
  case $? in
    0) ;;
    1) printf 'leak: the marker is in the store directory\n'; return 1 ;;
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
  case " $1 " in *" $2="*) return 0 ;; esac
  return 1
}

report_values() {
  printf '%s\n' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p"
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
  source_line=$(grep -m 1 -E '^halvo-capture: source [a-z_]+ did not start' "$2" 2>/dev/null)
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
  printf '%s\n' "$tree_listing" | awk -F '\t' 'index($2, "docs/baseline/") != 1' | git hash-object --stdin
}

head_read() {
  head_sha=$(git -C "$GATE_ROOT" rev-parse HEAD 2>/dev/null) || return 1
  head_tree=$(tree_id "$GATE_ROOT" HEAD) || return 1
  head_porcelain=$(git -C "$GATE_ROOT" status --porcelain 2>/dev/null) || return 1
  head_state=clean
  predicate_tree_clean "$head_porcelain" || head_state=dirty
  printf '%s %s %s\n' "$head_sha" "$head_tree" "$head_state"
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

run_capture_bin() {
  "$GATE_BIN" "$@"
}

standin_build() {
  standin=$GATE_TARGET/load-standin
  mkdir -p "$GATE_TARGET" || return 1
  cc -O2 -o "$standin" "$GATE_ROOT/scripts/r1-gate/helper/load-standin.c" > "$GATE_TARGET/load-standin.log" 2>&1
}

child_wrapper=''
child_pid=''
child_defect=''
child_status=''

timed_child() {
  child_dir=$1; child_bin=$2; shift 2
  child_defect=''; child_pid=''
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
    sleep 1
  done
  child_mean_kib=0
  [ "$child_samples" -eq 0 ] || child_mean_kib=$((child_sum_kib / child_samples))
  printf '%s %s %s\n' "$child_peak_kib" "$child_mean_kib" "$child_samples" > "$child_dir/samples"
  printf '%s\n' "$child_others" > "$child_dir/others"
  wait "$child_wrapper"
  child_status=$?
  child_wrapper=''
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

helper_pid=''

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
  need_build || return 1
  scratch_root
  run_store=$gate_scratch/store; run_dir=$gate_scratch/run
  mkdir "$run_store" "$run_dir" || { gate_defect="cannot make the scratch store"; return 1; }
  export HALVO_STORE_DIR="$run_store"
}

capture_run() {
  capture_seconds=$1; shift
  if [ "$capture_helper" = yes ]; then
    helper_build || return 1
    helper_start "$((capture_seconds + 60))" "$GATE_TARGET/window-helper" || return 1
  fi
  run_before=$(machine_state)
  timed_child "$run_dir" "$GATE_BIN" --timed-run "$capture_seconds" "$@"
  capture_status=$?
  helper_stop
  [ "$capture_status" -eq 0 ] || return 1
  run_after=$(machine_state)
  child_report_defect
}

run_machine_note() {
  run_others=$(cat "$run_dir/others" 2>/dev/null)
  printf 'start %s, end %s, other halvo-capture=%s, binary=%s\n' "$run_before" "$run_after" "${run_others:-unread}" "${GATE_BIN#"$GATE_ROOT"/}"
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
    if (cpu + 0 > cpu_max + 0) exit 1
    if (growth + 0 > seconds * per_day / 86400) exit 1
    exit 0
  }'
}

# PASS(0) if the CI conclusion is success.
predicate_ci_success() {
  case "$1" in success) return 0 ;; *) return 1 ;; esac
}

source_rule() {
  rule=$1; rule_pattern=$2; rule_allowed=$3; rule_repo=$4
  hits=$(grep -rnE --include='*.rs' -e "$rule_pattern" -- "$rule_repo/crates" "$rule_repo/src-tauri/src" 2>/dev/null)
  case $? in 1) return 0 ;; 0) ;; *) printf 'not scanned: grep could not read the sources for rule %s\n' "$rule"; return 2 ;; esac
  code_hits=$(printf '%s\n' "$hits" | grep -vE '^[^:]*:[0-9]+:[[:space:]]*//')
  case $? in 1) return 0 ;; 0) ;; *) printf 'not scanned: the comment filter failed for rule %s\n' "$rule"; return 2 ;; esac
  left=$(printf '%s\n' "$code_hits" | sed -e "$rule_allowed" | grep -E -e "$rule_pattern")
  case $? in 1) return 0 ;; 0) ;; *) printf 'not scanned: the second read failed for rule %s\n' "$rule"; return 2 ;; esac
  where=$(printf '%s\n' "$left" | sed -n '1s/^\([^:]*:[0-9]*\):.*/\1/p')
  printf '%s at %s\n' "$rule" "${where#"$rule_repo"/}"
  return 1
}

source_scan_defect() {
  scan_repo=$1
  for scan_root in crates src-tauri/src crates/halvo-capture/src/macos; do
    [ -d "$scan_repo/$scan_root" ] || { printf 'not scanned: %s is missing\n' "$scan_root"; return 2; }
  done
  first_rs=$(find "$scan_repo/crates" "$scan_repo/src-tauri/src" -type f -name '*.rs' 2>/dev/null | head -1)
  [ -n "$first_rs" ] || { printf 'not scanned: no .rs file\n'; return 2; }
  source_rule windows-hook 'SetWindowsHookEx|WH_KEYBOARD_LL|WH_MOUSE_LL' 's/^//' "$scan_repo" || return $?
  source_rule active-tap 'CGEventTapOptions::|kCGEventTapOptionDefault|CGEventTapCreate|CGEventPost' 's/CGEventTapOptions::ListenOnly\([^A-Za-z0-9_]\)/\1/g; s/CGEventTapOptions::ListenOnly$//' "$scan_repo" || return $?
  source_rule audio-capture 'setCapturesAudio|setCaptureMicrophone|SCStreamOutputType::' 's/SCStreamOutputType::Screen\([^A-Za-z0-9_]\)/\1/g; s/SCStreamOutputType::Screen$//' "$scan_repo" || return $?
  source_rule local-machine-key 'CRYPTPROTECT_LOCAL_MACHINE' 's/^//' "$scan_repo" || return $?
  grep -rqF --include='*.rs' -e 'CGEventTapOptions::ListenOnly' -- "$scan_repo/crates/halvo-capture/src/macos" 2>/dev/null
  case $? in
    0) return 0 ;;
    1) printf 'listen-only-absent under crates/halvo-capture/src/macos\n'; return 1 ;;
    *) printf 'not scanned: grep could not read crates/halvo-capture/src/macos\n'; return 2 ;;
  esac
}

predicate_no_ll_hook() {
  source_scan_defect "$1" > /dev/null
}
