#!/bin/sh
# jobs: macos
# rule: The backend's test script, sh scripts/with-env.sh node_modules/.bin/vitest run in backend/ as its package.json words it, run with vitest's json reporter beside its default one, exits 0 with no failed file or test, every test file that scripts/r7/backend-tests.txt lists runs with no test that the json report gives a status other than passed or failed (skipped, todo or pending) and at least as many passed tests as its line says, and no other test file runs; the windows job does not run it, because on windows-2025 the stack needs lsof and ps -o, which Git Bash lacks, and the test script does not end.
# lane: alone
set -u
name=21-r7-backend-test
list=scripts/r7/backend-tests.txt

compare_report() {
  node - "$1" "$2" "$3" "$4" <<'EOF'
const fs = require("fs");
const [report, listFile, root, problemsFile] = process.argv.slice(2);
const problems = [];
const listed = new Map();
for (const raw of fs.readFileSync(listFile, "utf8").split("\n")) {
  const line = raw.replace(/\r$/, "");
  if (line === "") continue;
  const match = /^(test\/[^\t]+\.test\.ts)\t([0-9]+)$/.exec(line);
  if (!match || listed.has(match[1])) problems.push(`list line not "test/<file>.test.ts<TAB><count>" or twice: ${line}`);
  else listed.set(match[1], Number(match[2]));
}
const ran = new Map();
for (const file of JSON.parse(fs.readFileSync(report, "utf8")).testResults ?? []) {
  const path = file.name.startsWith(`${root}/`) ? file.name.slice(root.length + 1) : file.name;
  const statuses = (file.assertionResults ?? []).map((a) => a.status);
  ran.set(path, { passed: statuses.filter((s) => s === "passed").length, skipped: statuses.filter((s) => s !== "passed" && s !== "failed").length });
}
const tally = { listed: listed.size, ran: ran.size, missing: 0, short: 0, skipped: 0, unlisted: 0 };
for (const [path, want] of listed) {
  const got = ran.get(path);
  if (!got) { tally.missing++; problems.push(`listed and did not run: ${path}`); continue; }
  if (got.passed < want) { tally.short++; problems.push(`ran ${got.passed} passed tests, fewer than the ${want} of its line: ${path}`); }
  if (got.skipped > 0) { tally.skipped++; problems.push(`${got.skipped} tests neither passed nor failed (skipped, todo or pending): ${path}`); }
}
for (const [path, got] of ran) {
  if (!listed.has(path)) { tally.unlisted++; problems.push(`ran and is not listed (add the line "${path}<TAB>${got.passed}"): ${path}`); }
}
fs.writeFileSync(problemsFile, problems.map((p) => `${p}\n`).join(""));
console.log(Object.entries(tally).map(([k, v]) => `${k}=${v}`).join(" ") + ` problems=${problems.length}`);
process.exit(problems.length === 0 ? 0 : 1);
EOF
}

test_in() {
  repo=$1
  out=$2
  if [ ! -f "$repo/backend/node_modules/.bin/vitest" ] || [ ! -f "$repo/backend/scripts/with-env.sh" ]; then
    cause="backend/node_modules/.bin/vitest or backend/scripts/with-env.sh is missing; sh scripts/prerequisites.sh installs the packages"
    counts=unmeasured
    return 1
  fi
  if [ ! -f "$repo/$list" ]; then
    cause="$list is missing"
    counts=unmeasured
    return 1
  fi
  report=$out/vitest.json
  : >"$report" || return 2
  (cd "$repo/backend" && sh scripts/with-env.sh node_modules/.bin/vitest run --reporter=default --reporter=json "--outputFile.json=$report") >"$out/test.txt" 2>&1
  code=$?
  esc=$(printf '\033')
  LC_ALL=C sed "s/$esc\[[0-9;]*m//g" "$out/test.txt" >"$out/test.plain.txt" || return 2
  tests=$(grep -E '^ *Tests ' "$out/test.plain.txt" | tail -n 1 | grep -o -E '[0-9]+ passed' | sed 's/ passed//')
  failed=$(grep -c -E '^ *(Test Files|Tests) .*[0-9]+ failed' "$out/test.plain.txt")
  root=$(cd "$repo/backend" && pwd -P) || return 2
  if [ -s "$report" ]; then
    listing=$(compare_report "$report" "$repo/$list" "$root" "$out/problems.txt")
    listed_ok=$?
  else
    listing='report=missing'
    listed_ok=1
  fi
  counts="exit=$code tests=${tests:-none} failed-lines=$failed $listing"
  cause=
  [ "$code" = 0 ] || cause="the test script exited $code"
  [ -n "$cause" ] || [ "$failed" = 0 ] || cause="the test script reports a failed test"
  [ -n "$cause" ] || [ "${tests:-0}" -gt 0 ] || cause="the test script ran no test"
  [ -n "$cause" ] || [ -s "$report" ] || cause="vitest wrote no json report"
  [ -n "$cause" ] || [ "$listed_ok" = 0 ] || cause="the test files differ from $list; the list is problems.txt among the artifacts"
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

report() {
  dir=$scratch/$1
  shift
  {
    printf '{"testResults":['
    separator=
    for file in "$@"; do
      path=${file%%:*}
      rest=${file#*:}
      passed=${rest%%:*}
      rest=${rest#*:}
      skipped=${rest%%:*}
      rest=${rest#"$skipped"}
      printf '%s{"name":"@ROOT@/%s","assertionResults":[' "$separator" "$path"
      comma=
      n=0
      while [ "$n" -lt "$passed" ]; do printf '%s{"status":"passed"}' "$comma"; comma=,; n=$((n + 1)); done
      n=0
      while [ "$n" -lt "$skipped" ]; do printf '%s{"status":"skipped"}' "$comma"; comma=,; n=$((n + 1)); done
      while [ -n "$rest" ]; do
        rest=${rest#:}
        status=${rest%%:*}
        rest=${rest#"$status"}
        printf '%s{"status":"%s"}' "$comma" "$status"
        comma=,
      done
      printf ']}'
      separator=,
    done
    printf ']}\n'
  } >"$dir/state/vitest.json"
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/node_modules/.bin" "$dir/backend/scripts" "$dir/scripts/r7" "$dir/state" "$dir/out" || exit 2
  cat >"$dir/backend/scripts/with-env.sh" <<'EOF'
#!/bin/sh
printf 'with-env|%s|%s\n' "$(pwd -P)" "$*" >>"$STUB_STATE/calls"
exec "$@"
EOF
  cat >"$dir/backend/node_modules/.bin/vitest" <<'EOF'
#!/bin/sh
printf 'vitest|%s|%s\n' "$(pwd -P)" "$*" >>"$STUB_STATE/calls"
for argument in "$@"; do
  case $argument in
    --outputFile.json=*)
      [ ! -f "$STUB_STATE/vitest.json" ] || sed "s|@ROOT@|$(pwd -P)|g" "$STUB_STATE/vitest.json" >"${argument#--outputFile.json=}"
      ;;
  esac
done
cat "$STUB_STATE/vitest.out"
exit "$(cat "$STUB_STATE/vitest.exit" 2>/dev/null || echo 0)"
EOF
  chmod +x "$dir/backend/node_modules/.bin/vitest"
  printf 'test/a.test.ts\t2\ntest/b/c.node.test.ts\t3\n' >"$dir/$list"
  printf '\n Test Files  2 passed (2)\n      Tests  5 passed (5)\n   Duration  1.00s\n' >"$dir/state/vitest.out"
  report "$1" test/a.test.ts:2:0 test/b/c.node.test.ts:3:0
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
    wanted="node_modules/.bin/vitest run --reporter=default --reporter=json --outputFile.json=$dir/out/vitest.json"
    printf 'with-env|%s|%s\nvitest|%s|%s\n' "$top" "$wanted" "$top" "${wanted#node_modules/.bin/vitest }" >"$dir/calls.wanted"
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
  scratch=$(cd "$scratch" && pwd -P) || exit 2
  rejected=0
  wrong=

  fixture good
  expect PASS good

  fixture more-tests-than-its-line
  report more-tests-than-its-line test/a.test.ts:4:0 test/b/c.node.test.ts:3:0
  expect PASS more-tests-than-its-line

  fixture coloured-output
  esc=$(printf '\033')
  printf '\n%s[2m Test Files %s[22m %s[1m%s[32m2 passed%s[39m%s[22m%s[90m (2)%s[39m\n%s[2m      Tests %s[22m %s[1m%s[32m5 passed%s[39m%s[22m%s[90m (5)%s[39m\n' \
    "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" >"$scratch/coloured-output/state/vitest.out"
  expect PASS coloured-output

  fixture coloured-failed-test
  printf '\n%s[2m Test Files %s[22m %s[1m%s[31m1 failed%s[39m%s[22m%s[2m | %s[22m%s[1m%s[32m1 passed%s[39m%s[22m%s[90m (2)%s[39m\n%s[2m      Tests %s[22m %s[1m%s[32m5 passed%s[39m%s[22m%s[90m (5)%s[39m\n' \
    "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" "$esc" >"$scratch/coloured-failed-test/state/vitest.out"
  expect FAIL coloured-failed-test

  fixture failed-file-with-passed-tests-and-exit-0
  printf '\n Test Files  1 failed | 2 passed (3)\n      Tests  5 passed (5)\n' >"$scratch/failed-file-with-passed-tests-and-exit-0/state/vitest.out"
  expect FAIL failed-file-with-passed-tests-and-exit-0

  fixture failed-test-with-exit-0
  printf '\n Test Files  1 failed | 2 passed (3)\n      Tests  1 failed | 5 passed (6)\n' >"$scratch/failed-test-with-exit-0/state/vitest.out"
  expect FAIL failed-test-with-exit-0

  fixture listed-file-did-not-run
  report listed-file-did-not-run test/a.test.ts:2:0
  expect FAIL listed-file-did-not-run

  fixture fewer-tests-than-its-line
  report fewer-tests-than-its-line test/a.test.ts:1:0 test/b/c.node.test.ts:3:0
  expect FAIL fewer-tests-than-its-line

  fixture skipped-test
  report skipped-test test/a.test.ts:2:1 test/b/c.node.test.ts:3:0
  expect FAIL skipped-test

  fixture todo-test
  report todo-test test/a.test.ts:2:0:todo test/b/c.node.test.ts:3:0
  expect FAIL todo-test

  fixture pending-test
  report pending-test test/a.test.ts:2:0 test/b/c.node.test.ts:3:0:pending
  expect FAIL pending-test

  fixture unlisted-file-ran
  report unlisted-file-ran test/a.test.ts:2:0 test/b/c.node.test.ts:3:0 test/d.test.ts:1:0
  expect FAIL unlisted-file-ran

  fixture list-line-of-another-form
  printf 'test/e.test.ts 1\n' >>"$scratch/list-line-of-another-form/$list"
  expect FAIL list-line-of-another-form

  fixture list-line-twice
  printf 'test/a.test.ts\t2\n' >>"$scratch/list-line-twice/$list"
  expect FAIL list-line-twice

  fixture list-missing
  mv "$scratch/list-missing/$list" "$scratch/list-missing/list.moved" || exit 2
  expect FAIL list-missing

  fixture json-report-missing
  mv "$scratch/json-report-missing/state/vitest.json" "$scratch/json-report-missing/state/vitest.json.moved" || exit 2
  expect FAIL json-report-missing

  fixture tests-fail
  printf '\n Test Files  1 failed | 1 passed (2)\n      Tests  1 failed | 4 passed (5)\n' >"$scratch/tests-fail/state/vitest.out"
  printf '1\n' >"$scratch/tests-fail/state/vitest.exit"
  expect FAIL tests-fail

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
