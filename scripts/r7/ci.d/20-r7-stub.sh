#!/bin/sh
# jobs: macos windows
# rule: The account backend's stand-in under account/stub/ locks no package and its self-test passes on Node 26 or later, with at least 40 tests and none failed, skipped, cancelled or left to do: each request that the vendored schema or route table refuses gets the rejection of contract 1.4, each accepted request an answer that the schema allows, and the scripted states, the recorder and the TLS form with a test authority made in a temp directory each hold.
# lane: alone
set -u
name=20-r7-stub
min_tests=40

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

summary() {
  awk -v key="# $1 " 'index($0, key) == 1 { value = substr($0, length(key) + 1) } END { print value }' "$2"
}

measure() {
  cd "$CI_D_REPO" || fail 'cannot enter the repository that CI_D_REPO names'
  command -v node >/dev/null 2>&1 || fail 'node is not on the PATH; each job sets up Node 26 before its prerequisites step'
  version=$(node --version) || fail 'node --version failed'
  major=${version#v}
  major=${major%%.*}
  case $major in
    '' | *[!0-9]*) fail 'node --version printed no version' ;;
    *) ;;
  esac
  [ "$major" -ge 26 ] || fail "node major version $major is older than 26, the version that account/stub/package.json asks for"
  printf 'node major version: %s\n' "$major"

  for file in account/stub/package.json account/stub/package-lock.json; do
    [ -f "$file" ] || fail "$file is missing"
  done
  if grep -E -q '"(dev|optional|peer|bundle|bundled)?[dD]ependencies"' account/stub/package.json; then
    fail 'account/stub/package.json names a dependency; no prerequisites step installs one, so a package enters only with its install line there, by a ruling'
  fi
  if grep -q '"node_modules/' account/stub/package-lock.json; then
    fail 'account/stub/package-lock.json locks a package; no prerequisites step installs one, so a package enters only with its install line there, by a ruling'
  fi

  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail 'mktemp failed'
  tap=$CI_D_ARTIFACTS/stub-tests.tap
  TMPDIR=$work TEMP=$work TMP=$work node --test --test-concurrency=1 --test-timeout=60000 --test-force-exit --test-reporter=tap account/stub/test/*.test.mjs >"$tap" 2>&1
  code=$?
  n_tests=$(summary tests "$tap") || fail 'awk failed on the test output'
  n_pass=$(summary pass "$tap") || fail 'awk failed on the test output'
  n_fail=$(summary fail "$tap") || fail 'awk failed on the test output'
  n_cancelled=$(summary cancelled "$tap") || fail 'awk failed on the test output'
  n_skipped=$(summary skipped "$tap") || fail 'awk failed on the test output'
  n_todo=$(summary todo "$tap") || fail 'awk failed on the test output'
  for key in "tests:$n_tests" "pass:$n_pass" "fail:$n_fail" "cancelled:$n_cancelled" "skipped:$n_skipped" "todo:$n_todo"; do
    case ${key#*:} in
      '' | *[!0-9]*) fail "the stub's test run printed no count of ${key%%:*} (exit $code); its output is the artifact stub-tests.tap" ;;
      *) ;;
    esac
  done
  printf 'stub tests: %s run, %s passed, %s failed, %s cancelled, %s skipped, %s todo, exit %s\n' \
    "$n_tests" "$n_pass" "$n_fail" "$n_cancelled" "$n_skipped" "$n_todo" "$code"
  [ "$code" -eq 0 ] || fail "$n_fail of $n_tests stub tests failed (exit $code); the output is the artifact stub-tests.tap"
  [ "$n_fail" -eq 0 ] && [ "$n_cancelled" -eq 0 ] && [ "$n_skipped" -eq 0 ] && [ "$n_todo" -eq 0 ] ||
    fail "a stub test failed, was cancelled, was skipped or is left to do; the output is the artifact stub-tests.tap"
  [ "$n_pass" -eq "$n_tests" ] || fail "only $n_pass of $n_tests stub tests passed"
  [ "$n_tests" -ge "$min_tests" ] || fail "$n_tests stub tests ran, fewer than the $min_tests that this script asks for"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=${STUB_PATH:-$work/bin:$PATH}
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_VERSION STUB_TAP STUB_EXIT
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
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    printf 'case %s/%s last line: %s\n' "$name" "$1" "$last"
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  reset
}

reset() {
  STUB_VERSION=v26.12.0
  STUB_TAP=$work/tap/good
  STUB_EXIT=0
  STUB_PATH=
  printf '{"name":"x","private":true}\n' >"$work/repo/account/stub/package.json"
  printf '{"name":"x","lockfileVersion":3,"packages":{"":{"name":"x"}}}\n' >"$work/repo/account/stub/package-lock.json"
}

tap() {
  printf 'ok 1 - a\n# tests %s\n# suites 0\n# pass %s\n# fail %s\n# cancelled %s\n# skipped %s\n# todo %s\n# duration_ms 5\n' \
    "$2" "$3" "$4" "$5" "$6" "$7" >"$work/tap/$1"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/nonode" "$work/repo/account/stub/test" "$work/artifacts" "$work/tap"
  : >"$work/repo/account/stub/test/a.test.mjs"
  cat >"$work/bin/node" <<'EOF'
#!/bin/sh
case ${1:-} in
  --version) printf '%s\n' "$STUB_VERSION"; exit 0 ;;
  --test) ;;
  *) printf 'stub node: got the arguments: %s\n' "$*"; exit 97 ;;
esac
[ -f account/stub/package.json ] || { printf 'stub node: started outside the repo root\n'; exit 98; }
[ "$*" = '--test --test-concurrency=1 --test-timeout=60000 --test-force-exit --test-reporter=tap account/stub/test/a.test.mjs' ] ||
  { printf 'stub node: got the arguments: %s\n' "$*"; exit 96; }
[ -n "${TMPDIR:-}" ] && [ "$TMPDIR" = "${TEMP:-}" ] && [ "$TMPDIR" = "${TMP:-}" ] && [ -d "$TMPDIR" ] ||
  { printf 'stub node: TMPDIR, TEMP and TMP do not name one directory\n'; exit 95; }
cat "$STUB_TAP"
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/node"
  tap good 40 40 0 0 0 0
  tap failed 40 39 1 0 0 0
  tap skipped 40 39 0 0 1 0
  tap skipped-counted-as-passed 40 40 0 0 1 0
  tap cancelled 40 40 0 1 0 0
  tap todo 40 40 0 0 0 1
  tap fewer 39 39 0 0 0 0
  tap short 40 39 0 0 0 0
  : >"$work/tap/none"
  printf 'ok 1 - a\n# tests 40\n# pass 40\n# fail 0\n' >"$work/tap/partial"
  rejected=0
  wrong=
  reset

  check passing PASS 'stub tests: 40 run, 40 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo, exit 0'
  STUB_TAP=$work/tap/failed
  STUB_EXIT=1
  check a-test-fails FAIL '1 of 40 stub tests failed (exit 1)'
  STUB_TAP=$work/tap/good
  STUB_EXIT=1
  check exit-without-a-failed-test FAIL '0 of 40 stub tests failed (exit 1)'
  STUB_TAP=$work/tap/skipped
  check a-test-skipped FAIL 'was skipped'
  STUB_TAP=$work/tap/skipped-counted-as-passed
  check skipped-counted-as-passed FAIL 'was skipped'
  STUB_TAP=$work/tap/cancelled
  check a-test-cancelled FAIL 'was cancelled'
  STUB_TAP=$work/tap/todo
  check a-test-left-to-do FAIL 'left to do'
  STUB_TAP=$work/tap/fewer
  check fewer-tests-than-the-floor FAIL 'fewer than the 40'
  STUB_TAP=$work/tap/none
  STUB_EXIT=1
  check no-summary FAIL 'printed no count of tests (exit 1)'
  STUB_TAP=$work/tap/partial
  check summary-without-cancelled FAIL 'printed no count of cancelled'
  STUB_VERSION=v24.11.1
  check node-older-than-26 FAIL 'node major version 24 is older than 26'
  STUB_VERSION=
  check node-prints-no-version FAIL 'node --version printed no version'
  STUB_PATH=$work/nonode:/usr/bin:/bin
  check node-missing FAIL 'node is not on the PATH'
  printf '{"name":"x","dependencies":{"a":"1.0.0"}}\n' >"$work/repo/account/stub/package.json"
  check manifest-names-a-dependency FAIL 'package.json names a dependency'
  printf '{"name":"x","devDependencies":{"a":"1.0.0"}}\n' >"$work/repo/account/stub/package.json"
  check manifest-names-a-dev-dependency FAIL 'package.json names a dependency'
  for key in optionalDependencies peerDependencies bundleDependencies bundledDependencies; do
    printf '{"name":"x","%s":{"a":"1.0.0"}}\n' "$key" >"$work/repo/account/stub/package.json"
    check "manifest-names-$key" FAIL 'package.json names a dependency'
  done
  STUB_TAP=$work/tap/short
  check fewer-passed-than-ran FAIL 'only 39 of 40 stub tests passed'
  printf '{"packages":{"":{},"node_modules/a":{"version":"1.0.0"}}}\n' >"$work/repo/account/stub/package-lock.json"
  check lockfile-locks-a-package FAIL 'package-lock.json locks a package'
  mv "$work/repo/account/stub/package-lock.json" "$work/repo/account/stub/package-lock.json.gone"
  check lockfile-missing FAIL 'package-lock.json is missing'

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
