import { fork, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = process.env.HALVO_ROOT ?? join(HERE, "../../..");
const STATE = process.env.GATE_STATE ?? join(ROOT, ".tmp");
const OWN_FILE = "test-kit/fake-meter/rerun/check.mjs";
const SEEDS = [1, 2, 3, 4, 5];
const ORDERINGS = 20000;
const SETTINGS = 41;
const WORKERS = 4;
const BOUNDED_SEEDS = [1];
const BOUNDED_ORDERINGS = 100;
const TEST_SECONDS = 600;
const MEMORY_SETTING = 25;
const MEMORY_ROUNDS = 4;
const MEMORY_DATABASES_PER_ROUND = 121;
const MEMORY_DATABASES_AT_LEAST = MEMORY_ROUNDS * MEMORY_DATABASES_PER_ROUND;
const MEMORY_LIVE_BOUND = 2;
const SEED_SECONDS = 1800;
const SETTING_SECONDS = 2400;
const ONCE_SECONDS = 120;
const MEMORY_SECONDS = 120;
const BOUNDS = {
  seed: ["SEED_SECONDS", SEED_SECONDS],
  case: ["SETTING_SECONDS", SETTING_SECONDS],
  once: ["ONCE_SECONDS", ONCE_SECONDS],
  memory: ["MEMORY_SECONDS", MEMORY_SECONDS],
};
const KILL_WAIT_SECONDS = 10;
const PARENT_GONE_SECONDS = 10;
const PARENT_GONE_ORDERINGS = 3000;
const TESTS = [
  [
    "expiry-on-time",
    "test/metering/sync.node.test.ts",
    "a hold whose expiry falls due while a sync wake waits on the provider ends at its expiry time",
  ],
  ["stripe-mock-accepts", "test/metering/stripe-meter.node.test.ts", "stripe-mock accepts each meter-event request that the mapping sends"],
  [
    "refusal-limit",
    "test/metering/sync.node.test.ts",
    "bound REFUSAL_LIMIT: a meter that refuses with unknown_account three times, one retry delay apart, closes the interval as not billable, logs one line, sends nowhere, and the account delete then finishes",
  ],
  [
    "send-limit",
    "test/metering/sync.node.test.ts",
    "bound SEND_LIMIT_MS: a meter that never answers gets one send, the first clock call past the interval's timestamp plus SEND_LIMIT_MS closes the interval as not billable with no further call, logs one line, and the account delete then finishes",
  ],
  [
    "late-read",
    "test/metering/late-read.node.test.ts",
    "case 10: a request truly lost, then a refusal that stays: rounds inside the window count nothing, the late read counts the refusal, and two more refused sends one retry delay apart close the interval as not billable, after which the account delete finishes",
  ],
  [
    "unexplained-total",
    "test/metering/late-read.node.test.ts",
    "case 14: an event at the meter that no interval of the module owns, of 5 minor units in the interval's minute, leaves the total unexplained, so the interval is read again and never sent",
  ],
  [
    "retry-delay",
    "test/metering/retry-delay.node.test.ts",
    "case 11: a meter that never answers gets 29 sends in the first 86,100 s, at a delay that doubles from the retry delay to RETRY_DELAY_CAP_MS; past the window it gets reads alone, the last one before the late point exactly at it, and the try after a change comes one retry delay later",
  ],
];

const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(name);
  return at === -1 ? undefined : args[at + 1];
};
const list = (text) => text.split(",").filter((each) => each !== "").map(Number);
const planVariable = process.env.HALVO_METERING_PLAN;
let planKind = "whole";
let planRefusal = null;
if (planVariable === "bounded") {
  if (process.env.OUTBOUND_RUN) planKind = "bounded";
  else planRefusal = "HALVO_METERING_PLAN=bounded is refused outside the sandbox's run of check 90: OUTBOUND_RUN is not set";
} else if (planVariable !== undefined) {
  planRefusal = `HALVO_METERING_PLAN=${planVariable} is refused: the variable is bounded or unset`;
}
const seeds = option("--seeds") === undefined ? (planKind === "bounded" ? BOUNDED_SEEDS : SEEDS) : list(option("--seeds"));
const orderings = Number(option("--orderings") ?? (planKind === "bounded" ? BOUNDED_ORDERINGS : ORDERINGS));
const settings = option("--settings") === undefined ? Array.from({ length: SETTINGS }, (_, index) => index) : list(option("--settings"));
const workers = Math.max(1, Math.min(WORKERS, Number(option("--workers") ?? WORKERS)));
const jobSecondsText = option("--job-seconds");
const jobSeconds = jobSecondsText === undefined ? Number.POSITIVE_INFINITY : Number(jobSecondsText);
const selfTestCopy = args.includes("--self-test-copy");
const LINE = selfTestCopy ? "self-test copy, no git: " : "";

const tokens = {
  plan: planKind,
  "fake-checks": "unmeasured",
  orderings: "unmeasured",
  voided: "unmeasured",
  sequences: "unmeasured",
  settings: "unmeasured",
};
for (const [token] of TESTS) tokens[token] = "unmeasured";
let firstFailure = null;
const note = (line) => console.log(`  ${line}`);
const fail = (sentence) => {
  firstFailure ??= sentence;
};

function trackedChanges(when) {
  if (selfTestCopy) return [];
  const run = spawnSync("git", ["-C", ROOT, "status", "--porcelain", "--untracked-files=no", "--", "."], { encoding: "utf8" });
  if (run.status !== 0) {
    const why = run.error?.message ?? `exit ${run.status ?? run.signal}: ${(run.stderr ?? "").split("\n").find((line) => line !== "") ?? "no message"}`;
    fail(`git could not say ${when} the check whether a tracked file differs from HEAD (${why})`);
    return [];
  }
  const own = spawnSync("git", ["-C", ROOT, "ls-files", "--error-unmatch", "--", OWN_FILE], { encoding: "utf8" });
  if (own.status !== 0) {
    fail(`git answers ${when} the check for a repository that does not track the check's own file ${OWN_FILE}`);
    return [];
  }
  return run.stdout.split("\n").filter((line) => line !== "");
}

async function fakeChecks() {
  const { selfCheck } = await import("../self-check.ts");
  const rules = selfCheck();
  for (const rule of rules.filter((r) => !r.ok)) note(`fake meter: FAILED its own check: ${rule.name}`);
  tokens["fake-checks"] = `${rules.filter((r) => r.ok).length} of ${rules.length}`;
  if (!rules.every((r) => r.ok)) fail(`the fake meter fails its own check ${rules.find((r) => !r.ok)?.name}`);
}

function boundOf(job) {
  const [name, seconds] = BOUNDS[job.kind];
  return jobSeconds < seconds ? { name: "--job-seconds", seconds: jobSeconds } : { name, seconds };
}

function endWorker(child) {
  child.kill("SIGKILL");
}

function megabytes(bytes) {
  return typeof bytes === "number" ? String(Math.round(bytes / 1048576)) : "?";
}

function runJobs(jobs, execArgv = []) {
  return new Promise((resolve) => {
    const results = new Map();
    const queue = [...jobs];
    let alive = 0;
    const finish = () => {
      if (alive === 0) resolve(results);
    };
    const start = () => {
      const child = fork(join(HERE, "worker.mjs"), [], { cwd: ROOT, stdio: ["ignore", "inherit", "inherit", "ipc"], execArgv: [...process.execArgv, ...execArgv] });
      alive += 1;
      let current = null;
      let timer = null;
      let exited = false;
      const ended = () => {
        if (exited) return false;
        exited = true;
        clearTimeout(timer);
        alive -= 1;
        return true;
      };
      const overrun = (job, bound) => {
        results.set(job.id, { id: job.id, bound: `the job ran past its bound of ${bound.seconds} s (${bound.name})` });
        console.error(`  job ${job.id} ran past its bound of ${bound.seconds} s (${bound.name}); the check ends its worker`);
        current = null;
        endWorker(child);
        setTimeout(() => {
          if (!ended()) return;
          fail(`a worker of check 55 did not end within ${KILL_WAIT_SECONDS} s (KILL_WAIT_SECONDS) of the check's kill after the job ${job.id} ran past its bound`);
          finish();
        }, KILL_WAIT_SECONDS * 1000).unref();
      };
      const next = () => {
        current = queue.shift();
        if (current === undefined) {
          child.send({ kind: "exit" });
          return;
        }
        const job = current;
        const bound = boundOf(job);
        timer = setTimeout(() => overrun(job, bound), bound.seconds * 1000);
        child.send(job);
      };
      child.on("message", (message) => {
        if (message.ready !== undefined) {
          if (message.ready !== SETTINGS) fail(`the search holds ${message.ready} settings, and the plan SETTINGS is ${SETTINGS}`);
          next();
          return;
        }
        clearTimeout(timer);
        results.set(message.id, message);
        const failure = message.error ?? message.result?.failure?.check ?? null;
        console.error(
          `  job ${message.id} ended after ${message.result?.seconds?.toFixed(1) ?? "?"} s, worker resident ${megabytes(message.memory?.resident)} MB, peak ${megabytes(message.memory?.peak)} MB: ${failure === null ? "every check passed" : failure}`,
        );
        current = null;
        next();
      });
      child.on("exit", (code, signal) => {
        if (!ended()) return;
        if (current !== null && current !== undefined && !results.has(current.id)) {
          results.set(current.id, { id: current.id, error: `the worker ended with ${signal ?? `exit ${code}`} during the job` });
        }
        finish();
      });
    };
    for (let each = 0; each < Math.min(workers, jobs.length); each += 1) start();
    if (jobs.length === 0) resolve(results);
  });
}

function describeFailure(where, failure) {
  return `${where}: ${failure.check} (${failure.detail})`;
}

async function rule() {
  const jobs = [
    ...settings.map((index) => ({ id: `setting-${index}`, kind: planKind === "bounded" ? "once" : "case", index })),
    ...seeds.map((seed) => ({ id: `seed-${seed}`, kind: "seed", seed, orderings })),
  ];
  const results = await runJobs(jobs);
  const coverage = {};
  let passedOrderings = 0;
  let passedSequences = 0;
  let searched = 0;
  const failures = [];
  for (const seed of seeds) {
    const message = results.get(`seed-${seed}`);
    if (message === undefined) {
      note(`seed ${seed}: not run, after a failure elsewhere`);
      continue;
    }
    if (message.bound !== undefined) {
      failures.push(`seed ${seed} of the generated orderings: ${message.bound}`);
      continue;
    }
    if (message.error !== undefined) {
      failures.push(`seed ${seed} of the generated orderings: the run threw ${message.error}`);
      continue;
    }
    const run = message.result;
    passedOrderings += run.passed;
    for (const [key, value] of Object.entries(run.coverage)) coverage[key] = (coverage[key] ?? 0) + value;
    note(`seed ${seed}: ${run.passed} of ${run.planned} orderings passed every check, ${run.seconds.toFixed(1)} s`);
    for (const each of run.failing) note(`  failing ordering: ${JSON.stringify({ seed, ...each })}`);
    for (const [check, scenario] of Object.entries(run.shortest)) note(`  shortest failing ordering for ${check}: ${JSON.stringify(scenario)}`);
    if (run.failure !== null) {
      const first = run.failing[0];
      failures.push(describeFailure(`seed ${seed} of the generated orderings, ordering ${first?.index ?? "?"} (${run.failing.length} of ${run.planned} fail)`, run.failure));
    }
  }
  for (const index of settings) {
    const message = results.get(`setting-${index}`);
    if (message === undefined) {
      note(`search setting ${index}: not run, after a failure elsewhere`);
      continue;
    }
    if (message.bound !== undefined) {
      failures.push(`the fault-sequence search, setting ${index}: ${message.bound}`);
      continue;
    }
    if (message.error !== undefined) {
      failures.push(`the fault-sequence search, setting ${index}: the run threw ${message.error}`);
      continue;
    }
    const run = message.result;
    note(`search setting ${index}: ${run.sequences} sequences, ${run.failure === null ? "every check passed" : "a check failed"}, ${run.seconds.toFixed(1)} s, reads whose range held another sealed interval: ${run.rangesWithAnotherInterval}`);
    if (run.failure === null) {
      searched += 1;
      passedSequences += run.sequences;
    } else {
      passedSequences += run.sequences - 1;
      note(`  failing setting: ${JSON.stringify({ setting: index, check: run.failure.check, detail: run.failure.detail, facts: run.failure.facts, faults: run.failure.scenario.faultsByProviderCall })}`);
      failures.push(
        describeFailure(`the fault-sequence search, setting ${index}, faults ${JSON.stringify(run.failure.scenario.faultsByProviderCall)}`, run.failure),
      );
    }
  }
  note("what the generated orderings held (counts over all of them):");
  for (const key of Object.keys(coverage).sort()) note(`  ${key}: ${coverage[key]}`);
  tokens.orderings = `${passedOrderings} of ${seeds.length * orderings}`;
  tokens.voided = String(coverage["hold.voided-at-expiry"] ?? 0);
  tokens.sequences = String(passedSequences);
  tokens.settings = `${searched} of ${settings.length}`;
  const scenarioRuns = settings.reduce((sum, index) => sum + (results.get(`setting-${index}`)?.result?.sequences ?? 0), 0);
  if (planKind === "bounded" && scenarioRuns !== settings.length) {
    failures.push(`the bounded plan ran ${scenarioRuns} scenarios for ${settings.length} settings, where each setting's scenario runs once`);
  }
  note(`failing jobs: ${failures.length}`);
  for (const failure of failures) note(`  ${failure}`);
  if (failures.length > 0) fail(failures[0]);
  else if (seeds.length > 0 && orderings > 0 && tokens.voided === "0") {
    fail("no generated ordering voided a hold at its expiry, so the line did not test that such a hold counts nothing");
  }
}

function escaped(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function runTests() {
  const missing = TESTS.map(([, file]) => file).filter((file) => !existsSync(join(ROOT, file)));
  if (missing.length > 0) {
    fail(`the test file ${missing[0]} is missing`);
    return;
  }
  mkdirSync(STATE, { recursive: true });
  const dir = mkdtempSync(join(STATE, "metering-tests."));
  const report = join(dir, "vitest.json");
  const files = [...new Set(TESTS.map(([, file]) => file))];
  const pattern = TESTS.map(([, , title]) => escaped(title)).join("|");
  const run = spawnSync(
    join(ROOT, "node_modules/.bin/vitest"),
    ["run", "--root", ROOT, "--project", "node", "--reporter=json", `--outputFile=${report}`, "-t", pattern, ...files],
    {
      cwd: ROOT,
      encoding: "utf8",
      timeout: TEST_SECONDS * 1000,
      env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME ?? join(ROOT, ".tmp/wrangler-config") },
    },
  );
  writeFileSync(join(dir, "vitest.out"), `${run.stdout ?? ""}${run.stderr ?? ""}`);
  note(`vitest exit ${run.status ?? run.signal}; its report: ${report}`);
  if (!existsSync(report)) {
    fail(`vitest wrote no report for the ${TESTS.length} tests (exit ${run.status ?? run.signal})`);
    return;
  }
  const results = JSON.parse(readFileSync(report, "utf8")).testResults ?? [];
  for (const [token, file, title] of TESTS) {
    const fileResult = results.find((each) => each.name === join(ROOT, file));
    const test = fileResult?.assertionResults?.find((each) => each.title === title);
    const status = test?.status ?? "missing";
    tokens[token] = status;
    note(`test ${title}: ${status}`);
    if (status !== "passed") fail(`the test "${title}" of ${file} is ${status}, not passed`);
  }
}

async function plan() {
  const { READ_WIDTH_SECONDS, searchCases } = await import("./property.ts");
  const held = searchCases().length;
  const text = `seeds=${SEEDS.join(",")} orderings-of-one-seed=${ORDERINGS} settings=${SETTINGS} read-width-seconds=${READ_WIDTH_SECONDS}`;
  if (held !== SETTINGS) {
    console.log(`FAIL metering-plan: ${text}; first failure: the search holds ${held} settings, and the plan SETTINGS is ${SETTINGS}`);
    process.exit(1);
  }
  console.log(`PASS metering-plan: ${text}`);
  process.exit(0);
}

async function memory() {
  const results = await runJobs([{ id: "memory", kind: "memory", index: MEMORY_SETTING, rounds: MEMORY_ROUNDS }], ["--expose-gc"]);
  const message = results.get("memory");
  const run = message?.result;
  const text =
    run === undefined
      ? "unmeasured"
      : `setting=${MEMORY_SETTING} rounds=${MEMORY_ROUNDS} databases=${run.databases} databases-at-least=${MEMORY_DATABASES_AT_LEAST} live-after-collection=${run.live} bound=${MEMORY_LIVE_BOUND} time-bound-seconds=${MEMORY_SECONDS} heap-after-collection-mb=${Math.round(run.heapUsed / 1048576)} resident-mb=${megabytes(message.memory?.resident)} peak-resident-mb=${megabytes(message.memory?.peak)}`;
  let failure = firstFailure;
  if (failure === null && message?.bound !== undefined) failure = `the memory job: ${message.bound}`;
  else if (failure === null && run === undefined) failure = `the worker's run threw ${message?.error ?? "nothing it reported"}`;
  else if (failure === null && run.databases < MEMORY_DATABASES_AT_LEAST) failure = `the worker opened ${run.databases} databases, fewer than the ${MEMORY_DATABASES_AT_LEAST} that the count needs`;
  else if (failure === null && run.live > MEMORY_LIVE_BOUND) failure = `${run.live} of ${run.databases} databases live on in the worker after their worlds closed, above the bound ${MEMORY_LIVE_BOUND}`;
  if (failure === null) {
    console.log(`PASS metering-memory: ${text}`);
    process.exit(0);
  }
  console.log(`FAIL metering-memory: ${text}; first failure: ${failure}`);
  process.exit(1);
}

const STAND_IN_PARENT = `
const { fork } = require("node:child_process");
const [workerPath, orderings, how] = process.argv.slice(-3);
const child = fork(workerPath, [], { stdio: ["ignore", "ignore", "inherit", "ipc"] });
child.on("message", (message) => {
  if (message.ready === undefined) return;
  child.send({ id: "parent-gone", kind: "seed", seed: 1, orderings: Number(orderings) });
  process.stdout.write("worker " + child.pid + " started its job\\n");
  if (how === "own-exit") setTimeout(() => process.exit(0), 500);
});
`;

function endParent(parent) {
  parent.kill("SIGKILL");
}

function endOrphan(pid) {
  process.kill(pid, "SIGKILL");
}

function startOf(pid) {
  const run = spawnSync("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8" });
  return run.status === 0 ? run.stdout.trim() : "unread";
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code !== "ESRCH";
  }
}

async function parentGone() {
  const real = process.env.GITHUB_ACTIONS === "true";
  const how = real ? "real-kill" : "own-exit";
  const parent = spawn(process.execPath, ["-e", STAND_IN_PARENT, join(HERE, "worker.mjs"), String(PARENT_GONE_ORDERINGS), how], {
    cwd: ROOT,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const lines = [];
  const closed = [];
  for (const stream of [parent.stdout, parent.stderr]) {
    let rest = "";
    stream.setEncoding("utf8");
    stream.on("data", (text) => {
      const parts = `${rest}${text}`.split("\n");
      rest = parts.pop() ?? "";
      lines.push(...parts);
    });
    closed.push(new Promise((resolve) => stream.on("close", resolve)));
  }
  let parentEnded = null;
  parent.on("exit", () => {
    parentEnded = performance.now();
  });
  const text = (worker, after) =>
    `bound-seconds=${PARENT_GONE_SECONDS} job-orderings=${PARENT_GONE_ORDERINGS} parent-ended-by=${how} worker-ended=${worker} ended-after-seconds=${after}`;
  const started = performance.now();
  let workerPid = null;
  while (workerPid === null && parentEnded === null && performance.now() - started < 60_000) {
    const named = lines.map((line) => /^worker (\d+) started its job$/.exec(line)).find((match) => match !== null && match !== undefined);
    if (named !== undefined) workerPid = Number(named[1]);
    else await sleep(50);
  }
  if (workerPid === null) {
    for (const line of lines) note(line);
    console.log(`FAIL metering-parent-gone: ${text("unmeasured", "unmeasured")}; first failure: the stand-in parent named no worker that started its job within 60 s`);
    if (parentEnded === null) endParent(parent);
    process.exit(1);
  }
  if (real) endParent(parent);
  else note(`the kill of the stand-in parent ${parent.pid}, started ${startOf(parent.pid)}, is recorded and not made: off a GitHub runner the parent ends by its own exit`);
  while (parentEnded === null && performance.now() - started < 90_000) await sleep(20);
  if (parentEnded === null) {
    console.log(`FAIL metering-parent-gone: ${text("unmeasured", "unmeasured")}; first failure: the stand-in parent did not end`);
    endParent(parent);
    process.exit(1);
  }
  while (alive(workerPid) && performance.now() - parentEnded < PARENT_GONE_SECONDS * 1000) await sleep(100);
  const after = (performance.now() - parentEnded) / 1000;
  const lived = alive(workerPid);
  if (!lived) await Promise.race([Promise.all(closed), sleep(2000)]);
  for (const line of lines) note(line);
  const said = lines.some((line) => /^check 55 worker \d+: its parent \d+ is gone; the worker ends itself$/.test(line));
  const windows = process.platform === "win32";
  let failure = null;
  if (lived) failure = `the worker still runs ${PARENT_GONE_SECONDS} s (PARENT_GONE_SECONDS) after its parent ended`;
  else if (!said && !windows) failure = "the worker ended with no line that says its parent is gone";
  if (lived && real) endOrphan(workerPid);
  else if (lived) note(`the kill of the worker ${workerPid}, started ${startOf(workerPid)}, is recorded and not made: off a GitHub runner its job of ${PARENT_GONE_ORDERINGS} orderings ends it`);
  const line = `${text(lived ? "no" : "yes", lived ? `over-${PARENT_GONE_SECONDS}` : after.toFixed(1))} worker-line=${said ? "yes" : windows ? "no-the-system-ended-it" : "no"}`;
  if (failure === null) {
    console.log(`PASS metering-parent-gone: ${line}`);
    process.exit(0);
  }
  console.log(`FAIL metering-parent-gone: ${line}; first failure: ${failure}`);
  process.exit(1);
}

async function ownCases() {
  const { modelCases } = await import("./property.ts");
  const cases = await modelCases();
  for (const each of cases) note(`model case: ${each.name}: expected ${each.expected}, got ${each.got}`);
  const held = cases.filter((each) => each.got === each.expected).length;
  const wrong = cases.find((each) => each.got !== each.expected);
  if (wrong === undefined) {
    console.log(`PASS metering-model-cases: ${held} of ${cases.length}`);
    process.exit(0);
  }
  console.log(`FAIL metering-model-cases: ${held} of ${cases.length}; first failure: the model case "${wrong.name}" read ${wrong.got}, not ${wrong.expected}`);
  process.exit(1);
}

if (args.includes("--plan")) await plan();
if (args.includes("--model-cases")) await ownCases();
if (args.includes("--memory-self-check")) await memory();
if (args.includes("--parent-gone-self-check")) await parentGone();

if (jobSecondsText !== undefined && !(Number.isInteger(jobSeconds) && jobSeconds >= 1)) {
  planRefusal = `--job-seconds ${jobSecondsText} is refused: it takes a whole number of seconds from 1, which lowers the bound of every job`;
}
if (planRefusal !== null) {
  console.log(`${LINE}FAIL metering: plan=refused; first failure: ${planRefusal}`);
  process.exit(1);
}

const before = trackedChanges("before");
for (const line of before) note(`a tracked file differs from HEAD before the check: ${line}`);
try {
  await fakeChecks();
  await rule();
  runTests();
} catch (error) {
  fail(`the check threw ${error?.name ?? "an error"}: ${String(error?.message ?? error).split("\n")[0]}`);
}
const after = trackedChanges("after");
if (after.length > 0) fail(`a tracked file differs from HEAD after the check: ${after[0].slice(3)}`);
for (const [name, value] of Object.entries(tokens)) {
  const counted = /^(\d+) of (\d+)$/.exec(value);
  if (value === "unmeasured") fail(`the token ${name} was not measured`);
  else if (counted !== null && counted[1] !== counted[2]) fail(`the token ${name} is ${value}`);
}
const text = Object.entries(tokens)
  .map(([name, value]) => `${name}=${value}`)
  .join(" ");
if (firstFailure === null) {
  console.log(`${LINE}PASS metering: ${text}`);
  process.exit(0);
}
console.log(`${LINE}FAIL metering: ${text}; first failure: ${firstFailure}`);
process.exit(1);
