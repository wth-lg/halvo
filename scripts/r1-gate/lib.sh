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

# Cost bounds. Derived, not measured on Halvo yet.
#   CPU: 20% of one core is the upper end of Screenpipe's self-reported 5-20%
#        capture CPU (researcher-3 findings 2 and 37). ps reports %cpu where 100
#        is one core, so the ceiling is 20.
#   Disk: 2 GB/day is Screenpipe's own storage-planning example (researcher-3
#        finding 4). Over an N-second run the ceiling is N * 2e9 / 86400 bytes
#        (13.9 MB at 600s). These ceilings hold until Halvo's measured R1 run
#        replaces them with its own figures.
GATE_CPU_MAX_PCT=${GATE_CPU_MAX_PCT:-20}
GATE_DISK_BYTES_PER_DAY=${GATE_DISK_BYTES_PER_DAY:-2000000000}

# Modalities a timed run must record for the capture sub-check to pass.
GATE_REQUIRED_MODALITIES="screen_frame keystroke pointer foreground_app"

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

emit_pass() { printf 'PASS %s\n' "$1"; }
emit_fail() { if [ $# -gt 1 ]; then printf 'FAIL %s (%s)\n' "$1" "$2"; else printf 'FAIL %s\n' "$1"; fi; }

# Run the capture bin from the repo root. Returns its exit code; the crate not
# existing yet (the P0 baseline) makes cargo fail here, which the caller treats
# as FAIL.
run_capture_bin() {
  ( cd "$GATE_ROOT" && cargo run -q -p halvo-capture --bin halvo-capture -- "$@" )
}

# Total bytes of a directory tree (0 for an empty or missing dir).
dir_bytes() {
  [ -d "$1" ] || { printf '0\n'; return 0; }
  du -sk "$1" 2>/dev/null | awk '{print $1*1024; found=1} END{if(!found) print 0}'
}

# PASS(0) if every required modality has a positive count in the report.
predicate_modalities_present() {
  report=$1
  for m in $GATE_REQUIRED_MODALITIES; do
    count=$(sed -n "s/^modality=$m count=\\([0-9][0-9]*\\)\$/\\1/p" "$report" 2>/dev/null | head -1)
    { [ -n "$count" ] && [ "$count" -gt 0 ]; } || return 1
  done
  return 0
}

# PASS(0) if the plaintext marker appears in no file under the store dir.
predicate_marker_absent() {
  dir=$1; marker=$2
  grep -rqF -- "$marker" "$dir" 2>/dev/null && return 1
  return 0
}

# PASS(0) if the file does not contain the plaintext marker (a proxy for "this
# is ciphertext to another reader").
predicate_ciphertext() {
  file=$1; marker=$2
  grep -qF -- "$marker" "$file" 2>/dev/null && return 1
  return 0
}

# The value of key=value in a report line; empty if the key is absent.
report_field() {
  printf '%s\n' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p" | head -1
}

# Print each key whose value in the report differs from the key=expected pairs
# that follow it; prints nothing when the report meets every pair. A missing
# key is a defect, so a report that omits a field cannot pass.
report_defects() {
  line=$1; shift
  for want in "$@"; do
    key=${want%%=*}; expected=${want#*=}
    [ "$(report_field "$line" "$key")" = "$expected" ] || printf '%s ' "$key"
  done
}

# What --verify-store must have measured: the owner read back the segment it
# wrote, SQLCipher ran on libtomcrypt with secure_delete on, and the unwrap
# returned without a custody prompt.
GATE_STORE_SPEC="segment_roundtrip=ok cipher_provider=libtomcrypt secure_delete=1 fts_secure_delete=1 unwrap=ok prompt=none"

# What --verify-deletion must have measured: the shredded segment's ciphertext
# decrypts under no remaining key and its file is gone, the live sibling still
# reads, the WAL holds no frames after the shred, and delete-all left a store
# key that no longer unwraps and a directory that opens as sealed.
GATE_DELETION_SPEC="decryptable=no file_gone=yes sibling_readable=yes wal_bytes=0 root_destroyed=yes store_sealed=yes"

# PASS(0) if the --verify-store report meets GATE_STORE_SPEC.
predicate_store_report() {
  # shellcheck disable=SC2086
  [ -z "$(report_defects "$1" $GATE_STORE_SPEC)" ]
}

# PASS(0) if the --verify-deletion report meets GATE_DELETION_SPEC.
predicate_deletion_report() {
  # shellcheck disable=SC2086
  [ -z "$(report_defects "$1" $GATE_DELETION_SPEC)" ]
}

# PASS(0) if zero keystrokes were recorded while secure input was active.
predicate_zero_secure_keystrokes() {
  report=$1
  count=$(sed -n 's/^secure_input_keystrokes=\([0-9][0-9]*\)$/\1/p' "$report" 2>/dev/null | head -1)
  [ "$count" = 0 ]
}

# PASS(0) if mean CPU is within the ceiling and disk growth within the derived
# rate bound for the run length.
predicate_cost_within_bound() {
  cpu=$1; disk=$2; seconds=$3
  cpu_x10=$(printf '%s' "$cpu" | awk '{printf "%d", $1*10}')
  max_x10=$((GATE_CPU_MAX_PCT * 10))
  [ "$cpu_x10" -le "$max_x10" ] || return 1
  bound=$((seconds * GATE_DISK_BYTES_PER_DAY / 86400))
  [ "$disk" -le "$bound" ] || return 1
  return 0
}

# PASS(0) if the CI conclusion is success.
predicate_ci_success() {
  case "$1" in success) return 0 ;; *) return 1 ;; esac
}

# PASS(0) if no low-level global input hook API appears under the given source
# dir. The Windows backend is a listen-only Raw Input path; SetWindowsHookEx or a
# WH_KEYBOARD_LL / WH_MOUSE_LL hook would be a lower-level global hook the crate
# forbids (comment-hunter-3 DELETE). The pattern allows them inside this file's
# own word boundaries by scanning .rs sources only.
predicate_no_ll_hook() {
  dir=$1
  grep -rqE --include='*.rs' 'SetWindowsHookEx|WH_KEYBOARD_LL|WH_MOUSE_LL' "$dir" 2>/dev/null && return 1
  return 0
}

predicate_tree_lines() {
  for required_name in $2; do
    printf '%s\n' "$1" | grep -q "^$required_name v" || {
      printf 'required line missing: %s v\n' "$required_name"
      return 1
    }
  done
  for forbidden_name in $3; do
    if forbidden_line=$(printf '%s\n' "$1" | grep "^$forbidden_name v"); then
      printf 'forbidden crate line: %s\n' "$forbidden_line" | sed 1q
      return 1
    fi
  done
  return 0
}

gate_bound() {
  bounds=$GATE_ROOT/docs/baseline/r1-gate-bounds.txt
  [ -f "$bounds" ] || return 1
  [ "$(grep -c "^$1=[0-9][0-9]*\$" "$bounds")" = 1 ] || return 1
  sed -n "s/^$1=\\([0-9][0-9]*\\)\$/\\1/p" "$bounds"
}
