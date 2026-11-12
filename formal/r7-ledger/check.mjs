import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { missingTool, OUTPUT_LIMIT, ROOT, scratch, tlc, WORKERS } from "./tlc.mjs";

const BAD_USAGE = 64;
const ARGUMENTS = process.argv.slice(2);
const ONLY_THE_REPLAY = ARGUMENTS[0] === "--replay-only" && ARGUMENTS.length <= 2;
const GIVEN_TRACES = ONLY_THE_REPLAY ? (ARGUMENTS[1] ?? "") : "";
const SPEC_FILES = ["CapLedger.tla", "CapLedger.cfg", "CapLedgerGateway.cfg", "CapLedgerTrace.tla", "CapLedgerTrace.cfg"];
const COUNTS = "counts.json";
const CONFIGURATIONS = [
  { config: "CapLedger.cfg", run: "check", token: "states" },
  { config: "CapLedgerGateway.cfg", run: "check-gateway", token: "gateway-states" },
];
const REPLAY = join("test", "ledger", "model-replay.mjs");
const NAMED = [
  "INVARIANT TypeOK",
  "INVARIANT StateA",
  "INVARIANT StateB",
  "INVARIANT StateF",
  "INVARIANT StateG",
  "PROPERTY StepA",
  "PROPERTY StepB",
  "PROPERTY StepC",
  "PROPERTY StepD",
  "PROPERTY StepE",
  "PROPERTY StepF",
  "PROPERTY StepG",
  "PROPERTY StepH",
];
const letterOf = (name) => (/^(?:State|Step)[A-H]$/.test(name) ? `(${name.slice(-1).toLowerCase()})` : name);

const tokens = {
  states: "unmeasured",
  "gateway-states": "unmeasured",
  properties: "unmeasured",
  violated: "unmeasured",
  seed: "unmeasured",
  traces: "unmeasured",
  actions: "unmeasured",
  differences: "unmeasured",
  elements: "unmeasured",
};
const line = () =>
  Object.entries(tokens)
    .map(([name, value]) => `${name}=${value}`)
    .join(" ");

class Stop extends Error {}
const stop = (sentence) => {
  throw new Stop(sentence);
};

function checkTheProperties(work) {
  const recorded = JSON.parse(readFileSync(join(ROOT, "formal", COUNTS), "utf8"));
  for (const { config } of CONFIGURATIONS) {
    const named = readFileSync(join(work, config), "utf8")
      .split("\n")
      .filter((each) => /^(INVARIANT|PROPERTY) /.test(each));
    tokens.properties = String(new Set(named.map((each) => /^PROPERTY Step([A-H])$/.exec(each)?.[1]).filter((each) => each !== undefined)).size);
    if (named.join(", ") !== NAMED.join(", ")) stop(`formal/${config} names ${named.join(", ") || "nothing to check"}, not ${NAMED.join(", ")}`);
  }
  for (const { config, run: name, token } of CONFIGURATIONS) {
    const run = tlc(work, name, ["-workers", WORKERS, "-config", config, "CapLedger"]);
    console.log(`model: the checker on ${config} exited ${run.status}; its output is ${run.file}`);
    if (run.distinct !== null) tokens[token] = run.distinct;
    if (run.violated.length > 0) {
      tokens.violated = run.violated.join(",");
      stop(`TLC reports that ${run.violated.map(letterOf).join(" and ")} is violated at the bounds of formal/${config}, with its trace in the output of the checker`);
    }
    if (!run.complete) stop(`TLC did not complete the check at the bounds of formal/${config} (exit ${run.status})`);
    if (tokens[token] !== String(recorded[config])) {
      stop(`TLC counted ${tokens[token]} distinct states at the bounds of formal/${config}, not the ${recorded[config]} that formal/${COUNTS} records`);
    }
  }
  tokens.violated = "none";
}

function generateTheTraces(work) {
  const run = tlc(work, "traces", ["-workers", "1", "-deadlock", "-config", "CapLedgerTrace.cfg", "CapLedgerTrace"]);
  console.log(`model: the trace walk exited ${run.status}; its output is ${run.file}`);
  const lines = run.output
    .split(/\r?\n/)
    .map((each) => /^"(\{.*\})"$/.exec(each)?.[1])
    .filter((each) => each !== undefined)
    .map((each) => each.replaceAll('\\"', '"'));
  const path = join(work, "traces.jsonl");
  writeFileSync(path, `${lines.join("\n")}\n`);
  const header = lines[0] === undefined ? {} : JSON.parse(lines[0]);
  if (run.status !== 0 || header.header !== true || lines.length !== header.count * header.depth + 1) {
    stop(`TLC did not print the traces of formal/CapLedgerTrace.tla (exit ${run.status}, ${lines.length} lines)`);
  }
  return path;
}

function replay(traces) {
  const run = spawnSync(process.execPath, [join(ROOT, REPLAY), traces], { encoding: "utf8", maxBuffer: OUTPUT_LIMIT });
  const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
  console.log(output);
  const read = /^replay: seed=(\d+) traces=(\d+) actions=(\d+) calls=\d+ differences=(\d+) elements=(\d+) of (\d+)$/m.exec(output);
  if (read === null) stop(`the replay printed no result line (exit ${run.status})`);
  const [, seed, replayed, actions, differences, seen, listed] = read;
  Object.assign(tokens, { seed, traces: replayed, actions, differences, elements: `${seen} of ${listed}` });
  if (differences !== "0" || run.status !== 0) {
    stop(`the object does not follow the model: ${/^difference: (.*)$/m.exec(output)?.[1] ?? `the replay exited ${run.status}`}`);
  }
  if (seen !== listed) stop(`the traces never reach ${/^missing: (.*)$/m.exec(output)?.[1] ?? "some element"}`);
}

function main() {
  for (const name of [...SPEC_FILES, COUNTS]) if (!existsSync(join(ROOT, "formal", name))) stop(`formal/${name} does not exist, so the checker cannot run`);
  if (!existsSync(join(ROOT, REPLAY))) stop(`${REPLAY} does not exist, so no trace can be replayed`);
  const missing = missingTool();
  if (missing !== null) stop(missing);
  const work = scratch("model");
  for (const name of SPEC_FILES) copyFileSync(join(ROOT, "formal", name), join(work, name));
  if (!ONLY_THE_REPLAY) checkTheProperties(work);
  replay(GIVEN_TRACES === "" ? generateTheTraces(work) : GIVEN_TRACES);
  if (ONLY_THE_REPLAY) stop("the two searches were left out, so this run cannot pass: it was asked for the replay alone");
}

if (ARGUMENTS.length > 0 && !ONLY_THE_REPLAY) {
  console.error(
    "usage: node formal/check.mjs [--replay-only [TRACES]]\n\nCheck 95: TLC on the two configurations of formal/, each held to the count that formal/counts.json records, then the walk and the replay.\nIt prints one line, PASS model: ... or FAIL model: ...; first failure: ...\n--replay-only leaves the two searches out and, with TRACES, replays that file in place of the traces of the walk. Such a run can fail and never passes; the self-test of the check uses it for the bad fixtures of the replay.\nExit 0 on PASS, 1 on FAIL, 64 on bad usage.",
  );
  process.exit(BAD_USAGE);
}
try {
  main();
  console.log(`PASS model: ${line()}`);
} catch (error) {
  if (!(error instanceof Stop)) throw error;
  console.log(`FAIL model: ${line()}; first failure: ${error.message}`);
  process.exit(1);
}
