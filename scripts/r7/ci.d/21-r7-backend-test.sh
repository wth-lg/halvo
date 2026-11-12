#!/bin/sh
# jobs: macos
# rule: The backend's test script, sh scripts/with-env.sh node_modules/.bin/vitest run in backend/ as its package.json words it, exits 0 with at least one test and no failed test.
set -u
name=21-r7-backend-test

test_in() {
  repo=$1
  out=$2
  if [ ! -f "$repo/backend/node_modules/.bin/vitest" ] || [ ! -f "$repo/backend/scripts/with-env.sh" ]; then
    cause="backend/node_modules/.bin/vitest or backend/scripts/with-env.sh is missing; sh scripts/prerequisites.sh installs the packages"
    counts=unmeasured
    return 1
  fi
  (cd "$repo/backend" && sh scripts/with-env.sh node_modules/.bin/vitest run) >"$out/test.txt" 2>&1
  code=$?
  files=$(sed -n 's/^ *Test Files  *\([0-9][0-9]*\) passed.*/\1/p' "$out/test.txt" | tail -n 1)
  tests=$(sed -n 's/^ *Tests  *\([0-9][0-9]*\) passed.*/\1/p' "$out/test.txt" | tail -n 1)
  failed=$(grep -c -E '^ *(Test Files|Tests) .*[0-9]+ failed' "$out/test.txt")
  counts="exit=$code test-files=${files:-none} tests=${tests:-none} failed-lines=$failed"
  cause=
  [ "$code" = 0 ] || cause="the test script exited $code"
  [ -n "$cause" ] || [ "$failed" = 0 ] || cause="the test script reports a failed test"
  [ -n "$cause" ] || [ "${tests:-0}" -gt 0 ] || cause="the test script ran no test"
  [ -z "$cause" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  if test_in "$repo" "$artifacts"; then
    printf '%s\n' "$counts"
    printf 'PASS %s\n' "$name"
  else
    printf '%s\n' "$counts"
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/node_modules/.bin" "$dir/backend/scripts" "$dir/state" "$dir/out" || exit 2
  cat >"$dir/backend/scripts/with-env.sh" <<'EOF'
#!/bin/sh
printf 'with-env|%s|%s\n' "$(pwd -P)" "$*" >>"$STUB_STATE/calls"
exec "$@"
EOF
  cat >"$dir/backend/node_modules/.bin/vitest" <<'EOF'
#!/bin/sh
printf 'vitest|%s|%s\n' "$(pwd -P)" "$*" >>"$STUB_STATE/calls"
cat "$STUB_STATE/vitest.out"
exit "$(cat "$STUB_STATE/vitest.exit" 2>/dev/null || echo 0)"
EOF
  chmod +x "$dir/backend/node_modules/.bin/vitest"
  printf '\n Test Files  2 passed (2)\n      Tests  5 passed (5)\n   Duration  1.00s\n' >"$dir/state/vitest.out"
}

expect() {
  want=$1
  case_name=$2
  dir=$scratch/$case_name
  (STUB_STATE=$dir/state test_in "$dir" "$dir/out") >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  if [ "$got" = PASS ]; then
    top=$(cd "$dir/backend" && pwd -P)
    printf 'with-env|%s|node_modules/.bin/vitest run\nvitest|%s|run\n' "$top" "$top" >"$dir/calls.wanted"
    cmp -s "$dir/calls.wanted" "$dir/state/calls" || got=PASS-with-other-calls
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  expect PASS good

  fixture tests-fail
  printf '\n Test Files  1 failed | 1 passed (2)\n      Tests  1 failed | 4 passed (5)\n' >"$scratch/tests-fail/state/vitest.out"
  printf '1\n' >"$scratch/tests-fail/state/vitest.exit"
  expect FAIL tests-fail

  fixture failed-test-with-exit-0
  printf '\n Test Files  1 failed | 1 passed (2)\n      Tests  1 failed | 4 passed (5)\n' >"$scratch/failed-test-with-exit-0/state/vitest.out"
  expect FAIL failed-test-with-exit-0

  fixture no-test-ran
  printf '\nNo test files found, exiting with code 0\n' >"$scratch/no-test-ran/state/vitest.out"
  expect FAIL no-test-ran

  fixture exit-1-with-passed-counts
  printf '1\n' >"$scratch/exit-1-with-passed-counts/state/vitest.exit"
  expect FAIL exit-1-with-passed-counts

  fixture exit-2-with-passed-counts
  printf '2\n' >"$scratch/exit-2-with-passed-counts/state/vitest.exit"
  expect FAIL exit-2-with-passed-counts

  fixture vitest-missing
  mv "$scratch/vitest-missing/backend/node_modules" "$scratch/vitest-missing/node_modules.moved" || exit 2
  expect FAIL vitest-missing

  fixture with-env-missing
  mv "$scratch/with-env-missing/backend/scripts" "$scratch/with-env-missing/scripts.moved" || exit 2
  expect FAIL with-env-missing

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) printf 'FAIL %s (usage)\n' "$name"; exit 1 ;;
esac
