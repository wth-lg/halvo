#!/bin/sh
# jobs: windows
# rule: The tests of halvo-schema pass, and export-names writes on two runs the names file that the crate commits as tests/golden/names.txt.
set -u
name=20-schema
golden=crates/halvo-schema/tests/golden/names.txt
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || fail "no repository"
  fi
  cd "$repo" || fail "the repository root cannot be entered"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"

  cargo test --locked -p halvo-schema >"$artifacts/cargo-test.txt" 2>&1
  code=$?
  [ "$code" -eq 0 ] || fail "cargo test --locked -p halvo-schema exited $code"
  passed=$(grep -c '\.\.\. ok' "$artifacts/cargo-test.txt")
  printf 'cargo test --locked -p halvo-schema: tests passed %s\n' "$passed"

  [ -f "$golden" ] || fail "$golden is missing"
  lines=$(grep -c '' "$golden")
  for run in 1 2; do
    cargo run -q --locked -p halvo-schema --bin export-names -- --out "$work/names.$run.txt" \
      >"$artifacts/export-names.$run.out" 2>"$artifacts/export-names.$run.err"
    code=$?
    [ "$code" -eq 0 ] || fail "export-names run $run exited $code"
    printed=$(cat "$artifacts/export-names.$run.out") || fail "the output of export-names run $run cannot be read"
    [ "$printed" = "names $lines" ] || fail "export-names run $run did not print the one line names $lines"
    cmp -s "$work/names.$run.txt" "$golden" ||
      fail "the names file of export-names run $run differs from $golden"
  done
  printf 'export-names runs=2 lines=%s same_as_golden=yes same_on_second_run=yes\n' "$lines"
  printf 'PASS %s\n' "$name"
}

note() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

check() {
  : >"$work/runs"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_ARTIFACTS=${art:-$work/artifacts/$1}
    STUB_RUNS=$work/runs
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_RUNS STUB_TEST_EXIT STUB_RUN_EXIT STUB_RUN2_EXIT STUB_OUT STUB_FIRST STUB_SECOND
    sh "$self" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  note "$1" "$2" "$got"
  STUB_TEST_EXIT=0
  STUB_RUN_EXIT=0
  STUB_RUN2_EXIT=0
  STUB_OUT='names 3'
  STUB_FIRST=$names
  STUB_SECOND=$names
  art=
}

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  repo=$work/repo
  mkdir -p "$work/bin" "$work/artifacts" "$repo/crates/halvo-schema/tests/golden" || self_fail "the fixture directories cannot be made"
  names='accessibility_text
active
address_field'
  printf '%s\n' "$names" >"$repo/$golden" || self_fail "the fixture names file cannot be written"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-schema')
    printf 'test golden_names ... ok\ntest golden_parse ... ok\n'
    exit "$STUB_TEST_EXIT"
    ;;
  'run -q --locked -p halvo-schema --bin export-names -- --out '*)
    shift 9
    [ "$#" -eq 1 ] || exit 97
    printf 'x\n' >>"$STUB_RUNS"
    if [ "$(grep -c '' "$STUB_RUNS")" -eq 1 ]; then
      content=$STUB_FIRST
      status=$STUB_RUN_EXIT
    else
      content=$STUB_SECOND
      status=$STUB_RUN2_EXIT
    fi
    [ "$content" = none ] || printf '%s\n' "$content" >"$1"
    [ -z "$STUB_OUT" ] || printf '%s\n' "$STUB_OUT"
    exit "$status"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo" || self_fail "the stub cargo cannot be made executable"
  rejected=0
  wrong=
  STUB_TEST_EXIT=0
  STUB_RUN_EXIT=0
  STUB_RUN2_EXIT=0
  STUB_OUT='names 3'
  STUB_FIRST=$names
  STUB_SECOND=$names

  check all-hold PASS "tests passed 2
export-names runs=2 lines=3 same_as_golden=yes same_on_second_run=yes
PASS $name"
  STUB_TEST_EXIT=101
  check tests-fail FAIL "FAIL $name (cargo test --locked -p halvo-schema exited 101)"
  STUB_RUN_EXIT=2
  STUB_OUT=
  check export-names-refuses FAIL "FAIL $name (export-names run 1 exited 2)"
  STUB_RUN2_EXIT=2
  check second-run-refuses FAIL "FAIL $name (export-names run 2 exited 2)"
  STUB_FIRST='accessibility_text
active'
  check file-lacks-a-name FAIL "FAIL $name (the names file of export-names run 1 differs from $golden)"
  STUB_SECOND='accessibility_text
address_field
active'
  check second-run-differs FAIL "FAIL $name (the names file of export-names run 2 differs from $golden)"
  STUB_FIRST=none
  check no-file-written FAIL "FAIL $name (the names file of export-names run 1 differs from $golden)"
  STUB_OUT='names 2'
  check count-differs FAIL "FAIL $name (export-names run 1 did not print the one line names 3)"
  STUB_OUT='names 3
names 3'
  check two-lines-printed FAIL "FAIL $name (export-names run 1 did not print the one line names 3)"
  STUB_OUT=
  check nothing-printed FAIL "FAIL $name (export-names run 1 did not print the one line names 3)"

  repo=$work/no-golden
  mkdir -p "$repo" || self_fail "the fixture directory cannot be made"
  check golden-missing FAIL "FAIL $name ($golden is missing)"
  repo=$work/missing
  check repository-missing FAIL "FAIL $name (the repository root cannot be entered)"
  repo=$work/repo
  : >"$work/a-file" || self_fail "the fixture file cannot be made"
  art=$work/a-file/artifacts
  check artifacts-not-made FAIL "FAIL $name (the artifacts directory cannot be made)"

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    note unknown-argument FAIL FAIL
  else
    note unknown-argument FAIL "exit-$code"
  fi

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
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
