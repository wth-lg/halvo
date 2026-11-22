#!/bin/sh
# jobs: macos
# rule: The backend's test script, sh scripts/with-env.sh node_modules/.bin/vitest run in backend/ as its package.json words it, run with vitest's json reporter beside its default one, exits 0 with no failed file or test, every test file that scripts/r7/backend-tests.txt of the commit C (HEAD, read before the tests; the list read from C's objects after them) lists runs with no test that the json report gives a status other than passed or failed (skipped, todo or pending) and at least as many passed tests as its line says, and no other test file runs; then check 50's records run from C's objects, backend/ at C exported by git archive into a new directory under TMPDIR with one link to the checkout's backend/node_modules and records.mjs run there through the export's scripts/with-env.sh with GATE_STATE and its lines under the artifacts, every .json path under backend/gate/fixtures/concurrency/records/ at C is a record named good-<name>.json or bad-<name>.json directly in that directory with no line break and in UTF-8, C holds at least one, and the output holds each record's own line as expected once, by name, no problem line whatever the exit code, no other line, and the summary "concurrency: records: N of N as expected" once as its last line, and before the records' lines each .json fixture directly under backend/gate/fixtures/concurrency/refused/ at C, at least one and each named by the same rules, has its one line "self-test concurrency: refused record-<name>: refused as expected: <the cause that the fixture names>" and the tally "concurrency: refused records: N of N refused as expected" stands once, no other line stands there, and a line that says that a fixture was not refused fails the run whatever the exit code; no tracked file differs from C or carries an index flag other than H (git diff and git ls-files -v with core.fsmonitor and core.untrackedCache off) before the node tests, after them and after the records' run; trusted and not checked, by ruling P7-101 and its Addition 1 and ruling P7-107: backend/node_modules, the tools on the PATH (node, git, tar, sh), code that a node test starts or arranges to run after the tests end (it can write the export, the lines under the artifacts and backend/node_modules), the clone's .git, the index against a change that is staged and put back, and records.mjs as C holds it; the windows job does not run them, because on windows-2025 the stack needs lsof and ps -o, which Git Bash lacks, and the test script does not end. In a pull request's run (GITHUB_EVENT_NAME pull_request, HEAD a merge commit) the check runs nothing and passes, with one line before PASS that names the count of the paths that it read, when at least one path differs between HEAD and its first parent (git diff-tree -r --no-renames) and none of them, in any letter case, lies under backend/ or formal/r7-ledger/, the roots of 24-r7-wall that scripts/r7/backend-roots.sh holds for both checks, or under scripts/r7/ or scripts/ci/, or is scripts/prerequisites.sh or .github/workflows/ci.yml, and git quotes no name; every other event, an unset event, a HEAD with one parent and a list that git cannot make run the tests.
# lane: alone
set -u
name=21-r7-backend-test
list=scripts/r7/backend-tests.txt
records_dir=backend/gate/fixtures/concurrency/records
refused_dir=backend/gate/fixtures/concurrency/refused
records_script=backend/gate/fixtures/concurrency/records.mjs
# shellcheck source=scripts/r7/backend-roots.sh
. "$(dirname "$0")/../backend-roots.sh"
run_paths='scripts/r7/ scripts/ci/ scripts/prerequisites.sh .github/workflows/ci.yml'

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

records_check() {
  node - "$@" <<'EOF'
const fs = require("fs");
const [mode, ...args] = process.argv.slice(2);
const quote = (text) => JSON.stringify(text.length > 200 ? `${text.slice(0, 200)}...` : text);
const show = (bytes) => Array.from(bytes, (b) => (b > 0x20 && b < 0x7f && b !== 0x5c ? String.fromCharCode(b) : `\\x${b.toString(16).padStart(2, "0")}`)).join("");
const fail = (causeFile, cause) => {
  fs.writeFileSync(causeFile, `${cause}\n`);
  process.exit(1);
};
const names = (listFile, dir, wantedFile, causeFile) => {
  const raw = fs.readFileSync(listFile);
  const paths = [];
  let start = 0;
  for (let at = 0; at < raw.length; at++) {
    if (raw[at] === 0) {
      paths.push(raw.subarray(start, at));
      start = at + 1;
    }
  }
  if (start < raw.length) paths.push(raw.subarray(start));
  paths.sort(Buffer.compare);
  const prefix = Buffer.from(`${dir}/`);
  const json = Buffer.from(".json");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const wanted = [];
  for (const path of paths) {
    if (path.length < json.length || !path.subarray(path.length - json.length).equals(json)) continue;
    if (path.length <= prefix.length || !path.subarray(0, prefix.length).equals(prefix)) fail(causeFile, `git listed a path outside ${dir}/: ${show(path)}`);
    const rest = path.subarray(prefix.length);
    if (rest.includes(0x2f)) fail(causeFile, `a record in a sub-directory of ${dir}/ at the commit, which records.mjs does not read: ${show(path)}`);
    if (rest.includes(0x0a) || rest.includes(0x0d)) fail(causeFile, `the name of a record at the commit holds a line break: ${show(path)}`);
    let name;
    try {
      name = decoder.decode(rest);
    } catch {
      fail(causeFile, `the name of a record at the commit is no UTF-8: ${show(path)}`);
    }
    const match = /^(good|bad)-(.+)\.json$/.exec(name);
    if (!match) fail(causeFile, `a .json path at the commit that is neither good-<name>.json nor bad-<name>.json: ${show(path)}`);
    wanted.push(JSON.stringify([`self-test concurrency: ${match[1]} record-${match[2]}: ${match[1] === "good" ? "PASS" : "FAIL"} as expected`, `${dir}/${name}`]));
  }
  fs.writeFileSync(wantedFile, wanted.map((line) => `${line}\n`).join(""));
  console.log(wanted.length);
};
const refused = (listFile, dir, wantedFile, causeFile) => {
  const raw = fs.readFileSync(listFile);
  const paths = [];
  let start = 0;
  for (let at = 0; at < raw.length; at++) {
    if (raw[at] === 0) {
      paths.push(raw.subarray(start, at));
      start = at + 1;
    }
  }
  if (start < raw.length) paths.push(raw.subarray(start));
  paths.sort(Buffer.compare);
  const prefix = Buffer.from(`${dir}/`);
  const json = Buffer.from(".json");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const wanted = [];
  for (const path of paths) {
    if (path.length < json.length || !path.subarray(path.length - json.length).equals(json)) continue;
    if (path.length <= prefix.length || !path.subarray(0, prefix.length).equals(prefix)) fail(causeFile, `git listed a path outside ${dir}/: ${show(path)}`);
    const rest = path.subarray(prefix.length);
    if (rest.includes(0x2f)) fail(causeFile, `a refused fixture in a sub-directory of ${dir}/ at the commit, which records.mjs does not read: ${show(path)}`);
    if (rest.includes(0x0a) || rest.includes(0x0d)) fail(causeFile, `the name of a refused fixture at the commit holds a line break: ${show(path)}`);
    let name;
    try {
      name = decoder.decode(rest);
    } catch {
      fail(causeFile, `the name of a refused fixture at the commit is no UTF-8: ${show(path)}`);
    }
    wanted.push(JSON.stringify([name.slice(0, -json.length), `${dir}/${name}`]));
  }
  fs.writeFileSync(wantedFile, wanted.map((line) => `${line}\n`).join(""));
  console.log(wanted.length);
};
const lines = (wantedFile, outputFile, code, script, causeFile, refusedFile, exportDir) => {
  const wanted = fs.readFileSync(wantedFile, "utf8").split("\n").filter((line) => line !== "").map((line) => JSON.parse(line));
  const paths = new Map(wanted);
  const count = wanted.length;
  const refusedWanted = [];
  for (const [fixture, path] of fs.readFileSync(refusedFile, "utf8").split("\n").filter((line) => line !== "").map((line) => JSON.parse(line))) {
    let cause;
    try {
      cause = JSON.parse(fs.readFileSync(`${exportDir}/${path}`, "utf8")).refusal;
    } catch {
      cause = undefined;
    }
    if (typeof cause !== "string" || cause === "") fail(causeFile, `the refused fixture ${path} at the commit names no cause of its refusal`);
    refusedWanted.push([`self-test concurrency: refused record-${fixture}: refused as expected: ${cause}`, path]);
  }
  const refusedPaths = new Map(refusedWanted);
  const tally = `concurrency: refused records: ${refusedWanted.length} of ${refusedWanted.length} refused as expected`;
  const output = fs.readFileSync(outputFile, "utf8").split("\n");
  if (output.length > 0 && output[output.length - 1] === "") output.pop();
  const summary = `concurrency: records: ${count} of ${count} as expected`;
  const problems = output.filter((line) => line.startsWith("self-test concurrency: record "));
  const lastOk = output.length > 0 && output[output.length - 1] === summary && output.filter((line) => line === summary).length === 1;
  const seen = new Set();
  const seenRefused = new Set();
  let tallies = 0;
  let notRefused = "";
  let afterTheRecords = "";
  let recordsBegan = false;
  let other = "";
  for (const line of lastOk ? output.slice(0, -1) : output) {
    if (refusedPaths.has(line) || line === tally) {
      if (line === tally) tallies++;
      else if (seenRefused.has(line)) other ||= `the records' output line ${quote(line)} stands a second time`;
      seenRefused.add(line);
      if (recordsBegan) afterTheRecords ||= line;
    } else if (line.startsWith("self-test concurrency: refused record-")) {
      if (line.includes(": not refused as expected")) notRefused ||= line;
      else other ||= `the records' output line ${quote(line)} names no refused fixture of the commit`;
      if (recordsBegan) afterTheRecords ||= line;
    } else if (line.startsWith("concurrency: refused records: ")) {
      other ||= `the records' output line ${quote(line)} is not the refused tally ${quote(tally)}`;
      if (recordsBegan) afterTheRecords ||= line;
    } else if (paths.has(line)) {
      recordsBegan = true;
      if (seen.has(line)) other ||= `the records' output line ${quote(line)} stands a second time`;
      seen.add(line);
    } else if (/^self-test concurrency: (good|bad) record-.*: (PASS|FAIL) as expected$/.test(line)) {
      recordsBegan = true;
      other ||= `the records' output line ${quote(line)} names no record of the commit`;
    } else if (line.startsWith("self-test concurrency: record ")) {
      recordsBegan = true;
    } else if (line !== summary) {
      other ||= `the records' output line ${quote(line)} has another form`;
    }
  }
  const missing = wanted.find(([line]) => !seen.has(line));
  const missingRefused = refusedWanted.find(([line]) => !seenRefused.has(line));
  console.log(`exit=${code} at-commit=${count} as-expected=${seen.size} problems=${problems.length} last-line=${lastOk ? "ok" : "other"}`);
  if (notRefused) fail(causeFile, `check 50's records printed that a refused fixture was not refused: ${quote(notRefused)}`);
  if (problems.length > 0 && code !== "0") fail(causeFile, `check 50's records exited ${code}; the first problem line: ${quote(problems[0])}`);
  if (problems.length > 0) fail(causeFile, `check 50's records printed a problem line with exit 0: ${quote(problems[0])}`);
  if (code !== "0") fail(causeFile, `check 50's records exited ${code}; the lines are records.txt among the artifacts`);
  if (!lastOk) fail(causeFile, `the last line of ${script} is not ${quote(summary)}, once, for the ${count} records of the commit; the lines are records.txt among the artifacts`);
  if (missing) fail(causeFile, `record ${missing[1]} of the commit has no line ${quote(missing[0])} in the records' output`);
  if (missingRefused) fail(causeFile, `refused fixture ${missingRefused[1]} of the commit has no line ${quote(missingRefused[0])} in the records' output`);
  if (tallies !== 1) fail(causeFile, `the refused tally ${quote(tally)} for the ${refusedWanted.length} refused fixtures of the commit stands ${tallies} times in the records' output, not once`);
  if (afterTheRecords) fail(causeFile, `the records' output line ${quote(afterTheRecords)} of the refused pass stands after the records' lines`);
  if (other) fail(causeFile, other);
};
try {
  if (mode === "names") names(...args);
  else if (mode === "refused") refused(...args);
  else if (mode === "lines") lines(...args);
  else process.exit(3);
} catch (error) {
  console.error(error);
  process.exit(3);
}
EOF
}

test_in() {
  repo=$1
  out=$2
  t_commit=$3
  cause=
  counts=unmeasured
  if [ ! -f "$repo/backend/node_modules/.bin/vitest" ] || [ ! -f "$repo/backend/scripts/with-env.sh" ]; then
    cause="backend/node_modules/.bin/vitest or backend/scripts/with-env.sh is missing; sh scripts/prerequisites.sh installs the packages"
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
  listed_at=yes
  git -C "$repo" show "$t_commit:$list" >"$out/backend-tests-at-commit.txt" 2>"$out/backend-tests-git.err.txt" || listed_at=no
  if [ "$listed_at" = no ]; then
    listing='list=missing'
    listed_ok=1
  elif [ -s "$report" ]; then
    listing=$(compare_report "$report" "$out/backend-tests-at-commit.txt" "$root" "$out/problems.txt")
    listed_ok=$?
  else
    listing='report=missing'
    listed_ok=1
  fi
  counts="exit=$code tests=${tests:-none} failed-lines=$failed $listing"
  cause=
  [ "$listed_at" = yes ] || cause="$list is missing at the commit"
  [ -n "$cause" ] || [ "$code" = 0 ] || cause="the test script exited $code"
  [ -n "$cause" ] || [ "$failed" = 0 ] || cause="the test script reports a failed test"
  [ -n "$cause" ] || [ "${tests:-0}" -gt 0 ] || cause="the test script ran no test"
  [ -n "$cause" ] || [ -s "$report" ] || cause="vitest wrote no json report"
  [ -n "$cause" ] || [ "$listed_ok" = 0 ] || cause="the test files differ from $list; the list is problems.txt among the artifacts"
  [ -z "$cause" ]
}

records_in() {
  r_repo=$1
  r_out=$2
  r_commit=$3
  records_counts='records=unmeasured'
  records_cause=
  if [ ! -d "$r_repo/backend/node_modules" ]; then
    records_cause="backend/node_modules is missing; sh scripts/prerequisites.sh installs the packages"
    return 1
  fi
  if ! git -C "$r_repo" ls-tree -r -z --name-only "$r_commit" -- "$records_dir/" >"$r_out/records-at-commit.z" 2>"$r_out/records-git.err.txt"; then
    records_cause="git could not list $records_dir/ at the commit"
    return 1
  fi
  if ! git -C "$r_repo" ls-tree --name-only "$r_commit" -- backend/node_modules >"$r_out/node-modules-at-commit.txt" 2>>"$r_out/records-git.err.txt"; then
    records_cause="git could not list backend/node_modules at the commit"
    return 1
  fi
  if [ -s "$r_out/node-modules-at-commit.txt" ]; then
    records_cause="the commit holds backend/node_modules, where the records' run puts the link to the checkout's packages"
    return 1
  fi
  if ! git -C "$r_repo" cat-file -e "$r_commit:$records_script" 2>/dev/null || ! git -C "$r_repo" cat-file -e "$r_commit:backend/scripts/with-env.sh" 2>/dev/null; then
    records_cause="$records_script or backend/scripts/with-env.sh is missing at the commit"
    return 1
  fi
  : >"$r_out/records-cause.txt" || return 2
  if ! r_count=$(records_check names "$r_out/records-at-commit.z" "$records_dir" "$r_out/records-wanted.txt" "$r_out/records-cause.txt"); then
    records_cause=$(cat "$r_out/records-cause.txt")
    [ -n "$records_cause" ] || records_cause="the check of the records' names stopped"
    return 1
  fi
  if [ "$r_count" = 0 ]; then
    records_cause="the commit holds no record under $records_dir/"
    return 1
  fi
  if ! git -C "$r_repo" ls-tree -r -z --name-only "$r_commit" -- "$refused_dir/" >"$r_out/refused-at-commit.z" 2>>"$r_out/records-git.err.txt"; then
    records_cause="git could not list $refused_dir/ at the commit"
    return 1
  fi
  if ! r_refused=$(records_check refused "$r_out/refused-at-commit.z" "$refused_dir" "$r_out/refused-wanted.txt" "$r_out/records-cause.txt"); then
    records_cause=$(cat "$r_out/records-cause.txt")
    [ -n "$records_cause" ] || records_cause="the check of the refused fixtures' names stopped"
    return 1
  fi
  if [ "$r_refused" = 0 ]; then
    records_cause="the commit holds no refused fixture under $refused_dir/"
    return 1
  fi
  r_top=$(cd "$r_repo" && pwd -P) || return 2
  r_arts=$(cd "$r_out" && pwd -P) || return 2
  r_export=$(mktemp -d "${TMPDIR:-/tmp}/$name-records.XXXXXX") || return 2
  r_export=$(cd "$r_export" && pwd -P) || return 2
  case $r_export/ in
    "$r_top"/*|"$r_arts"/*)
      records_cause="the records' directory under TMPDIR lies inside the checkout or the artifacts; set TMPDIR outside both"
      return 1
      ;;
  esac
  (git --no-replace-objects -C "$r_repo" archive --format=tar "$r_commit" backend; printf '%s\n' "$?" >"$r_export/archive.status") | tar -xf - -C "$r_export" 2>"$r_out/records-export.err.txt"
  r_tar=$?
  r_archive=$(cat "$r_export/archive.status")
  if [ "$r_archive" != 0 ] || [ "$r_tar" != 0 ]; then
    records_cause="git archive of backend/ at the commit exited ${r_archive:-unknown} and tar exited $r_tar"
    return 1
  fi
  ln -s "$r_top/backend/node_modules" "$r_export/backend/node_modules" || return 2
  mkdir -p "$r_arts/records-state" || return 2
  (cd "$r_export/backend" && GATE_STATE=$r_arts/records-state sh scripts/with-env.sh node gate/fixtures/concurrency/records.mjs) >"$r_arts/records.txt" 2>"$r_arts/records.err.txt"
  r_code=$?
  r_seen=$(records_check lines "$r_out/records-wanted.txt" "$r_arts/records.txt" "$r_code" "$records_script" "$r_out/records-cause.txt" "$r_out/refused-wanted.txt" "$r_export")
  r_matched=$?
  records_counts="records ${r_seen:-exit=$r_code lines=unread}"
  if [ "$r_matched" != 0 ]; then
    records_cause=$(cat "$r_out/records-cause.txt")
    [ -n "$records_cause" ] || records_cause="the check of the records' lines stopped with status $r_matched"
    return 1
  fi
}

guard_in() {
  g_repo=$1
  g_commit=$2
  g_moment=$3
  g_file=$5/tree-$4.txt
  guard_cause=
  git -C "$g_repo" -c core.fsmonitor=false -c core.untrackedCache=false diff --quiet "$g_commit" --
  g_diff=$?
  : >"$g_file" || return 2
  if [ "$g_diff" = 1 ]; then
    git -C "$g_repo" -c core.fsmonitor=false -c core.untrackedCache=false diff --name-only -z "$g_commit" -- >"$g_file.z" || return 2
    LC_ALL=C tr '\n\000' '?\n' <"$g_file.z" >"$g_file.differ" || return 2
    sed 's/^/differs: /' "$g_file.differ" >>"$g_file" || return 2
    g_first=$(sed -n 1p "$g_file.differ")
    guard_cause="a tracked file differs from the commit $g_moment: $g_first; the paths are ${g_file##*/} among the artifacts"
  elif [ "$g_diff" != 0 ]; then
    guard_cause="git could not compare the tracked files with the commit $g_moment"
  fi
  if ! git -C "$g_repo" -c core.fsmonitor=false -c core.untrackedCache=false ls-files -v -z >"$g_file.index.z"; then
    guard_cause="${guard_cause:+$guard_cause; }git could not list the index $g_moment"
  else
    LC_ALL=C tr '\n\000' '?\n' <"$g_file.index.z" >"$g_file.index" || return 2
    grep -v '^H ' "$g_file.index" >"$g_file.flagged"
    sed 's/^/flag /' "$g_file.flagged" >>"$g_file" || return 2
    g_flagged=$(sed -n 1p "$g_file.flagged")
    [ -z "$g_flagged" ] || guard_cause="${guard_cause:+$guard_cause; }a tracked file carries the index flag ${g_flagged%% *} $g_moment: ${g_flagged#* }; the paths are ${g_file##*/} among the artifacts"
  fi
  [ -z "$guard_cause" ]
}

decide() {
  decision=run
  case $2 in
    pull_request) ;;
    push|schedule|workflow_dispatch) decision_why="the event $2 runs them always"; return 0 ;;
    '') decision_why='GITHUB_EVENT_NAME is not set'; return 0 ;;
    *) decision_why="the event $2 is none that this check knows"; return 0 ;;
  esac
  if [ -z "${backend_root:-}" ] || [ -z "${model_root:-}" ]; then
    decision_why='scripts/r7/backend-roots.sh names no root'
    return 0
  fi
  if ! git -C "$1" rev-parse -q --verify 'HEAD^2^{commit}' >/dev/null 2>&1; then
    decision_why='HEAD has no second parent'
    return 0
  fi
  if ! d_paths=$(git -C "$1" -c core.quotePath=false diff-tree -r --no-renames --name-only 'HEAD^1' HEAD 2>/dev/null); then
    decision_why='git could not list the paths that HEAD changes against its first parent'
    return 0
  fi
  d_counts=$(printf '%s\n' "$d_paths" | awk -v paths="$backend_root/ $model_root/ $run_paths" '
    BEGIN { n = split(paths, p, " "); read = 0; hit = 0; quoted = 0 }
    $0 == "" { next }
    {
      read++
      l = tolower($0)
      if (substr(l, 1, 1) == "\"") quoted++
      for (k = 1; k <= n; k++) {
        if (l == p[k] || (substr(p[k], length(p[k])) == "/" && (index(l, p[k]) == 1 || l "/" == p[k]))) { hit++; break }
      }
    }
    END { printf "%d %d %d\n", read, hit, quoted }') || d_counts=
  d_read=${d_counts%% *}
  d_rest=${d_counts#* }
  d_hit=${d_rest%% *}
  d_quoted=${d_rest#* }
  case "$d_read:$d_hit:$d_quoted" in
    *[!0-9:]*|:*|*::*|*:) decision_why='the list of the changed paths could not be read' ;;
    *:*:0)
      if [ "$d_hit" -gt 0 ]; then
        decision_why="$d_hit of the $d_read paths that the pull request changes can change the verdict of the backend's tests"
      elif [ "$d_read" -eq 0 ]; then
        decision_why='the pull request changes no path'
      else
        decision=skip
        decision_why="none of the $d_read paths that the pull request changes can change the verdict of the backend's tests"
      fi
      ;;
    *) decision_why="git quotes the name of $d_quoted of the $d_read paths that the pull request changes" ;;
  esac
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  decide "$repo" "${GITHUB_EVENT_NAME:-}"
  if [ "$decision" = skip ]; then
    printf '%s: not run: %s\n' "$name" "$decision_why"
    printf 'PASS %s\n' "$name"
    exit 0
  fi
  printf '%s: runs the tests: %s\n' "$name" "$decision_why"
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  if ! at_commit=$(git -C "$repo" rev-parse --verify -q 'HEAD^{commit}'); then
    printf 'FAIL %s (git could not read the commit of HEAD)\n' "$name"
    exit 1
  fi
  guard_in "$repo" "$at_commit" 'before the node tests' 1-before-the-node-tests "$artifacts"
  tree_before=$?
  cause_before=$guard_cause
  test_in "$repo" "$artifacts" "$at_commit"
  tested=$?
  [ "$tested" = 0 ] || [ -n "$cause" ] || cause="the test run stopped with status $tested"
  guard_in "$repo" "$at_commit" 'after the node tests' 2-after-the-node-tests "$artifacts"
  tree_tests=$?
  cause_tests=$guard_cause
  records_in "$repo" "$artifacts" "$at_commit"
  recorded=$?
  [ "$recorded" = 0 ] || [ -n "$records_cause" ] || records_cause="the records' run stopped with status $recorded"
  guard_in "$repo" "$at_commit" "after the records' run" 3-after-the-records-run "$artifacts"
  tree_records=$?
  cause_records=$guard_cause
  printf '%s\n' "$counts"
  printf '%s\n' "$records_counts"
  printf 'tree before-the-node-tests=%s after-the-node-tests=%s after-the-records-run=%s\n' "$tree_before" "$tree_tests" "$tree_records"
  all=
  [ "$tested" = 0 ] || all=$cause
  [ "$recorded" = 0 ] || all="${all:+$all; }$records_cause"
  [ -n "$cause_before" ] || cause_before="the tree guard before the node tests stopped with status $tree_before"
  [ -n "$cause_tests" ] || cause_tests="the tree guard after the node tests stopped with status $tree_tests"
  [ -n "$cause_records" ] || cause_records="the tree guard after the records' run stopped with status $tree_records"
  [ "$tree_before" = 0 ] || all="${all:+$all; }$cause_before"
  [ "$tree_tests" = 0 ] || all="${all:+$all; }$cause_tests"
  [ "$tree_records" = 0 ] || all="${all:+$all; }$cause_records"
  if [ -z "$all" ]; then
    printf 'PASS %s\n' "$name"
  else
    printf 'FAIL %s (%s)\n' "$name" "$all"
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
  git -C "$dir" init -q >/dev/null 2>&1 || exit 2
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
[ ! -f "$STUB_STATE/stand-in.sh" ] || sh "$STUB_STATE/stand-in.sh" || exit 9
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
  git_commit "$dir" "$case_name" backend/scripts/with-env.sh "$list"
  at=$(git -C "$dir" rev-parse HEAD) || exit 2
  (STUB_STATE=$dir/state test_in "$dir" "$dir/out" "$at") >/dev/null 2>&1
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
  node - "$scratch/$1/$records_dir" "$2" "$3" "$4" "${5:-0}" <<'EOF' || exit 2
const fs = require("fs");
const path = require("path");
const [dir, kind, prefix, body, nth] = process.argv.slice(2);
const file = fs.readdirSync(dir)
  .filter((name) => name.startsWith(prefix) && name.endsWith(".json"))
  .sort()
  .filter((name) => JSON.parse(fs.readFileSync(path.join(dir, name), "utf8")).kind === kind)[Number(nth)];
if (!file) process.exit(3);
console.log(file);
if (body === "") process.exit(0);
const before = fs.readFileSync(path.join(dir, file), "utf8");
const record = JSON.parse(before);
new Function("record", body)(record);
const after = `${JSON.stringify(record, null, 1)}\n`;
if (JSON.stringify(JSON.parse(after)) === JSON.stringify(JSON.parse(before))) process.exit(4);
fs.writeFileSync(path.join(dir, file), after);
EOF
}

git_commit() {
  c_dir=$1
  c_message=$2
  shift 2
  for c_path in "$@"; do
    [ ! -e "$c_dir/$c_path" ] || git -C "$c_dir" add -- "$c_path" || exit 2
  done
  git -C "$c_dir" -c maintenance.auto=false commit -q -m "$c_message" >/dev/null || exit 2
}

stub_records() {
  case_name=$1
  records_exit=$2
  shift 2
  dir=$scratch/$case_name
  mkdir -p "$dir/backend/scripts" "$dir/backend/node_modules" "$dir/$records_dir" "$dir/out" || exit 2
  cat >"$dir/backend/scripts/with-env.sh" <<'EOF'
#!/bin/sh
exec "$@"
EOF
  printf '{}\n' >"$dir/$records_dir/good-a.json" || exit 2
  printf '{}\n' >"$dir/$records_dir/bad-b.json" || exit 2
  stub_refused_fixture "$dir"
  {
    stub_prelude_lines
    for line in "$@"; do printf 'console.log("%s");\n' "$line"; done
    printf 'process.exit(%s);\n' "$records_exit"
  } >"$dir/$records_script"
  git -C "$dir" init -q >/dev/null 2>&1 || exit 2
}

stub_refused_fixture() {
  [ "${stub_refused:-one}" = one ] || return 0
  mkdir -p "$1/$refused_dir" || exit 2
  printf '{"refusal": "%s"}\n' "$refused_cause" >"$1/$refused_dir/r.json" || exit 2
}

stub_prelude_lines() {
  printf '%s\n' "${stub_prelude-$refused_prelude}" | while IFS= read -r line; do
    [ -z "$line" ] || printf 'console.log("%s");\n' "$line"
  done
}

commit_stub_records() {
  git_commit "$scratch/$1" "$1" backend/scripts/with-env.sh backend/gate
}

commit_record_name() {
  n_dir=$scratch/$1
  n_blob=$(git -C "$n_dir" hash-object -w "$n_dir/$records_dir/good-a.json") || exit 2
  printf '100644 %s\t%s/%s\000' "$n_blob" "$records_dir" "$2" | git -C "$n_dir" update-index -z --add --index-info || exit 2
  git -C "$n_dir" -c maintenance.auto=false commit -q -m "$1" >/dev/null || exit 2
}

commit_refused_name() {
  n_dir=$scratch/$1
  n_blob=$(git -C "$n_dir" hash-object -w "$n_dir/$refused_dir/r.json") || exit 2
  printf '100644 %s\t%s/%s\000' "$n_blob" "$refused_dir" "$2" | git -C "$n_dir" update-index -z --add --index-info || exit 2
  git -C "$n_dir" -c maintenance.auto=false commit -q -m "$1" >/dev/null || exit 2
}

expect_records() {
  wanted_verdict=$1
  case_name=$2
  dir=$scratch/$case_name
  at=$(git -C "$dir" rev-parse --verify -q HEAD)
  (records_in "$dir" "$dir/out" "$at" >/dev/null 2>&1; code=$?; printf '%s\n' "$records_cause" >"$dir/cause"; exit "$code")
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  [ "$got" != FAIL ] || grep -q -F -- "$3" "$dir/cause" || got=FAIL-for-another-cause
  shift 3
  for broken in "$@"; do
    grep -q -F "self-test concurrency: record ${broken%.json}: " "$dir/out/records.txt" 2>/dev/null || got=$got-without-the-problem-line-of-${broken%.json}
  done
  verdict "$wanted_verdict" "$got" "$case_name"
}

measure_fixture() {
  case_name=$1
  records_exit=$2
  shift 2
  fixture "$case_name"
  mkdir -p "$dir/$records_dir" "$dir/docs" "$scratch/tmp-$case_name" || exit 2
  printf '{}\n' >"$dir/$records_dir/good-a.json" || exit 2
  printf 'a tracked file outside backend/\n' >"$dir/docs/note.txt" || exit 2
  stub_refused_fixture "$dir"
  {
    printf 'import { appendFileSync, existsSync, writeFileSync } from "node:fs";\nwriteFileSync(process.env.GATE_STATE + "/written", "");\n'
    printf 'if (existsSync(process.env.STUB_STATE + "/records-write")) appendFileSync(process.env.STUB_CHECKOUT + "/docs/note.txt", "written by the records\\n");\n'
    stub_prelude_lines
    for line in "$@"; do printf 'console.log("%s");\n' "$line"; done
    printf 'console.error("a line on standard error");\nprocess.exit(%s);\n' "$records_exit"
  } >"$dir/$records_script"
  git_commit "$dir" "$case_name" backend/scripts/with-env.sh "$list" backend/gate docs
}

stand_in() {
  cat >"$scratch/$1/state/stand-in.sh" || exit 2
}

forger() {
  cat >"$scratch/$1/state/forger.mjs" <<'EOF' || exit 2
import { copyFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
console.log("self-test concurrency: good record-a: PASS as expected");
console.log("concurrency: records: 1 of 1 as expected");
if (existsSync(process.env.STUB_STATE + "/original.mjs")) copyFileSync(process.env.STUB_STATE + "/original.mjs", fileURLToPath(import.meta.url));
EOF
}

expect_measure() {
  case_name=$2
  dir=$scratch/$case_name
  (CI_D_REPO=$dir CI_D_ARTIFACTS=$dir/out STUB_STATE=$dir/state STUB_CHECKOUT=$dir TMPDIR=$scratch/tmp-$case_name measure) >"$dir/measure.txt" 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  records_line=$(grep '^records ' "$dir/measure.txt")
  last_line=$(tail -n 1 "$dir/measure.txt")
  [ -z "$3" ] || [ "$records_line" = "$3" ] || got=$got-with-another-records-line
  case $last_line in
    *"$4"*) ;;
    *) got=$got-with-another-last-line ;;
  esac
  tmp_top=$(cd "$scratch/tmp-$case_name" && pwd -P) || exit 2
  grep -F "|node gate/fixtures/concurrency/records.mjs" "$dir/state/calls" 2>/dev/null | grep -q -F "with-env|$tmp_top/" || got=$got-without-the-records-run-in-the-export
  tests_at=$(grep -n -F '|node_modules/.bin/vitest run ' "$dir/state/calls" 2>/dev/null | sed -n '1s/:.*//p')
  records_at=$(grep -n -F '|node gate/fixtures/concurrency/records.mjs' "$dir/state/calls" 2>/dev/null | sed -n '1s/:.*//p')
  if [ -n "$tests_at" ] && { [ -z "$records_at" ] || [ "$records_at" -lt "$tests_at" ]; }; then
    got=$got-with-the-records-before-the-tests
  fi
  [ -f "$dir/out/records-state/written" ] || got=$got-without-the-state-under-the-artifacts
  verdict "$1" "$got" "$case_name"
}

decision_repo() {
  d_dir=$scratch/decide-$1
  shift
  mkdir -p "$d_dir/docs" || exit 2
  git -C "$d_dir" init -q >/dev/null 2>&1 || exit 2
  printf 'the base\n' >"$d_dir/docs/base.txt" || exit 2
  git_commit "$d_dir" 'the base' docs/base.txt
  [ "$#" -gt 0 ] || return 0
  git -C "$d_dir" checkout -q -b pull-request || exit 2
  for d_path in "$@"; do
    mkdir -p "$d_dir/${d_path%/*}" || exit 2
    printf 'changed by the pull request\n' >"$d_dir/$d_path" || exit 2
  done
  git_commit "$d_dir" 'the pull request' "$@"
  git -C "$d_dir" checkout -q - || exit 2
  git -C "$d_dir" -c maintenance.auto=false -c gc.auto=0 merge -q --no-ff --no-edit pull-request >/dev/null || exit 2
}

expect_decision() {
  case_name=decide-$2
  decide "$scratch/decide-$3" "$4"
  got="$decision: $decision_why"
  if [ -n "${5:-}" ]; then
    (CI_D_REPO=$scratch/decide-$3 GITHUB_EVENT_NAME=$4 measure) >"$scratch/$case_name.measure" 2>&1
    d_code=$?
    printf '%s\n0\n' "$5" >"$scratch/$case_name.wanted" || exit 2
    printf '%s\n' "$d_code" >>"$scratch/$case_name.measure" || exit 2
    cmp -s "$scratch/$case_name.wanted" "$scratch/$case_name.measure" || got="$got (with another output of the check)"
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$1" "$got"
  [ "$got" = "$1" ] || wrong="$wrong $case_name"
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  scratch=$(cd "$scratch" && pwd -P) || exit 2
  checkout=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  rejected=0
  wrong=
  refused_cause='its list cases is empty'
  refused_line="self-test concurrency: refused record-r: refused as expected: $refused_cause"
  refused_tally='concurrency: refused records: 1 of 1 refused as expected'
  refused_prelude="$refused_line
$refused_tally"

  decision_repo a-path-under-backend backend/src/a.ts docs/a.md
  expect_decision "run: 1 of the 2 paths that the pull request changes can change the verdict of the backend's tests" a-path-under-backend a-path-under-backend pull_request

  decision_repo a-file-of-the-wire-contract backend/contract/schema/v1.json
  expect_decision "run: 1 of the 1 paths that the pull request changes can change the verdict of the backend's tests" a-file-of-the-wire-contract a-file-of-the-wire-contract pull_request

  decision_repo outside-the-list-alone account/schema/schema.v1.json backends/a.ts scripts/r7x/a.sh docs/r7/a.md formal/r7-ledgers/a.tla
  outside="none of the 5 paths that the pull request changes can change the verdict of the backend's tests"
  expect_decision "skip: $outside" paths-outside-the-list-alone outside-the-list-alone pull_request "$name: not run: $outside
PASS $name"

  decision_repo a-commit-with-one-parent
  printf 'one parent\n' >"$scratch/decide-a-commit-with-one-parent/docs/one.txt" || exit 2
  git_commit "$scratch/decide-a-commit-with-one-parent" 'a commit with one parent' docs/one.txt
  expect_decision 'run: HEAD has no second parent' a-commit-with-one-parent a-commit-with-one-parent pull_request

  decision_repo a-diff-that-fails docs/a.md
  d_tree=$(git -C "$scratch/decide-a-diff-that-fails" rev-parse 'HEAD^1^{tree}') || exit 2
  mv "$scratch/decide-a-diff-that-fails/.git/objects/${d_tree%"${d_tree#??}"}/${d_tree#??}" "$scratch/a-diff-that-fails.tree" || exit 2
  expect_decision 'run: git could not list the paths that HEAD changes against its first parent' a-diff-that-fails a-diff-that-fails pull_request

  expect_decision 'run: the event push runs them always' the-event-push outside-the-list-alone push

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

  shared=$scratch/files-that-are-no-records-a-name-that-is-not-ascii-and-an-untracked-record
  records_fixture files-that-are-no-records-a-name-that-is-not-ascii-and-an-untracked-record
  printf 'not a record\n' >"$shared/$records_dir/notes.txt" || exit 2
  printf 'not a record\n' >"$shared/$records_dir/notes.json.txt" || exit 2
  cp "$shared/$first_good" "$shared/$records_dir/good-caf$(printf '\303\251')-a-copy.json" || exit 2
  git -C "$shared" add -- "$records_dir" || exit 2
  commit_fixture files-that-are-no-records-a-name-that-is-not-ascii-and-an-untracked-record
  cp "$shared/$first_good" "$shared/$records_dir/good-a-copy-that-head-does-not-hold.json" || exit 2
  expect_records PASS files-that-are-no-records-a-name-that-is-not-ascii-and-an-untracked-record ''

  records_fixture a-record-in-a-sub-directory
  mkdir -p "$scratch/a-record-in-a-sub-directory/$records_dir/kept" || exit 2
  git -C "$scratch/a-record-in-a-sub-directory" mv "$first_good" "$records_dir/kept/" || exit 2
  commit_fixture a-record-in-a-sub-directory
  expect_records FAIL a-record-in-a-sub-directory 'a record in a sub-directory'

  records_fixture records-mjs-prints-only-the-last-line
  edit_file "$scratch/records-mjs-prints-only-the-last-line/$records_script" 'const failedRefusals = failed;' 'console.log("concurrency: records: " + files.length + " of " + files.length + " as expected");
process.exit(0);
const failedRefusals = failed;'
  commit_fixture records-mjs-prints-only-the-last-line
  expect_records FAIL records-mjs-prints-only-the-last-line 'of the commit has no line'

  records_fixture a-good-record-expects-another-count
  break_record a-good-record-expects-another-count judge good- 'record.expect.counted = !record.expect.counted' >/dev/null
  commit_fixture a-good-record-expects-another-count
  expect_records FAIL a-good-record-expects-another-count "check 50's records exited 1"

  records_fixture four-records-broken-in-one-tree
  bad_judge=$(break_record four-records-broken-in-one-tree judge bad- 'record.expect.violations = []') || exit 2
  good_line_record=$(break_record four-records-broken-in-one-tree line good- 'record.expect.line += "."') || exit 2
  no_reading=$(break_record four-records-broken-in-one-tree judge good- 'record.kind = "no-such-reading"' 1) || exit 2
  no_json=$(break_record four-records-broken-in-one-tree send good- '') || exit 2
  printf ',\n' >>"$scratch/four-records-broken-in-one-tree/$records_dir/$no_json" || exit 2
  commit_fixture four-records-broken-in-one-tree
  expect_records FAIL four-records-broken-in-one-tree "check 50's records exited 1" "$bad_judge" "$good_line_record" "$no_reading" "$no_json"

  records_fixture records-mjs-skips-a-record
  edit_file "$scratch/records-mjs-skips-a-record/$records_script" '  .sort();' '  .sort().slice(1);'
  commit_fixture records-mjs-skips-a-record
  expect_records FAIL records-mjs-skips-a-record 'the last line of'

  records_fixture records-mjs-exits-0-on-a-broken-record
  edit_file "$scratch/records-mjs-exits-0-on-a-broken-record/$records_script" 'process.exit(failed === 0 ? 0 : 1);' 'process.exit(0);'
  break_record records-mjs-exits-0-on-a-broken-record judge good- 'record.expect.counted = !record.expect.counted' >/dev/null
  commit_fixture records-mjs-exits-0-on-a-broken-record
  expect_records FAIL records-mjs-exits-0-on-a-broken-record 'a problem line with exit 0'

  records_fixture a-record-in-a-sub-directory-beside-an-untracked-record
  mkdir -p "$scratch/a-record-in-a-sub-directory-beside-an-untracked-record/$records_dir/kept" || exit 2
  git -C "$scratch/a-record-in-a-sub-directory-beside-an-untracked-record" mv "$first_good" "$records_dir/kept/" || exit 2
  commit_fixture a-record-in-a-sub-directory-beside-an-untracked-record
  cp "$scratch/a-record-in-a-sub-directory-beside-an-untracked-record/$records_dir/kept/${first_good##*/}" "$scratch/a-record-in-a-sub-directory-beside-an-untracked-record/$records_dir/good-an-untracked-copy.json" || exit 2
  expect_records FAIL a-record-in-a-sub-directory-beside-an-untracked-record 'a record in a sub-directory'

  records_fixture records-mjs-missing
  mv "$scratch/records-mjs-missing/$records_script" "$scratch/records-mjs-missing/records.mjs.moved" || exit 2
  commit_fixture records-mjs-missing
  expect_records FAIL records-mjs-missing "$records_script or backend/scripts/with-env.sh is missing at the commit"

  records_fixture records-with-env-missing
  mv "$scratch/records-with-env-missing/backend/scripts/with-env.sh" "$scratch/records-with-env-missing/with-env.sh.moved" || exit 2
  commit_fixture records-with-env-missing
  expect_records FAIL records-with-env-missing "$records_script or backend/scripts/with-env.sh is missing at the commit"

  records_fixture no-record-at-head
  mv "$scratch/no-record-at-head/$records_dir" "$scratch/no-record-at-head/records.moved" || exit 2
  commit_fixture no-record-at-head
  mkdir -p "$scratch/no-record-at-head/$records_dir" || exit 2
  expect_records FAIL no-record-at-head 'holds no record under'

  records_fixture packages-missing without-packages
  expect_records FAIL packages-missing 'backend/node_modules is missing'

  records_fixture head-without-a-commit
  git -C "$scratch/head-without-a-commit" update-ref -d HEAD || exit 2
  expect_records FAIL head-without-a-commit "git could not list $records_dir/ at the commit"

  good_line='self-test concurrency: good record-a: PASS as expected'
  bad_line='self-test concurrency: bad record-b: FAIL as expected'
  summary_line='concurrency: records: 2 of 2 as expected'

  stub_records stub-records-as-expected 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records stub-records-as-expected
  expect_records PASS stub-records-as-expected ''

  stub_records a-record-line-twice-and-another-never 0 "$good_line" "$good_line" "$summary_line"
  commit_stub_records a-record-line-twice-and-another-never
  expect_records FAIL a-record-line-twice-and-another-never "record $records_dir/bad-b.json of the commit has no line"

  stub_records a-line-for-a-name-that-the-commit-lacks 0 "$good_line" 'self-test concurrency: bad record-z: FAIL as expected' "$summary_line"
  commit_stub_records a-line-for-a-name-that-the-commit-lacks
  expect_records FAIL a-line-for-a-name-that-the-commit-lacks "record $records_dir/bad-b.json of the commit has no line"

  stub_records an-extra-line-for-a-name-that-the-commit-lacks 0 "$good_line" "$bad_line" 'self-test concurrency: bad record-z: FAIL as expected' "$summary_line"
  commit_stub_records an-extra-line-for-a-name-that-the-commit-lacks
  expect_records FAIL an-extra-line-for-a-name-that-the-commit-lacks 'names no record of the commit'

  stub_records a-good-record-printed-as-failed 0 'self-test concurrency: good record-a: FAIL as expected' "$bad_line" "$summary_line"
  commit_stub_records a-good-record-printed-as-failed
  expect_records FAIL a-good-record-printed-as-failed "record $records_dir/good-a.json of the commit has no line"

  stub_records a-record-line-twice-beside-every-record 0 "$good_line" "$bad_line" "$good_line" "$summary_line"
  commit_stub_records a-record-line-twice-beside-every-record
  expect_records FAIL a-record-line-twice-beside-every-record 'stands a second time'

  stub_records a-line-of-another-form 0 "$good_line" "$bad_line" 'a line of another form' "$summary_line"
  commit_stub_records a-line-of-another-form
  expect_records FAIL a-line-of-another-form 'has another form'

  stub_records a-json-name-of-another-form 0 "$good_line" "$bad_line" "$summary_line"
  printf '{}\n' >"$scratch/a-json-name-of-another-form/$records_dir/other-c.json" || exit 2
  commit_stub_records a-json-name-of-another-form
  expect_records FAIL a-json-name-of-another-form "$records_dir/other-c.json"

  stub_records a-record-name-with-a-newline 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-record-name-with-a-newline
  commit_record_name a-record-name-with-a-newline "$(printf 'good-c\nd.json')"
  expect_records FAIL a-record-name-with-a-newline 'holds a line break'

  stub_records a-record-name-with-a-carriage-return 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-record-name-with-a-carriage-return
  commit_record_name a-record-name-with-a-carriage-return "$(printf 'good-c\rd.json')"
  expect_records FAIL a-record-name-with-a-carriage-return 'holds a line break'

  stub_records a-record-name-that-is-no-utf-8 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-record-name-that-is-no-utf-8
  commit_record_name a-record-name-that-is-no-utf-8 "$(printf 'good-c\377.json')"
  expect_records FAIL a-record-name-that-is-no-utf-8 'is no UTF-8'

  stub_records a-problem-line-with-exit-0 0 "$good_line" "$bad_line" 'self-test concurrency: record good-a: counted is false, expected true' "$summary_line"
  commit_stub_records a-problem-line-with-exit-0
  expect_records FAIL a-problem-line-with-exit-0 'a problem line with exit 0: "self-test concurrency: record good-a: counted is false, expected true"'

  stub_records a-line-after-a-right-summary 0 "$good_line" "$bad_line" "$summary_line" 'a line after the summary'
  commit_stub_records a-line-after-a-right-summary
  expect_records FAIL a-line-after-a-right-summary 'the last line of'

  stub_records the-summary-twice 0 "$good_line" "$bad_line" "$summary_line" "$summary_line"
  commit_stub_records the-summary-twice
  expect_records FAIL the-summary-twice 'the last line of'

  stub_records a-right-output-with-exit-1 1 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-right-output-with-exit-1
  expect_records FAIL a-right-output-with-exit-1 "check 50's records exited 1; the lines are records.txt"

  stub_records a-tmpdir-inside-the-checkout 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-tmpdir-inside-the-checkout
  mkdir -p "$scratch/a-tmpdir-inside-the-checkout/tmp" || exit 2
  tmpdir_kept=${TMPDIR:-}
  TMPDIR=$scratch/a-tmpdir-inside-the-checkout/tmp
  expect_records FAIL a-tmpdir-inside-the-checkout 'inside the checkout or the artifacts'
  TMPDIR=$tmpdir_kept

  stub_records the-commit-holds-backend-node-modules 0 "$good_line" "$bad_line" "$summary_line"
  printf 'a file of the commit\n' >"$scratch/the-commit-holds-backend-node-modules/backend/node_modules/tracked.txt" || exit 2
  git_commit "$scratch/the-commit-holds-backend-node-modules" the-commit-holds-backend-node-modules backend/scripts/with-env.sh backend/gate backend/node_modules/tracked.txt
  expect_records FAIL the-commit-holds-backend-node-modules 'the commit holds backend/node_modules'

  stub_prelude="$refused_line
self-test concurrency: refused record-z: refused as expected: $refused_cause
$refused_tally"
  stub_records a-refused-line-that-names-no-fixture-of-the-commit 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-line-that-names-no-fixture-of-the-commit
  expect_records FAIL a-refused-line-that-names-no-fixture-of-the-commit 'names no refused fixture of the commit'

  stub_prelude=$refused_tally
  stub_records a-refused-fixture-with-no-line 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-fixture-with-no-line
  expect_records FAIL a-refused-fixture-with-no-line "refused fixture $refused_dir/r.json of the commit has no line"

  stub_prelude='self-test concurrency: refused record-r: not refused as expected: accepted
concurrency: refused records: 0 of 1 refused as expected'
  stub_records a-refused-fixture-that-was-accepted 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-fixture-that-was-accepted
  expect_records FAIL a-refused-fixture-that-was-accepted 'printed that a refused fixture was not refused: "self-test concurrency: refused record-r: not refused as expected: accepted"'

  stub_records a-refused-fixture-that-was-accepted-with-exit-1 1 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-fixture-that-was-accepted-with-exit-1
  expect_records FAIL a-refused-fixture-that-was-accepted-with-exit-1 'printed that a refused fixture was not refused'

  stub_prelude="$refused_line
concurrency: refused records: 1 of 2 refused as expected"
  stub_records a-wrong-refused-tally 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-wrong-refused-tally
  expect_records FAIL a-wrong-refused-tally "stands 0 times in the records' output, not once"

  stub_prelude="$refused_line
$refused_line
$refused_tally"
  stub_records a-refused-line-twice 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-line-twice
  expect_records FAIL a-refused-line-twice 'stands a second time'

  stub_prelude="$refused_line
$refused_tally
$refused_tally"
  stub_records the-refused-tally-twice 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records the-refused-tally-twice
  expect_records FAIL the-refused-tally-twice "stands 2 times in the records' output, not once"

  stub_prelude="$refused_line
self-test concurrency: refused record r: not refused, where it pins nothing
$refused_tally"
  stub_records a-refused-line-of-the-earlier-second-form 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-line-of-the-earlier-second-form
  expect_records FAIL a-refused-line-of-the-earlier-second-form 'has another form'

  stub_prelude=''
  stub_records refused-lines-after-the-records 0 "$good_line" "$bad_line" "$refused_line" "$refused_tally" "$summary_line"
  commit_stub_records refused-lines-after-the-records
  expect_records FAIL refused-lines-after-the-records "of the refused pass stands after the records' lines"

  stub_prelude='concurrency: refused records: 0 of 0 refused as expected'
  stub_refused=none
  stub_records no-refused-fixture-at-the-commit 1 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records no-refused-fixture-at-the-commit
  expect_records FAIL no-refused-fixture-at-the-commit "holds no refused fixture under $refused_dir/"
  unset stub_refused stub_prelude

  stub_records a-refused-fixture-that-names-no-cause 0 "$good_line" "$bad_line" "$summary_line"
  printf '{}\n' >"$scratch/a-refused-fixture-that-names-no-cause/$refused_dir/r.json" || exit 2
  commit_stub_records a-refused-fixture-that-names-no-cause
  expect_records FAIL a-refused-fixture-that-names-no-cause "the refused fixture $refused_dir/r.json at the commit names no cause"

  stub_records a-refused-fixture-in-a-sub-directory 0 "$good_line" "$bad_line" "$summary_line"
  mkdir -p "$scratch/a-refused-fixture-in-a-sub-directory/$refused_dir/kept" || exit 2
  cp "$scratch/a-refused-fixture-in-a-sub-directory/$refused_dir/r.json" "$scratch/a-refused-fixture-in-a-sub-directory/$refused_dir/kept/s.json" || exit 2
  commit_stub_records a-refused-fixture-in-a-sub-directory
  expect_records FAIL a-refused-fixture-in-a-sub-directory 'a refused fixture in a sub-directory'

  stub_records a-refused-fixture-name-with-a-newline 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-fixture-name-with-a-newline
  commit_refused_name a-refused-fixture-name-with-a-newline "$(printf 'r\ns.json')"
  expect_records FAIL a-refused-fixture-name-with-a-newline 'the name of a refused fixture at the commit holds a line break'

  stub_records a-refused-fixture-name-that-is-no-utf-8 0 "$good_line" "$bad_line" "$summary_line"
  commit_stub_records a-refused-fixture-name-that-is-no-utf-8
  commit_refused_name a-refused-fixture-name-that-is-no-utf-8 "$(printf 'r\377.json')"
  expect_records FAIL a-refused-fixture-name-that-is-no-utf-8 'the name of a refused fixture at the commit is no UTF-8'

  passed_records='concurrency: records: 1 of 1 as expected'
  measure_fixture measure-runs-the-tests-and-the-records 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  expect_measure PASS measure-runs-the-tests-and-the-records 'records exit=0 at-commit=1 as-expected=1 problems=0 last-line=ok' "PASS $name"

  measure_fixture measure-fails-on-the-records-alone 1 'self-test concurrency: record good-a: broken' 'concurrency: records: 0 of 1 as expected'
  expect_measure FAIL measure-fails-on-the-records-alone 'records exit=1 at-commit=1 as-expected=0 problems=1 last-line=other' "FAIL $name (check 50's records exited 1; the first problem line: \"self-test concurrency: record good-a: broken\")"

  measure_fixture measure-fails-on-the-tests-alone 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  printf '1\n' >"$scratch/measure-fails-on-the-tests-alone/state/vitest.exit"
  expect_measure FAIL measure-fails-on-the-tests-alone 'records exit=0 at-commit=1 as-expected=1 problems=0 last-line=ok' "FAIL $name (the test script exited 1)"

  measure_fixture measure-fails-on-both 1 'self-test concurrency: record good-a: broken' 'concurrency: records: 0 of 1 as expected'
  printf '1\n' >"$scratch/measure-fails-on-both/state/vitest.exit"
  expect_measure FAIL measure-fails-on-both 'records exit=1 at-commit=1 as-expected=0 problems=1 last-line=other' "FAIL $name (the test script exited 1; check 50's records exited 1; the first problem line: \"self-test concurrency: record good-a: broken\")"

  measure_fixture a-test-run-that-stops-with-status-2 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  mkdir -p "$scratch/a-test-run-that-stops-with-status-2/out/vitest.json" || exit 2
  expect_measure FAIL a-test-run-that-stops-with-status-2 'records exit=0 at-commit=1 as-expected=1 problems=0 last-line=ok' "FAIL $name (the test run stopped with status 2)"

  measure_fixture the-node-tests-forge-records-mjs-and-put-it-back 1 'self-test concurrency: record good-a: broken' 'concurrency: records: 0 of 1 as expected'
  forger the-node-tests-forge-records-mjs-and-put-it-back
  stand_in the-node-tests-forge-records-mjs-and-put-it-back <<'EOF'
cp gate/fixtures/concurrency/records.mjs "$STUB_STATE/original.mjs" && cp "$STUB_STATE/forger.mjs" gate/fixtures/concurrency/records.mjs
EOF
  expect_measure FAIL the-node-tests-forge-records-mjs-and-put-it-back '' "check 50's records exited 1; the first problem line"

  measure_fixture the-node-tests-forge-every-path-made-before-them 1 'self-test concurrency: record good-a: broken' 'concurrency: records: 0 of 1 as expected'
  forger the-node-tests-forge-every-path-made-before-them
  stand_in the-node-tests-forge-every-path-made-before-them <<'EOF'
find "$CI_D_ARTIFACTS" "$TMPDIR" -type d >"$STUB_STATE/made-before" || exit 1
while IFS= read -r made; do
  cp "$STUB_STATE/forger.mjs" "$made/records.mjs" || exit 1
done <"$STUB_STATE/made-before"
find "$CI_D_ARTIFACTS" "$TMPDIR" -name records.mjs -type f -exec cp "$STUB_STATE/forger.mjs" {} \;
EOF
  expect_measure FAIL the-node-tests-forge-every-path-made-before-them '' "check 50's records exited 1; the first problem line"

  measure_fixture the-node-tests-take-a-line-out-of-the-list 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  report the-node-tests-take-a-line-out-of-the-list test/a.test.ts:2:0
  stand_in the-node-tests-take-a-line-out-of-the-list <<'EOF'
grep -v 'c.node.test.ts' ../scripts/r7/backend-tests.txt >"$STUB_STATE/list" && cp "$STUB_STATE/list" ../scripts/r7/backend-tests.txt
EOF
  expect_measure FAIL the-node-tests-take-a-line-out-of-the-list '' "the test files differ from $list"

  measure_fixture the-node-tests-change-a-file-under-docs 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  stand_in the-node-tests-change-a-file-under-docs <<'EOF'
printf 'changed by the node tests\n' >>../docs/note.txt
EOF
  expect_measure FAIL the-node-tests-change-a-file-under-docs '' 'a tracked file differs from the commit after the node tests: docs/note.txt'

  for flagged in assume-unchanged:h skip-worktree:S both:s; do
    flag_case=the-node-tests-rewrite-a-record-behind-${flagged%%:*}
    measure_fixture "$flag_case" 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
    case ${flagged%%:*} in
      both) printf 'git -C .. update-index --assume-unchanged %s && git -C .. update-index --skip-worktree %s || exit 1\n' "$records_dir/good-a.json" "$records_dir/good-a.json" ;;
      *) printf 'git -C .. update-index --%s %s || exit 1\n' "${flagged%%:*}" "$records_dir/good-a.json" ;;
    esac | stand_in "$flag_case"
    printf 'printf %s >gate/fixtures/concurrency/records/good-a.json\n' "'{\"rewritten\": true}\\n'" >>"$scratch/$flag_case/state/stand-in.sh"
    expect_measure FAIL "$flag_case" '' "a tracked file carries the index flag ${flagged#*:} after the node tests: $records_dir/good-a.json"
  done

  measure_fixture the-node-tests-commit-a-record 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  stand_in the-node-tests-commit-a-record <<'EOF'
printf '{}\n' >gate/fixtures/concurrency/records/good-c.json || exit 1
git -C .. add -- backend/gate/fixtures/concurrency/records/good-c.json || exit 1
git -C .. -c maintenance.auto=false commit -q -m 'a node test commits a record' || exit 1
EOF
  expect_measure FAIL the-node-tests-commit-a-record 'records exit=0 at-commit=1 as-expected=1 problems=0 last-line=ok' "a tracked file differs from the commit after the node tests: $records_dir/good-c.json"

  measure_fixture the-checkout-is-dirty-before-the-run 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  printf 'left by an earlier step\n' >>"$scratch/the-checkout-is-dirty-before-the-run/docs/note.txt" || exit 2
  expect_measure FAIL the-checkout-is-dirty-before-the-run '' 'a tracked file differs from the commit before the node tests: docs/note.txt'

  measure_fixture the-records-write-a-tracked-file 0 'self-test concurrency: good record-a: PASS as expected' "$passed_records"
  : >"$scratch/the-records-write-a-tracked-file/state/records-write" || exit 2
  expect_measure FAIL the-records-write-a-tracked-file '' "a tracked file differs from the commit after the records' run: docs/note.txt"

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
