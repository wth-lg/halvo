#!/bin/sh
# jobs: macos
# rule: The backend's test script, sh scripts/with-env.sh node_modules/.bin/vitest run in backend/ as its package.json words it, exits 0 with no failed file or test, passes at least as many files and tests as git tracks test files that its two projects collect (backend/test/**/*.test.ts), and skips no more files or tests than backend/test marks with skip, skipIf, runIf or todo; the windows job does not run it, because on windows-2025 the stack needs lsof and ps -o, which Git Bash lacks, and the test script does not end.
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
  esc=$(printf '\033')
  LC_ALL=C sed "s/$esc\[[0-9;]*m//g" "$out/test.txt" >"$out/test.plain.txt" || return 2
  files=$(sed -n 's/^ *Test Files  *\([0-9][0-9]*\) passed.*/\1/p' "$out/test.plain.txt" | tail -n 1)
  tests=$(sed -n 's/^ *Tests  *\([0-9][0-9]*\) passed.*/\1/p' "$out/test.plain.txt" | tail -n 1)
  failed=$(grep -c -E '^ *(Test Files|Tests) .*[0-9]+ failed' "$out/test.plain.txt")
  skipped=$(grep -E '^ *(Test Files|Tests) ' "$out/test.plain.txt" | grep -o -E '[0-9]+ (skipped|todo)' | awk '{ n += $1 } END { print n + 0 }')
  tracked=$(git -C "$repo" ls-files -- ':(glob)backend/test/**/*.test.ts' | awk 'END { print NR }')
  marked=$(git -C "$repo" grep -h -o -E '[.](skip|skipIf|runIf|todo)[(]' -- backend/test/ | awk 'END { print NR }')
  counts="exit=$code test-files=${files:-none} tests=${tests:-none} failed-lines=$failed skipped=$skipped tracked-test-files=$tracked skip-marks=$marked"
  cause=
  [ "$code" = 0 ] || cause="the test script exited $code"
  [ -n "$cause" ] || [ "$failed" = 0 ] || cause="the test script reports a failed test"
  [ -n "$cause" ] || [ "${tests:-0}" -gt 0 ] || cause="the test script ran no test"
  [ -n "$cause" ] || [ "$tracked" -gt 0 ] || cause="git tracks no test file under backend/test"
  [ -n "$cause" ] || [ "${files:-0}" -ge "$tracked" ] || cause="${files:-no} test files passed, fewer than the $tracked that git tracks"
  [ -n "$cause" ] || [ "${tests:-0}" -ge "$tracked" ] || cause="${tests:-no} tests passed, fewer than the $tracked test files that git tracks"
  [ -n "$cause" ] || [ "$skipped" -le "$marked" ] || cause="$skipped files or tests were skipped, and backend/test marks $marked"
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
  mkdir -p "$dir/backend/node_modules/.bin" "$dir/backend/scripts" "$dir/backend/test/b" "$dir/state" "$dir/out" || exit 2
  git -C "$dir" init -q || exit 2
  printf 'it("a", () => {});\n' >"$dir/backend/test/a.test.ts"
  printf 'it("c", () => {});\n' >"$dir/backend/test/b/c.node.test.ts"
  printf 'export const helper = 1;\n' >"$dir/backend/test/b/helper.ts"
  git -C "$dir" add backend/test || exit 2
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

  fixture coloured-output
  esc=$(printf '\033')
  printf '\n%s[2m Test Files %s[22m %s[1m%s[32m2 passed%s[39m%s[22m%s[90m (2)%s[39m\n%s[2m      Tests %s[22m %s[1m%s[32m5 passed%s[39m%s[22m%s[90m (5)%s[39m\n' \
    "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" >"$scratch/coloured-output/state/vitest.out"
  expect PASS coloured-output

  fixture coloured-failed-test
  printf '\n%s[2m Test Files %s[22m %s[1m%s[31m1 failed%s[39m%s[22m%s[2m | %s[22m%s[1m%s[32m1 passed%s[39m%s[22m%s[90m (2)%s[39m\n' \
    "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" >"$scratch/coloured-failed-test/state/vitest.out"
  expect FAIL coloured-failed-test

  fixture failed-file-with-passed-tests-and-exit-0
  printf '\n Test Files  1 failed | 1 passed (2)\n      Tests  5 passed (5)\n' >"$scratch/failed-file-with-passed-tests-and-exit-0/state/vitest.out"
  expect FAIL failed-file-with-passed-tests-and-exit-0

  fixture one-file-and-one-test
  printf '\n Test Files  1 passed (1)\n      Tests  1 passed (1)\n' >"$scratch/one-file-and-one-test/state/vitest.out"
  expect FAIL one-file-and-one-test

  fixture fewer-tests-than-files
  printf '\n Test Files  2 passed (2)\n      Tests  1 passed (1)\n' >"$scratch/fewer-tests-than-files/state/vitest.out"
  expect FAIL fewer-tests-than-files

  fixture skipped-test-that-the-tree-does-not-mark
  printf '\n Test Files  2 passed (2)\n      Tests  4 passed | 1 skipped (5)\n' >"$scratch/skipped-test-that-the-tree-does-not-mark/state/vitest.out"
  expect FAIL skipped-test-that-the-tree-does-not-mark

  fixture skipped-file-that-the-tree-does-not-mark
  printf '\n Test Files  2 passed | 1 skipped (3)\n      Tests  5 passed (5)\n' >"$scratch/skipped-file-that-the-tree-does-not-mark/state/vitest.out"
  expect FAIL skipped-file-that-the-tree-does-not-mark

  fixture skipped-test-that-the-tree-marks
  printf 'it.skip("d", () => {});\n' >>"$scratch/skipped-test-that-the-tree-marks/backend/test/a.test.ts"
  git -C "$scratch/skipped-test-that-the-tree-marks" add backend/test || exit 2
  printf '\n Test Files  2 passed (2)\n      Tests  4 passed | 1 skipped (5)\n' >"$scratch/skipped-test-that-the-tree-marks/state/vitest.out"
  expect PASS skipped-test-that-the-tree-marks

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
