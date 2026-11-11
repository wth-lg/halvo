#!/bin/sh
# jobs: windows
# rule: cargo test -p halvo-seam passes with the committed lockfile and shows the four conformance tests, the reduction test and the memory test as passed, each in the block of its own test file.
set -u
name=21-seam
tests='conformance:conformance_resume conformance:conformance_shredded conformance:conformance_filters conformance:conformance_store_changed reduction:reduction_app_title memory:transport_memory'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

block() {
  awk -v f="$1" '
    { line = $0; sub(/^[ \t]+/, "", line) }
    line ~ /^(Running|Doc-tests) / {
      inside = (index(line, "Running tests/" f ".rs ") == 1 || index(line, "Running tests\\" f ".rs ") == 1)
      next
    }
    inside { print }
  ' "$2"
}

decide() {
  [ "$1" -eq 0 ] || fail "cargo test -p halvo-seam --locked exited $1"
  tr -d '\r' <"$2" >"$2.lf" || fail "the output of cargo test cannot be read"
  for pair in $tests; do
    file=${pair%%:*}
    test=${pair#*:}
    block "$file" "$2.lf" | grep -x "test $test \.\.\. ok" || fail "cargo test -p halvo-seam --locked does not show $test as passed in tests/$file.rs"
  done
  passed=$(grep -c -x 'test .* \.\.\. ok' "$2.lf")
  printf 'halvo-seam tests passed: %s, of them read by name: 6\n' "$passed"
}

measure() {
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  if ! command -v cargo >/dev/null 2>&1; then
    [ "${CI:-}" = true ] && fail "cargo is not installed"
    printf 'SKIP %s (cargo is not installed)\n' "$name"
    exit 77
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  cargo test -p halvo-seam --locked >"$work/test.txt" 2>&1
  code=$?
  cp "$work/test.txt" "$CI_D_ARTIFACTS/cargo-test.txt" || fail "the output of cargo test cannot be saved"
  grep '^test result:' "$work/test.txt"
  decide "$code" "$work/test.txt"
  printf 'PASS %s\n' "$name"
}

passing_output() {
  cat <<'EOF'
   Compiling halvo-seam v0.0.0 (/work/halvo/crates/halvo-seam)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.70s
     Running unittests src/lib.rs (target/debug/deps/halvo_seam-d0a1cca15648a4e0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/conformance.rs (target/debug/deps/conformance-44843f9d15f6ca55)

running 4 tests
test conformance_shredded ... ok
test conformance_resume ... ok
test conformance_filters ... ok
test conformance_store_changed ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s

     Running tests/memory.rs (target/debug/deps/memory-592ca756897cbf3d)

running 1 test
test transport_memory ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.08s

     Running tests/reduction.rs (target/debug/deps/reduction-944dac51f07a8804)

running 4 tests
test the_set_files_of_c6_parse ... ok
test the_reduction_serves_the_listed_keys_and_the_marks_and_nothing_else ... ok
test reduction_app_title ... ok
test set_reader_refusals_report_the_path_and_the_line ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/set_reader.rs (target/debug/deps/set_reader-09bd73a5d516c7cf)

running 1 test
test a_set_path_that_is_no_regular_file_is_unreadable ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests halvo_seam

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
EOF
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

check() {
  passing_output >"$work/$1.base" || self_fail "the fixture cannot be written"
  sed "$4" "$work/$1.base" >"$work/$1.txt" || self_fail "the fixture of $1 cannot be made"
  out=$( (decide "$3" "$work/$1.txt") 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:halvo-seam tests passed: "*) got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  verdict "$1" "$2" "$got"
}

run_measure() {
  mkdir -p "$work/$1/bin" "$work/$1/artifacts" || self_fail "the directories of $1 cannot be made"
  search="$work/$1/bin"
  if [ "$3" != no-cargo ]; then
    printf '#!/bin/sh\ncat "%s"\nexit %s\n' "$5" "$3" >"$work/$1/bin/cargo" || self_fail "the stub of $1 cannot be written"
    chmod +x "$work/$1/bin/cargo" || self_fail "the stub of $1 cannot be made executable"
    search="$search:$PATH"
  fi
  out=$( (
    export PATH="$search" CI_D_ARTIFACTS="$work/$1/artifacts"
    if [ -n "$4" ]; then export CI_D_REPO="$4"; else unset CI_D_REPO; fi
    [ "$3" != no-cargo ] || export CI=true
    measure
  ) 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  verdict "$1" "$2" "$got"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  rejected=0
  wrong=
  cr=$(printf '\r')
  check all-passed PASS 0 's/^//'
  check crlf-lines PASS 0 "s/\$/$cr/"
  check windows-paths PASS 0 's|Running tests/|Running tests\\|; s|target/debug/deps/|target\\debug\\deps\\|'
  check cargo-test-fails FAIL 101 's/^//'
  check filters-missing FAIL 0 '/conformance_filters/d'
  check reduction-failed FAIL 0 's/test reduction_app_title ... ok/test reduction_app_title ... FAILED/'
  check memory-ignored FAIL 0 's/test transport_memory ... ok/test transport_memory ... ignored/'
  check resume-renamed FAIL 0 's/conformance_resume/conformance_resume_old/'
  check store-changed-with-a-suffix FAIL 0 's/conformance_store_changed ... ok/conformance_store_changed ... ok, 61 s/'
  check no-output FAIL 0 'd'
  check reduction-ok-only-in-another-file FAIL 0 's/^test reduction_app_title \.\.\. ok$/test reduction_app_title ... ignored/
s/^test conformance_filters \.\.\. ok$/&\
test reduction_app_title ... ok/'
  check memory-ok-only-in-another-file FAIL 0 '/^test transport_memory \.\.\. ok$/d
s/^test a_set_path_that_is_no_regular_file_is_unreadable \.\.\. ok$/&\
test transport_memory ... ok/'
  check conformance-ok-only-in-the-unit-tests FAIL 0 '/^test conformance_shredded \.\.\. ok$/d
s/^running 0 tests$/&\
test conformance_shredded ... ok/'
  check memory-ok-only-after-the-doc-tests-line FAIL 0 '/^test transport_memory \.\.\. ok$/d
s|Running tests/set_reader\.rs|Running tests/memory.rs|
s/^   Doc-tests halvo_seam$/&\
test transport_memory ... ok/'
  passing_output >"$work/real.txt" || self_fail "the fixture cannot be written"
  : >"$work/empty.txt" || self_fail "the empty output cannot be written"
  run_measure measure-passes PASS 0 "$work" "$work/real.txt"
  run_measure measure-takes-the-failing-status FAIL 101 "$work" "$work/real.txt"
  run_measure measure-with-no-output FAIL 0 "$work" "$work/empty.txt"
  run_measure measure-in-a-missing-directory FAIL 0 "$work/missing" "$work/real.txt"
  run_measure measure-with-ci-d-repo-not-set FAIL 0 "" "$work/real.txt"
  run_measure measure-in-ci-with-no-cargo FAIL no-cargo "$work" "$work/real.txt"
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (unknown argument)\n' "$name"
    exit 64
    ;;
esac
