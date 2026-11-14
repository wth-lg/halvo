import { spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { config } from "../../../stress/config.mjs";
import * as driver from "../../../stress/driver.mjs";
import * as rule from "../../../stress/rule.mjs";
import { spikeApi, testEntryApi } from "../../../stress/targets.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const RECORDS = join(HERE, "records");
const base = JSON.parse(readFileSync(join(HERE, "record-base.json"), "utf8"));

function judgeInput(record) {
  const input = { ...structuredClone(base), ...structuredClone(record.top ?? {}) };
  const request = input.requests.findIndex((each) => each.id === "r");
  if (record.request && request >= 0) input.requests[request] = { ...input.requests[request], ...record.request };
  const row = input.final.holds.findIndex((each) => each.request_id === "r");
  if (record.row && row >= 0) input.final.holds[row] = { ...input.final.holds[row], ...record.row };
  if (record.periods) input.final.periods = record.periods;
  if (record.mockEvents) input.mockEvents = record.mockEvents;
  return { ...input, chatStatus: testEntryApi.chatStatus, ledger: record.ledger === "spike" ? spikeApi.ledger : testEntryApi.ledger };
}

const pairs = (violations) => violations.map((violation) => `${violation.clause} ${violation.id ?? "-"}`).sort();

function readJudge(record) {
  const verdict = rule.judge(judgeInput(record));
  const problems = [];
  if (verdict.counted !== record.expect.counted) problems.push(`counted is ${verdict.counted}, expected ${record.expect.counted}`);
  const got = pairs(verdict.violations);
  const want = pairs(record.expect.violations);
  if (!isDeepStrictEqual(got, want)) problems.push(`violations [${got.join(", ")}], expected [${want.join(", ")}]`);
  for (const [name, value] of Object.entries(record.expect.facts ?? {})) {
    if (!isDeepStrictEqual(verdict.facts[name], value)) problems.push(`fact ${name} is ${JSON.stringify(verdict.facts[name])}, expected ${JSON.stringify(value)}`);
  }
  return { good: want.length === 0, problems };
}

function readSend(record) {
  const good = record.expect.resend;
  if (typeof driver.lostOnTheWayIn !== "function") return { good, problems: ["stress/driver.mjs exports no lostOnTheWayIn"] };
  const got = driver.lostOnTheWayIn(record.attempt);
  return { good, problems: got === good ? [] : [`sent again is ${got}, expected ${good}`] };
}

function readSendsAgain(record) {
  const good = record.expect.sendsAgain;
  if (typeof driver.sendsAgain !== "function") return { good, problems: ["stress/driver.mjs exports no sendsAgain"] };
  const got = driver.sendsAgain(record.attempt, record.paused);
  return { good, problems: got === good ? [] : [`sent again is ${got}, expected ${good}`] };
}

function readLostAttempt(record) {
  if (typeof driver.lostAttemptOf !== "function") return { good: true, problems: ["stress/driver.mjs exports no lostAttemptOf"] };
  const got = driver.lostAttemptOf(record.attempt);
  return { good: true, problems: isDeepStrictEqual(got, record.expect.entry) ? [] : [`the entry is ${JSON.stringify(got)}, expected ${JSON.stringify(record.expect.entry)}`] };
}

function readReceipt(record) {
  const good = record.expect.now !== null;
  if (driver.Receipt === undefined) return { good, problems: ["stress/driver.mjs exports no Receipt"] };
  const parsed = driver.Receipt.safeParse(JSON.parse(record.header));
  const now = parsed.success ? (parsed.data.now ?? "missing") : null;
  return { good, problems: now === record.expect.now ? [] : [`now is ${now}, expected ${record.expect.now}`] };
}

function readGateway(record) {
  if (typeof rule.gatewayOf !== "function") return { good: true, problems: ["stress/rule.mjs exports no gatewayOf"] };
  const got = rule.gatewayOf(config, record.cell);
  return { good: true, problems: isDeepStrictEqual(got, record.expect) ? [] : [`the gateway is ${JSON.stringify(got)}, expected ${JSON.stringify(record.expect)}`] };
}

async function readAttempts(record) {
  const state = process.env.GATE_STATE ?? join(HERE, "..", "..", "..", ".tmp");
  mkdirSync(state, { recursive: true });
  const attemptsOf = (runNo) => (record.attemptsByRun ? record.attemptsByRun[runNo - 1] : record.attempts);
  const scripted = (_ctx, cell, variant, runNo, withKill, attempt) => ({
    key: `attempt-${attempt + 1}`,
    cell: cell.name,
    kind: cell.kind,
    level: cell.level,
    switch: cell.switch,
    variant,
    withKill,
    runNo,
    counted: attemptsOf(runNo)[attempt].counted,
    violations: [],
    facts: attemptsOf(runNo)[attempt].facts,
    ms: 1,
    error: null,
  });
  const reported = [];
  const ctx = {
    cfg: { attempts: record.cfgAttempts ?? (record.attempts ?? record.attemptsByRun[0]).length },
    target: { command: () => ({}) },
    outDir: state,
    jobs: 1,
    stopAtFirstViolation: record.stopAtFirstViolation === true,
    progress: (result) => reported.push(result.key),
    runOnce: async (...args) => scripted(...args),
  };
  const runs = await driver.runCells(ctx, [record.cell], "counts-open-holds");
  const [run] = runs;
  const problems = [];
  const discarded = record.expect.discarded ?? [];
  if (record.expect.discarded !== undefined && run.discarded.length !== discarded.length) problems.push(`${run.discarded.length} discarded attempts, expected ${discarded.length}`);
  if (record.expect.reported !== undefined && !isDeepStrictEqual(reported, record.expect.reported)) problems.push(`the driver reported ${JSON.stringify(reported)}, expected ${JSON.stringify(record.expect.reported)}`);
  if (record.expect.halted !== undefined && (ctx.halted === true) !== record.expect.halted) problems.push(`the driver halted is ${ctx.halted === true}, expected ${record.expect.halted}`);
  discarded.forEach((want, at) => {
    for (const [name, value] of Object.entries(want)) {
      if (!isDeepStrictEqual(run.discarded[at]?.[name], value)) problems.push(`discarded attempt ${at + 1} keeps ${name} as ${JSON.stringify(run.discarded[at]?.[name])}, expected ${JSON.stringify(value)}`);
    }
  });
  (record.expect.runs ?? []).forEach((want, at) => {
    const got = runs[at];
    const facts = { key: got?.key, rigUnbuilt: got?.rigUnbuilt === true, discarded: got?.discarded.length };
    for (const [name, value] of Object.entries(want)) {
      if (!isDeepStrictEqual(facts[name], value)) problems.push(`run ${at + 1} has ${name} ${JSON.stringify(facts[name])}, expected ${JSON.stringify(value)}`);
    }
  });
  return { good: true, problems };
}

const CHECK = join(HERE, "..", "..", "..", "stress", "check.mjs");
async function readLine(record) {
  const good = record.expect.line.startsWith("PASS ");
  if (!readFileSync(CHECK, "utf8").includes("export function lineOf(")) return { good, problems: ["stress/check.mjs exports no lineOf, and importing it would start the check"] };
  const { lineOf } = await import(pathToFileURL(CHECK).href);
  const got = lineOf({ selected: record.selected, runs: record.runs, halted: record.halted ?? false, attempts: record.attempts ?? 6 });
  const problems = [];
  if (got.line !== record.expect.line) problems.push(`the line is "${got.line}", expected "${record.expect.line}"`);
  if (record.expect.lostInTransport !== undefined && got.lostInTransport !== record.expect.lostInTransport) problems.push(`lost in transport ${got.lostInTransport}, expected ${record.expect.lostInTransport}`);
  return { good, problems };
}

async function readAttemptLine(record) {
  if (!readFileSync(CHECK, "utf8").includes("export function attemptLineOf(")) return { good: true, problems: ["stress/check.mjs exports no attemptLineOf, and importing it would start the check"] };
  const { attemptLineOf } = await import(pathToFileURL(CHECK).href);
  const got = attemptLineOf(record.result, record.discarded);
  return { good: true, problems: got === record.expect.line ? [] : [`the line is "${got}", expected "${record.expect.line}"`] };
}

async function loadCheck() {
  if (!readFileSync(CHECK, "utf8").includes("export function lineOf(")) return null;
  return import(pathToFileURL(CHECK).href);
}

async function readExitCode(record) {
  const check = await loadCheck();
  if (typeof check?.exitCodeOf !== "function") return { good: true, problems: ["stress/check.mjs exports no exitCodeOf"] };
  const problems = record.cases.filter((each) => check.exitCodeOf(each.line) !== each.code).map((each) => `the line "${each.line.slice(0, 40)}" gives exit ${check.exitCodeOf(each.line)}, expected ${each.code}`);
  return { good: true, problems };
}

async function readMainGuard(record) {
  const check = await loadCheck();
  if (typeof check?.isMainScript !== "function") return { good: true, problems: ["stress/check.mjs exports no isMainScript"] };
  const paths = { check: CHECK, records: fileURLToPath(import.meta.url), none: undefined };
  const problems = record.cases
    .filter((each) => check.isMainScript(paths[each.argv1], pathToFileURL(CHECK).href) !== each.main)
    .map((each) => `started as ${each.argv1}, main runs is ${!each.main}, expected ${each.main}`);
  return { good: true, problems };
}

function readScriptRun(record) {
  const env = record.root ? { ...process.env, HALVO_ROOT: join(HERE, record.root) } : process.env;
  const run = spawnSync(process.execPath, [CHECK, ...record.args], { encoding: "utf8", timeout: 30_000, env });
  const problems = [];
  if (run.status !== record.expect.status) problems.push(`node stress/check.mjs ${record.args.join(" ")} exited ${run.status}, expected ${record.expect.status}`);
  if (!`${run.stdout}${run.stderr}`.includes(record.expect.text)) problems.push(`its output lacks "${record.expect.text}"`);
  return { good: true, problems };
}

function readServerOptions(record) {
  if (typeof driver.serverOptionsOf !== "function") return { good: true, problems: ["stress/driver.mjs exports no serverOptionsOf"] };
  const got = driver.serverOptionsOf({ ...config, killRuns: 1 }, record.cell, "counts-open-holds", "run", { port: 1, inspectorPort: 2, mockPort: 3 });
  return { good: true, problems: got.holdLifeMs === record.expect.holdLifeMs ? [] : [`the server starts with a hold life of ${got.holdLifeMs}, expected ${record.expect.holdLifeMs}`] };
}

function readGatewayDefault(record) {
  const problems = [];
  for (const each of record.cases) {
    const input = judgeInput({ top: { cell: each.cell } });
    const withoutGateway = Object.fromEntries(Object.entries(input).filter(([name]) => name !== "gateway"));
    const byDefault = rule.judge(withoutGateway).facts.slowBoundMs;
    const asTheDriverRecords = rule.judge({ ...input, gateway: rule.gatewayOf({ ...config, killRuns: 1 }, each.cell) }).facts.slowBoundMs;
    if (byDefault !== each.slowBoundMs || asTheDriverRecords !== each.slowBoundMs) {
      problems.push(`${each.cell.name}: the default gives ${byDefault}, the driver's record ${asTheDriverRecords}, expected ${each.slowBoundMs}`);
    }
  }
  return { good: true, problems };
}

const READINGS = {
  "exit-code": readExitCode,
  "main-guard": readMainGuard,
  "server-options": readServerOptions,
  "script-run": readScriptRun,
  "gateway-default": readGatewayDefault, judge: readJudge, send: readSend, receipt: readReceipt, gateway: readGateway, attempts: readAttempts, line: readLine, "attempt-line": readAttemptLine, "sends-again": readSendsAgain, "lost-attempt": readLostAttempt };

const files = readdirSync(RECORDS)
  .filter((name) => name.endsWith(".json"))
  .sort();
if (files.length === 0) {
  console.log("self-test concurrency: records: no record under gate/fixtures/concurrency/records");
  process.exit(1);
}
let failed = 0;
for (const file of files) {
  const name = file.slice(0, -".json".length);
  const said = name.startsWith("good-") ? "good" : name.startsWith("bad-") ? "bad" : null;
  let result;
  try {
    const record = JSON.parse(readFileSync(join(RECORDS, file), "utf8"));
    const reading = READINGS[record.kind];
    result = reading ? await reading(record) : { good: null, problems: [`the kind ${record.kind} has no reading`] };
  } catch (error) {
    result = { good: null, problems: [`it threw: ${String(error?.message ?? error).split("\n")[0]}`] };
  }
  if (said === null) result.problems.push("its name starts with neither good- nor bad-");
  else if (result.good !== null && result.good !== (said === "good")) result.problems.push(`its expectation is ${result.good ? "good" : "bad"}, its name says ${said}`);
  if (result.problems.length === 0) {
    console.log(`self-test concurrency: ${said} record-${name.slice(said.length + 1)}: ${said === "good" ? "PASS" : "FAIL"} as expected`);
  } else {
    failed++;
    console.log(`self-test concurrency: record ${name}: ${result.problems.join("; ")}`);
  }
}
console.log(`concurrency: records: ${files.length - failed} of ${files.length} as expected`);
process.exit(failed === 0 ? 0 : 1);
