#!/bin/sh
# jobs: macos
# rule: Every stall phase of tests/macos_stall_fixture.rs runs in its own process against the fixture and passes, each listed known fault fails by its own assertion of a recorded key, 12-test ran none of these tests, and neither a build without debug assertions that passes another bound of the hold nor a print of a held key compiles.
set -u
name=51-macos-stall
test_limit_s=${MS_TEST_LIMIT_S:-900}
fixture_source=crates/halvo-capture/tests/macos_stall_fixture/fixture.m
leak='recorded keys beside the markers in rounds that counted'
LC_ALL=C
export LC_ALL

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

expected_tests() {
  cat <<'EOF'
the_probe_reports_what_the_stall_phases_rest_on not-run
a_key_typed_ahead_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded phase
a_key_typed_on_a_return_to_a_secure_field_is_not_recorded three-way
the_up_of_the_key_that_leaves_a_secure_field_is_not_recorded phase
the_control_records_the_keys_typed_in_the_plain_field_and_none_of_the_secure_field phase
at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys phase
known_fault_a_key_typed_ahead_of_a_stall_longer_than_the_bound_is_not_recorded known-fault
known_fault_a_password_typed_whole_ahead_of_a_stall_is_not_recorded known-fault
EOF
}

proof_lines() {
  cat <<'EOF'
hold_bound_in_a_release_build E0599
held_key_printed E0277
EOF
}

seam_manifests() {
  git grep -l -F -e stall-seam -- 'Cargo.toml' '*/Cargo.toml' >"$work/seam" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the manifests" ;; esac
  named=$(tr '\n' ' ' <"$work/seam")
  [ "$named" = "crates/halvo-capture/Cargo.toml " ] ||
    fail "the feature stall-seam must stand in crates/halvo-capture/Cargo.toml alone; manifests that name it: ${named:-none}"
  printf 'seam: the feature stall-seam stands in crates/halvo-capture/Cargo.toml alone\n'
}

tracked_changes() {
  git status --porcelain --untracked-files=no
}

judge() {
  verdict=FAIL
  cause=
  out=$(tr -d '\r' <"$3")
  summary=$(printf '%s\n' "$out" | sed -n 's/^test result: //p' | tail -n 1)
  case $2 in
    phase | three-way)
      if [ "$1" -ne 0 ]; then
        cause="exit $1"
        return 0
      fi
      case $summary in
        'ok. 1 passed; 0 failed;'*) verdict=pass ;;
        *) cause='exit 0, and not one test ran and passed' ;;
      esac
      return 0
      ;;
  esac
  if [ "$1" -eq 0 ]; then
    case $summary in
      'ok. 1 passed; 0 failed;'*)
        verdict=FIXED-BUT-LISTED
        cause='the listed known fault passed'
        ;;
      *) cause='exit 0, and not one test ran and passed' ;;
    esac
    return 0
  fi
  case $1:$summary in
    '101:FAILED. 0 passed; 1 failed;'*) ;;
    *)
      cause="exit $1, and not the exit 101 of one failed test"
      return 0
      ;;
  esac
  case $out in
    *"$leak"*) verdict='known-fault' ;;
    *) cause='the listed known fault failed without its assertion of a recorded key' ;;
  esac
}

run_test() {
  HALVO_STALL_FIXTURE=$fixture perl -e 'alarm shift; exec @ARGV or exit 127' "$test_limit_s" \
    "$exe" --ignored --exact --nocapture --test-threads=1 "$1" </dev/null >"$2" 2>&1
}

proof_build() {
  build_list=$1
  build_log=$2
  set --
  for proof in $build_list; do
    set -- "$@" --cfg "halvo_proof=\"$proof\""
  done
  cargo rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off "$@" >"$build_log" 2>&1
}

twelve_test() {
  list=$CI_D_REPO/target/ci-artifacts/12-test/tests.txt
  if [ ! -f "$list" ]; then
    printf '12-test: no list of tests in this job\n'
    return 0
  fi
  awk '
    /^ *Running / { inside = index($0, "tests/macos_stall_fixture.rs") > 0; next }
    /^ *Doc-tests / { inside = 0; next }
    inside && /^test / { if ($0 ~ / \.\.\. ignored(,|$)/) ignored++; else ran++ }
    END { printf "%d %d\n", ignored, ran }' "$list" >"$work/twelve" || fail "the list of 12-test cannot be read"
  read -r ignored ran <"$work/twelve" || fail "the list of 12-test cannot be read"
  printf '12-test: tests of macos_stall_fixture ignored=%s ran=%s\n' "$ignored" "$ran"
  [ "$ran" -eq 0 ] || fail "12-test ran $ran tests of tests/macos_stall_fixture.rs"
  [ "$ignored" -gt 0 ] || fail "12-test listed no test of tests/macos_stall_fixture.rs as ignored"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  artifacts=${CI_D_ARTIFACTS:-$CI_D_REPO/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  if [ -n "${MS_EXPECTED_FILE:-}" ]; then
    tr -d '\r' <"$MS_EXPECTED_FILE" >"$work/expected" || fail "the expected list cannot be read"
  else
    expected_tests >"$work/expected"
  fi
  proof_lines >"$work/proofs"
  before=$(tracked_changes) || fail "git status failed before the run"

  twelve_test
  seam_manifests
  printf 'session reader of the phases: %s\n' "${HALVO_STALL_SESSION:-real}"

  proofs=$(cut -d ' ' -f 1 "$work/proofs" | tr '\n' ' ')
  twins=$(cut -d ' ' -f 1 "$work/proofs" | sed 's/$/_twin/' | tr '\n' ' ')
  proof_build "$twins" "$work/twins.txt"
  status=$?
  [ "$status" -eq 0 ] || fail "the twins of the proofs did not build: cargo rustc exited $status"
  proof_build "$proofs" "$work/proofs.txt"
  status=$?
  [ "$status" -ne 0 ] || fail "the proofs built: a build without debug assertions passed another bound or printed a held key"
  while read -r proof code; do
    grep -q "error\[$code\]" "$work/proofs.txt" || fail "the proof $proof did not fail with $code"
    printf 'proof %s: refused with %s\n' "$proof" "$code"
  done <"$work/proofs"

  fixture=$work/stall-fixture
  clang -fobjc-arc -Wall -Wextra -Werror -O1 -framework AppKit -framework CoreGraphics \
    -o "$fixture" "$fixture_source" >"$work/fixture.txt" 2>&1 ||
    fail "the fixture did not compile: clang exited $?"

  cargo test --locked -p halvo-capture --test macos_stall_fixture --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  exe=$(sed -n 's/.*"name":"macos_stall_fixture".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target macos_stall_fixture"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target macos_stall_fixture" ;;
  esac
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  sed -n 's/: test$//p' "$work/list.txt" | sort >"$work/listed" || fail "the test list cannot be read"
  cut -d ' ' -f 1 "$work/expected" | sort >"$work/expected-names" || fail "the expected list cannot be read"
  missing=$(comm -23 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$missing" ] || fail "the test executable does not list these tests of this script's list: ${missing% }"
  extra=$(comm -13 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$extra" ] || fail "the test executable lists tests that this script's list does not hold: ${extra% }"

  : >"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  passed=0
  known=0
  fixed=0
  failed=0
  not_driven=0
  index=0
  while read -r test kind; do
    [ "$kind" != not-run ] || continue
    index=$((index + 1))
    log=$work/test-$index.log
    started=$(date +%s)
    run_test "$test" "$log"
    code=$?
    seconds=$(($(date +%s) - started))
    tr -d '\r' <"$log" | grep -E "^$test (fixture=|round=|control=|verdict=|notdriven )" | sed 's/^/    | /'
    judge "$code" "$kind" "$log"
    three=
    if [ "$kind" = three-way ] && [ "$verdict" = pass ]; then
      three=$(tr -d '\r' <"$log" | sed -n -E "s/^$test verdict=(held|notdriven) .*/\1/p" | tail -n 1)
      case $three in
        held) ;;
        notdriven) not_driven=$((not_driven + 1)) ;;
        *)
          verdict=FAIL
          cause='the three-way phase printed no verdict line of held or notdriven'
          ;;
      esac
    fi
    case $verdict in
      pass) passed=$((passed + 1)) ;;
      known-fault) known=$((known + 1)) ;;
      FIXED-BUT-LISTED) fixed=$((fixed + 1)) ;;
      *) failed=$((failed + 1)) ;;
    esac
    [ -z "$cause" ] || printf '%s: %s: %s\n' "$name" "$test" "$cause"
    line="MS $test result=$verdict${three:+ verdict=$three} seconds=$seconds"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  done <"$work/expected"

  after=$(tracked_changes) || fail "git status failed after the run"
  [ "$after" = "$before" ] || fail "a tracked file differs from HEAD after the run"
  line="MS total=$index pass=$passed known_fault=$known notdriven=$not_driven fail=$((failed + fixed))"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  [ "$fixed" -eq 0 ] || fail "$fixed listed known faults passed (FIXED-BUT-LISTED): take each out of this script's list"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  printf 'PASS %s\n' "$name"
}

check() {
  case_name=$1
  want=$2
  want_text=$3
  spec_file=$work/spec-$case_name
  printf '%s\n' "$4" >"$spec_file"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts-$case_name
    STUB_SPEC=$spec_file
    STUB_EXE=$work/bin/stall-stub
    MS_EXPECTED_FILE=$work/expected-small
    MS_TEST_LIMIT_S=${limit:-30}
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_EXE MS_EXPECTED_FILE MS_TEST_LIMIT_S
    sh "$0" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$want_text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    printf '%s\n' "$out" | tail -n 3 | sed 's/^/    | /'
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  limit=
}

write_stubs() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
case $* in
  'rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off --cfg halvo_proof="hold_bound_in_a_release_build_twin" --cfg halvo_proof="held_key_printed_twin"')
    spec twins-fail && exit 101
    exit 0
    ;;
  'rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off --cfg halvo_proof="hold_bound_in_a_release_build" --cfg halvo_proof="held_key_printed"')
    spec proofs-build && exit 0
    spec proof-other-code && { printf 'error[E0308]: mismatched types\nerror[E0277]: no Debug\n'; exit 101; }
    printf 'error[E0599]: no function or associated item named `with_hold_bound`\nerror[E0277]: `HeldKey` does not implement `Debug`\n'
    exit 101
    ;;
  'test --locked -p halvo-capture --test macos_stall_fixture --no-run --message-format json')
    spec test-build-fails && exit 101
    printf '{"reason":"compiler-artifact","target":{"kind":["test"],"crate_types":["bin"],"name":"macos_stall_fixture","test":true},"executable":"%s","fresh":true}\n' "$STUB_EXE"
    exit 0
    ;;
esac
exit 97
EOF
  cat >"$work/bin/clang" <<'EOF'
#!/bin/sh
grep -q '^fixture-fails$' "$STUB_SPEC" && exit 1
for arg; do
  case $prev in -o) printf '#!/bin/sh\nexit 0\n' >"$arg" && chmod +x "$arg" ;; esac
  prev=$arg
done
exit 0
EOF
  cat >"$work/bin/stall-stub" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
if [ "$*" = --list ]; then
  printf 'the_probe: test\nphase_one: test\nphase_two: test\nknown_one: test\nthree_one: test\n'
  spec extra-test && printf 'unlisted: test\n'
  exit 0
fi
test=$5
[ -x "$HALVO_STALL_FIXTURE" ] || exit 98
printf '%s round=0 wait_ms=150 bound_ms=1000 counted=1 ahead=0 markers=2 plain=2 secure=8\n' "$test"
spec "touch-$test" && printf 'changed\n' >>"$CI_D_REPO/tracked.txt"
spec "hang-$test" && sleep 5
spec "fail-$test" && { printf "thread 'x' panicked at y:\n%s: rounds that did not count: wait 150 ms\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "leak-$test" && { printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 150 ms: 3\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "empty-$test" && { printf 'test result: ok. 0 passed; 0 failed; 1 ignored\n'; exit 0; }
case $test in
  three_*)
    spec "silent-$test" || { spec "notdriven-$test" && printf '%s verdict=notdriven held=3 failed=0 notdriven=2\n' "$test" || printf '%s verdict=held held=5 failed=0 notdriven=0\n' "$test"; }
    printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'
    exit 0
    ;;
  known_*)
    spec "pass-$test" && { printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'; exit 0; }
    printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 1500 ms: 9\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"
    exit 101
    ;;
esac
printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'
exit 0
EOF
  chmod +x "$work/bin/cargo" "$work/bin/clang" "$work/bin/stall-stub" || exit 2
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name-self-test.XXXXXX") || {
    printf 'FAIL self-test:%s (mktemp failed)\n' "$name"
    exit 1
  }
  mkdir -p "$work/bin" "$work/repo/target/ci-artifacts/12-test" || exit 2
  write_stubs
  printf 'the_probe not-run\nphase_one phase\nphase_two phase\nknown_one known-fault\nthree_one three-way\n' >"$work/expected-small"
  git -C "$work/repo" init -q || exit 2
  printf 'tracked\n' >"$work/repo/tracked.txt" || exit 2
  mkdir -p "$work/repo/crates/halvo-capture" "$work/repo/crates/other" || exit 2
  printf '[features]\nstall-seam = []\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add tracked.txt crates || exit 2
  twelve=$work/repo/target/ci-artifacts/12-test/tests.txt
  ignored='     Running tests/macos_stall_fixture.rs (target/debug/deps/macos_stall_fixture-0)
test a_key_typed_ahead_into_a_secure_field_is_not_recorded ... ignored, posts keys and shows a window: the hosted step alone runs it
test the_probe_reports_what_the_stall_phases_rest_on ... ignored
     Running tests/source_rules.rs (target/debug/deps/source_rules-0)
test a_rule ... ok'
  printf '%s\n' "$ignored" >"$twelve"
  wrong=
  rejected=0
  limit=
  check all-pass PASS 'MS total=4 pass=3 known_fault=1 notdriven=0 fail=0' ''
  check three-way-not-driven PASS 'MS three_one result=pass verdict=notdriven' 'notdriven-three_one'
  check three-way-without-a-verdict FAIL 'printed no verdict line of held or notdriven' 'silent-three_one'
  check phase-fails FAIL 'FAIL 51-macos-stall (1 tests failed)' 'fail-phase_one'
  check phase-runs-no-test FAIL 'phase_two: exit 0, and not one test ran and passed' 'empty-phase_two'
  check known-fault-passes FAIL 'FIXED-BUT-LISTED' 'pass-known_one'
  check known-fault-fails-otherwise FAIL 'failed without its assertion of a recorded key' 'fail-known_one'
  check phase-leaks FAIL 'phase_one result=FAIL' 'leak-phase_one'
  check extra-test FAIL 'lists tests that this script' 'extra-test'
  check twins-fail FAIL 'the twins of the proofs did not build' 'twins-fail'
  check proofs-build FAIL 'the proofs built' 'proofs-build'
  check proof-fails-with-another-code FAIL 'did not fail with E0599' 'proof-other-code'
  check fixture-fails FAIL 'the fixture did not compile' 'fixture-fails'
  check test-build-fails FAIL 'the test build failed' 'test-build-fails'
  check tracked-file-changed FAIL 'a tracked file differs from HEAD after the run' 'touch-phase_two'
  git -C "$work/repo" checkout -q -- tracked.txt 2>/dev/null || printf 'tracked\n' >"$work/repo/tracked.txt"
  limit=1
  check time-limit FAIL 'phase_one: exit 142' 'hang-phase_one'
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest a_key_typed_ahead_into_a_secure_field_is_not_recorded ... ok\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-ran-one FAIL '12-test ran 1 tests of tests/macos_stall_fixture.rs' ''
  printf '     Running tests/source_rules.rs (x)\ntest a_rule ... ok\n' >"$twelve"
  check twelve-test-listed-none FAIL '12-test listed no test of tests/macos_stall_fixture.rs as ignored' ''
  mv "$twelve" "$work/twelve-moved" || exit 2
  check twelve-test-absent PASS '12-test: no list of tests in this job' ''
  printf '[dependencies]\nhalvo-capture = { path = "../halvo-capture", features = ["stall-seam"] }\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add crates/other/Cargo.toml || exit 2
  check seam-named-by-another-manifest FAIL 'manifests that name it: crates/halvo-capture/Cargo.toml crates/other/Cargo.toml' ''
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  printf '[features]\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  check seam-in-no-manifest FAIL 'manifests that name it: none' ''
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (cases that gave another verdict:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'usage: sh %s [--self-test]\n' "$0" >&2
    exit 64
    ;;
esac
