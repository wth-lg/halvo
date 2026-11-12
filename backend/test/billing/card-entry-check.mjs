import { spawnSync } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.env.HALVO_ROOT;
const CALL_TIMEOUT_MS = 30_000;
const PAGE_TIMEOUT_MS = 30_000;
const NOT_MEASURED = "not measured";
const NOT_MEASURED_IDS = ["three-d-secure", "wallet-prompt"];
const PAIR_IDS = ["automatic-tax-off", "stripe-seller-stripe-tax", "managed-payments"];
const BUYER = {
  email: "card.entry@example.test",
  "card-number": "4242424242424242",
  "card-expiry": "12/34",
  "card-cvc": "123",
  "cardholder-name": "Card Entry Tester",
  country: "US",
  "postal-code": "10001",
};
const WANT = [
  ["fields", "=", "7"],
  ["docs", "=", "7"],
  ["unknown", "=", "0"],
  ["page-inputs", "=", "8 of 8"],
  ["paid", "=", "1"],
  ["automatic-tax-off", "=", "7"],
  ["stripe-seller-stripe-tax", ">=", "7"],
  ["managed-payments", ">=", "7"],
  ["three-d-secure", "=", NOT_MEASURED],
  ["wallet-prompt", "=", NOT_MEASURED],
];

const tokens = Object.fromEntries(WANT.map(([name]) => [name, { sign: "=", value: "unmeasured" }]));
let firstFailure = null;

function note(text) {
  console.log(`  ${text}`);
}

function fail(sentence) {
  note(`failure: ${sentence}`);
  if (firstFailure === null) firstFailure = sentence;
}

class StepFailed extends Error {}

function stop(sentence) {
  fail(sentence);
  throw new StepFailed(sentence);
}

function set(name, value, sign = "=") {
  tokens[name] = { sign, value: String(value) };
}

function stackUrl(name) {
  const result = spawnSync("sh", [join(ROOT, "scripts/stack.sh"), "url", name], { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

async function http(base, method, path, { headers = {}, body } = {}) {
  let response;
  try {
    response = await fetch(`${base}${path}`, { method, headers, body, signal: AbortSignal.timeout(CALL_TIMEOUT_MS) });
  } catch (error) {
    return { status: 0, text: "", json: null, error: String(error?.message ?? error) };
  }
  const text = await response.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {}
  return { status: response.status, text, json };
}

function readJson(path) {
  return JSON.parse(readFileSync(join(ROOT, path), "utf8"));
}

function sameRow(a, b) {
  return a.where === b.where && a.class === b.class && JSON.stringify(a.source) === JSON.stringify(b.source);
}

function onlyTheTwo(rows, file) {
  const ids = rows.map((each) => each.id);
  const others = ids.filter((id) => !NOT_MEASURED_IDS.includes(id));
  if (others.length > 0) fail(`${file} holds a row that is not measured beside the two: ${others.join(", ")}`);
}

function notMeasuredTokens(fields, budget) {
  const budgetRows = Array.isArray(budget.rows) ? budget.rows : [];
  const fieldRows = Array.isArray(fields.notMeasured) ? fields.notMeasured : [];
  onlyTheTwo(fieldRows, "test-kit/checkout-fields.json");
  onlyTheTwo(
    budgetRows.filter((each) => each.status === NOT_MEASURED || "id" in each),
    "gate/prompt-budget.json",
  );
  for (const id of NOT_MEASURED_IDS) {
    const copies = fieldRows.filter((each) => each.id === id);
    const otherCopies = budgetRows.filter((each) => each.id === id);
    const row = copies[0];
    const other = otherCopies[0];
    if (copies.length > 1 || otherCopies.length > 1) {
      set(id, "repeated");
      fail(`the row ${id} stands more than once in test-kit/checkout-fields.json or gate/prompt-budget.json`);
    } else if (row === undefined) {
      set(id, "missing");
      fail(`test-kit/checkout-fields.json has no not-measured row ${id}`);
    } else if (row.status !== NOT_MEASURED) {
      set(id, `status ${row.status}`);
      fail(`the row ${id} of test-kit/checkout-fields.json has the status ${row.status}, not ${NOT_MEASURED}`);
    } else if ("count" in row || "inputs" in row) {
      set(id, "counted");
      fail(`the row ${id} of test-kit/checkout-fields.json carries a count`);
    } else if (other === undefined || other.status !== NOT_MEASURED || !sameRow(row, other)) {
      set(id, "differs");
      fail(`test-kit/checkout-fields.json and gate/prompt-budget.json differ on the row ${id}`);
    } else {
      set(id, NOT_MEASURED);
    }
  }
}

async function fileTokens() {
  const fields = readJson("test-kit/checkout-fields.json");
  const budget = readJson("gate/prompt-budget.json");
  notMeasuredTokens(fields, budget);
  const kit = await import(join(ROOT, "test-kit/checkout-page/fields.mjs"));
  const session = { country: BUYER.country, emailLocked: false };
  const count = kit.countFor(fields, session);
  set("fields", count.fields.total);
  set("docs", count.fields.docs);
  set("unknown", count.fields.unknown);
  const pairs = kit.pairCounts(fields, session);
  for (const id of PAIR_IDS) {
    const pair = pairs.find((each) => each.id === id);
    if (pair === undefined) {
      set(id, "missing");
      fail(`test-kit/checkout-fields.json has no seller pair ${id}`);
    } else {
      set(id, pair.fields, pair.unstated.length > 0 ? ">=" : "=");
    }
  }
  try {
    kit.validateFields(fields);
  } catch (error) {
    fail(String(error?.message ?? error));
  }
}

async function drive(worker, fake) {
  const account = { id: randomUUID(), credential: randomBytes(32).toString("hex") };
  const headers = { "halvo-account": account.id, authorization: `Bearer ${account.credential}` };
  const started = await http(worker, "POST", "/v1/checkout", {
    headers: { ...headers, "content-type": "application/json" },
    body: JSON.stringify({ cap: { amount: 500, period: "month" } }),
  });
  if (started.json?.result !== "session") stop(`checkout start on the deployable entry answered ${started.json?.result ?? `status ${started.status}`}`);
  const url = started.json.url;
  const found = await http(fake, "POST", "/test/lookup", { headers: { "content-type": "application/json" }, body: JSON.stringify({ url }) });
  if (found.json?.result !== "session") stop("the fake does not know the checkout URL");
  const sessionId = found.json.session;
  const pageFields = await http(new URL(url).origin, "GET", "/fields.json");
  if (pageFields.json === null) stop("the stand-in page serves no fields.json");
  const kit = await import(join(ROOT, "test-kit/checkout-page/fields.mjs"));
  const rows = kit.requiredRows(pageFields.json, { country: BUYER.country, emailLocked: false });

  const { openBrowser } = await import(join(ROOT, "test-kit/browser/index.mjs"));
  const browser = await openBrowser();
  let actions = 0;
  try {
    const context = await browser.newContext();
    const page = await context.newPage();
    page.setDefaultTimeout(PAGE_TIMEOUT_MS);
    await page.goto(url);
    await page.locator("#checkout[data-ready]").waitFor();
    for (const row of rows) {
      const control = page.locator(`[data-row="${row.id}"]`);
      if (row.kind === "typed") await control.fill(BUYER[row.id] ?? stop(`the driver has no value for the row ${row.id}`));
      else if (row.kind === "selection") await control.selectOption(BUYER[row.id] ?? stop(`the driver has no value for the row ${row.id}`));
      else await control.click();
      actions += 1;
    }
    const state = await page.locator("#status[data-state]").getAttribute("data-state");
    note(`the page says ${state} after ${actions} actions`);
    await context.close();
  } finally {
    await browser.close();
  }

  const session = await http(fake, "GET", `/test/sessions/${encodeURIComponent(sessionId)}`);
  const inputs = session.json?.inputs?.count;
  set("page-inputs", `${Number.isInteger(inputs) ? inputs : "none"} of ${actions}`);
  if (inputs !== actions) fail(`the page counted ${inputs} inputs and the driver made ${actions} actions`);
  const status = await http(worker, "GET", "/v1/subscription", { headers });
  const paid = session.json?.status === "complete" && session.json?.payment_status === "paid" && status.json?.result === "active";
  set("paid", paid ? 1 : 0);
  if (!paid) {
    fail(`the session is ${session.json?.status}/${session.json?.payment_status} at the fake and status answered ${status.json?.result ?? `status ${status.status}`}`);
  }
}

function line() {
  for (const [name, sign, value] of WANT) {
    const got = tokens[name];
    if (got.sign !== sign || got.value !== value) fail(`the token ${name}${got.sign}${got.value} is not ${name}${sign}${value}`);
  }
  const text = WANT.map(([name]) => `${name}${tokens[name].sign}${tokens[name].value}`).join(" ");
  if (firstFailure === null) return { pass: true, text: `PASS card-entry: ${text}` };
  return { pass: false, text: `FAIL card-entry: ${text}; first failure: ${firstFailure}` };
}

async function main() {
  try {
    await fileTokens();
  } catch (error) {
    if (!(error instanceof StepFailed)) fail(`the field files cannot be read: ${String(error?.message ?? error).split("\n")[0]}`);
  }
  const worker = stackUrl("worker");
  const fake = stackUrl("provider-fake");
  if (fake === null) {
    fail("the stack has no member provider-fake, which unit 3.3 adds");
    return;
  }
  for (const [name, url] of [
    ["worker", worker],
    ["provider-fake", fake],
  ]) {
    if ((await http(url, "GET", "/")).status === 0) {
      fail(`the stack member ${name} does not answer at ${url}: run sh scripts/stack.sh up`);
      return;
    }
  }
  try {
    await drive(worker, fake);
  } catch (error) {
    if (!(error instanceof StepFailed)) fail(`the drive threw ${error?.name ?? "an error"}: ${String(error?.message ?? error).split("\n")[0]}`);
  }
}

await main();
const result = line();
console.log(result.text);
process.exit(result.pass ? 0 : 1);
