#!/bin/sh
# jobs: macos windows
# rule: Every proof line of the content floor fails to build with its named error code and its twin builds, no source outside the secure_input tree implements the probe, the synthetic doors have no caller outside daemon/prefill.rs, and TextFilterPoint has at most one implementor, under daemon/.
set -u
name=30-core-boundary-proofs
crate=crates/halvo-capture
src=$crate/src
proofs_file=$src/floor/boundary_proofs.rs
prefill=$src/daemon/prefill.rs

# name, the error code that its build must report, the build it belongs to.
PROOFS='clearance_made_outside_secure_input E0624 debug
probe_implemented_outside_secure_input E0277 debug
record_from_an_old_path_event E0308 debug
received_value_made_outside_the_sink E0451 debug
emitted_value_without_the_keep_verdict E0624 debug
keep_verdict_of_another_kind E0308 debug
decoded_record_emitted_into_the_sink E0308 debug
fold_record_through_the_blocking_emit E0308 debug
key_event_with_a_character E0308 debug
pointer_event_with_a_coordinate E0061 debug
fold_count_from_an_integer E0308 debug
frame_event_with_bytes E0061 debug
camera_event E0425 debug
captured_text_read_outside_floor E0616 debug
captured_text_from_a_string E0308 debug
app_identity_from_a_string E0308 debug
site_from_a_string E0308 debug
event_id_made_outside_its_maker E0603 debug
synthetic_clearance_in_a_release_build E0425 release
synthetic_keep_in_a_release_build E0425 release'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# The line ranges of the proofs in the text of the proof file on stdin:
# "<name> <first line> <last line>", one line for each cfg of a proof or twin.
spans() {
  tr -d '\r' | awk '
    /^#\[cfg\(halvo_proof = "[a-z0-9_]+"\)\]/ {
      if (open != "") print open, first, NR - 1
      open = $0
      sub(/^#\[cfg\(halvo_proof = "/, "", open)
      sub(/"\)\].*$/, "", open)
      first = NR
    }
    END { if (open != "") print open, first, NR }
  '
}

# Verdict on the short messages of one build of proofs ($1), the spans ($2) and
# the proofs of that build ($3: "<name> <code>" lines). Prints one line for each
# proof and fails when a proof has no error with its code inside its span.
judge() {
  printf '%s\n' "$3" | {
    status=0
    while read -r proof code; do
      [ -n "$proof" ] || continue
      range=$(printf '%s\n' "$2" | awk -v p="$proof" '$1 == p { print $2, $3 }')
      if [ -z "$range" ]; then
        printf 'proof %s missing from %s\n' "$proof" "$proofs_file"
        status=1
        continue
      fi
      first=${range% *}
      last=${range#* }
      hits=$(printf '%s\n' "$1" | tr -d '\r' | awk -v f="$first" -v l="$last" -v c="$code" '
        index($0, "boundary_proofs.rs:") && index($0, "error[" c "]") {
          rest = substr($0, index($0, "boundary_proofs.rs:") + 19)
          split(rest, part, ":")
          if (part[1] + 0 >= f + 0 && part[1] + 0 <= l + 0) n++
        }
        END { print n + 0 }
      ')
      if [ "$hits" -gt 0 ]; then
        printf 'proof %s fails with %s\n' "$proof" "$code"
      else
        printf 'proof %s does not fail with %s\n' "$proof" "$code"
        status=1
      fi
    done
    exit "$status"
  }
}

# Prints the lines of "<file>:<line>:..." on stdin whose file is none of the
# files given as arguments.
outside() {
  awk -v allowed="$*" '
    BEGIN { n = split(allowed, keep, " "); for (i = 1; i <= n; i++) ok[keep[i]] = 1 }
    { file = $0; sub(/:.*/, "", file); gsub(/\\/, "/", file); if (!(file in ok)) print }
  '
}

# The source scans over the tree under $1. Prints counts and fails with a cause.
scan() {
  root=$1
  impls=$(grep -rnE --include='*.rs' 'impl[^{;]*TextFilterPoint[^{;]*[[:space:]]for[[:space:]]' "$root/$src") ||
    impls=
  impls=$(printf '%s\n' "$impls" | sed -e "s|^$root/||" -e '/^$/d' | outside "$src/floor/cases.rs")
  count=$(printf '%s' "$impls" | grep -c '')
  printf 'implementors of TextFilterPoint outside floor/cases.rs: %s\n' "$count"
  [ "$count" -le 1 ] || return 11
  if [ "$count" = 1 ]; then
    case $impls in
      "$src/daemon/"*) ;;
      *) return 12 ;;
    esac
  fi
  doors=$(grep -rnE --include='*.rs' 'synthetic_(clearance|keep)' "$root/$src") || doors=
  doors=$(printf '%s\n' "$doors" | sed -e "s|^$root/||" -e '/^$/d' | grep -vE ':[0-9]+:[[:space:]]*pub fn synthetic_(clearance|keep)')
  callers=$(printf '%s\n' "$doors" | sed '/^$/d' | outside "$proofs_file")
  count=$(printf '%s' "$callers" | grep -c '')
  printf 'callers of the synthetic doors outside the proof file: %s\n' "$count"
  stray=$(printf '%s\n' "$callers" | sed '/^$/d' | outside "$prefill")
  [ -z "$stray" ] || return 13
  probes=$(grep -rnE --include='*.rs' 'impl[^{;]*SecureInputProbe[^{;]*[[:space:]]for[[:space:]]' "$root/$src") ||
    probes=
  probes=$(printf '%s\n' "$probes" | sed -e "s|^$root/||" -e '/^$/d')
  stray=$(printf '%s\n' "$probes" | sed '/^$/d' |
    outside "$src/secure_input.rs" "$src/secure_input_macos.rs" "$src/windows/secure_input.rs" "$proofs_file")
  count=$(printf '%s' "$probes" | grep -c '')
  printf 'implementors of SecureInputProbe: %s\n' "$count"
  [ -z "$stray" ] || return 14
  return 0
}

scan_cause() {
  case $1 in
    11) printf 'TextFilterPoint has more than one implementor outside floor/cases.rs' ;;
    12) printf 'TextFilterPoint has an implementor outside daemon/' ;;
    13) printf 'a synthetic door has a caller outside daemon/prefill.rs' ;;
    14) printf 'SecureInputProbe has an implementor outside the secure_input module tree' ;;
    *) printf 'the source scan stopped with %s' "$1" ;;
  esac
}

# One check build of the library with the cfgs of the proofs named in $2 and
# the extra rustc arguments in $3; its short messages go to $1.
build() {
  build_log=$1
  build_list=$2
  build_extra=$3
  set --
  for build_proof in $build_list; do
    set -- "$@" --cfg "halvo_proof=\"$build_proof\""
  done
  for build_arg in $build_extra; do
    set -- "$@" "$build_arg"
  done
  cargo rustc --locked -p halvo-capture --lib --profile check --message-format short -- "$@" >"$build_log" 2>&1
}

clean_tree() {
  changed=$(git status --porcelain --untracked-files=no -- "$crate") || return 1
  [ -z "$changed" ]
}

measure() {
  cd "${CI_D_REPO:-.}" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  out=${CI_D_ARTIFACTS:-$work}
  clean_tree || fail "a tracked file of $crate differs from HEAD before the build"
  committed=$(git show "HEAD:$proofs_file") || fail "HEAD has no $proofs_file"
  spans_text=$(printf '%s\n' "$committed" | spans)
  ran=$(printf '%s\n' "$PROOFS" | grep -c '')
  [ "$ran" -gt 0 ] || fail "no proof to run"
  scan . >"$work/scan.txt"
  status=$?
  cat "$work/scan.txt"
  if [ "$status" != 0 ]; then
    cause=$(scan_cause "$status")
    fail "$cause"
  fi

  debug=$(printf '%s\n' "$PROOFS" | awk '$3 == "debug" { print $1 }')
  release=$(printf '%s\n' "$PROOFS" | awk '$3 == "release" { print $1 }')
  twins=$(printf '%s\n' "$PROOFS" | awk '{ print $1 "_twin" }')

  build "$out/twins.txt" "$twins" ''
  twin_status=$?
  printf 'twins: %s built with exit %s\n' "$ran" "$twin_status"
  [ "$twin_status" = 0 ] || fail "a twin does not build: see twins.txt"
  build "$out/proofs.txt" "$debug" ''
  printf 'debug proofs: build exit %s\n' "$?"
  build "$out/release-proofs.txt" "$release" '-C debug-assertions=off'
  printf 'release proofs: build exit %s\n' "$?"
  clean_tree || fail "a tracked file of $crate differs from HEAD after the builds"

  failed=0
  messages=$(cat "$out/proofs.txt") || fail "cannot read proofs.txt"
  listed=$(printf '%s\n' "$PROOFS" | awk '$3 == "debug" { print $1, $2 }')
  verdicts=$(judge "$messages" "$spans_text" "$listed") || failed=1
  printf '%s\n' "$verdicts" >"$work/verdicts.txt"
  messages=$(cat "$out/release-proofs.txt") || fail "cannot read release-proofs.txt"
  listed=$(printf '%s\n' "$PROOFS" | awk '$3 == "release" { print $1, $2 }')
  verdicts=$(judge "$messages" "$spans_text" "$listed") || failed=1
  printf '%s\n' "$verdicts" >>"$work/verdicts.txt"
  if [ "$failed" = 1 ]; then
    stuck=$(awk '/ does not fail / { print $2 }' "$work/verdicts.txt")
    for proof in $stuck; do
      mode=$(printf '%s\n' "$PROOFS" | awk -v p="$proof" '$1 == p { print $3 }')
      extra=
      [ "$mode" = release ] && extra='-C debug-assertions=off'
      build "$out/alone-$proof.txt" "$proof" "$extra"
      code=$(printf '%s\n' "$PROOFS" | awk -v p="$proof" '$1 == p { print $2 }')
      messages=$(cat "$out/alone-$proof.txt") || fail "cannot read alone-$proof.txt"
      if judge "$messages" "$spans_text" "$proof $code" >/dev/null; then
        sed "s/^proof $proof does not fail with .*/proof $proof fails with $code (built alone)/" "$work/verdicts.txt" >"$work/verdicts.new"
        mv "$work/verdicts.new" "$work/verdicts.txt"
      fi
    done
  fi
  cat "$work/verdicts.txt"
  missing=$(grep -c ' does not fail \| missing from ' "$work/verdicts.txt")
  passed=$(grep -c ' fails with ' "$work/verdicts.txt")
  printf 'proofs failing with their code: %s of %s; twins building: %s\n' "$passed" "$ran" "$ran"
  [ "$missing" = 0 ] || fail "$missing proof lines do not fail with their code"
  [ "$passed" = "$ran" ] || fail "ran $passed proofs of $ran"
  printf 'PASS %s\n' "$name"
}

case_of() {
  label=$1
  want=$2
  shift 2
  got=PASS
  "$@" >/dev/null 2>&1 || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
  elif [ "$got" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

plant() {
  mkdir -p "$(dirname "$1")" || exit 2
  printf '%s\n' "$2" >"$1" || exit 2
}

tree_case() {
  root=$work/tree-$1
  plant "$root/$src/floor/cases.rs" 'impl TextFilterPoint for RecordingPoint {}'
  plant "$root/$src/secure_input.rs" 'pub fn synthetic_clearance() -> KeystrokeClearance {
impl SecureInputProbe for FixedProbe {}'
  plant "$root/$src/windows/secure_input.rs" 'impl SecureInputProbe for PasswordFieldProbe {}'
  plant "$root/$src/floor/boundary_proofs.rs" 'crate::secure_input::synthetic_clearance()
impl SecureInputProbe for Elsewhere {}'
  plant "$root/$src/floor/exclude.rs" 'pub fn synthetic_keep<K: Recorded>(subject: Subject) -> Keep<K> {'
  if [ -n "$2" ]; then
    plant "$root/$2" "$3"
  fi
  scan "$root"
}

self_test() {
  rejected=0
  wrong=
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  text='#[cfg(halvo_proof = "one")]
fn one() {}

#[cfg(halvo_proof = "one_twin")]
fn one_twin() {}
#[cfg(halvo_proof = "two")]
fn two() {}'
  spans_text=$(printf '%s\n' "$text" | spans)
  both='crates/halvo-capture/src/floor/boundary_proofs.rs:2:9: error[E0624]: associated function is private
crates\halvo-capture\src\floor\boundary_proofs.rs:7:1: error[E0308]: mismatched types'
  case_of spans-of-a-file PASS test "$spans_text" = 'one 1 3
one_twin 4 5
two 6 7'
  case_of both-proofs-fail PASS judge "$both" "$spans_text" 'one E0624
two E0308'
  case_of a-proof-that-builds FAIL judge "$both" "$spans_text" 'one E0624
two E0308
one_twin E0308'
  case_of another-code FAIL judge "$both" "$spans_text" 'one E0308'
  case_of an-error-outside-the-span FAIL judge 'crates/halvo-capture/src/floor/boundary_proofs.rs:9:1: error[E0624]: x' "$spans_text" 'one E0624'
  case_of an-error-in-another-file FAIL judge 'crates/halvo-capture/src/sink.rs:2:1: error[E0624]: x' "$spans_text" 'one E0624'
  case_of a-proof-not-in-the-file FAIL judge "$both" "$spans_text" 'three E0624'
  case_of the-tree-at-this-unit PASS tree_case clean '' ''
  case_of a-second-caller-of-a-door FAIL tree_case second-door "$src/macos/input.rs" '    let clearance = crate::secure_input::synthetic_clearance();'
  case_of a-caller-in-prefill PASS tree_case prefill "$prefill" '    let keep = crate::floor::exclude::synthetic_keep::<kind::Keystroke>(subject);'
  case_of a-text-filter-implementor-in-daemon PASS tree_case daemon-point "$src/daemon/batch.rs" 'impl TextFilterPoint for TextFilters {'
  case_of a-text-filter-implementor-outside-daemon FAIL tree_case store-point "$src/store/db.rs" 'impl TextFilterPoint for Passthrough {'
  case_of a-probe-implementor-on-windows FAIL tree_case windows-probe "$src/windows/input.rs" 'impl crate::secure_input::SecureInputProbe for Always {'
  case_of a-probe-implementor-on-macos FAIL tree_case macos-probe "$src/macos/input.rs" 'impl<T> SecureInputProbe for Inactive<T> {'
  root=$work/tree-two-points
  tree_case two-points "$src/daemon/batch.rs" 'impl TextFilterPoint for TextFilters {' >/dev/null 2>&1
  plant "$root/$src/daemon/owner.rs" 'impl TextFilterPoint for Second {'
  case_of a-second-text-filter-implementor FAIL scan "$root"
  case_of no-proof FAIL judge '' "$spans_text" 'one E0624'
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
