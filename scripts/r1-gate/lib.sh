# shellcheck shell=sh
# Shared logic for the R1 gate harness.
#
# This file is the measuring instrument. Every sub-check decides PASS or FAIL by
# calling one of the predicate functions below, and `run.sh --self-test` proves
# those predicates on fixtures. The predicates take data (a report file, a
# directory, a number) and never acquire it, so the same logic runs against a
# fixture in self-test and against the real pipeline in a measured run.
#
# POSIX sh only. Safe to source more than once.

# Repo root (scripts/r1-gate/ is two levels below it).
GATE_ROOT=${GATE_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}
# origin repo slug, for the Windows CI sub-check.
GATE_REPO=${GATE_REPO:-wth-lg/halvo}
# Default measured-run length. The whole-plan gate uses 600s; callers override.
GATE_DURATION=${GATE_DURATION:-600}

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
  printf '%s\n' "$line"
}
emit_pass() { emit_line PASS "$@"; }
emit_fail() { emit_line FAIL "$@"; }

name=${name:-gate}
gate_scratch=''
gate_remove_scratch=no
gate_verbose=''
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
      ''|*[!0-9]*) printf 'check-%s.sh: unknown argument: %s\n' "$name" "$1" >&2; exit 64 ;;
      *) seconds=$1 ;;
    esac
    shift
  done
  trap gate_exit EXIT
  if [ "$mode" = self-test ]; then selftest_run; else measure "$seconds"; fi
}

# Run the capture bin from the repo root. Returns its exit code; the crate not
# existing yet (the P0 baseline) makes cargo fail here, which the caller treats
# as FAIL.
run_capture_bin() {
  ( cd "$GATE_ROOT" && cargo run -q -p halvo-capture --bin halvo-capture -- "$@" )
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

predicate_zero_secure_keystrokes() {
  count=$(sed -n 's/^secure_input_keystrokes=\([0-9][0-9]*\)$/\1/p' "$1" 2>/dev/null)
  [ "$count" = 0 ]
}

predicate_cost_within_bound() {
  cpu=$1; disk=$2; seconds=$3; cpu_max=$4; per_day=$5
  cpu_x10=$(printf '%s' "$cpu" | awk '{printf "%d", $1*10}')
  max_x10=$((cpu_max * 10))
  [ "$cpu_x10" -le "$max_x10" ] || return 1
  disk_max=$((seconds * per_day / 86400))
  [ "$disk" -le "$disk_max" ] || return 1
  return 0
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
