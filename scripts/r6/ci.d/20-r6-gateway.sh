#!/bin/sh
# jobs: macos windows
# rule: The gateway crate's tests pass, and gateway-check reads every body vector and answer vector as its line states, a closed schema with the six keys of the body, and the limits of the tier file.
set -u
name=20-r6-gateway
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
gateway=docs/r6/gateway
names_file=crates/halvo-schema/tests/golden/names.txt

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || exit 2
  fi
  cd "$repo" || exit 2
}

line_holds() {
  case $1 in
    *[!a-z0-9_=\ ]* | *'  '* | ' '* | *' ') return 1 ;;
  esac
  read -r word vectors ok answers answers_ok closed keys forbidden limits extra <<EOF
$1
EOF
  [ "$word" = gateway ] && [ -z "$extra" ] || return 1
  case "$vectors $ok $answers $answers_ok" in
    'vectors='*' ok='*' answers='*' answers_ok='*) ;;
    *) return 1 ;;
  esac
  vectors=${vectors#vectors=}
  ok=${ok#ok=}
  answers=${answers#answers=}
  answers_ok=${answers_ok#answers_ok=}
  for count in "$vectors" "$ok" "$answers" "$answers_ok"; do
    case $count in
      '' | *[!0-9]*) return 1 ;;
    esac
  done
  [ "$vectors" = "$ok" ] && [ "$answers" = "$answers_ok" ] || return 1
  [ "$vectors" != 0 ] && [ "$answers" != 0 ] || return 1
  [ "$closed $keys $limits" = 'closed=yes keys=6 limits=ok' ] || return 1
  [ "$forbidden" = forbidden=0 ] || [ "$forbidden" = forbidden=not_checked ]
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  cause=

  if ! cargo test --locked -p halvo-route-wire >"$artifacts/cargo-test.txt" 2>&1; then
    cause=tests
  fi

  set -- --dir "$gateway"
  if [ -f "$names_file" ]; then
    set -- "$@" --names "$names_file"
  fi
  cargo run -q --locked -p halvo-route-wire --bin gateway-check -- "$@" \
    >"$artifacts/gateway.out" 2>"$artifacts/gateway.err"
  code=$?
  grep '^gateway ' "$artifacts/gateway.out"
  grep '^REFUSED ' "$artifacts/gateway.err"
  last=$(tail -n 1 "$artifacts/gateway.out")
  if [ "$code" -ne 0 ] || ! line_holds "$last"; then
    cause=${cause:-gateway}
  fi

  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
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
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts/$1
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TEST_EXIT STUB_OUT STUB_ERR STUB_RUN_EXIT STUB_NAMES
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
  STUB_OUT=$good
  STUB_ERR=
  STUB_RUN_EXIT=0
  STUB_NAMES=
}

real() {
  out=$(cargo run -q --locked -p halvo-route-wire --bin gateway-check -- "$@" 2>&1)
  code=$?
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" "$work/artifacts" "$work/gateway" "$work/flipped" || exit 2
  for file in vectors.jsonl answers.jsonl body.schema.json tier-limits.tsv; do
    cp "$gateway/$file" "$work/gateway/$file" || exit 2
    cp "$gateway/$file" "$work/flipped/$file" || exit 2
  done
  sed '1s/"canon":"yes"/"canon":"no"/' "$gateway/vectors.jsonl" >"$work/flipped/vectors.jsonl" || exit 2
  cmp -s "$work/flipped/vectors.jsonl" "$gateway/vectors.jsonl" && exit 2
  printf 'alpha\ncontent\nzeta\n' >"$work/names-content.txt" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-route-wire')
    exit "$STUB_TEST_EXIT"
    ;;
  'run -q --locked -p halvo-route-wire --bin gateway-check -- --dir docs/r6/gateway')
    [ -z "$STUB_NAMES" ] || exit 97
    ;;
  'run -q --locked -p halvo-route-wire --bin gateway-check -- --dir docs/r6/gateway --names crates/halvo-schema/tests/golden/names.txt')
    [ -n "$STUB_NAMES" ] || exit 97
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
[ -z "$STUB_OUT" ] || printf '%s\n' "$STUB_OUT"
[ -z "$STUB_ERR" ] || printf '%s\n' "$STUB_ERR" >&2
exit "$STUB_RUN_EXIT"
EOF
  chmod +x "$work/bin/cargo"
  rejected=0
  wrong=
  good='gateway vectors=81 ok=81 answers=36 answers_ok=36 closed=yes keys=6 forbidden=not_checked limits=ok'
  STUB_TEST_EXIT=0
  STUB_OUT=$good
  STUB_ERR=
  STUB_RUN_EXIT=0
  STUB_NAMES=

  check all-hold PASS "$good
PASS $name"
  STUB_TEST_EXIT=101
  check tests-fail FAIL "$good
FAIL $name (tests)"
  STUB_OUT="gateway miss file=vectors n=3 name=three_roles
gateway vectors=81 ok=80 answers=36 answers_ok=36 closed=yes keys=6 forbidden=not_checked limits=ok"
  STUB_RUN_EXIT=1
  check one-vector-missed FAIL "gateway miss file=vectors n=3 name=three_roles
gateway vectors=81 ok=80"
  STUB_OUT=
  STUB_ERR='REFUSED file: vectors.jsonl:4'
  STUB_RUN_EXIT=2
  check tool-refuses FAIL "REFUSED file: vectors.jsonl:4
FAIL $name (gateway)"
  STUB_OUT='gateway vectors=81 ok=81 answers=36 answers_ok=36 closed=no keys=6 forbidden=not_checked limits=ok'
  check open-schema-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT='gateway vectors=81 ok=80 answers=36 answers_ok=36 closed=yes keys=6 forbidden=not_checked limits=ok'
  check unequal-vectors-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT='gateway vectors=81 ok=81 answers=36 answers_ok=35 closed=yes keys=6 forbidden=not_checked limits=ok'
  check unequal-answers-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT='gateway vectors=81 ok=81 answers=36 answers_ok=36 closed=yes keys=7 forbidden=not_checked limits=ok'
  check seven-keys-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT='gateway vectors=81 ok=81 answers=36 answers_ok=36 closed=yes keys=6 forbidden=1 limits=ok'
  check forbidden-key-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT='gateway vectors=81 ok=81 answers=36 answers_ok=36 closed=yes keys=6 forbidden=not_checked limits=miss'
  check limits-miss-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT='gateway vectors=0 ok=0 answers=36 answers_ok=36 closed=yes keys=6 forbidden=not_checked limits=ok'
  check no-vectors-with-exit-0 FAIL "FAIL $name (gateway)"
  STUB_OUT="$good extra=1"
  check line-with-another-key FAIL "FAIL $name (gateway)"
  STUB_RUN_EXIT=1
  check right-line-with-exit-1 FAIL "FAIL $name (gateway)"
  STUB_OUT=
  check no-line FAIL "FAIL $name (gateway)"
  STUB_TEST_EXIT=101
  STUB_RUN_EXIT=1
  check two-causes-name-the-first FAIL "FAIL $name (tests)"
  mkdir -p "$work/repo/crates/halvo-schema/tests/golden" || exit 2
  printf 'alpha\n' >"$work/repo/$names_file" || exit 2
  STUB_NAMES=1
  STUB_OUT='gateway vectors=81 ok=81 answers=36 answers_ok=36 closed=yes keys=6 forbidden=0 limits=ok'
  check names-file-is-passed PASS "forbidden=0 limits=ok
PASS $name"

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    note unknown-argument FAIL FAIL
  else
    note unknown-argument FAIL "exit-$code"
  fi

  real --dir "$work/gateway"
  case $code:$out in
    "0:gateway vectors="*" closed=yes keys=6 forbidden=not_checked limits=ok") note tool-on-the-files PASS PASS ;;
    *) note tool-on-the-files PASS "exit-$code" ;;
  esac
  real --dir "$work/flipped"
  case $code:$out in
    "1:gateway miss file=vectors n=1 name=least
gateway vectors="*) note tool-on-a-flipped-result FAIL FAIL ;;
    *) note tool-on-a-flipped-result FAIL "exit-$code" ;;
  esac
  real --dir "$work/gateway" --names "$work/names-content.txt"
  case $code:$out in
    "1:gateway vectors="*" forbidden=1 limits=ok") note tool-with-content-named FAIL FAIL ;;
    *) note tool-with-content-named FAIL "exit-$code" ;;
  esac

  if [ -z "${R6_GATEWAY_NESTED:-}" ]; then
    nested=$(cd crates && R6_GATEWAY_NESTED=1 sh "../scripts/r6/ci.d/$name.sh" --self-test 2>&1 | tail -n 1)
    case $nested in
      "PASS self-test:$name "*) note self-test-from-a-subdirectory PASS PASS ;;
      *) note self-test-from-a-subdirectory PASS "$nested" ;;
    esac
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
