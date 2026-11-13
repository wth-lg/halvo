import { execFileSync, fork, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = process.env.HALVO_ROOT ?? join(HERE, "../../..");
const STATE = process.env.GATE_STATE ?? join(ROOT, ".tmp");
const SEEDS = [1, 2, 3, 4, 5];
const ORDERINGS = 20000;
const SETTINGS = 34;
const WORKERS = 4;
const TEST_SECONDS = 600;
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
];

const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(name);
  return at === -1 ? undefined : args[at + 1];
};
const list = (text) => text.split(",").filter((each) => each !== "").map(Number);
const seeds = option("--seeds") === undefined ? SEEDS : list(option("--seeds"));
const orderings = Number(option("--orderings") ?? ORDERINGS);
const settings = option("--settings") === undefined ? Array.from({ length: SETTINGS }, (_, index) => index) : list(option("--settings"));
const workers = Math.max(1, Math.min(WORKERS, Number(option("--workers") ?? WORKERS)));

const tokens = {
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

function trackedChanges() {
  try {
    const out = execFileSync("git", ["-C", ROOT, "status", "--porcelain", "--untracked-files=no", "--", "."], { encoding: "utf8" });
    return out.split("\n").filter((line) => line !== "");
  } catch {
    return [];
  }
}

async function fakeChecks() {
  const { selfCheck } = await import("../self-check.ts");
  const rules = selfCheck();
  for (const rule of rules.filter((r) => !r.ok)) note(`fake meter: FAILED its own check: ${rule.name}`);
  tokens["fake-checks"] = `${rules.filter((r) => r.ok).length} of ${rules.length}`;
  if (!rules.every((r) => r.ok)) fail(`the fake meter fails its own check ${rules.find((r) => !r.ok)?.name}`);
}

function runJobs(jobs) {
  return new Promise((resolve) => {
    const results = new Map();
    const queue = [...jobs];
    let alive = 0;
    const finish = () => {
      if (alive === 0) resolve(results);
    };
    const start = () => {
      const child = fork(join(HERE, "worker.mjs"), [], { cwd: ROOT, stdio: ["ignore", "inherit", "inherit", "ipc"] });
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
    ...settings.map((index) => ({ id: `setting-${index}`, kind: "case", index })),
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
    fail(`vitest wrote no report for the four tests (exit ${run.status ?? run.signal})`);
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

const before = trackedChanges();
for (const line of before) note(`a tracked file differs from HEAD before the check: ${line}`);
try {
  await fakeChecks();
  await rule();
  runTests();
} catch (error) {
  fail(`the check threw ${error?.name ?? "an error"}: ${String(error?.message ?? error).split("\n")[0]}`);
}
const after = trackedChanges();
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
  console.log(`PASS metering: ${text}`);
  process.exit(0);
}
console.log(`FAIL metering: ${text}; first failure: ${firstFailure}`);
process.exit(1);
