import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import * as driver from "../../../stress/driver.mjs";
import { judge } from "../../../stress/rule.mjs";
import { testEntryApi } from "../../../stress/targets.mjs";

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
  return { ...input, chatStatus: testEntryApi.chatStatus, ledger: testEntryApi.ledger };
}

const pairs = (violations) => violations.map((violation) => `${violation.clause} ${violation.id ?? "-"}`).sort();

function readJudge(record) {
  const verdict = judge(judgeInput(record));
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

const READINGS = { judge: readJudge, send: readSend };

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
    result = reading ? reading(record) : { good: null, problems: [`the kind ${record.kind} has no reading`] };
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
