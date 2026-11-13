import { fork, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = process.env.HALVO_ROOT ?? join(HERE, "../../..");
const STATE = process.env.GATE_STATE ?? join(ROOT, ".tmp");
const OWN_FILE = "test-kit/fake-meter/rerun/check.mjs";
const SEEDS = [1, 2, 3, 4, 5];
const ORDERINGS = 20000;
const SETTINGS = 38;
const WORKERS = 4;
const BOUNDED_SEEDS = [1];
const BOUNDED_ORDERINGS = 100;
const TEST_SECONDS = 600;
const MEMORY_SETTING = 25;
const MEMORY_ROUNDS = 4;
const MEMORY_DATABASES_PER_ROUND = 121;
const MEMORY_DATABASES_AT_LEAST = MEMORY_ROUNDS * MEMORY_DATABASES_PER_ROUND;
const MEMORY_LIVE_BOUND = 2;
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
      const next = () => {
        current = queue.shift();
        if (current === undefined) {
          child.send({ kind: "exit" });
          return;
        }
        child.send(current);
      };
      child.on("message", (message) => {
        if (message.ready !== undefined) {
          if (message.ready !== SETTINGS) fail(`the search holds ${message.ready} settings, and the plan SETTINGS is ${SETTINGS}`);
          next();
          return;
        }
        results.set(message.id, message);
        const failure = message.error ?? message.result?.failure?.check ?? null;
        console.error(`  job ${message.id} ended after ${message.result?.seconds?.toFixed(1) ?? "?"} s: ${failure === null ? "every check passed" : failure}`);
        current = null;
        next();
      });
      child.on("exit", (code, signal) => {
        alive -= 1;
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
      : `setting=${MEMORY_SETTING} rounds=${MEMORY_ROUNDS} databases=${run.databases} live-after-collection=${run.live} bound=${MEMORY_LIVE_BOUND} heap-after-collection-mb=${Math.round(run.heapUsed / 1048576)}`;
  let failure = firstFailure;
  if (failure === null && run === undefined) failure = `the worker's run threw ${message?.error ?? "nothing it reported"}`;
  else if (failure === null && run.databases < MEMORY_DATABASES_AT_LEAST) failure = `the worker opened ${run.databases} databases, fewer than the ${MEMORY_DATABASES_AT_LEAST} that the count needs`;
  else if (failure === null && run.live > MEMORY_LIVE_BOUND) failure = `${run.live} of ${run.databases} databases live on in the worker after their worlds closed, above the bound ${MEMORY_LIVE_BOUND}`;
  if (failure === null) {
    console.log(`PASS metering-memory: ${text}`);
    process.exit(0);
  }
  console.log(`FAIL metering-memory: ${text}; first failure: ${failure}`);
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
