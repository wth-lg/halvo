#!/bin/sh
# jobs: macos windows
# rule: The tests of halvo-redact pass, the planted-identifier test shows as passed and the output holds its line mask titles=N ns_per_title=N with N above 0.
set -u
name=22-redact
ok_line='test planted_identifiers_are_masked ... ok'
titles_line='mask titles=0*[1-9][0-9]* ns_per_title=[0-9][0-9]*'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

judge() {
  cause=
  if [ "$1" -ne 0 ]; then
    cause="cargo test -p halvo-redact exited $1"
    return 1
  fi
  tr -d '\r' <"$2" >"$2.lf" || {
    cause="the output of cargo test cannot be copied without carriage returns"
    return 1
  }
  if ! grep -q -x -F -e "$ok_line" "$2.lf"; then
    cause="the output does not show planted_identifiers_are_masked ... ok"
    return 1
  fi
  if ! grep -q -x -e "$titles_line" "$2.lf"; then
    cause="the output holds no line mask titles=N ns_per_title=N with N above 0"
    return 1
  fi
  return 0
}

measure() {
  cd "$CI_D_REPO" || exit 2
  output=$CI_D_ARTIFACTS/cargo-test.txt
  cargo test -p halvo-redact --locked -- --nocapture >"$output" 2>&1
  code=$?
  cat "$output" || fail "the output of cargo test cannot be read"
  judge "$code" "$output" || fail "$cause"
  printf 'PASS %s\n' "$name"
}

check() {
  mkdir -p "$work/artifacts/$1" || exit 2
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts/$1
    STUB_OUTPUT=$work/fixtures/$1
    STUB_EXIT=$2
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_OUTPUT STUB_EXIT
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
    *"${4:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$3" "$got"
  if [ "$got" != "$3" ]; then
    wrong="$wrong $1"
  elif [ "$3" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

fixture() {
  case=$1
  shift
  : >"$work/fixtures/$case"
  for line in "$@"; do
    printf '%s\n' "$line" >>"$work/fixtures/$case"
  done
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" "$work/fixtures" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
[ -f fixture-root ] || { printf 'stub cargo: started outside the repo root\n'; exit 98; }
[ "$*" = 'test -p halvo-redact --locked -- --nocapture' ] || { printf 'stub cargo: got the arguments: %s\n' "$*"; exit 97; }
cat "$STUB_OUTPUT"
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo" || exit 2
  : >"$work/repo/fixture-root"
  rejected=0
  wrong=
  cr=$(printf '\r')

  fixture passes 'running 1 test' 'test key::tests::key_rows ... ok' 'running 1 test' \
    'mask titles=252 ns_per_title=21011' "$ok_line" 'test result: ok. 1 passed; 0 failed'
  check passes 0 PASS 'mask titles=252'

  fixture passes-with-cr-line-ends "running 1 test$cr" "mask titles=252 ns_per_title=19387$cr" "$ok_line$cr"
  check passes-with-cr-line-ends 0 PASS

  fixture passes-with-one-title 'mask titles=1 ns_per_title=0' 'test other ... ok' "$ok_line"
  check passes-with-one-title 0 PASS

  fixture tests-fail 'mask titles=252 ns_per_title=19387' 'test planted_identifiers_are_masked ... FAILED' \
    'test result: FAILED. 0 passed; 1 failed'
  check tests-fail 101 FAIL 'cargo test -p halvo-redact exited 101)'

  fixture exit-0-without-the-planted-test 'mask titles=252 ns_per_title=19387' 'test key::tests::key_rows ... ok'
  check exit-0-without-the-planted-test 0 FAIL 'does not show planted_identifiers_are_masked ... ok)'

  fixture planted-test-ignored 'test planted_identifiers_are_masked ... ignored' 'mask titles=252 ns_per_title=1'
  check planted-test-ignored 0 FAIL 'does not show planted_identifiers_are_masked ... ok)'

  fixture planted-line-inside-another 'mask titles=252 ns_per_title=1' "x $ok_line"
  check planted-line-inside-another 0 FAIL 'does not show planted_identifiers_are_masked ... ok)'

  fixture no-titles-line "$ok_line" 'test result: ok. 1 passed; 0 failed'
  check no-titles-line 0 FAIL 'holds no line mask titles=N ns_per_title=N with N above 0)'

  fixture zero-titles 'mask titles=0 ns_per_title=0' "$ok_line"
  check zero-titles 0 FAIL 'holds no line mask titles=N ns_per_title=N with N above 0)'

  fixture titles-line-without-time 'mask titles=252' "$ok_line"
  check titles-line-without-time 0 FAIL 'holds no line mask titles=N ns_per_title=N with N above 0)'

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
