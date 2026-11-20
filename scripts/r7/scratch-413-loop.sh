#!/bin/sh
# Scratch only (billing run, unit 4.13; never merged): runs the unit's two test files of the node tests LOOP_ROUNDS times
# (20 unless given) from backend/, and after each round counts the processes that the round left, by the ids that the
# round itself recorded: a preload in every node process of the round writes the pid and start time of each child that
# a node process spawns, and of each node process itself; a process left is one of them that still runs with the same
# start time, or a member of a group whose mark the round left. Colour codes are taken out of vitest's text before a
# line of it is printed. Prints one line per round and a last line, and exits 1 unless every round passed every test
# and nothing was left.
set +e
cd "$(dirname "$0")/../../backend" || exit 1
rounds=${LOOP_ROUNDS:-20}
work=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/loop-413
mkdir -p "$work" || exit 1
files="test/ledger/server-stops.node.test.ts test/ledger/process-group-start.node.test.ts"
expected=${LOOP_EXPECTED:-"21 21 0"}
cat >"$work/spawn-log.mjs" <<'EOF'
import { appendFileSync } from "node:fs";
import { createRequire, syncBuiltinESMExports } from "node:module";
const log = process.env.LOOP_SPAWN_LOG;
const cp = createRequire(import.meta.url)("node:child_process");
const exec = cp.execFileSync;
const startOf = (pid) => {
  try {
    return exec("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8", env: { ...process.env, TZ: "UTC0", LC_ALL: "C" } }).trim().replace(/\s+/g, " ");
  } catch {
    return "";
  }
};
const write = (pid) => {
  const started = startOf(pid);
  if (log && started) appendFileSync(log, `${pid}|${started}\n`);
};
if (log) {
  write(process.pid);
  for (const name of ["spawn", "fork"]) {
    const real = cp[name];
    cp[name] = function (...args) {
      const child = real.apply(this, args);
      if (child.pid !== undefined) write(child.pid);
      return child;
    };
  }
  syncBuiltinESMExports();
}
EOF
cat >"$work/left.mjs" <<'EOF'
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
const [spawnLog, ...markDirs] = process.argv.slice(2);
const rows = new Map();
for (const line of execFileSync("ps", ["-A", "-o", "pid=,pgid=,stat=,lstart="], { encoding: "utf8", env: { ...process.env, TZ: "UTC0", LC_ALL: "C" } }).split("\n")) {
  const m = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S.*\S)\s*$/.exec(line);
  if (m && !m[3].startsWith("Z")) rows.set(Number(m[1]), { pgid: Number(m[2]), started: m[4].replace(/\s+/g, " ") });
}
const left = new Set();
let logged = 0;
for (const line of existsSync(spawnLog) ? readFileSync(spawnLog, "utf8").split("\n") : []) {
  const [pid, started] = line.split("|");
  if (!started) continue;
  logged += 1;
  const row = rows.get(Number(pid));
  if (row && row.started === started && Number(pid) !== process.pid && Number(pid) !== process.ppid) left.add(Number(pid));
}
let marks = 0;
for (const dir of markDirs) {
  for (const name of existsSync(dir) ? readdirSync(dir).filter((each) => /^group-\d+\.json$/.test(each)) : []) {
    let mark;
    try {
      mark = JSON.parse(readFileSync(join(dir, name), "utf8"));
    } catch {
      continue;
    }
    marks += 1;
    const leader = rows.get(mark.pid);
    if (typeof mark.started === "string" && leader && leader.started !== mark.started) continue;
    for (const [pid, row] of rows) if (row.pgid === mark.pid) left.add(pid);
  }
}
console.log(`${left.size} ${logged} ${marks}`);
EOF
start=$(date +%s)
passed=0
left_total=0
round=1
while [ "$round" -le "$rounds" ]; do
  out="$work/round-$round.json"
  text="$work/round-$round.txt"
  spawned="$work/spawned-$round.txt"
  marks="$work/marks-$round"
  : >"$spawned"
  before="$work/case-dirs-before-$round.txt"
  ls -d .tmp/server-stops.* 2>/dev/null | sort >"$before"
  t0=$(date +%s)
  LOOP_SPAWN_LOG="$spawned" HALVO_GROUP_MARKS="$marks" NO_COLOR=1 FORCE_COLOR=0 NODE_OPTIONS="--import=file://$work/spawn-log.mjs" \
    sh scripts/with-env.sh node_modules/.bin/vitest run --project node $files --reporter=default --reporter=json "--outputFile.json=$out" >"$text" 2>&1
  code=$?
  got=$(node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); console.log(r.numTotalTests + " " + r.numPassedTests + " " + r.numFailedTests)' "$out" 2>/dev/null || echo no-report)
  dirs=$(ls -d .tmp/server-stops.* 2>/dev/null | sort | comm -13 "$before" - | sed 's|$|/marks|')
  count=$(node "$work/left.mjs" "$spawned" "$marks" $dirs)
  left=${count%% *}
  echo "loop 4.13 round $round: exit=$code total-passed-failed=$got left=$left (recorded ids, marks: ${count#* }) seconds=$(($(date +%s) - t0))"
  if [ "$code" = 0 ] && [ "$got" = "$expected" ] && [ "$left" = 0 ]; then
    passed=$((passed + 1))
  else
    node -e 'const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); for (const f of r.testResults) for (const t of f.assertionResults) if (t.status !== "passed") console.log("loop 4.13 round failed test: " + t.title)' "$out" 2>/dev/null
    perl -pe 's/\e\[[0-9;]*[A-Za-z]//g' "$text" | tail -n 40
  fi
  left_total=$((left_total + left))
  round=$((round + 1))
done
echo "loop 4.13: server-stops.node.test.ts and process-group-start.node.test.ts passed ${expected%% *} of ${expected%% *} in $passed of $rounds rounds; processes left after the rounds by the ids that each round recorded: $left_total; seconds=$(($(date +%s) - start))"
[ "$passed" = "$rounds" ] && [ "$left_total" = 0 ]
