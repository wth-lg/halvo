import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { cpus, totalmem } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cells, config, controlCells, rowSizeCell } from "../../stress/config.mjs";
import { isStillThatProcess, runCells, startedAt, stopAll, sweep } from "../../stress/driver.mjs";
import { CLAUSES } from "../../stress/rule.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const RESULT = join(HERE, "RESULT.md");

const flags = process.argv.slice(2);
const option = (name) => {
  const at = flags.indexOf(name);
  return at < 0 ? undefined : flags[at + 1];
};
const USAGE = [
  "node spikes/s1/run.mjs [--runs N] [--cells REGEX] [--jobs N] [--no-control]",
  "Spike S1: runs the stress matrix against the minimal ledger of this directory, then the",
  "control variant whose admission ignores open holds, then one run that measures the bytes per",
  "stored row. With no option it rewrites spikes/s1/RESULT.md (its Rule section is copied",
  "unchanged). With an option it is a partial run: it prints its summary and leaves RESULT.md.",
  "",
  "Every run starts its own `wrangler dev` and its own mock upstream on ports the system picks.",
  "Raw records go to .tmp/s1/STAMP/ (results.json, and runs/KEY/ with the server log, the mock's",
  "log, the state directory and run.json). Nothing is removed. Safe to run twice: a run first",
  "stops what an interrupted run left behind. Run one at a time per checkout, for the same reason.",
  "Exit codes: 0 the rule is met, 1 it is missed or the run is incomplete, 64 bad usage.",
].join("\n");
const known = ["--runs", "--cells", "--jobs", "--no-control"];
if (flags.some((flag) => flag.startsWith("--") && !known.includes(flag))) {
  console.error(USAGE);
  process.exit(64);
}
const cfg = { ...config };
for (const [name, field] of [
  ["--runs", "killRuns"],
  ["--jobs", "jobs"],
]) {
  if (!flags.includes(name)) continue;
  const value = Number(option(name));
  if (!Number.isSafeInteger(value) || value < 1) {
    console.error(`s1: ${name} takes a whole number of 1 or more`);
    process.exit(64);
  }
  cfg[field] = value;
}
const only = option("--cells") ? new RegExp(option("--cells")) : null;
const withControl = !flags.includes("--no-control");
const partial = flags.length > 0;

function ruleSectionOfResult() {
  const before = readFileSync(RESULT, "utf8");
  const from = before.indexOf("## Rule\n");
  if (from < 0) {
    console.error("s1: spikes/s1/RESULT.md has no Rule section; restore the file from git and run again");
    process.exit(1);
  }
  const next = before.indexOf("\n## ", from + 1);
  return (next < 0 ? before.slice(from) : before.slice(from, next)).trimEnd();
}
const ruleAsOneLine = ruleSectionOfResult().replace(/^> ?/gm, "").replace(/\s+/g, " ");
const clausesNotInTheRule = Object.entries(CLAUSES).filter(([name, text]) => name !== "integrity" && !ruleAsOneLine.includes(text));
if (clausesNotInTheRule.length > 0) {
  console.error(`s1: the Rule section of spikes/s1/RESULT.md does not hold the text of ${clausesNotInTheRule.map(([name]) => name).join(", ")} as stress/rule.mjs states it; nothing was run`);
  process.exit(1);
}

function envInsideCheckout() {
  const env = Object.fromEntries(
    execFileSync("sh", [join(ROOT, "scripts/with-env.sh"), "env"], { encoding: "utf8" })
      .split("\n")
      .filter((line) => line.includes("="))
      .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]),
  );
  env.XDG_CONFIG_HOME = env.HALVO_WRANGLER_CONFIG_HOME;
  env.TMPDIR = join(ROOT, ".tmp", "s1", "tmp");
  mkdirSync(env.TMPDIR, { recursive: true });
  return env;
}
const env = envInsideCheckout();

const target = {
  command: ({ port, inspectorPort, stateDir, mockPort, holdLifeMs, variant }) => ({
    file: join(ROOT, "node_modules/.bin/wrangler"),
    args: [
      "dev",
      ...["--config", join(HERE, "wrangler.jsonc")],
      ...["--ip", "127.0.0.1", "--port", String(port), "--inspector-port", String(inspectorPort)],
      ...["--persist-to", stateDir, "--show-interactive-dev-session=false"],
      ...Object.entries({
        UPSTREAM_PORT: mockPort,
        HOLD_LIFE_MS: holdLifeMs,
        DEADLINE_MS: cfg.deadlineMs,
        MARGIN_MS: cfg.marginMs,
        INPUT_PRICE: cfg.price.input,
        OUTPUT_PRICE: cfg.price.output,
        ADMISSION: variant,
      }).flatMap(([name, value]) => ["--var", `${name}:${value}`]),
    ],
    env,
  }),
};

const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const outDir = join(ROOT, ".tmp", "s1", stamp);
mkdirSync(outDir, { recursive: true });
const lock = join(ROOT, ".tmp", "s1", "runner.json");
if (existsSync(lock)) {
  const owner = JSON.parse(readFileSync(lock, "utf8"));
  if (isStillThatProcess(owner)) {
    console.error(`s1: another execution (pid ${owner.pid}) is running in this checkout; run one at a time`);
    process.exit(1);
  }
}
writeFileSync(lock, JSON.stringify({ pid: process.pid, started: startedAt(process.pid) }));
const git = (...args) => execFileSync("git", ["-C", ROOT, ...args], { encoding: "utf8" }).trim();
const revision = () => ({
  head: git("rev-parse", "HEAD"),
  changed: git("status", "--porcelain", "--", "spikes/s1/src", "spikes/s1/wrangler.jsonc", "spikes/s1/run.mjs", "stress"),
});
const revisionAtStart = revision();
const ctx = {
  cfg,
  target,
  outDir,
  jobs: cfg.jobs,
  registry: join(ROOT, ".tmp", "s1", "started.jsonl"),
  started: new Set(),
  progress: (result, discarded) => {
    const state = result.error ? "FAILED TO RUN" : result.counted ? "counted" : "did not count";
    const extra = discarded > 0 ? `, ${discarded} earlier attempt(s) drawn again` : "";
    console.error(`s1: ${result.key}: ${state}, ${result.violations.length} violation(s), ${result.ms} ms${extra}`);
  },
};
const leftovers = sweep(ctx.registry);
if (leftovers.length > 0) console.error(`s1: stopped ${leftovers.length} process(es) that an earlier run left behind`);
process.on("exit", () => stopAll(ctx));
for (const name of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(name, () => {
    stopAll(ctx);
    process.exit(130);
  });
}

const pick = (list) => (only ? list.filter((cell) => only.test(cell.name)) : list);
const matrixCells = pick(cells(cfg));
const matrixBegan = Date.now();
const matrix = await runCells(ctx, matrixCells, "counts-open-holds");
const matrixMs = Date.now() - matrixBegan;
const controlList = withControl ? pick(controlCells(cfg)) : [];
const control = await runCells(ctx, controlList, "ignores-open-holds");
const rowSizeRuns = await runCells(ctx, partial ? (only ? pick([rowSizeCell(cfg)]) : []) : [rowSizeCell(cfg)], "counts-open-holds");
writeFileSync(ctx.registry, "");
writeFileSync(lock, "{}");
const revisionAtEnd = revision();
const sameCode = revisionAtStart.head === revisionAtEnd.head && revisionAtStart.changed === "" && revisionAtEnd.changed === "";


const everyViolation = (run) => [...run.violations, ...run.discarded.flatMap((attempt) => attempt.violations)];
const good = (run) => run.counted && !run.error;
const clauseCounts = (runs) => {
  const counts = new Map();
  for (const run of runs) for (const { clause } of everyViolation(run)) counts.set(clause, (counts.get(clause) ?? 0) + 1);
  return [...counts].map(([clause, count]) => `${clause} ${count}`).join(", ");
};
const sum = (runs, fact) => runs.reduce((total, run) => total + (run.facts?.[fact] ?? 0), 0);
const most = (runs, fact) => runs.reduce((worst, run) => (run.facts?.[fact] == null ? worst : Math.max(worst ?? run.facts[fact], run.facts[fact])), null);
const percentile = (values, share) => [...values].sort((a, b) => a - b)[Math.max(0, Math.ceil(share * values.length) - 1)];
const median = (values) => percentile(values, 0.5);

function summarize(list, runs) {
  return list.map((cell) => {
    const mine = runs.filter((run) => run.cell === cell.name);
    const kill = mine.filter((run) => run.withKill);
    const plain = mine.filter((run) => !run.withKill);
    const counted = mine.filter(good);
    const exercised =
      cell.kind === "void"
        ? sum(counted, "voided") > 0
        : cell.kind === "quiet"
          ? sum(counted, "expired") > 0
          : sum(counted, "settled") > 0 && (cell.switch !== "over-report" || sum(counted, "overruns") > 0);
    return {
      cell,
      mine,
      killCounted: kill.filter(good).length,
      plainCounted: plain.filter(good).length,
      complete: exercised && kill.filter(good).length === cell.killRuns && plain.filter(good).length === cell.plainRuns,
      violating: mine.filter((run) => everyViolation(run).length > 0).length,
      violations: mine.reduce((total, run) => total + everyViolation(run).length, 0),
    };
  });
}
const matrixSummary = summarize(matrixCells, matrix);
const controlSummary = summarize(controlList, control);
const incomplete = matrixSummary.filter((entry) => !entry.complete);
const settledAt200 = matrix.some((run) => run.level === 200 && !run.withKill && good(run) && run.facts.settleMs.length > 0);
const judged = [...matrix, ...rowSizeRuns];
const allViolations = judged.flatMap(everyViolation);
const ruleViolations = allViolations.filter((violation) => violation.clause !== "integrity");
const runsWith = (test) => judged.filter((run) => everyViolation(run).some(test)).length;
const breaksCap = (run) => good(run) && run.violations.some((violation) => violation.clause === "cap" || violation.clause === "wave-admits-k");
const controlMissed = (level) => {
  const at = controlSummary.filter((entry) => entry.cell.level === level);
  return at.length > 0 && at.every((entry) => entry.mine.some(breaksCap));
};
const controlCounts = withControl && controlMissed(20) && controlMissed(200);
const rowRun = rowSizeRuns.find(good);
const gaps = [
  ...(incomplete.length > 0 ? [`${incomplete.map((entry) => entry.cell.name).join(", ")} lack counted runs or did not exercise their case`] : []),
  ...(!partial && !settledAt200 ? ["no settle was acknowledged at 200 without the kill, so the settle latency was not measured"] : []),
  ...(!partial && !rowRun ? ["the row-size run did not count, so the bytes per stored row were not measured"] : []),
];

let line;
if (ruleViolations.length > 0) {
  line = `rule: MISSED (${ruleViolations.length} violations of the rule in ${runsWith((violation) => violation.clause !== "integrity")} of ${judged.length} runs: ${clauseCounts(judged)})`;
} else if (gaps.length > 0) {
  line = `rule: MISSED (the run is incomplete, so nothing is shown: ${gaps.join("; ")})`;
} else if (allViolations.length > 0) {
  line = `rule: MISSED (no clause of the rule was broken, but the result does not count: ${allViolations.length} violation(s) of integrity in ${runsWith(() => true)} of ${judged.length} runs; see Violations)`;
} else if (!partial && !sameCode) {
  line = "rule: MISSED (no violation, but the result does not count: the spike's files were not the committed ones, or changed during the run)";
} else if (!controlCounts) {
  line = `rule: MISSED (no violation in ${matrix.length} runs, but the result does not count: the control did not break the cap at 20 and at 200)`;
} else {
  line = `rule: MET (0 violations in ${matrix.length} runs of ${matrixSummary.length} cells; the control missed the rule at 20 and at 200, so the result counts)`;
}
const met = line.startsWith("rule: MET");
if (partial) line = `partial run, not a result: ${line.slice("rule: ".length)}`;


const at200 = matrix.filter((run) => run.level === 200 && good(run));
const settlePlain = at200.filter((run) => !run.withKill).flatMap((run) => run.facts.settleMs);
const settleKill = at200.filter((run) => run.withKill).flatMap((run) => run.facts.settleMs);
const settleWave = at200.filter((run) => run.kind === "wave").flatMap((run) => run.facts.settleMs);
const killRuns = matrix.filter((run) => run.withKill && good(run));
const discarded = matrix.flatMap((run) => run.discarded);
const rowBytes = rowRun ? (rowRun.facts.databaseSize - rowRun.facts.emptyDatabaseSize) / rowRun.facts.admitted : null;
const quiet = matrix.filter((run) => run.kind === "quiet" && good(run));
const pnpmSiblingManifest = (manifest, name) => realpathSync(join(dirname(dirname(manifest)), name, "package.json"));
const versionOf = (manifest) => JSON.parse(readFileSync(manifest, "utf8")).version;
const wranglerPackage = realpathSync(join(ROOT, "node_modules/wrangler/package.json"));
const miniflarePackage = pnpmSiblingManifest(wranglerPackage, "miniflare");
const versions = {
  wrangler: versionOf(wranglerPackage),
  miniflare: versionOf(miniflarePackage),
  workerd: versionOf(pnpmSiblingManifest(miniflarePackage, "workerd")),
};
const machine = `${cpus()[0].model}, ${cpus().length} cores, ${Math.round(totalmem() / 2 ** 30)} GiB, macOS ${execFileSync("sw_vers", ["-productVersion"], { encoding: "utf8" }).trim()}, Node ${process.version}`;
const around = (ms) => `${Math.abs(ms)} ms ${ms < 0 ? "before" : "after"}`;
const clock = (ms) => `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`;

const results = { stamp, line, cfg, versions, machine, matrixMs, matrix, control, rowSizeRuns };
writeFileSync(join(outDir, "results.json"), `${JSON.stringify(results)}\n`);

const cellRow = (entry) => {
  const { cell, mine } = entry;
  const runs = mine.filter(good);
  const worst = most(runs, "worstOver");
  return `| ${cell.name} | ${entry.killCounted}/${cell.killRuns} | ${entry.plainCounted}/${cell.plainRuns} | ${sum(runs, "requests")} | ${sum(runs, "admitted")} | ${sum(runs, "settled")} | ${sum(runs, "expired")} | ${sum(runs, "voided")} | ${sum(runs, "refused")} | ${worst ?? "-"} | ${sum(runs, "overruns")} | ${entry.violations} |`;
};
const tableHead = [
  "| cell | kill runs counted | runs without the kill | requests | admitted | settled | expired, full hold | voided | refused | worst counted spend minus cap | overruns | violations |",
  "|---|---|---|---|---|---|---|---|---|---|---|---|",
];

const out = [];
out.push("# S1: cap ledger under concurrent streams", "", line, "");
out.push(
  `Measured ${new Date().toISOString()} on ${machine}; wrangler ${versions.wrangler}, miniflare ${versions.miniflare}, workerd ${versions.workerd}.`,
  `Code: commit ${revisionAtEnd.head.slice(0, 12)}${sameCode ? ", with the files of spikes/s1 and stress as committed at the start and at the end of the run" : "; NOT A CLEAN MEASUREMENT: the spike's files differed from the commit, or the commit changed during the run"}.`,
  `One execution of \`node spikes/s1/run.mjs\`: ${matrix.length} runs in ${matrixSummary.length} cells, then ${control.length} control runs and ${rowSizeRuns.length} row-size run.`,
  "",
);
if (partial) out.push(`PARTIAL RUN (${flags.join(" ")}): not a result.`, "");

const summaryText = () => {
  const text = [];
  text.push("## What ran", "");
  text.push(
    "The server under test is `spikes/s1`: a Worker that plays the gateway, and one SQLite-backed Durable Object per account that is the ledger. Each run starts its own `wrangler dev` (one process tree: wrangler, esbuild and two workerd processes) and its own mock upstream (`stress/upstream.mjs`), on ports the system picks.",
    `A run sends a seeded plan of streaming requests on one account: ${cfg.levels.join(", ")} concurrent clients, ${Object.values(cfg.requestsPerClient).join(", ")} requests per client. Client 0 aborts every stream before the usage chunk; of the other streams one in ${cfg.abortMidEvery} is aborted mid-stream. With a switch on, about half of the requests take the fault and the rest stream normally. The cap fits ${Object.values(cfg.capHolds).join(", ")} holds, so admission has to refuse.`,
    `In a kill run the whole server tree gets SIGKILL ${cfg.killDelayMs.min} to ${cfg.killDelayMs.max} ms (seeded) after the stream that arms it began (the delay is drawn from the seed), and the server restarts on the same state directory. The arming stream is one of the first three admitted streams, or the first one at level 1. In even-numbered runs, on their first attempt, it is the first stream admitted after a settle was acknowledged, so that settled spend and open holds are both in storage at the kill. In the rollover cell it is the first stream admitted after the period's boundary. A kill run counts only if a hold was open and a stream in flight at the kill. A run that does not count (also a rollover run with no hold open across the boundary, and a void run with a hold that was not voided) or that fails to run is drawn again with a new seed, up to ${cfg.attempts} attempts. A failure after the kill stands as a violation, and every failure is listed. Each matrix cell and the rollover cell also get ${cfg.plainRuns} run without the kill, which the rule's last clause and the settle latency need. The wave and the void cell run without the kill; the quiet cell runs only with it.`,
    `Stress config: hold life ${cfg.holdLifeMs} ms, upstream deadline ${cfg.deadlineMs} ms, margin ${cfg.marginMs} ms (${cfg.voidHoldLifeMs} ms hold life in the void cell). After the last request the driver waits until every hold is ${cfg.alarmGraceMs} ms past its expiry and reads the ledger with a call that runs no ledger step.`,
    "",
    "Sums over the counted runs of each cell. Money is in micro-units. \"Worst counted spend minus cap\" is the highest value over every ledger step of every run: 0 or less means the cap held. It is rebuilt from the rows, whose amounts and charges are checked against the driver's and the mock's records; the order of the steps is the ledger's own sequence number on each row.",
    "",
    ...tableHead,
    ...matrixSummary.map(cellRow),
    "",
  );
  return text;
};
out.push("%RULE%", "", ...summaryText());

out.push("## Measured values", "");
if (settlePlain.length > 0) {
  out.push(
    `- Settle latency at 200, 99th percentile: ${percentile(settlePlain, 0.99)} ms (median ${median(settlePlain)} ms, highest ${Math.max(...settlePlain)} ms, ${settlePlain.length} settles in the ${at200.filter((run) => !run.withKill).length} runs at 200 without the kill, each of them the only run of the spike at that time).${settleWave.length > 0 ? ` In the wave alone, where the mock lets every admitted stream end at once: ${percentile(settleWave, 0.99)} ms over ${settleWave.length} settles.` : ""}`,
    "  How: the gateway reads its clock before the settle call and after the ledger's answer and writes the difference into the stream's last event; the driver collects it from every stream whose client stayed connected. The Workers runtime is documented to move a Worker's clock only when I/O completes, so the figure runs from the last I/O the gateway saw before the call, the end of the upstream stream, to the arrival of the ledger's answer, in whole milliseconds.",
    `  Limits: one Mac, loopback only, with the driver, the mock and the server sharing its cores; the Worker and the object are in one local workerd process, so the figure holds no network hop between them. It is a floor for the gateway's margin, not the margin. ${settleKill.length > 0 ? `In the kill runs at 200, which also ran one at a time, the 99th percentile was ${percentile(settleKill, 0.99)} ms over ${settleKill.length} settles.` : ""} Other work on this Mac was not stopped: its one-minute load average at the start of those ${at200.filter((run) => !run.withKill).length} runs was ${Math.min(...at200.filter((run) => !run.withKill).map((run) => run.facts.load))} to ${Math.max(...at200.filter((run) => !run.withKill).map((run) => run.facts.load))} on ${cpus().length} cores.`,
  );
}
if (rowRun) {
  out.push(
    `- Bytes per stored row: ${rowBytes.toFixed(0)} (${rowRun.facts.databaseSize - rowRun.facts.emptyDatabaseSize} bytes of growth over ${rowRun.facts.admitted} rows: ${rowRun.facts.settled} settled, ${rowRun.facts.expired} expired). At that size 1 GB holds about ${(Math.floor(2 ** 30 / rowBytes / 1e5) / 10).toFixed(1)} million rows.`,
    `  How: the object's \`sql.databaseSize\` after the row-size run minus the same figure before its first request. Limits: SQLite grows in 4096-byte pages, so the figure is read to about ${Math.ceil(4096 / rowRun.facts.admitted)} bytes per row; it is this spike's schema (18 columns, a 36-character request id as primary key) and not the schema of P4.`,
  );
}
const lag = most(matrix.filter(good), "worstExpiryLagMs");
out.push(
  `- Expiry: ${sum(matrix.filter(good), "expired")} holds ended as expired: ${sum(matrix.filter(good), "endedByAlarm")} by the alarm and ${sum(matrix.filter(good), "expired") - sum(matrix.filter(good), "endedByAlarm")} by a ledger step that found them due${sum(matrix.filter(good), "expired") === sum(matrix.filter(good), "endedByAlarm") ? " (so expiry inside a step was not exercised)" : ""}. The longest time a hold stayed open in storage past its expiry was ${lag} ms${quiet.length > 0 ? `; in the quiet cell, where the alarm alone ends the holds after a restart, ${most(quiet, "worstExpiryLagMs")} ms` : ""}.`,
  "  How: `ended_at - expires_at` on each expired row, both on the ledger's clock. Limits: a kill run includes the time the server was down, and in every second quiet run the driver keeps it down past the expiry on purpose.",
);
const servedNeverSettled = { holds: 0, charged: 0, usageWorth: 0 };
for (const run of matrix.filter(good)) for (const name of Object.keys(servedNeverSettled)) servedNeverSettled[name] += run.facts.servedNeverSettled[name];
out.push(
  `- Served but never settled: ${servedNeverSettled.holds} holds expired at the full amount although the mock had delivered a final usage (the causes were not separated; the design allows three: the kill, a client that left, and the gateway's cut). They were charged ${servedNeverSettled.charged} micro-units; the usage the mock reported was worth ${servedNeverSettled.usageWorth}.`,
  "  How: expired rows whose request has a final usage in the mock's log. Limits: a stream cut before its usage chunk served tokens too, and the mock's log does not count them.",
);
const plainRuns = matrix.filter((run) => !run.withKill && good(run) && run.facts.latestStreamEndPastExpiryMs !== null);
if (plainRuns.length > 0) {
  out.push(
    `- Streams against their holds: in the ${plainRuns.length} runs without the kill the upstream stream that ended latest ended ${around(most(plainRuns, "latestStreamEndPastExpiryMs"))} its hold's expiry. The gateway cut ${sum(matrix.filter(good), "cutByGateway")} slow streams at its deadline, the expiry minus the margin.`,
    "  How: the end of each stream in the mock's log against `expires_at` on the request's row; a cut stream is one whose last event to the client says so. Limits: the slow switch draws stream lengths from 0.75 to 1.25 times the time the gateway allows, so about a fifth of the slow streams would end after the expiry if the gateway did not cut them.",
  );
}
const resetRuns = matrix.filter((run) => good(run) && run.facts.resets + run.facts.proxyLost > 0);
out.push(
  `- Requests lost on the way in: in ${resetRuns.length} runs ${sum(resetRuns, "resets")} sends were reset before the server accepted the connection, and ${sum(resetRuns, "proxyLost")} were answered 500 by wrangler's dev proxy, which had lost its own connection to the Worker ("Error inside ProxyWorker ... Network connection lost" in the server log, ${sum(resetRuns, "proxyErrorsLogged")} such lines). The driver sent each of them again under the same request id.`,
  "  How: outside a kill, a send that ends with a connection reset and no answer, or with a 500 whose body carries no stack through the gateway's source. macOS keeps at most 128 connections waiting on a listening socket (`kern.ipc.somaxconn`) and resets the rest; a level of 200 opens 200 at once to the proxy, and the proxy opens 200 to the Worker. wrangler's proxy sends only GET and HEAD again. Limits: had a lost send reached the ledger after all, the second send would be answered as a repeat, which the join reports as a failure; a 500 from the gateway itself, and any other error line in a server log, is also reported as one.",
);
const overdueRuns = killRuns.filter((run) => run.facts.overdueAtRestart > 0);
if (overdueRuns.length > 0) {
  const quietOverdue = overdueRuns.filter((run) => run.kind === "quiet");
  out.push(
    `- Holds overdue at the restart: in ${overdueRuns.length} kill runs ${sum(overdueRuns, "overdueAtRestart")} holds were past their expiry when the server answered again. ${sum(overdueRuns, "overdueByAlarm")} of them were ended by the alarm and the rest by a ledger step; the last one ended ${around(most(overdueRuns, "overdueEndedAfterReadyMs"))} the restarted server's first answer to the driver. ${quietOverdue.length > 0 ? `${quietOverdue.length} of these runs are quiet runs in which the driver keeps the server down until ${cfg.quietOverdueMs} ms after the holds' expiry and then sends nothing: ${sum(quietOverdue, "overdueAtRestart")} holds, ${sum(quietOverdue, "overdueByAlarm")} of them ended by the alarm, the last one ${around(most(quietOverdue, "overdueEndedAfterReadyMs"))} that first answer.` : ""}`,
    "  How: rows that were open at the kill, expired, with an expiry at or before the time the restarted server first answered the driver's health call; `ended_at` on the ledger's clock against that time. \"Before\" means the alarm ran while the server was starting, before it answered the driver.",
  );
}
const aborted = (fact) => {
  const total = { admitted: 0, settled: 0, fullHold: 0, free: 0 };
  for (const run of matrix.filter(good)) for (const name of Object.keys(total)) total[name] += run.facts[fact]?.[name] ?? 0;
  return `${total.admitted} admitted, ${total.settled} settled at the usage the mock reported, ${total.fullHold} charged the full hold, ${total.free} charged nothing`;
};
out.push(
  `- Aborted streams: of the streams of the client that aborts before the usage chunk, ${aborted("abortedBeforeUsage")}. Of the streams aborted mid-stream, ${aborted("abortedMid")}.`,
  "  How: the state of each such request's row. The gateway cancels the upstream call when a write to the client fails, and the hold then expires. A stream settles after its client left when the gateway read the final usage before a write failed; when the runtime tells a Worker that its client left was not measured.",
);
if (killRuns.length > 0) {
  out.push(
    `- The kill: ${killRuns.length} kill runs counted, ${discarded.filter((attempt) => !attempt.error).length} attempt(s) did not count and ${discarded.filter((attempt) => attempt.error).length} failed to run; each was drawn again. At the kill a median of ${median(killRuns.map((run) => run.facts.openAtKill))} holds were open (fewest ${Math.min(...killRuns.map((run) => run.facts.openAtKill))}) and a median of ${median(killRuns.map((run) => run.facts.inFlightAtKill))} streams were in flight as the driver saw them. A restarted server took a median of ${median(killRuns.map((run) => run.facts.restartMs))} ms from its start to its first answer, at most ${Math.max(...killRuns.map((run) => run.facts.restartMs))} ms. Settles that arrived after the expiry in kill runs: ${sum(killRuns, "lateSettles")}.`,
    `  What the kill found in storage: in ${killRuns.filter((run) => run.facts.settledBeforeKill > 0).length} of the ${killRuns.length} kill runs at least one settle was stored before the kill (${sum(killRuns, "settledBeforeKill")} settles in all), and in ${killRuns.filter((run) => run.facts.admittedAfterRestart > 0).length} runs the restarted ledger admitted again (${sum(killRuns, "admittedAfterRestart")} holds), against totals it read from storage.`,
  );
}
out.push(
  `- Running time of the matrix: ${clock(matrixMs)} for ${matrix.length} runs, up to ${cfg.jobs} runs at once at levels 1 and 20 and one at a time at 200. The counted attempts add up to ${clock(matrix.reduce((total, run) => total + run.ms, 0))}. A server took a median of ${median(matrix.filter(good).map((run) => run.facts.startMs))} ms to answer after its start. The machine's load average over the matrix was ${Math.min(...matrix.filter(good).map((run) => run.facts.load))} to ${Math.max(...matrix.filter(good).map((run) => run.facts.load))}; other work was running on this Mac, so the time is what this execution took.`,
  "",
);

out.push("## Control", "");
if (!withControl || controlList.length === 0) {
  out.push("The control did not run.", "");
} else {
  out.push(
    "The same server with `ADMISSION:ignores-open-holds`: admission counts settled and expired spend and leaves open holds out. The unit requires this variant to miss the rule at 20 and at 200.",
    "",
    "| cell | runs | runs that broke the cap or the wave's count | clauses broken | worst counted spend minus cap |",
    "|---|---|---|---|---|",
    ...controlSummary.map((entry) => `| ${entry.cell.name} | ${entry.mine.filter(good).length} | ${entry.mine.filter(breaksCap).length} | ${clauseCounts(entry.mine) || "-"} | ${most(entry.mine.filter(good), "worstOver") ?? "-"} |`),
    "",
    controlCounts ? "The control missed the rule at 20 and at 200, so the result counts." : "The control did NOT miss the rule at 20 and at 200: the result does not count.",
    "",
  );
}

const broken = [...matrix, ...rowSizeRuns].flatMap((run) => [
  ...run.violations.map((violation) => ({ key: run.key, ...violation })),
  ...run.discarded.flatMap((attempt) => attempt.violations.map((violation) => ({ key: `${attempt.key} (not counted)`, ...violation }))),
]);
const failed = [...matrix, ...control, ...rowSizeRuns].flatMap((run) => [run, ...run.discarded]).filter((attempt) => attempt.error);
if (broken.length > 0 || failed.length > 0) {
  out.push("## Violations", "");
  for (const [clause, text] of Object.entries(CLAUSES)) {
    const mine = broken.filter((violation) => violation.clause === clause);
    if (mine.length === 0) continue;
    out.push(`- ${clause} ("${text}"): ${mine.length}. First ${Math.min(5, mine.length)}:`);
    for (const violation of mine.slice(0, 5)) out.push(`  - ${violation.key}${violation.id ? `, request ${violation.id}` : ""}: ${violation.detail}`);
  }
  for (const run of failed) out.push(`- ${run.key} failed to run: ${run.error.split("\n")[0]}`);
  out.push("");
}

out.push(
  "## Reproduce",
  "",
  "```sh",
  "sh scripts/prerequisites.sh    # once; the only step that uses the network",
  "node spikes/s1/run.mjs         # the matrix, the control and the row-size run; rewrites this file",
  "```",
  "",
  "Run one at a time per checkout: a run first stops the servers that an interrupted run left behind. It exits 0 when the rule is met and 1 when it is missed or the run is incomplete. Each run's records stay under `.tmp/s1/STAMP/runs/KEY/` (the driver's requests, the mock's log, the ledger's rows, the server log, the state directory); `results.json` beside them holds every run's verdict and seed. `--cells REGEX`, `--runs N`, `--jobs N` and `--no-control` make a partial run, which prints its summary and leaves this file alone.",
  "",
);

const text = out.join("\n").replace("%RULE%", ruleSectionOfResult());
if (partial) {
  process.stdout.write(text);
} else {
  writeFileSync(RESULT, text);
  console.log(line);
}
console.error(`s1: records in ${outDir}`);
if (failed.length > 0) console.error(`s1: ${failed.length} attempt(s) failed to run; they are listed under Violations`);
process.exit(met ? 0 : 1);
