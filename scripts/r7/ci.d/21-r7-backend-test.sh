#!/bin/sh
# jobs: macos
# rule: The backend's test script, sh scripts/with-env.sh node_modules/.bin/vitest run in backend/ as its package.json words it, run with vitest's json reporter beside its default one, exits 0 with no failed file or test, every test file that scripts/r7/backend-tests.txt lists runs with no test that the json report gives a status other than passed or failed (skipped, todo or pending) and at least as many passed tests as its line says, and no other test file runs; check 50's records, sh scripts/with-env.sh node gate/fixtures/concurrency/records.mjs in backend/ with GATE_STATE under the artifacts, exit 0 with one line of a record as expected for each of N records and the last line "concurrency: records: N of N as expected", where N is the number of .json files under backend/gate/fixtures/concurrency/records/ and its sub-directories at HEAD and is above 0; no tracked file differs from HEAD after both runs; the windows job does not run them, because on windows-2025 the stack needs lsof and ps -o, which Git Bash lacks, and the test script does not end.
set -u
name=21-r7-backend-test
list=scripts/r7/backend-tests.txt
records_dir=backend/gate/fixtures/concurrency/records
records_script=backend/gate/fixtures/concurrency/records.mjs

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
  cause=
  counts=unmeasured
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

records_in() {
  r_repo=$1
  r_out=$2
  records_counts='records=unmeasured'
  records_cause=
  if [ ! -d "$r_repo/backend/node_modules" ]; then
    records_cause="backend/node_modules is missing; sh scripts/prerequisites.sh installs the packages"
    return 1
  fi
  if [ ! -f "$r_repo/$records_script" ] || [ ! -f "$r_repo/backend/scripts/with-env.sh" ]; then
    records_cause="$records_script or backend/scripts/with-env.sh is missing"
    return 1
  fi
  if ! git -C "$r_repo" ls-tree -r -z --name-only HEAD -- "$records_dir/" >"$r_out/records-at-head.z"; then
    records_cause="git could not list $records_dir/ at HEAD"
    return 1
  fi
  LC_ALL=C tr '\n\000' '?\n' <"$r_out/records-at-head.z" >"$r_out/records-at-head.txt" || return 2
  r_want=$(grep -c '\.json$' "$r_out/records-at-head.txt")
  mkdir -p "$r_out/records-state" || return 2
  (cd "$r_repo/backend" && GATE_STATE=$r_out/records-state sh scripts/with-env.sh node gate/fixtures/concurrency/records.mjs) >"$r_out/records.txt" 2>"$r_out/records.err.txt"
  r_code=$?
  r_last=$(tail -n 1 "$r_out/records.txt")
  r_failed=$(grep -c '^self-test concurrency: record ' "$r_out/records.txt")
  r_read=$(grep -c -E '^self-test concurrency: (good record-.*: PASS|bad record-.*: FAIL) as expected$' "$r_out/records.txt")
  r_line=other
  [ "$r_last" != "concurrency: records: $r_want of $r_want as expected" ] || r_line=all-of-head
  git -C "$r_repo" diff --quiet HEAD --
  r_tree=$?
  records_counts="records exit=$r_code at-head=$r_want as-expected=$r_read failed=$r_failed last-line=$r_line tree-diff=$r_tree"
  [ "$r_want" -gt 0 ] || records_cause="HEAD holds no record under $records_dir/"
  [ -n "$records_cause" ] || [ "$r_code" = 0 ] || records_cause="check 50's records exited $r_code; the lines are records.txt among the artifacts"
  [ -n "$records_cause" ] || [ "$r_line" = all-of-head ] || records_cause="the last line of $records_script is not \"concurrency: records: $r_want of $r_want as expected\" for the $r_want records at HEAD; the lines are records.txt among the artifacts"
  [ -n "$records_cause" ] || [ "$r_read" = "$r_want" ] || records_cause="$records_script printed $r_read lines of a record as expected for the $r_want records at HEAD; the lines are records.txt among the artifacts"
  case $r_tree in
    0) ;;
    1) records_cause="${records_cause:+$records_cause; }a tracked file differs from HEAD after the node tests and the records" ;;
    *) records_cause="${records_cause:+$records_cause; }git could not compare the tracked files with HEAD" ;;
  esac
  [ -z "$records_cause" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  test_in "$repo" "$artifacts"
  tested=$?
  [ "$tested" = 0 ] || [ -n "$cause" ] || cause="the test run stopped with status $tested"
  printf '%s\n' "$counts"
  records_in "$repo" "$artifacts"
  recorded=$?
  [ "$recorded" = 0 ] || [ -n "$records_cause" ] || records_cause="the records' run stopped with status $recorded"
  printf '%s\n' "$records_counts"
  if [ "$tested" = 0 ] && [ "$recorded" = 0 ]; then
    printf 'PASS %s\n' "$name"
  else
    [ "$tested" = 0 ] || [ "$recorded" = 0 ] || cause="$cause; $records_cause"
    [ "$tested" != 0 ] || cause=$records_cause
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

verdict() {
  want=$1
  got=$2
  printf 'case %s/%s want=%s got=%s\n' "$name" "$3" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $3"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

expect() {
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
  verdict "$1" "$got" "$case_name"
}

records_base() {
  base=$scratch/records-base
  mkdir -p "$base" || exit 2
  git -C "$checkout" archive --format=tar HEAD -- backend >"$scratch/backend.tar" || exit 2
  tar -xf "$scratch/backend.tar" -C "$base" || exit 2
  git -C "$base" init -q >/dev/null 2>&1 || exit 2
  git -C "$base" add -A || exit 2
  git -C "$base" -c maintenance.auto=false commit -q -m "the backend at HEAD" >/dev/null || exit 2
  first_good=$(git -C "$base" ls-tree --name-only HEAD -- "$records_dir/" | grep -E '/good-[^/]*\.json$' | head -n 1)
  [ -n "$first_good" ] || exit 2
}

records_fixture() {
  dir=$scratch/$1
  git clone -q "$scratch/records-base" "$dir" || exit 2
  mkdir -p "$dir/out" || exit 2
  [ "${2:-}" = without-packages ] || ln -s "$checkout/backend/node_modules" "$dir/backend/node_modules" || exit 2
}

commit_fixture() {
  git -C "$scratch/$1" -c maintenance.auto=false commit -q -a -m "$1" >/dev/null || exit 2
}

edit_file() {
  node - "$1" "$2" "$3" <<'EOF' || exit 2
const fs = require("fs");
const [file, from, to] = process.argv.slice(2);
const text = fs.readFileSync(file, "utf8");
if (text.split(from).length !== 2) process.exit(3);
fs.writeFileSync(file, text.replace(from, () => to));
EOF
}

break_record() {
  node - "$scratch/$1/$records_dir" "$2" "$3" "$4" <<'EOF' || exit 2
const fs = require("fs");
const path = require("path");
const [dir, kind, prefix, body] = process.argv.slice(2);
const file = fs.readdirSync(dir)
  .filter((name) => name.startsWith(prefix) && name.endsWith(".json"))
  .sort()
  .find((name) => JSON.parse(fs.readFileSync(path.join(dir, name), "utf8")).kind === kind);
if (!file) process.exit(3);
const before = fs.readFileSync(path.join(dir, file), "utf8");
const record = JSON.parse(before);
new Function("record", body)(record);
const after = `${JSON.stringify(record, null, 1)}\n`;
if (JSON.stringify(JSON.parse(after)) === JSON.stringify(JSON.parse(before))) process.exit(4);
fs.writeFileSync(path.join(dir, file), after);
EOF
}

expect_records() {
  case_name=$2
  dir=$scratch/$case_name
  (records_in "$dir" "$dir/out" >/dev/null 2>&1; code=$?; printf '%s\n' "$records_cause" >"$dir/cause"; exit "$code")
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  [ "$got" != FAIL ] || grep -q -F -- "$3" "$dir/cause" || got=FAIL-for-another-cause
  verdict "$1" "$got" "$case_name"
}

measure_fixture() {
  case_name=$1
  records_exit=$2
  shift 2
  fixture "$case_name"
  mkdir -p "$dir/$records_dir" || exit 2
  printf '{}\n' >"$dir/$records_dir/good-a.json" || exit 2
  {
    printf 'import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.GATE_STATE + "/written", "");\n'
    for line in "$@"; do printf 'console.log("%s");\n' "$line"; done
    printf 'console.error("a line on standard error");\nprocess.exit(%s);\n' "$records_exit"
  } >"$dir/$records_script"
  git -C "$dir" init -q >/dev/null 2>&1 || exit 2
  git -C "$dir" add backend scripts || exit 2
  git -C "$dir" -c maintenance.auto=false commit -q -m "$case_name" >/dev/null || exit 2
}

expect_measure() {
  case_name=$2
  dir=$scratch/$case_name
  (CI_D_REPO=$dir CI_D_ARTIFACTS=$dir/out STUB_STATE=$dir/state measure) >"$dir/measure.txt" 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  records_line=$(grep '^records ' "$dir/measure.txt")
  last_line=$(tail -n 1 "$dir/measure.txt")
  [ "$records_line" = "$3" ] || got=$got-with-another-records-line
  [ "$last_line" = "$4" ] || got=$got-with-another-last-line
  top=$(cd "$dir/backend" && pwd -P)
  grep -q -x -F "with-env|$top|node gate/fixtures/concurrency/records.mjs" "$dir/state/calls" || got=$got-without-with-env
  [ -f "$dir/out/records-state/written" ] || got=$got-without-the-state-under-the-artifacts
  verdict "$1" "$got" "$case_name"
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  scratch=$(cd "$scratch" && pwd -P) || exit 2
  checkout=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
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

  if [ ! -d "$checkout/backend/node_modules" ]; then
    printf 'FAIL self-test:%s (backend/node_modules is missing, and the cases of the records run the records of HEAD with it; sh scripts/prerequisites.sh installs the packages)\n' "$name"
    exit 1
  fi
  records_base

  records_fixture records-of-head
  expect_records PASS records-of-head ''

  records_fixture a-file-that-is-no-record-at-head
  printf 'not a record\n' >"$scratch/a-file-that-is-no-record-at-head/$records_dir/notes.txt" || exit 2
  git -C "$scratch/a-file-that-is-no-record-at-head" add "$records_dir/notes.txt" || exit 2
  commit_fixture a-file-that-is-no-record-at-head
  expect_records PASS a-file-that-is-no-record-at-head ''

  records_fixture a-record-with-a-name-that-is-not-ascii
  cp "$scratch/a-record-with-a-name-that-is-not-ascii/$first_good" "$scratch/a-record-with-a-name-that-is-not-ascii/$records_dir/good-caf$(printf '\303\251')-a-copy.json" || exit 2
  git -C "$scratch/a-record-with-a-name-that-is-not-ascii" add "$records_dir" || exit 2
  commit_fixture a-record-with-a-name-that-is-not-ascii
  expect_records PASS a-record-with-a-name-that-is-not-ascii ''

  records_fixture a-record-in-a-sub-directory
  mkdir -p "$scratch/a-record-in-a-sub-directory/$records_dir/kept" || exit 2
  git -C "$scratch/a-record-in-a-sub-directory" mv "$first_good" "$records_dir/kept/" || exit 2
  commit_fixture a-record-in-a-sub-directory
  expect_records FAIL a-record-in-a-sub-directory 'the last line of'

  records_fixture records-mjs-prints-only-the-last-line
  edit_file "$scratch/records-mjs-prints-only-the-last-line/$records_script" 'let failed = 0;' 'console.log("concurrency: records: " + files.length + " of " + files.length + " as expected");
process.exit(0);
let failed = 0;'
  commit_fixture records-mjs-prints-only-the-last-line
  expect_records FAIL records-mjs-prints-only-the-last-line 'lines of a record as expected'

  records_fixture a-good-record-expects-another-count
  break_record a-good-record-expects-another-count judge good- 'record.expect.counted = !record.expect.counted'
  commit_fixture a-good-record-expects-another-count
  expect_records FAIL a-good-record-expects-another-count "check 50's records exited 1"

  records_fixture a-bad-record-expects-no-violation
  break_record a-bad-record-expects-no-violation judge bad- 'record.expect.violations = []'
  commit_fixture a-bad-record-expects-no-violation
  expect_records FAIL a-bad-record-expects-no-violation "check 50's records exited 1"

  records_fixture a-line-record-expects-another-line
  break_record a-line-record-expects-another-line line good- 'record.expect.line += "."'
  commit_fixture a-line-record-expects-another-line
  expect_records FAIL a-line-record-expects-another-line "check 50's records exited 1"

  records_fixture a-record-of-a-kind-with-no-reading
  break_record a-record-of-a-kind-with-no-reading judge good- 'record.kind = "no-such-reading"'
  commit_fixture a-record-of-a-kind-with-no-reading
  expect_records FAIL a-record-of-a-kind-with-no-reading "check 50's records exited 1"

  records_fixture a-record-that-is-no-json
  printf ',\n' >>"$scratch/a-record-that-is-no-json/$first_good" || exit 2
  commit_fixture a-record-that-is-no-json
  expect_records FAIL a-record-that-is-no-json "check 50's records exited 1"

  records_fixture records-mjs-skips-a-record
  edit_file "$scratch/records-mjs-skips-a-record/$records_script" '  .sort();' '  .sort().slice(1);'
  commit_fixture records-mjs-skips-a-record
  expect_records FAIL records-mjs-skips-a-record 'the last line of'

  records_fixture records-mjs-exits-0-on-a-broken-record
  edit_file "$scratch/records-mjs-exits-0-on-a-broken-record/$records_script" 'process.exit(failed === 0 ? 0 : 1);' 'process.exit(0);'
  break_record records-mjs-exits-0-on-a-broken-record judge good- 'record.expect.counted = !record.expect.counted'
  commit_fixture records-mjs-exits-0-on-a-broken-record
  expect_records FAIL records-mjs-exits-0-on-a-broken-record 'the last line of'

  records_fixture a-record-that-head-does-not-hold
  cp "$scratch/a-record-that-head-does-not-hold/$first_good" "$scratch/a-record-that-head-does-not-hold/$records_dir/good-a-copy-that-head-does-not-hold.json" || exit 2
  expect_records FAIL a-record-that-head-does-not-hold 'the last line of'

  records_fixture the-records-write-a-tracked-file
  edit_file "$scratch/the-records-write-a-tracked-file/$records_script" 'process.exit(failed === 0 ? 0 : 1);' '(await import("node:fs")).appendFileSync(join(HERE, "record-base.json"), " ");
process.exit(failed === 0 ? 0 : 1);'
  commit_fixture the-records-write-a-tracked-file
  expect_records FAIL the-records-write-a-tracked-file 'a tracked file differs from HEAD'

  records_fixture records-mjs-missing
  mv "$scratch/records-mjs-missing/$records_script" "$scratch/records-mjs-missing/records.mjs.moved" || exit 2
  commit_fixture records-mjs-missing
  expect_records FAIL records-mjs-missing "$records_script or backend/scripts/with-env.sh is missing"

  records_fixture records-with-env-missing
  mv "$scratch/records-with-env-missing/backend/scripts/with-env.sh" "$scratch/records-with-env-missing/with-env.sh.moved" || exit 2
  commit_fixture records-with-env-missing
  expect_records FAIL records-with-env-missing "$records_script or backend/scripts/with-env.sh is missing"

  records_fixture no-record-at-head
  mv "$scratch/no-record-at-head/$records_dir" "$scratch/no-record-at-head/records.moved" || exit 2
  commit_fixture no-record-at-head
  mkdir -p "$scratch/no-record-at-head/$records_dir" || exit 2
  expect_records FAIL no-record-at-head 'HEAD holds no record'

  records_fixture packages-missing without-packages
  expect_records FAIL packages-missing 'backend/node_modules is missing'

  records_fixture head-without-a-commit
  git -C "$scratch/head-without-a-commit" update-ref -d HEAD || exit 2
  expect_records FAIL head-without-a-commit "git could not list $records_dir/ at HEAD"

  passed_records='concurrency: records: 1 of 1 as expected'
  measure_fixture measure-runs-the-tests-and-the-records 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  expect_measure PASS measure-runs-the-tests-and-the-records 'records exit=0 at-head=1 as-expected=1 failed=0 last-line=all-of-head tree-diff=0' "PASS $name"

  measure_fixture measure-fails-on-the-records-alone 1 'self-test concurrency: record good-a: broken' 'concurrency: records: 0 of 1 as expected'
  expect_measure FAIL measure-fails-on-the-records-alone 'records exit=1 at-head=1 as-expected=0 failed=1 last-line=other tree-diff=0' "FAIL $name (check 50's records exited 1; the lines are records.txt among the artifacts)"

  measure_fixture measure-fails-on-the-tests-alone 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  printf '1\n' >"$scratch/measure-fails-on-the-tests-alone/state/vitest.exit"
  expect_measure FAIL measure-fails-on-the-tests-alone 'records exit=0 at-head=1 as-expected=1 failed=0 last-line=all-of-head tree-diff=0' "FAIL $name (the test script exited 1)"

  measure_fixture measure-fails-on-both 1 'self-test concurrency: record good-a: broken' 'concurrency: records: 0 of 1 as expected'
  printf '1\n' >"$scratch/measure-fails-on-both/state/vitest.exit"
  expect_measure FAIL measure-fails-on-both 'records exit=1 at-head=1 as-expected=0 failed=1 last-line=other tree-diff=0' "FAIL $name (the test script exited 1; check 50's records exited 1; the lines are records.txt among the artifacts)"

  measure_fixture a-test-run-that-stops-with-status-2 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  mkdir -p "$scratch/a-test-run-that-stops-with-status-2/out/vitest.json" || exit 2
  expect_measure FAIL a-test-run-that-stops-with-status-2 'records exit=0 at-head=1 as-expected=1 failed=0 last-line=all-of-head tree-diff=0' "FAIL $name (the test run stopped with status 2)"

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
