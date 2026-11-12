import { spawnSync } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { existsSync, mkdtempSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { readTables, searchBuyerValues } from "./buyer-search.mjs";

const ROOT = process.env.HALVO_ROOT;
const STACK_STATE = process.env.STACK_STATE || join(ROOT, ".stack");
const STATE = process.env.GATE_STATE || join(ROOT, ".tmp");
const CALL_TIMEOUT_MS = 30_000;
const STALE_SECONDS = 330;
const CAP = { amount: 500, period: "month" };
const ORDER = [
  "active",
  "accounts-before-payment",
  "delete-refused",
  "rejected",
  "redelivered",
  "reordered",
  "credential-mismatch",
  "expired-removed",
  "dropped-at-bound",
  "account-from-read",
  "no-platform-alarm",
  "start-refused",
  "deleted-after-cancel",
  "unpaid-completed",
  "pii-in-event",
  "pii-in-storage",
];
const WANT = {
  active: "1",
  "accounts-before-payment": "0",
  "delete-refused": "1",
  rejected: "5 of 5",
  redelivered: "8 of 8",
  reordered: "4 of 4",
  "credential-mismatch": "1",
  "expired-removed": "1",
  "dropped-at-bound": "1",
  "account-from-read": "1",
  "no-platform-alarm": "1",
  "start-refused": "1",
  "deleted-after-cancel": "1",
  "unpaid-completed": "1",
  "pii-in-event": "7",
  "pii-in-storage": "0",
};
const TESTS = [
  ["test/billing/accounts-bounds.node.test.ts", "start answers unavailable after START_RESTARTS restarts on stale reads"],
  ["test/billing/accounts-bounds.node.test.ts", "a processed event id is gone after EVENT_ID_KEEP_MS and a redelivery changes nothing"],
  ["test/billing/accounts-bounds.node.test.ts", "a read that answers unknown at the first bound removes the pending checkout"],
  [
    "test/billing/accounts-bounds.node.test.ts",
    "a read that gives no answer keeps the pending checkout at the first bound and removes it at the second",
  ],
  ["test/billing/accounts-bounds.node.test.ts", "a read that says open after reads that gave no answer removes the pending checkout at once"],
  ["test/billing/accounts-bounds.node.test.ts", "complete with no-payment-required makes the account in the state payment-failed"],
  ["test/billing/stripe-read-session.node.test.ts", "a read of a session tells unknown from failed"],
  ["test/billing/provider-fake-unknown.node.test.ts", "the fake answers a session it does not hold so that the adapter reads unknown"],
];
const REJECTION_CASES = ["unsigned", "signed with another secret", "tampered", "re-serialized", "stale"];
const PAIRS = [
  "the completed event with the subscription event of the same payment",
  "active with a later payment failed",
  "payment failed with a later cancelled",
  "payment failed with active of the same second",
];
const PAIR_ENDS = ["active", "payment-failed", "cancelled", "active"];
const BUYER_KEYS = ["name", "email", "phone", "line1", "line2", "city", "postal_code"];
const CREATE_CALL = "POST /v1/checkout/sessions";

const STARTED = Date.now();
const tokens = Object.fromEntries(ORDER.map((name) => [name, "unmeasured"]));
const counts = { rejected: null, redelivered: null, reordered: null };
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

function stackUrl(name) {
  const result = spawnSync("sh", [join(ROOT, "scripts/stack.sh"), "url", name], { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

function localValues() {
  const values = {};
  for (const line of readFileSync(join(ROOT, "stack.d/vars.env"), "utf8").split("\n")) {
    const match = /^([A-Z][A-Z0-9_]*)=([A-Za-z0-9_]+)$/.exec(line);
    if (match) values[match[1]] = match[2];
  }
  return values;
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

function newAccount() {
  return { id: randomUUID(), credential: randomBytes(32).toString("hex") };
}

function appHeaders(account, credential = account.credential) {
  return { "halvo-account": account.id, authorization: `Bearer ${credential}` };
}

async function checkout(entry, account) {
  return http(entry, "POST", "/v1/checkout", {
    headers: { ...appHeaders(account), "content-type": "application/json" },
    body: JSON.stringify({ cap: CAP }),
  });
}

async function status(entry, account, credential) {
  return http(entry, "GET", "/v1/subscription", { headers: appHeaders(account, credential) });
}

async function remove(entry, account) {
  return http(entry, "DELETE", "/v1/account", { headers: appHeaders(account) });
}

function resultOf(answer) {
  return answer.json && typeof answer.json.result === "string" ? answer.json.result : `status ${answer.status}`;
}

function expectResult(answer, want, what) {
  if (answer.status !== 200 || resultOf(answer) !== want) {
    stop(`${what} answered ${resultOf(answer)} (status ${answer.status}), not ${want}`);
  }
  return answer.json;
}

let FAKE;
let WORKER;
let WORKER_TEST;

async function fake(path, body) {
  const answer = await http(FAKE, "POST", path, { headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  if (answer.status !== 200 || answer.json === null) stop(`the fake's ${path} answered status ${answer.status}: ${answer.text.slice(0, 200)}`);
  return answer.json;
}

async function fakeGet(path) {
  const answer = await http(FAKE, "GET", path);
  if (answer.status !== 200 || answer.json === null) stop(`the fake's ${path} answered status ${answer.status}`);
  return answer.json;
}

async function fakeResult(path, body, want) {
  const answer = await fake(path, body);
  if (answer.result !== want) stop(`the fake's ${path} answered ${answer.result}, not ${want}`);
  return answer;
}

async function sessionOf(url, what) {
  const found = await fake("/test/lookup", { url });
  if (found.result !== "session") stop(`the fake does not know the checkout URL of ${what}`);
  return found.session;
}

async function event(id) {
  const found = await fakeGet(`/test/events/${encodeURIComponent(id)}`);
  if (found.result !== "event") stop(`the fake does not know the event ${id}`);
  return found;
}

async function requestCount() {
  return (await fakeGet("/test/requests")).requests.length;
}

async function readsSince(count, sessionId) {
  const list = (await fakeGet("/test/requests")).requests.slice(count);
  return list.filter((each) => each.method === "GET" && each.path === `/v1/checkout/sessions/${sessionId}`).length;
}

function allDelivered(answer, what) {
  const bad = (answer.deliveries ?? []).filter((each) => each.status !== 200);
  if (bad.length > 0) stop(`${what}: the Worker answered ${bad.map((each) => each.status).join(", ")} to a delivery`);
}

function sqliteFiles(dir) {
  const found = [];
  if (!existsSync(dir)) return found;
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const info = statSync(path);
    if (info.isDirectory()) found.push(...sqliteFiles(path));
    else if (name.endsWith(".sqlite")) found.push(path);
  }
  return found;
}

function isOwnTable(name) {
  return !name.startsWith("sqlite_") && !name.startsWith("_cf_") && name !== "__miniflare_do_name";
}

function touchedSinceStart(path) {
  const times = [path, `${path}-wal`, `${path}-shm`].filter(existsSync).map((each) => statSync(each).mtimeMs);
  return Math.max(...times) >= STARTED - 2_000;
}

function readDatabase(path) {
  return readTables(path, STATE, (name) => isOwnTable(name) || name === "__miniflare_do_name");
}

function normalized(tables) {
  const out = {};
  for (const name of Object.keys(tables).filter(isOwnTable).sort()) {
    out[name] = tables[name]
      .map((row) => {
        const copy = { ...row };
        if (name === "skeleton") delete copy.last_now;
        return JSON.stringify(Object.fromEntries(Object.entries(copy).sort(([a], [b]) => (a < b ? -1 : 1))));
      })
      .sort();
  }
  return out;
}

function holdsId(tables, id) {
  for (const [name, rows] of Object.entries(tables)) {
    if (!isOwnTable(name)) continue;
    for (const row of rows) if (Object.values(row).some((value) => value === id)) return true;
  }
  return false;
}

function fileState(id) {
  const holders = [];
  for (const path of sqliteFiles(join(STACK_STATE, "worker")).filter(touchedSinceStart)) {
    const tables = readDatabase(path);
    const names = tables.__miniflare_do_name;
    if (names !== undefined) {
      const good = names.length === 1 && Object.keys(names[0]).sort().join(",") === "id,name";
      if (!good) stop(`${path} holds a table __miniflare_do_name with other rows than its one row of id and name`);
    }
    if (holdsId(tables, id)) holders.push({ path, tables });
  }
  if (holders.length === 0) return { holds: false, tables: {} };
  if (holders.length > 1) stop(`two object files hold the account id ${id}`);
  return { holds: true, tables: normalized(holders[0].tables), raw: holders[0].tables, path: holders[0].path };
}

function ownTablesIn(path) {
  if (!existsSync(path)) return [];
  return Object.keys(readDatabase(path)).filter(isOwnTable).sort();
}

async function inspectState(id) {
  const answer = await http(WORKER_TEST, "POST", "/test/inspect", {
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ account: id }),
  });
  if (answer.status !== 200 || answer.json === null) stop(`POST /test/inspect answered status ${answer.status}`);
  if (answer.json.result === "no-such-account") return { holds: false, tables: {}, snapshot: answer.json };
  if (answer.json.result !== "snapshot") stop(`POST /test/inspect answered ${answer.json.result}`);
  return { holds: true, tables: normalized(answer.json.tables), raw: answer.json.tables, snapshot: answer.json };
}

function rowsOf(state, table) {
  return state.raw?.[table] ?? [];
}

function same(a, b) {
  return JSON.stringify(a.tables) === JSON.stringify(b.tables) && a.holds === b.holds;
}

function processedIds(state) {
  return rowsOf(state, "processed_event").map((row) => row.event_id);
}

function accountRow(state) {
  const rows = rowsOf(state, "account");
  return rows.length === 1 ? rows[0] : null;
}

function comparableAccount(row) {
  if (row === null) return null;
  const { account_id, credential_hash, customer_id, subscription_id, ...rest } = row;
  return JSON.stringify(Object.fromEntries(Object.entries(rest).sort(([a], [b]) => (a < b ? -1 : 1))));
}

async function startAndFind(entry, account, what) {
  const started = expectResult(await checkout(entry, account), "session", `checkout start for ${what}`);
  return { url: started.url, session: await sessionOf(started.url, what) };
}

async function redeliver(eventId, kind, readState) {
  const before = await readState();
  const answer = await fake("/test/deliver", { events: [eventId] });
  const after = await readState();
  const ok = answer.result === "delivered" && (answer.deliveries ?? []).every((each) => each.status === 200) && same(before, after);
  if (ok) counts.redelivered.ok += 1;
  else fail(`a second delivery of ${kind} did not get 200 or changed the object`);
  note(`redelivered ${kind}: ${ok ? "same" : "changed"}`);
}

function expectProcessed(state, eventId, kind) {
  if (!processedIds(state).includes(eventId)) fail(`after the first delivery of ${kind} the processed-event list does not hold its id`);
}

async function createCall(count) {
  const calls = (await fakeGet("/test/requests")).requests.slice(count).filter((each) => `${each.method} ${each.path}` === CREATE_CALL);
  if (calls.length !== 1) stop(`the fake recorded ${calls.length} create calls for the checkout start of id 1, not 1`);
  const [call] = calls;
  if (JSON.stringify(call.params) !== JSON.stringify(params[CREATE_CALL])) {
    stop(`the create call of id 1 sent the names ${call.params.join(" ")}, not those of test/billing/stripe-params.json; the fake answered ${call.fake_status}`);
  }
  if (call.fake_status !== 200) stop(`the fake refused the create call of id 1 with status ${call.fake_status}`);
}

async function idOne(secret) {
  const account = newAccount();
  const count = await requestCount();
  const started = await checkout(WORKER, account);
  await createCall(count);
  const session = await sessionOf(expectResult(started, "session", "checkout start for id 1").url, "id 1");
  const pending = fileState(account.id);
  tokens["accounts-before-payment"] = String(rowsOf(pending, "account").length);
  if (rowsOf(pending, "pending_checkout").length !== 1) fail("after checkout start the object of id 1 holds no pending checkout");
  const refused = await remove(WORKER, account);
  const afterDelete = fileState(account.id);
  const deleteRefused = resultOf(refused) === "pending" && rowsOf(afterDelete, "pending_checkout").length === 1;
  tokens["delete-refused"] = deleteRefused ? "1" : "0";
  if (!deleteRefused) fail(`an account delete during the pending checkout of id 1 answered ${resultOf(refused)} or removed it`);

  await fakeResult("/test/hold", { session }, "held");
  const paid = await fakeResult("/test/pay", { session }, "paid");
  const [completedId, createdId, invoicePaidId] = paid.events;
  const completed = await event(completedId);
  const body = completed.body;
  tokens["pii-in-event"] = String(BUYER_KEYS.filter((key) => body.includes(buyer[key])).length);

  counts.rejected = { ok: 0, of: REJECTION_CASES.length };
  const accepted = [];
  const now = Math.floor(Date.now() / 1000);
  const tampered = body.replace(/"created": (\d*)(\d)/, (_, head, last) => `"created": ${head}${(Number(last) + 1) % 10}`);
  const cases = [
    [REJECTION_CASES[0], body, {}],
    [REJECTION_CASES[1], body, { "stripe-signature": sign(`${secret}_other`, body, now) }],
    [REJECTION_CASES[2], tampered, { "stripe-signature": sign(secret, body, now) }],
    [REJECTION_CASES[3], JSON.stringify(JSON.parse(body)), { "stripe-signature": sign(secret, body, now) }],
    [REJECTION_CASES[4], body, { "stripe-signature": sign(secret, body, now - STALE_SECONDS) }],
  ];
  for (const [name, sent, headers] of cases) {
    const before = fileState(account.id);
    const answer = await http(WORKER, "POST", "/v1/webhook", { headers: { "content-type": "application/json", ...headers }, body: sent });
    const after = fileState(account.id);
    const ok = answer.status === 400 && same(before, after);
    note(`rejection case ${name}: status ${answer.status}, state ${same(before, after) ? "same" : "changed"}`);
    if (ok) counts.rejected.ok += 1;
    else accepted.push(name);
  }
  if (accepted.length > 0) {
    fail(`the webhook route did not refuse ${accepted.length} of the 5 rejection cases with 400 and no change: ${accepted.join(", ")}`);
  }

  const released = await fakeResult("/test/release", { session }, "released");
  allDelivered(released, "the release of id 1");
  const active = fileState(account.id);
  const answer = await status(WORKER, account);
  const isActive = resultOf(answer) === "active" && accountRow(active) !== null && rowsOf(active, "pending_checkout").length === 0;
  tokens.active = isActive ? "1" : "0";
  if (!isActive) fail(`after the release status of id 1 answered ${resultOf(answer)} or the object holds no account`);
  expectProcessed(active, completedId, "the completed event (paid)");
  expectProcessed(active, createdId, "the subscription created event");

  const other = randomBytes(32).toString("hex");
  const mismatch = await status(WORKER, account, other);
  const afterMismatch = fileState(account.id);
  const mismatched = resultOf(mismatch) === "credential-mismatch" && same(active, afterMismatch);
  tokens["credential-mismatch"] = mismatched ? "1" : "0";
  if (!mismatched) fail(`status with another credential for id 1 answered ${resultOf(mismatch)} or changed the object`);

  const read = () => fileState(account.id);
  await redeliver(completedId, "checkout.session.completed (paid)", read);
  await redeliver(createdId, "customer.subscription.created", read);
  await redeliver(invoicePaidId, "invoice.paid", read);

  const failed = await fakeResult("/test/fail-payment", { subscription: paid.subscription }, "payment-failed");
  const [paymentFailedId, updatedId] = failed.events;
  expectProcessed(fileState(account.id), updatedId, "the subscription updated event");
  await redeliver(paymentFailedId, "invoice.payment_failed", read);
  await redeliver(updatedId, "customer.subscription.updated", read);
}

async function idFive() {
  const account = newAccount();
  const { session } = await startAndFind(WORKER, account, "id 5");
  const done = await fakeResult("/test/complete-unpaid", { session }, "completed");
  const state = fileState(account.id);
  const row = accountRow(state);
  const unpaid = row !== null && row.state === "payment-failed" && rowsOf(state, "pending_checkout").length === 0;
  tokens["unpaid-completed"] = unpaid ? "1" : "0";
  if (!unpaid) fail("after a completion as unpaid the object of id 5 does not hold the account in the state payment-failed alone");
  expectProcessed(state, done.events[0], "the completed event (unpaid)");
  await redeliver(done.events[0], "checkout.session.completed (unpaid)", () => fileState(account.id));
}

async function idFour() {
  const account = newAccount();
  const { session } = await startAndFind(WORKER, account, "id 4");
  const paid = await fakeResult("/test/pay", { session }, "paid");
  const cancelled = await fakeResult("/test/cancel", { subscription: paid.subscription }, "cancelled");
  const state = fileState(account.id);
  if (accountRow(state)?.state !== "cancelled") fail("after the fake's cancel the account of id 4 is not cancelled");
  expectProcessed(state, cancelled.events[0], "the subscription deleted event");
  await redeliver(cancelled.events[0], "customer.subscription.deleted", () => fileState(account.id));
  const before = fileState(account.id);
  const again = await checkout(WORKER, account);
  const after = fileState(account.id);
  const refused = resultOf(again) === "account-exists" && same(before, after);
  tokens["start-refused"] = refused ? "1" : "0";
  if (!refused) fail(`a new start for the cancelled account of id 4 answered ${resultOf(again)} or changed the object`);
  const removed = await remove(WORKER, account);
  const gone = fileState(account.id);
  const left = after.holds ? ownTablesIn(after.path) : null;
  const deleted = resultOf(removed) === "deleted" && !gone.holds && left !== null && left.length === 0;
  tokens["deleted-after-cancel"] = deleted ? "1" : "0";
  if (left !== null && left.length > 0) fail(`after the account delete of id 4 the object file that held its id still holds the tables ${left.join(", ")}`);
  else if (!deleted) fail(`the account delete of id 4 answered ${resultOf(removed)} or left state behind`);
}

function pendingExpiry(state, what) {
  const row = rowsOf(state, "pending_checkout")[0];
  if (row === undefined) stop(`the snapshot of ${what} holds no pending checkout`);
  return Number(row.expires_at);
}

async function clock(account, time) {
  const answer = await http(WORKER_TEST, "POST", "/test/clock", {
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ account: account.id, time }),
  });
  if (answer.json?.result !== "clock-set") stop(`POST /test/clock answered ${resultOf(answer)}`);
}

async function idTwo() {
  const account = newAccount();
  const { session } = await startAndFind(WORKER_TEST, account, "id 2");
  const expired = await fakeResult("/test/expire", { session }, "expired");
  allDelivered(expired, "the expiry of id 2");
  await redeliver(expired.events[0], "checkout.session.expired", () => inspectState(account.id));
  const expiry = pendingExpiry(await inspectState(account.id), "id 2");
  await clock(account, expiry + 1);
  const gone = !(await inspectState(account.id)).holds;
  tokens["expired-removed"] = gone ? "1" : "0";
  if (!gone) fail("after the clock passed the expiry of the expired session of id 2 the object still holds state");
}

async function idSix(dropMs) {
  const account = newAccount();
  const { session } = await startAndFind(WORKER_TEST, account, "id 6");
  const expiry = pendingExpiry(await inspectState(account.id), "id 6");
  const count = await requestCount();
  await clock(account, expiry + dropMs);
  const reads = await readsSince(count, session);
  const gone = !(await inspectState(account.id)).holds;
  tokens["dropped-at-bound"] = gone && reads === 1 ? "1" : "0";
  if (tokens["dropped-at-bound"] !== "1") fail(`at the first bound the object of id 6 still holds state, or the fake saw ${reads} reads of its session, not 1`);
}

async function idThree(dropMs) {
  const account = newAccount();
  const { session } = await startAndFind(WORKER_TEST, account, "id 3");
  await fakeResult("/test/hold", { session }, "held");
  const paid = await fakeResult("/test/pay", { session }, "paid");
  const expiry = pendingExpiry(await inspectState(account.id), "id 3");
  const count = await requestCount();
  await clock(account, expiry + dropMs);
  const reads = await readsSince(count, session);
  const made = await inspectState(account.id);
  const row = accountRow(made);
  const delivered = (await Promise.all(paid.events.map(event))).reduce((sum, each) => sum + each.deliveries.length, 0);
  const statusAnswer = await status(WORKER_TEST, account);
  const fromRead =
    reads >= 1 && row !== null && rowsOf(made, "pending_checkout").length === 0 && delivered === 0 && resultOf(statusAnswer) === "active";
  if (!fromRead) fail(`at the first bound a read of the paid session of id 3 (reads ${reads}, events delivered ${delivered}) did not make the active account`);
  const released = await fakeResult("/test/release", { session }, "released");
  allDelivered(released, "the release of id 3");
  const fakeSession = await fakeGet(`/test/sessions/${encodeURIComponent(session)}`);
  const after = await inspectState(account.id);
  const afterRow = accountRow(after);
  const statusAfter = await status(WORKER_TEST, account);
  const ids = processedIds(after);
  const kept =
    resultOf(statusAfter) === "active" &&
    rowsOf(after, "account").length === 1 &&
    afterRow?.customer_id === fakeSession.customer &&
    afterRow?.subscription_id === fakeSession.subscription &&
    ids.includes(paid.events[0]) &&
    ids.includes(paid.events[1]);
  tokens["account-from-read"] = fromRead && kept ? "1" : "0";
  if (fromRead && !kept) fail("after the release of id 3 the account changed, or the processed-event list lacks the completed or the subscription event");
  const noAlarm = after.snapshot.due !== null && after.snapshot.alarm === null;
  tokens["no-platform-alarm"] = noAlarm ? "1" : "0";
  if (!noAlarm) fail(`the snapshot of id 3 under a test time shows due ${after.snapshot.due} and alarm ${after.snapshot.alarm}`);
}

async function pairIds(created, setup, orders) {
  const ends = [];
  for (const order of orders) {
    const account = newAccount();
    const { session } = await startAndFind(WORKER, account, "a reordered pair");
    await fakeResult("/test/hold", { session }, "held");
    const events = await setup(session, created);
    const answer = await fake("/test/deliver", { events: order(events) });
    if (answer.result !== "delivered") stop("the fake did not deliver a reordered pair");
    allDelivered(answer, "a reordered pair");
    const state = fileState(account.id);
    const row = accountRow(state);
    const fakeSession = await fakeGet(`/test/sessions/${encodeURIComponent(session)}`);
    const ids = row !== null && row.customer_id === fakeSession.customer && row.subscription_id === fakeSession.subscription;
    ends.push({ row: comparableAccount(row), pending: rowsOf(state, "pending_checkout").length, ids });
  }
  return ends;
}

async function reordered() {
  counts.reordered = { ok: 0, of: PAIRS.length };
  const created = Math.floor(Date.now() / 1000) - 3600;
  const pay = async (session, at) => {
    const paid = await fakeResult("/test/pay", { session, created: at }, "paid");
    return { completed: paid.events[0], created: paid.events[1], subscription: paid.subscription };
  };
  const pairs = [
    [
      async (session, at) => pay(session, at),
      [(e) => [e.completed, e.created], (e) => [e.created, e.completed]],
    ],
    [
      async (session, at) => {
        const paid = await pay(session, at);
        const failed = await fakeResult("/test/fail-payment", { subscription: paid.subscription, created: at + 1 }, "payment-failed");
        return { ...paid, updated: failed.events[1] };
      },
      [(e) => [e.completed, e.created, e.updated], (e) => [e.completed, e.updated, e.created]],
    ],
    [
      async (session, at) => {
        const paid = await pay(session, at);
        const failed = await fakeResult("/test/fail-payment", { subscription: paid.subscription, created: at + 1 }, "payment-failed");
        const ended = await fakeResult("/test/cancel", { subscription: paid.subscription, created: at + 2 }, "cancelled");
        return { ...paid, updated: failed.events[1], deleted: ended.events[0] };
      },
      [(e) => [e.completed, e.updated, e.deleted], (e) => [e.completed, e.deleted, e.updated]],
    ],
    [
      async (session, at) => {
        const paid = await pay(session, at);
        const failed = await fakeResult("/test/fail-payment", { subscription: paid.subscription, created: at + 1 }, "payment-failed");
        const back = await fakeResult("/test/recover", { subscription: paid.subscription, created: at + 1 }, "recovered");
        return { ...paid, updated: failed.events[1], recovered: back.events[1] };
      },
      [(e) => [e.completed, e.updated, e.recovered], (e) => [e.completed, e.recovered, e.updated]],
    ],
  ];
  for (const [index, [setup, orders]] of pairs.entries()) {
    const [first, second] = await pairIds(created, setup, orders);
    const ok =
      first.row !== null &&
      first.row === second.row &&
      JSON.parse(first.row).state === PAIR_ENDS[index] &&
      first.pending === 0 &&
      second.pending === 0 &&
      first.ids &&
      second.ids;
    note(`reordered pair ${index + 1}: ${first.row} / ${second.row}; customer and subscription ids as the fake session ${first.ids} / ${second.ids}`);
    if (ok) counts.reordered.ok += 1;
    else fail(`the pair ${PAIRS[index]} does not end in one account row in the state ${PAIR_ENDS[index]}, with the customer id and the subscription id of the fake session, in both orders`);
  }
}

function piiInStorage() {
  const result = searchBuyerValues(
    [join(STACK_STATE, "worker"), join(STACK_STATE, "worker-test")],
    BUYER_KEYS.map((key) => buyer[key]),
    STATE,
  );
  for (const text of result.lines) note(text);
  return result.count;
}

function runTests() {
  const missing = TESTS.map(([file]) => file).filter((file, index, all) => all.indexOf(file) === index && !existsSync(join(ROOT, file)));
  if (missing.length > 0) {
    fail(`the test file ${missing[0]} is missing`);
    return;
  }
  const dir = mkdtempSync(join(STATE, "subscription-tests."));
  const report = join(dir, "vitest.json");
  const files = [...new Set(TESTS.map(([file]) => file))];
  const run = spawnSync(
    join(ROOT, "node_modules/.bin/vitest"),
    ["run", "--root", ROOT, "--project", "node", "--reporter=json", `--outputFile=${report}`, ...files],
    { cwd: ROOT, encoding: "utf8", env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME } },
  );
  note(`vitest exit ${run.status}; its report: ${report}`);
  if (!existsSync(report)) {
    fail(`vitest wrote no report for the eight tests (exit ${run.status})`);
    return;
  }
  const results = JSON.parse(readFileSync(report, "utf8")).testResults ?? [];
  for (const [file, title] of TESTS) {
    const fileResult = results.find((each) => each.name === join(ROOT, file));
    const test = fileResult?.assertionResults?.find((each) => each.title === title);
    note(`test ${title}: ${test?.status ?? "missing"}`);
    if (test === undefined) fail(`the test "${title}" is missing from ${file}`);
    else if (test.status !== "passed") fail(`the test "${title}" of ${file} ${test.status}`);
  }
}

function line() {
  if (counts.rejected) tokens.rejected = `${counts.rejected.ok} of ${counts.rejected.of}`;
  if (counts.redelivered) tokens.redelivered = `${counts.redelivered.ok} of ${counts.redelivered.of}`;
  if (counts.reordered) tokens.reordered = `${counts.reordered.ok} of ${counts.reordered.of}`;
  for (const name of ORDER) {
    if (tokens[name] !== WANT[name]) fail(`the token ${name} is ${tokens[name]}, not ${WANT[name]}`);
  }
  const text = ORDER.map((name) => `${name}=${tokens[name]}`).join(" ");
  if (firstFailure === null) return { pass: true, text: `PASS subscription: ${text}` };
  return { pass: false, text: `FAIL subscription: ${text}; first failure: ${firstFailure}` };
}

async function step(name, run) {
  try {
    await run();
  } catch (error) {
    if (!(error instanceof StepFailed)) fail(`the step ${name} threw ${error?.name ?? "an error"}: ${String(error?.message ?? error).split("\n")[0]}`);
  }
}

let sign;
let buyer;
let params;

async function main() {
  WORKER = stackUrl("worker");
  WORKER_TEST = stackUrl("worker-test");
  FAKE = stackUrl("provider-fake");
  if (FAKE === null) {
    fail("the stack has no member provider-fake, which unit 3.3 adds");
    return;
  }
  for (const [name, url] of [
    ["worker", WORKER],
    ["worker-test", WORKER_TEST],
    ["provider-fake", FAKE],
  ]) {
    const answer = await http(url, "GET", "/");
    if (answer.status === 0) {
      fail(`the stack member ${name} does not answer at ${url}: run sh scripts/stack.sh up`);
      return;
    }
  }
  try {
    sign = (await import(join(ROOT, "test-kit/provider-fake/sign.mjs"))).sign;
    buyer = JSON.parse(readFileSync(join(ROOT, "test-kit/provider-fake/buyer.json"), "utf8"));
    params = JSON.parse(readFileSync(join(ROOT, "test/billing/stripe-params.json"), "utf8"));
  } catch (error) {
    fail(`the fake's sign.mjs or buyer.json, or test/billing/stripe-params.json, cannot be read: ${String(error?.message ?? error).split("\n")[0]}`);
    return;
  }
  const { PENDING_DROP_MS } = await import(join(ROOT, "src/accounts/index.ts"));
  const secret = localValues().WEBHOOK_SIGNING_SECRET;
  counts.redelivered = { ok: 0, of: 8 };
  await step("id 1", () => idOne(secret));
  await step("id 5", idFive);
  await step("id 4", idFour);
  await step("id 2", idTwo);
  await step("id 6", () => idSix(PENDING_DROP_MS));
  await step("id 3", () => idThree(PENDING_DROP_MS));
  await step("reordered", reordered);
  await step("pii-in-storage", async () => {
    tokens["pii-in-storage"] = String(piiInStorage());
  });
  runTests();
}

await main().catch((error) => fail(`the check threw ${error?.name}: ${String(error?.message ?? error).split("\n")[0]}`));
const result = line();
console.log(result.text);
process.exit(result.pass ? 0 : 1);
