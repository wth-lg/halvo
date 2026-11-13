#!/bin/sh
# jobs: macos windows
# rule: Every proof line of the content floor, of the daemon's port and of the store's boundary (the store's database type, its key file and its segment files cannot be named outside crate::store, by a path, a glob, a re-export or a macro) fails to build with its named error code and its twin builds, the daemon's in the macos job alone, where a build without tests holds src/daemon/. In the macos job every file under the crate's src/daemon/ that the dep-info of the lib's test build lists is listed by the dep-info of its product build too, or is one of the test-support files model_store.rs, owner_props.rs and scripted.rs.The name searches read the tracked files of HEAD through git: the synthetic doors stand only at their two definitions, in the proof file, in daemon/prefill.rs and in this check and the changelog; TextFilterPoint only under the crate's src/floor/ and src/daemon/ and in those two files; SecureInputProbe only in its five files and those two; and the proof file holds every proof and its twin. The tests of crates/halvo-capture/tests/source_rules.rs read as tokens every file that the compiler can read there: TextFilterPoint has at most one implementor outside floor/cases.rs, under src/daemon/, SecureInputProbe has implementors only in the secure_input tree and the proof file, neither trait is renamed with as or stands inside an attribute or a macro, and #[path] and the include macros stand only at their five listed uses.
set -u
name=30-core-boundary-proofs
crate=crates/halvo-capture
src=$crate/src
proofs_file=$src/floor/boundary_proofs.rs
prefill=$src/daemon/prefill.rs
# The files that are no code and may name the guarded items.
nocode="scripts/r1/ci.d/$name.sh CHANGELOG.md"
# The definitions of the two synthetic doors, each a file and its whole line.
clearance_door="$src/secure_input.rs:pub fn synthetic_clearance(age: Option<Duration>) -> KeystrokeClearance {"
keep_door="$src/floor/exclude.rs:pub fn synthetic_keep<K: Recorded>(subject: Subject) -> Keep<K> {"
# The files that may name SecureInputProbe.
probe_files="$src/secure_input.rs $src/secure_input_macos.rs $src/windows/secure_input.rs $src/floor/exclude.rs $proofs_file"
# The tests of the source rules, which read the files as the compiler reads them.
source_tests='the_reader_counts_an_implementor_however_it_is_spelled
the_reader_finds_a_path_attribute_and_an_include_however_they_are_spelled
the_text_filter_point_has_one_implementor_at_most_and_it_is_under_daemon
the_secure_input_probe_is_implemented_only_in_the_secure_input_tree
path_attributes_and_include_macros_stand_only_at_the_listed_uses'

# name, the error code that its build must report, the build it belongs to.
PROOFS='clearance_made_outside_secure_input E0624 debug
probe_implemented_outside_secure_input E0603 debug
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
segment_write_filled_outside_the_daemon E0451 daemon
batch_filled_outside_the_daemon E0624 daemon
store_database_named_by_its_path E0603 debug
store_key_file_named_by_its_path E0603 debug
store_segment_files_named_by_their_module E0603 debug
store_database_named_after_a_glob_of_the_crate_root E0603 debug
store_database_named_through_a_glob_of_the_store E0603 debug
store_database_named_by_a_macro_of_path_segments E0603 debug
store_database_named_by_an_alias_in_a_brace_list E0603 debug
store_database_re_exported_under_a_new_name E0603 debug
synthetic_clearance_in_a_release_build E0425 release
synthetic_keep_in_a_release_build E0425 release'
proof_names=$(printf '%s\n' "$PROOFS" | awk '{ printf "%s ", $1 }')
# The files under src/daemon/ that the lib's test build alone may read.
test_support="$src/daemon/model_store.rs $src/daemon/owner_props.rs $src/daemon/scripted.rs"

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

# Prints the proofs and twins of the names in $1 (separated by spaces) that
# the proof file on stdin lacks, and exits 17 when it lacks one.
twins_missing() {
  awk -v names="$1" '
    { sub(/\r$/, "") }
    /^#\[cfg\(halvo_proof = "[a-z0-9_]+"\)\]/ {
      proof = $0
      sub(/^#\[cfg\(halvo_proof = "/, "", proof)
      sub(/"\)\].*$/, "", proof)
      have[proof] = 1
    }
    END {
      n = split(names, list, " ")
      for (i = 1; i <= n; i++) {
        if (!(list[i] in have)) lacks = lacks " " list[i]
        if (!((list[i] "_twin") in have)) lacks = lacks " " list[i] "_twin"
      }
      if (lacks != "") {
        print substr(lacks, 2)
        exit 17
      }
    }
  '
}

# The name searches over the tracked files of the tree $2 in the repository $1.
# Prints the count line of the doors and, when a rule breaks, the places that
# break it on the last line, and returns 0 or a code that scan_cause names.
scan() {
  hits=$(git -C "$1" grep -n -a -E -e 'synthetic_(clearance|keep)' -e TextFilterPoint -e SecureInputProbe "$2" --)
  code=$?
  [ "$code" -le 1 ] || return 18
  printf '%s\n' "$hits" | awk -v clearance="$clearance_door" -v keep="$keep_door" -v nocode="$nocode" \
    -v proofs="$proofs_file" -v prefill="$prefill" -v floor="$src/floor/" -v daemon="$src/daemon/" \
    -v probes="$probe_files" '
    BEGIN {
      n = split(nocode, list, " ")
      for (i = 1; i <= n; i++) skip[list[i]] = 1
      n = split(probes, list, " ")
      for (i = 1; i <= n; i++) probe[list[i]] = 1
    }
    $0 != "" {
      sub(/\r$/, "")
      sub(/^[^:]*:/, "")
      file = $0
      sub(/:.*/, "", file)
      rest = substr($0, length(file) + 2)
      line = rest
      sub(/:.*/, "", line)
      text = substr(rest, length(line) + 2)
      if (file in skip) next
      whole = file ":" text
      if (text ~ /synthetic_(clearance|keep)/ && whole != clearance && whole != keep && file != proofs) {
        callers++
        if (file != prefill) doors = doors " " file ":" line
      }
      if (text ~ /TextFilterPoint/ && index(file, floor) != 1 && index(file, daemon) != 1) points = points " " file ":" line
      if (text ~ /SecureInputProbe/ && !(file in probe)) probes_at = probes_at " " file ":" line
    }
    END {
      printf "callers of the synthetic doors outside the proof file: %d\n", callers
      if (doors != "") { print substr(doors, 2); exit 13 }
      if (points != "") { print substr(points, 2); exit 15 }
      if (probes_at != "") { print substr(probes_at, 2); exit 16 }
    }
  '
  code=$?
  [ "$code" = 0 ] || return "$code"
  committed=$(git -C "$1" show "$2:$proofs_file") || return 18
  printf '%s\n' "$committed" | twins_missing "$proof_names"
}

scan_cause() {
  case $1 in
    13) printf 'a synthetic door has a caller outside daemon/prefill.rs' ;;
    15) printf 'TextFilterPoint is named outside src/floor/ and src/daemon/' ;;
    16) printf 'SecureInputProbe is named outside its five files' ;;
    17) printf 'the proof file lacks a proof or a twin' ;;
    18) printf 'git cannot read the tree' ;;
    *) printf 'the source scan stopped with %s' "$1" ;;
  esac
}

# Runs the tests of the source rules by their names; the output goes to $1.
source_rules() {
  source_log=$1
  set --
  for source_test in $source_tests; do
    set -- "$@" "$source_test"
  done
  cargo test --workspace --locked --test source_rules -- --exact --nocapture --test-threads=1 "$@" >"$source_log.raw" 2>&1
  source_status=$?
  tr -d '\r' <"$source_log.raw" >"$source_log" || return 2
  return "$source_status"
}

# The verdict on the output $1 of the source-rule tests: prints the lines that
# the tests print and fails unless every test of $source_tests ran and passed.
source_verdict() {
  sed -n -e 's/^.*\(implementors of .*: [0-9][0-9]*\)$/\1/p' \
    -e 's/^.*\(uses of path attributes and include macros: .*\)$/\1/p' "$1"
  source_want=$(printf '%s\n' "$source_tests" | grep -c '')
  grep -q -x "test result: ok\. $source_want passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in .*" "$1" ||
    return 1
  printf 'source rules: %s of %s tests passed\n' "$source_want" "$source_want"
}

# The names of the source-rule tests that the output $1 lists as failed.
source_refusals() {
  refused=
  for source_test in $source_tests; do
    if grep -q -x "    $source_test" "$1"; then
      refused="$refused $source_test"
    fi
  done
  printf '%s' "${refused# }"
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

# The dep-info file that cargo wrote for the lib target of halvo-capture, from
# the JSON messages of a build on stdin; nothing when the messages name none.
depinfo_of() {
  tr -d '\r' | grep -F '"reason":"compiler-artifact"' | grep -F '"name":"halvo_capture"' |
    grep -F '"kind":["lib"]' | sed -n 's/.*"filenames":\[\([^]]*\)\].*/\1/p' | tr ',' '\n' | tr -d '"' |
    grep -F '/deps/' | awk 'NR == 1 {
      n = split($0, part, "/")
      file = part[n]
      sub(/^lib/, "", file)
      sub(/\.(rlib|rmeta|exe)$/, "", file)
      print substr($0, 1, length($0) - length(part[n])) file ".d"
    }'
}

# The files under src/daemon/ that the dep-info file $1 lists, one a line.
daemon_reads() {
  tr -d '\r' <"$1" | sed -n '1p' | tr ' ' '\n' |
    sed -n "s|^.*$src/daemon/|$src/daemon/|p" | sort -u
}

# The verdict on the daemon's files that the compiler read: the dep-info of the
# product build is named by the JSON messages in $1, that of the lib's test
# build by $2, and the lists go to $3. Prints the counts, and each file that the
# test build alone reads outside $test_support, and fails on one.
module_verdict() {
  product_d=$(depinfo_of <"$1")
  test_d=$(depinfo_of <"$2")
  if [ ! -f "$product_d" ] || [ ! -f "$test_d" ]; then
    printf 'the builds name no dep-info file of the lib\n'
    return 1
  fi
  daemon_reads "$product_d" >"$3/product-daemon-files.txt" || return 1
  daemon_reads "$test_d" >"$3/test-daemon-files.txt" || return 1
  if ! grep -q -x -F "$src/daemon/mod.rs" "$3/product-daemon-files.txt"; then
    printf 'the product build read no %s\n' "$src/daemon/mod.rs"
    return 1
  fi
  awk -v support="$test_support" -v product="$3/product-daemon-files.txt" '
    BEGIN { n = split(support, list, " "); for (i = 1; i <= n; i++) allowed[list[i]] = 1 }
    FILENAME == product { read[$0] = 1; reads++; next }
    { tests++ }
    !($0 in read) && !($0 in allowed) { print "read by the test build alone: " $0; bad++ }
    END {
      printf "daemon files read: product build %d, test build %d, test build alone outside the support list %d\n", reads, tests, bad
      exit bad > 0
    }
  ' "$3/product-daemon-files.txt" "$3/test-daemon-files.txt"
}

# The two builds of the lib whose dep-info module_verdict reads; their JSON
# messages go to $1/product-build.json and $1/test-build.json.
module_builds() {
  cargo build --locked -p halvo-capture --lib --message-format json >"$1/product-build.json" 2>"$1/product-build.err" ||
    return 1
  cargo test --locked -p halvo-capture --lib --no-run --message-format json >"$1/test-build.json" 2>"$1/test-build.err"
}

clean_tree() {
  changed=$(git status --porcelain --untracked-files=no -- "$crate" src-tauri/src src-tauri/build.rs) || return 1
  [ -z "$changed" ]
}

measure() {
  cd "${CI_D_REPO:-.}" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  out=${CI_D_ARTIFACTS:-$work}
  clean_tree || fail "a tracked file of $crate or of src-tauri differs from HEAD before the build"
  committed=$(git show "HEAD:$proofs_file") || fail "HEAD has no $proofs_file"
  spans_text=$(printf '%s\n' "$committed" | spans)
  here=$(uname -s)
  if [ "$here" = Darwin ]; then
    runs=$PROOFS
  else
    runs=$(printf '%s\n' "$PROOFS" | awk '$3 != "daemon"')
    away=$(printf '%s\n' "$PROOFS" | awk '$3 == "daemon"' | grep -c '')
    printf 'daemon proofs: %s, run in the macos job, where a build without tests holds src/daemon/\n' "$away"
  fi
  ran=$(printf '%s\n' "$runs" | grep -c '')
  [ "$ran" -gt 0 ] || fail "no proof to run"
  scan . HEAD >"$work/scan.txt"
  status=$?
  cat "$work/scan.txt"
  if [ "$status" != 0 ]; then
    cause=$(scan_cause "$status")
    case $status in
      13 | 15 | 16 | 17)
        places=$(tail -n 1 "$work/scan.txt") || fail "cannot read scan.txt"
        cause="$cause: $places"
        ;;
    esac
    fail "$cause"
  fi
  source_rules "$out/source-rules.txt"
  source_status=$?
  if ! source_verdict "$out/source-rules.txt"; then
    sed -n '/^running [0-9]* tests*$/,/^test result: /p' "$out/source-rules.txt"
    refused=$(source_refusals "$out/source-rules.txt")
    [ -n "$refused" ] ||
      fail "the tests of $crate/tests/source_rules.rs did not all run: cargo exited $source_status, see source-rules.txt"
    fail "the source rules of $crate/tests/source_rules.rs refuse the tree: $refused"
  fi
  [ "$source_status" = 0 ] || fail "cargo exited $source_status after every source rule passed"
  if [ "$here" = Darwin ]; then
    module_builds "$out" || fail "a build of the lib for its dep-info failed: see product-build.err and test-build.err"
    module_verdict "$out/product-build.json" "$out/test-build.json" "$out" ||
      fail "the daemon files that the compiler read for the lib's two builds break the rule: see the line above"
  else
    printf 'daemon files read: run in the macos job, where a build without tests holds src/daemon/\n'
  fi

  debug=$(printf '%s\n' "$runs" | awk '$3 == "debug" || $3 == "daemon" { print $1 }')
  release=$(printf '%s\n' "$runs" | awk '$3 == "release" { print $1 }')
  twins=$(printf '%s\n' "$runs" | awk '{ print $1 "_twin" }')

  build "$out/twins.txt" "$twins" ''
  twin_status=$?
  printf 'twins: %s built with exit %s\n' "$ran" "$twin_status"
  [ "$twin_status" = 0 ] || fail "a twin does not build: see twins.txt"
  build "$out/proofs.txt" "$debug" ''
  printf 'debug proofs: build exit %s\n' "$?"
  build "$out/release-proofs.txt" "$release" '-C debug-assertions=off'
  printf 'release proofs: build exit %s\n' "$?"
  clean_tree || fail "a tracked file of $crate or of src-tauri differs from HEAD after the builds"

  failed=0
  messages=$(cat "$out/proofs.txt") || fail "cannot read proofs.txt"
  listed=$(printf '%s\n' "$runs" | awk '$3 == "debug" || $3 == "daemon" { print $1, $2 }')
  verdicts=$(judge "$messages" "$spans_text" "$listed") || failed=1
  printf '%s\n' "$verdicts" >"$work/verdicts.txt"
  messages=$(cat "$out/release-proofs.txt") || fail "cannot read release-proofs.txt"
  listed=$(printf '%s\n' "$runs" | awk '$3 == "release" { print $1, $2 }')
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
  mkdir -p "${1%/*}" || exit 2
  printf '%s\n' "$2" >"$1" || exit 2
}

# The files of this unit that the scan reads, in a repository at $work/base
# whose index holds them; their tree is $base_tree.
base_tree() {
  base=$work/base
  plant "$base/$src/secure_input.rs" "${clearance_door#*:}
impl SecureInputProbe for FixedProbe {}"
  plant "$base/$src/windows/secure_input.rs" 'impl SecureInputProbe for PasswordFieldProbe {}'
  plant "$base/$src/floor/exclude.rs" "${keep_door#*:}
    probe: &dyn SecureInputProbe,"
  plant "$base/$src/floor/text.rs" 'pub trait TextFilterPoint {'
  plant "$base/$src/floor/cases.rs" 'impl TextFilterPoint for RecordingPoint {}'
  plant "$base/$proofs_file" "$proof_text"
  plant "$base/scripts/r1/ci.d/$name.sh" 'synthetic_clearance synthetic_keep TextFilterPoint SecureInputProbe'
  plant "$base/CHANGELOG.md" '- Added: the TextFilterPoint of the floor'
  git -C "$base" init -q >/dev/null 2>&1 || exit 2
  git -C "$base" add -A >/dev/null 2>&1 || exit 2
  base_tree=$(git -C "$base" write-tree) || exit 2
}

# Scans the tree of the base with the file $2 planted with the text $3 ($2
# empty: no file). The index goes back to the base tree; the work tree keeps
# the file, which no later tree holds.
tree_case() {
  root=$work/base
  tree=$base_tree
  if [ -n "$2" ]; then
    plant "$root/$2" "$3"
    git -C "$root" add -- "$2" >/dev/null 2>&1 || exit 2
    tree=$(git -C "$root" write-tree) || exit 2
    git -C "$root" read-tree "$base_tree" || exit 2
  fi
  scan "$root" "$tree"
}

# The output of a run of the source-rule tests in which the test $2 fails or,
# with $3 set, is left out, written to $1.
source_output() {
  {
    printf '\nrunning 5 tests\n'
    failed=0
    passed=0
    for source_test in $source_tests; do
      if [ "$source_test" = "$2" ]; then
        if [ -n "${3:-}" ]; then
          continue
        fi
        printf 'test %s ... FAILED\n' "$source_test"
        failed=$((failed + 1))
      else
        printf 'test %s ... implementors of TextFilterPoint outside floor/cases.rs: 0\nok\n' "$source_test"
        passed=$((passed + 1))
      fi
    done
    if [ "$failed" = 0 ]; then
      printf '\ntest result: ok. %s passed; 0 failed; 0 ignored; 0 measured; %s filtered out; finished in 0.94s\n' "$passed" "${3:-0}"
    else
      printf '\nfailures:\n    %s\n\ntest result: FAILED. %s passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.94s\n' "$2" "$passed"
    fi
  } >"$1"
}

# A fixture of module_verdict in $work/$1: the product build reads the files
# $2 and the test build the files $3; with $4 set the product build's messages
# name no artifact of the lib.
module_case() {
  dir=$work/$1
  mkdir -p "$dir" || exit 2
  artifact='{"reason":"compiler-artifact","target":{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture"},"filenames":["'
  if [ -n "${4:-}" ]; then
    printf '{"reason":"build-finished","success":true}\n' >"$dir/product.json"
  else
    printf '%s%s","%s"]}\n' "$artifact" "$dir/libhalvo_capture.rlib" "$dir/deps/libhalvo_capture-p.rmeta" >"$dir/product.json"
  fi
  printf '%s%s"]}\n' "$artifact" "$dir/deps/halvo_capture-t" >"$dir/test.json"
  plant "$dir/deps/halvo_capture-p.d" "$dir/deps/halvo_capture-p.d: $2"
  plant "$dir/deps/halvo_capture-t.d" "$dir/deps/halvo_capture-t.d: $3"
  module_verdict "$dir/product.json" "$dir/test.json" "$dir"
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
  case_of no-proof FAIL judge '' "$spans_text" 'one E0624'

  proof_text=$(printf '%s\n' "$PROOFS" | awk '{
    printf "#[cfg(halvo_proof = \"%s\")]\nfn %s() {}\n#[cfg(halvo_proof = \"%s_twin\")]\nfn %s_twin() {}\n", $1, $1, $1, $1
  }')
  proof_text="$proof_text
    crate::secure_input::synthetic_clearance(None)
    super::exclude::synthetic_keep(subject)
    impl SecureInputProbe for Elsewhere {}
fn twin<P: super::text::TextFilterPoint>() {}"
  base_tree
  case_of the-tree-at-this-unit PASS tree_case clean '' ''
  case_of a-door-caller-outside-prefill FAIL tree_case second-door "$src/macos/input.rs" '    let clearance = crate::secure_input::synthetic_clearance(None);'
  case_of a-door-caller-in-prefill PASS tree_case prefill "$prefill" '    let keep = crate::floor::exclude::synthetic_keep::<kind::Keystroke>(subject);'
  case_of a-door-caller-in-src-tauri FAIL tree_case tauri-door src-tauri/src/main.rs '    let clearance = halvo_capture::secure_input::synthetic_clearance(None);'
  case_of a-function-named-after-a-door-beside-its-definition FAIL tree_case door-prefix "$src/floor/exclude.rs" "${keep_door#*:}
pub fn synthetic_keep_any<K: Recorded>(subject: Subject) -> Keep<K> { synthetic_keep(subject) }"
  case_of a-definition-line-in-another-file FAIL tree_case definition-elsewhere "$src/store/db.rs" "${keep_door#*:}"
  case_of a-definition-line-changed FAIL tree_case definition-changed "$src/floor/exclude.rs" 'pub(crate) fn synthetic_keep<K: Recorded>(subject: Subject) -> Keep<K> {'
  case_of a-door-caller-in-a-text-file FAIL tree_case door-text "$src/floor/extra.txt" 'fn extra() { crate::secure_input::synthetic_clearance(None); }'
  case_of a-door-caller-in-an-included-file FAIL tree_case door-included "$src/store/doors.in" '{ crate::floor::exclude::synthetic_keep::<Keystroke>(subject) }'
  case_of a-door-named-in-the-changelog PASS tree_case door-changelog CHANGELOG.md '- Changed: synthetic_keep reads its site flag.'
  case_of a-text-filter-point-alias-in-event-rs FAIL tree_case point-alias "$src/event.rs" 'use crate::floor::text::TextFilterPoint as Stored;
impl Stored for Passthrough {}'
  case_of a-text-filter-point-in-daemon PASS tree_case daemon-point "$src/daemon/batch.rs" 'impl TextFilterPoint for TextFilters {'
  case_of a-probe-named-outside-its-files FAIL tree_case macos-probe "$src/macos/input.rs" 'impl<T> SecureInputProbe for Inactive<T> {'
  lacking=$(printf '%s\n' "$proof_text" | grep -v -F 'camera_event_twin')
  case_of a-twin-deleted FAIL tree_case twin-deleted "$proofs_file" "$lacking"
  lacking=$(printf '%s\n' "$proof_text" | grep -v -F -e '"camera_event"' -e 'fn camera_event()')
  case_of a-proof-deleted FAIL tree_case proof-deleted "$proofs_file" "$lacking"
  tree_case reverted "$src/store/db.rs" '    let clearance = crate::secure_input::synthetic_clearance(None);' >/dev/null 2>&1
  plant "$root/$src/store/db.rs" '    let clearance = ();'
  case_of a-door-caller-in-the-tree-and-not-in-the-work-tree FAIL scan "$root" "$tree"
  tree_case work-tree-only '' '' >/dev/null 2>&1
  plant "$root/$src/store/db.rs" '    let clearance = crate::secure_input::synthetic_clearance(None);'
  case_of a-door-caller-in-the-work-tree-and-not-in-the-tree PASS scan "$root" "$tree"
  case_of a-tree-that-git-cannot-read FAIL scan "$root" 0000000000000000000000000000000000000000

  source_output "$work/source-ok.txt" '' ''
  case_of source-rules-that-pass PASS source_verdict "$work/source-ok.txt"
  source_output "$work/source-failed.txt" the_text_filter_point_has_one_implementor_at_most_and_it_is_under_daemon ''
  case_of a-source-rule-that-fails FAIL source_verdict "$work/source-failed.txt"
  named=$(source_refusals "$work/source-failed.txt")
  case_of the-failed-source-rule-is-named PASS test "$named" = the_text_filter_point_has_one_implementor_at_most_and_it_is_under_daemon
  source_output "$work/source-left-out.txt" path_attributes_and_include_macros_stand_only_at_the_listed_uses 1
  case_of a-source-rule-left-out FAIL source_verdict "$work/source-left-out.txt"
  printf 'error[E0425]: cannot find value x in this scope\nerror: could not compile halvo-capture (test "source_rules")\n' >"$work/source-unbuilt.txt"
  case_of source-rules-that-never-ran FAIL source_verdict "$work/source-unbuilt.txt"

  d=$src/daemon
  product="$src/lib.rs $d/mod.rs $d/owner.rs"
  case_of daemon-files-read-by-both-builds PASS module_case both "$product" "$product $d/model_store.rs $d/scripted.rs"
  case_of a-module-that-the-test-build-alone-reads FAIL module_case test-alone "$product" "$product $d/v10state_c.rs"
  case_of a-module-under-a-directory-that-the-test-build-alone-reads FAIL module_case test-alone-nested "$product" "$product $d/r2/mod.rs"
  case_of a-product-build-without-the-daemon FAIL module_case no-daemon "$src/lib.rs" "$product"
  case_of a-product-build-that-names-no-dep-info FAIL module_case no-dep-info "$product" "$product" yes
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
