import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cells, config } from "./config.mjs";
import { runCells, stopAll, sweep } from "./driver.mjs";
import { testEntryTarget } from "./targets.mjs";

const USAGE = [
  "node stress/check.mjs [--cells REGEX] [--runs N]",
  "Check 50, concurrency: runs S1's matrix of stress/config.mjs on servers that it starts itself",
  "from wrangler.test.jsonc of HALVO_ROOT (or of this checkout), kills each server once in every",
  "kill run, and judges every run by stress/rule.mjs. It prints one line, PASS concurrency: ... or",
  "FAIL concurrency: ..., and stops scheduling runs at the first violation.",
  "--cells and --runs make a partial run for the self-test.",
  "Records go to a new directory concurrency.XXXXXX under GATE_STATE, or under .tmp/ of the root.",
  "Exit codes: 0 PASS, 1 FAIL, 64 bad usage.",
].join("\n");

const ROOT = process.env.HALVO_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), "..");
const flags = process.argv.slice(2);
const known = new Set(["--cells", "--runs"]);
const option = (name) => {
  const at = flags.indexOf(name);
  return at < 0 ? undefined : flags[at + 1];
};
for (let at = 0; at < flags.length; at += 2) {
  if (!known.has(flags[at]) || flags[at + 1] === undefined) {
    console.error(USAGE);
    process.exit(64);
  }
}
const cfg = { ...config };
if (option("--runs") !== undefined) {
  const runs = Number(option("--runs"));
  if (!Number.isSafeInteger(runs) || runs < 1) {
    console.error(USAGE);
    process.exit(64);
  }
  cfg.killRuns = runs;
}
const only = option("--cells") === undefined ? null : new RegExp(option("--cells"));
const selected = cells(cfg).filter((cell) => only === null || only.test(cell.name));

function finish(line) {
  console.log(line);
  process.exit(line.startsWith("PASS ") ? 0 : 1);
}

const prices = JSON.parse(readFileSync(join(ROOT, "test-kit/prices/v1.json"), "utf8"));
if (prices.low_cost_hosted.input !== cfg.price.input || prices.low_cost_hosted.output !== cfg.price.output) {
  finish("FAIL concurrency: runs=unmeasured; first failure: the price of stress/config.mjs is not the low_cost_hosted rates of test-kit/prices/v1.json, so the driver's holds are not the gateway's");
}

const state = process.env.GATE_STATE ?? join(ROOT, ".tmp");
mkdirSync(state, { recursive: true });
const outDir = mkdtempSync(join(state, "concurrency."));
console.log(`concurrency: records in ${outDir}`);
const ctx = {
  cfg,
  target: testEntryTarget(ROOT, cfg, join(outDir, "tmp")),
  outDir,
  jobs: cfg.jobs,
  registry: join(ROOT, ".tmp", "concurrency-started.jsonl"),
  started: new Set(),
  stopAtFirstViolation: true,
  progress: (result, discarded) => {
    const said = result.error ? "FAILED TO RUN" : result.counted ? "counted" : "did not count";
    const extra = discarded > 0 ? `, ${discarded} earlier attempt(s) drawn again` : "";
    console.log(`concurrency: ${result.key}: ${said}, ${result.violations.length} violation(s), ${result.ms} ms${extra}`);
    for (const violation of result.violations.slice(0, 5)) console.log(`concurrency:   ${violation.clause} ${violation.id ?? "-"}: ${violation.detail}`);
    if (result.error) console.log(`concurrency:   ${result.error.split("\n")[0]}`);
  },
};
mkdirSync(join(ROOT, ".tmp"), { recursive: true });
const leftovers = sweep(ctx.registry);
if (leftovers.length > 0) console.log(`concurrency: stopped ${leftovers.length} process(es) that an earlier run left behind`);
process.on("exit", () => stopAll(ctx));
for (const name of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(name, () => {
    stopAll(ctx);
    process.exit(130);
  });
}

const began = Date.now();
let runs;
try {
  runs = await runCells(ctx, selected, "counts-open-holds");
} catch (error) {
  finish(`FAIL concurrency: runs=unmeasured; first failure: the driver stopped: ${String(error?.message ?? error).split("\n")[0]}`);
}
writeFileSync(ctx.registry, "");

const everyViolation = (run) => [...run.violations, ...run.discarded.flatMap((attempt) => attempt.violations)];
const good = (run) => run.counted && !run.error;
const sum = (list, fact) => list.reduce((total, run) => total + (run.facts?.[fact] ?? 0), 0);
const expected = selected.reduce((total, cell) => total + cell.killRuns + cell.plainRuns, 0);
const summary = selected.map((cell) => {
  const mine = runs.filter((run) => run.cell === cell.name);
  const counted = mine.filter(good);
  const exercised =
    cell.kind === "void"
      ? sum(counted, "voided") > 0
      : cell.kind === "quiet"
        ? sum(counted, "expired") > 0
        : sum(counted, "settled") > 0 && (cell.switch !== "over-report" || sum(counted, "overruns") > 0);
  const killCounted = counted.filter((run) => run.withKill).length;
  const plainCounted = counted.filter((run) => !run.withKill).length;
  const violations = mine.reduce((total, run) => total + everyViolation(run).length, 0);
  return { cell, mine, exercised, complete: exercised && killCounted === cell.killRuns && plainCounted === cell.plainRuns, violations };
});
const allViolations = runs.flatMap((run) => everyViolation(run).map((violation) => ({ run, violation })));
const goodRuns = runs.filter(good).length;
const cleanCells = summary.filter((entry) => entry.complete && entry.violations === 0).length;
const waves = summary.filter((entry) => entry.cell.kind === "wave");
const waveWanted = waves.reduce((total, entry) => total + entry.cell.k * entry.cell.plainRuns, 0);
const waveAdmitted = waves.reduce((total, entry) => total + sum(entry.mine.filter(good), "admitted"), 0);
const lost = runs.reduce((total, run) => total + (run.facts?.resets ?? 0) + (run.facts?.proxyLost ?? 0), 0);
const settles = runs.filter((run) => run.level === 200 && !run.withKill && good(run)).flatMap((run) => run.facts.settleMs);
const p99 = settles.length === 0 ? "unmeasured" : `${[...settles].sort((a, b) => a - b)[Math.max(0, Math.ceil(0.99 * settles.length) - 1)]}ms`;
const uncrossed = runs.filter((run) => run.kind === "rollover" && run.facts?.crossed === false);
const tokens = [
  `runs=${goodRuns} of ${expected}`,
  `cells=${cleanCells} of ${selected.length}`,
  `violations=${allViolations.length}`,
  `wave=${waves.length === 0 ? "unmeasured" : `${waveAdmitted} of ${waveWanted}`}`,
  `lost-sends=${lost}`,
  `settle-p99-at-200=${p99}`,
  `second-attempts=${runs.filter((run) => run.discarded.length > 0).length}`,
  `rollover-uncrossed=${uncrossed.length}`,
].join(" ");

const results = { root: ROOT, cfg, ms: Date.now() - began, tokens, runs };
writeFileSync(join(outDir, "results.json"), `${JSON.stringify(results)}\n`);
console.log(`concurrency: ${runs.length} runs in ${Math.round((Date.now() - began) / 1000)} s; results in ${join(outDir, "results.json")}`);

let failure = null;
if (allViolations.length > 0) {
  const { run, violation } = allViolations[0];
  failure = `${run.key}: ${violation.clause}${violation.id ? ` ${violation.id}` : ""}: ${violation.detail}`;
} else {
  const failedToRun = runs.find((run) => run.error && !run.counted);
  const neverCounted = runs.find((run) => !good(run));
  const incomplete = summary.find((entry) => !entry.complete);
  if (uncrossed.length > 0) {
    failure = `${uncrossed.length} attempt(s) of a rollover cell lacked the crossing, which the driver builds before any kill (ruling B-15), first ${uncrossed[0].key}`;
  } else if (failedToRun) failure = `${failedToRun.key} failed to run in ${config.attempts} attempts: ${failedToRun.error.split("\n")[0].slice(0, 200)}`;
  else if (neverCounted) {
    const facts = neverCounted.facts ?? {};
    failure = `${neverCounted.key} did not count in ${config.attempts} attempts (admitted ${facts.admitted ?? "unmeasured"}, holds open at the kill ${facts.openAtKill ?? "unmeasured"}, a stream in flight at the kill ${facts.streamAtKill ?? "unmeasured"}, voided ${facts.voided ?? "unmeasured"})`;
  } else if (ctx.halted) failure = "the driver stopped before every run had its turn";
  else if (incomplete) failure = `${incomplete.cell.name} lacks counted runs or did not exercise its case`;
  else if (goodRuns !== expected) failure = `${goodRuns} of ${expected} runs counted`;
  else if (waves.length > 0 && waveAdmitted !== waveWanted) failure = `the waves admitted ${waveAdmitted}, K times their runs is ${waveWanted}`;
  else if (settles.length === 0 && selected.some((cell) => cell.level === 200)) failure = "no settle was acknowledged at 200 without the kill";
}
finish(failure === null ? `PASS concurrency: ${tokens}` : `FAIL concurrency: ${tokens}; first failure: ${failure}`);
