#!/bin/sh
# jobs: macos windows
# rule: Every test of the workspace passes with the committed lockfile, and the test run leaves no entry in its temp directory.
set -u
name=12-test
exempt=halvo-consent-test-

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

keychain_items() {
  [ "$CI_D_JOB" = macos ] || return 0
  dump=$(security dump-keychain login.keychain-db 2>/dev/null)
  security_code=$?
  if [ "$security_code" -ne 0 ]; then
    printf 'login-keychain items of com.halvo.capture.dev-fallback %s the tests: unknown, security exited %s\n' "$1" "$security_code"
    return 0
  fi
  count=$(printf '%s\n' "$dump" | grep -c '"svce"<blob>="com\.halvo\.capture\.dev-fallback')
  printf 'login-keychain items of com.halvo.capture.dev-fallback %s the tests: %s\n' "$1" "$count"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --workspace --locked --no-run || fail "the test build failed: cargo test --no-run exited $?"
  scratch=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/$name.XXXXXX") || fail "mktemp failed"
  keychain_items before
  TMPDIR=$scratch TMP=$scratch TEMP=$scratch cargo test --workspace --locked
  code=$?
  keychain_items after
  [ "$code" -eq 0 ] || fail "cargo test exited $code"

  left=$(ls -A "$scratch") || fail "the temp directory of the test run cannot be listed"
  total=0
  exempted=0
  if [ -n "$left" ]; then
    total=$(printf '%s\n' "$left" | grep -c '')
    git grep -q -F -e "$exempt" -- '*.rs'
    code=$?
    case $code in
      0)
        exempted=$(printf '%s\n' "$left" | grep -c "^$exempt")
        left=$(printf '%s\n' "$left" | grep -v "^$exempt")
        ;;
      1) ;;
      *) fail "git grep exited $code" ;;
    esac
  fi
  printf 'entries that the test run left in its temp directory: %s, of them exempt: %s\n' "$total" "$exempted"
  if [ "$total" -gt "$exempted" ]; then
    names=$(printf '%s\n' "$left" | tr '\n' ' ')
    fail "the tests left $((total - exempted)) entries in their temp directory: ${names% }"
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  STUB_CALLS=$work/calls.$1
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_JOB=macos
    RUNNER_TEMP=$work/runner-temp
    export PATH CI_D_REPO CI_D_JOB RUNNER_TEMP STUB_CALLS STUB_BUILD_EXIT STUB_TEST_EXIT STUB_LEAVES
    measure 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_BUILD_EXIT=0
  STUB_TEST_EXIT=0
  STUB_LEAVES=
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/runner-temp" "$work/literal-tracked/src" "$work/literal-gone/src"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_CALLS"
case $* in
  'test --workspace --locked --no-run') exit "$STUB_BUILD_EXIT" ;;
  'test --workspace --locked') ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
grep -q -e '--no-run' "$STUB_CALLS" || exit 94
case ${TMPDIR:-} in
  "$RUNNER_TEMP"/?*) ;;
  *) exit 93 ;;
esac
[ "${TMP:-}" = "$TMPDIR" ] || exit 96
[ "${TEMP:-}" = "$TMPDIR" ] || exit 96
left=$(ls -A "$TMPDIR")
[ -z "$left" ] || exit 95
for entry in $STUB_LEAVES; do
  mkdir "$TMPDIR/$entry"
done
exit "$STUB_TEST_EXIT"
EOF
  cat >"$work/bin/security" <<'EOF'
#!/bin/sh
[ "$*" = 'dump-keychain login.keychain-db' ] || exit 97
printf '    "svce"<blob>="com.halvo.capture.dev-fallback"\n'
printf '    "svce"<blob>="org.example.other"\n'
printf '    "svce"<blob>="com.halvo.capture.dev-fallback"\n'
EOF
  chmod +x "$work/bin/cargo" "$work/bin/security"
  printf 'let dir = temp_dir().join(format!("halvo-consent-test-{n}"));\n' >"$work/literal-tracked/src/a.rs"
  printf 'let dir = private_temp_dir();\n' >"$work/literal-gone/src/a.rs"
  for repo in "$work/literal-tracked" "$work/literal-gone"; do
    git -C "$repo" init -q >/dev/null 2>&1
    git -C "$repo" add src/a.rs >/dev/null 2>&1
  done
  rejected=0
  wrong=
  STUB_BUILD_EXIT=0
  STUB_TEST_EXIT=0
  STUB_LEAVES=

  repo=$work/literal-tracked
  check tests-pass PASS 'login-keychain items of com.halvo.capture.dev-fallback before the tests: 2'
  STUB_BUILD_EXIT=101
  check build-fails FAIL 'the test build failed: cargo test --no-run exited 101'
  STUB_TEST_EXIT=101
  check test-fails FAIL 'cargo test exited 101'
  STUB_LEAVES=stray
  check stray-entry FAIL 'the tests left 1 entries in their temp directory: stray)'
  STUB_LEAVES='halvo-consent-test-7-0 halvo-consent-test-7-1'
  check consent-directories-while-the-literal-is-tracked PASS 'temp directory: 2, of them exempt: 2'
  STUB_LEAVES='halvo-consent-test-7-0 stray'
  check stray-beside-consent-directories FAIL 'the tests left 1 entries in their temp directory: stray)'
  repo=$work/literal-gone
  STUB_LEAVES='halvo-consent-test-7-0 halvo-consent-test-7-1'
  check consent-directories-after-the-literal-is-gone FAIL 'the tests left 2 entries in their temp directory: halvo-consent-test-7-0 halvo-consent-test-7-1)'

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
