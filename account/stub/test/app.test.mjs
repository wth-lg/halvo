import { test } from "node:test";
import assert from "node:assert/strict";
import { makeApp } from "../src/app.mjs";
import { readVendored } from "../src/stub.mjs";
import { keysAgree } from "../src/schema.mjs";

const vendored = readVendored();
const { schema, routes, samples } = vendored;
const ID = "0f8fad5b-d9cb-469f-a165-70867728950e";
const OTHER_ID = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
const CRED = "0123456789abcdef".repeat(4);
const WRONG = "f".repeat(64);
const CAP = '{"cap":{"amount":1,"period":"week"}}';
const ROUTE = Object.fromEntries(routes.routes.map((route) => [route.name, route]));
const APP_ROUTES = routes.routes.filter((route) => route.kind === "app").map((route) => route.name);
const SESSION = samples.answers["checkout-start"].session;

function call(app, method, target, { id = ID, credential = CRED, headers, body = "", type } = {}) {
  const list = headers ?? [
    ["halvo-account", id],
    ["authorization", `Bearer ${credential}`],
  ];
  if (type !== undefined) list.push(["content-type", type]);
  return app.handle({ method, target, headers: list, body: Buffer.isBuffer(body) ? body : Buffer.from(body, "utf8") });
}

function route(app, name, options = {}) {
  const { method, path, request } = ROUTE[name];
  const extra = request === null ? {} : { body: CAP, type: "application/json" };
  return call(app, method, path, { ...extra, ...options });
}

const result = (answer) => JSON.parse(answer.body).result;

function assertRejected(answer, status, label) {
  assert.equal(answer.status, status, label);
  assert.equal(answer.type, "application/json", label);
  assert.equal(answer.body, '{"result":"rejected"}', label);
}

function assertNoState(app) {
  assert.equal(result(route(app, "status")), "no-such-account");
}

test("an unknown path gets 404 and a known path with another method gets 405", () => {
  const app = makeApp(vendored);
  for (const target of ["/", "/v1", "/v1/cap/", "/V1/cap", "/v1/%63ap", "//x/v1/cap", "*", "/v1/webhooks", "/test/clock", "http://[x/v1/cap"]) {
    assertRejected(call(app, "GET", target), 404, target);
  }
  assertRejected(call(app, "GET", "/nope?x=1"), 404, "query on an unknown path");
  for (const [method, target] of [["POST", "/v1/subscription"], ["HEAD", "/v1/cap"], ["DELETE", "/v1/cap"], ["GET", "/v1/checkout"], ["PUT", "/v1/account"], ["GET", "/v1/webhook"], ["POST", "/v1/done"]]) {
    assertRejected(call(app, method, target), 405, `${method} ${target}`);
  }
  assertRejected(call(app, "POST", "/v1/cap?"), 405, "query on a known path with another method");
  assertNoState(app);
});

test("a dot segment is resolved before the path is compared", () => {
  const app = makeApp(vendored);
  assert.equal(call(app, "GET", "/v1/x/../subscription").status, 200);
  assert.equal(call(app, "GET", "/v1/%2e%2e/v1/subscription").status, 200);
  assert.equal(call(app, "GET", "http://127.0.0.1:1/v1/subscription").status, 200);
});

test("a query string gets 400, also when nothing follows the question mark", () => {
  const app = makeApp(vendored);
  for (const target of ["/v1/subscription?", "/v1/subscription?a=1", "/v1/done?", "/v1/webhook?x"]) {
    const method = target.startsWith("/v1/webhook") ? "POST" : "GET";
    assertRejected(call(app, method, target), 400, target);
  }
  assertRejected(call(app, "POST", "/v1/checkout?", { body: CAP, type: "application/json" }), 400, "checkout with a query");
  assertNoState(app);
});

test("an app route refuses a missing or malformed account or authorization header with 400", () => {
  const app = makeApp(vendored);
  const bad = {
    "no account header": [["authorization", `Bearer ${CRED}`]],
    "no authorization header": [["halvo-account", ID]],
    "no header": [],
    "upper-case id": [["halvo-account", ID.toUpperCase()], ["authorization", `Bearer ${CRED}`]],
    "version 1 id": [["halvo-account", "0f8fad5b-d9cb-169f-a165-70867728950e"], ["authorization", `Bearer ${CRED}`]],
    "id with a variant outside 8 to b": [["halvo-account", "0f8fad5b-d9cb-469f-c165-70867728950e"], ["authorization", `Bearer ${CRED}`]],
    "id with a trailing character": [["halvo-account", `${ID}0`], ["authorization", `Bearer ${CRED}`]],
    "id with a leading character": [["halvo-account", `0${ID}`], ["authorization", `Bearer ${CRED}`]],
    "lower-case bearer": [["halvo-account", ID], ["authorization", `bearer ${CRED}`]],
    "63 characters": [["halvo-account", ID], ["authorization", `Bearer ${CRED.slice(1)}`]],
    "65 characters": [["halvo-account", ID], ["authorization", `Bearer ${CRED}0`]],
    "upper-case credential": [["halvo-account", ID], ["authorization", `Bearer ${CRED.toUpperCase()}`]],
    "two authorization headers": [["halvo-account", ID], ["authorization", `Bearer ${CRED}`], ["authorization", `Bearer ${CRED}`]],
    "two account headers": [["halvo-account", ID], ["halvo-account", ID], ["authorization", `Bearer ${CRED}`]],
  };
  for (const name of APP_ROUTES) {
    for (const [label, headers] of Object.entries(bad)) {
      const { method, path, request } = ROUTE[name];
      const list = headers.map((pair) => [...pair]);
      if (request !== null) list.push(["content-type", "application/json"]);
      assertRejected(call(app, method, path, { headers: list, body: request === null ? "" : CAP }), 400, `${name}: ${label}`);
    }
  }
  assertNoState(app);
  const mixedCase = [["Halvo-Account", ID], ["AUTHORIZATION", `Bearer ${CRED}`]];
  assert.equal(call(app, "GET", "/v1/subscription", { headers: mixedCase }).status, 200);
});

test("a route that takes no body refuses one byte with 400, and the webhook refuses a body above its limit with 413", () => {
  const app = makeApp(vendored);
  for (const name of APP_ROUTES.filter((n) => ROUTE[n].request === null)) {
    assertRejected(route(app, name, { body: " " }), 400, name);
    assertRejected(route(app, name, { body: "{}", type: "application/json" }), 400, `${name} with a JSON body`);
  }
  assertRejected(call(app, "GET", "/v1/done", { headers: [], body: "x" }), 400, "done with a body");
  const limit = ROUTE.webhook.limit;
  assertRejected(call(app, "POST", "/v1/webhook", { headers: [], body: "x".repeat(limit + 1) }), 413, "webhook above its limit");
  assertRejected(call(app, "POST", "/v1/webhook", { headers: [], body: "x".repeat(limit) }), 400, "webhook at its limit");
  assertRejected(call(app, "POST", "/v1/webhook", { headers: [] }), 400, "empty webhook");
  assertNoState(app);
});

test("the done page answers a fixed page with no script and needs no header", () => {
  const app = makeApp(vendored);
  const page = call(app, "GET", "/v1/done", { headers: [] });
  assert.equal(page.status, 200);
  assert.equal(page.type, "text/html; charset=utf-8");
  assert.equal(page.result, null);
  assert.doesNotMatch(page.body, /<script|https?:|src=|href=/i);
  assert.equal(call(app, "GET", "/v1/done", { headers: [["x-a", "canary"]] }).body, page.body);
});

test("a body route checks the content type, then the size, then the JSON, its bytes and the schema", () => {
  const app = makeApp(vendored);
  const types = [undefined, "application/json; charset=utf-8", "text/plain", "Application/JSON", "application/json "];
  for (const name of ["checkout-start", "cap-set"]) {
    for (const type of types) assertRejected(route(app, name, { type }), 415, `${name} type ${type}`);
    assertRejected(route(app, name, { type: "text/plain", body: "x".repeat(46) }), 415, `${name}: type before size`);
    const largest = '{"cap":{"amount":100000000,"period":"month"}}';
    assert.equal(Buffer.byteLength(largest), ROUTE[name].limit);
    assertRejected(route(app, name, { body: `${largest} ` }), 413, `${name} one byte above the limit`);
    assertRejected(route(app, name, { body: `{"cap":{"amount":100000000,"period":"month"},"x":1}` }), 413, `${name}: size before schema`);
    const bodies = [
      "",
      "{",
      "null",
      '{"cap": {"amount":1,"period":"week"}}',
      '{"cap":{"amount":1,"period":"week"}}\n',
      '{"cap":{"period":"week","period":"week"}}',
      '{"cap":{"amount":1.0,"period":"week"}}',
      '{"cap":{"amount":1e0,"period":"week"}}',
      '{"cap":{"amount":01,"period":"week"}}',
      '{"cap":{"amount":1,"period":"w\\u0065ek"}}',
      '﻿{"cap":{"amount":1,"period":"week"}}',
      '{"cap":{"amount":0,"period":"week"}}',
      '{"cap":{"amount":1,"period":"day"}}',
      '{"cap":{"amount":1}}',
      '{"cap":{"amount":1,"period":"week"},"a":1}',
      '{"cap":{"amount":"1","period":"week"}}',
    ];
    for (const body of bodies) assertRejected(route(app, name, { body }), 400, `${name} body ${JSON.stringify(body)}`);
    const invalidUtf8 = Buffer.concat([Buffer.from('{"cap":{"amount":1,"period":"'), Buffer.from([0xff]), Buffer.from('"}}')]);
    assertRejected(route(app, name, { body: invalidUtf8 }), 400, `${name} invalid UTF-8`);
    assertRejected(route(app, name, { headers: [["content-type", "text/plain"]] }), 400, `${name}: headers before the type`);
  }
  assertNoState(app);
  assert.equal(result(route(app, "checkout-start", { body: '{"cap":{"period":"month","amount":100000000}}' })), "session");
});

test("the scripted states answer as the backend's entries do", () => {
  const app = makeApp(vendored);
  const as = (name, options) => result(route(app, name, options));
  for (const name of APP_ROUTES.filter((n) => n !== "checkout-start")) assert.equal(as(name), "no-such-account", name);
  assert.equal(app.complete(), 0);

  const start = route(app, "checkout-start");
  assert.deepEqual(JSON.parse(start.body), SESSION);
  assert.equal(as("status"), "pending");
  for (const name of ["cap-get", "cap-set", "manage", "account-delete"]) assert.equal(as(name), "pending", name);
  for (const name of APP_ROUTES) assert.equal(as(name, { credential: WRONG }), "credential-mismatch", `pending ${name}`);
  assert.equal(as("status", { id: OTHER_ID }), "no-such-account");
  assert.deepEqual(JSON.parse(route(app, "checkout-start", { body: '{"cap":{"amount":7,"period":"month"}}' }).body), SESSION);

  assert.equal(app.failPayment(), 0);
  assert.equal(app.cancel(), 0);
  assert.equal(app.complete(), 1);
  assert.equal(app.complete(), 0);
  assert.equal(as("status"), "active");
  assert.equal(as("checkout-start"), "account-exists");
  assert.deepEqual(JSON.parse(route(app, "cap-get").body), { result: "cap", cap: { amount: 7, period: "month" }, spend: { spent: 0, held: 0, period_end: 0 } });
  const set = route(app, "cap-set", { body: '{"cap":{"amount":100000000,"period":"week"}}' });
  assert.deepEqual(JSON.parse(set.body), { result: "cap", cap: { amount: 100000000, period: "week" }, spend: { spent: 0, held: 0, period_end: 0 } });
  assert.deepEqual(JSON.parse(route(app, "cap-get").body).cap, { amount: 100000000, period: "week" });
  assert.deepEqual(JSON.parse(route(app, "manage").body), samples.answers.manage.page);
  for (const name of APP_ROUTES) assert.equal(as(name, { credential: WRONG }), "credential-mismatch", `account ${name}`);

  assert.equal(app.failPayment(), 1);
  assert.equal(as("status"), "payment-failed");
  assert.equal(app.failPayment(), 0);
  assert.equal(app.cancel(), 1);
  assert.equal(as("status"), "cancelled");
  assert.equal(app.failPayment(), 0);
  assert.equal(app.cancel(), 0);
  assert.equal(as("checkout-start"), "account-exists");
  assert.equal(as("manage"), "page");

  assert.equal(app.armNotFinished(), 1);
  assert.equal(app.armNotFinished(), 0);
  assert.equal(as("account-delete", { credential: WRONG }), "credential-mismatch");
  assert.equal(as("account-delete"), "not-finished");
  assert.equal(as("status"), "cancelled");
  assert.equal(as("account-delete"), "deleted");
  assert.equal(as("status"), "no-such-account");
  assert.equal(as("account-delete"), "no-such-account");
});

test("a cancel moves an active account as well as one whose payment failed", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start");
  app.complete();
  assert.equal(app.cancel(), 1);
  assert.equal(result(route(app, "status")), "cancelled");
});

test("an expired checkout leaves no account, and a new start begins again", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start");
  route(app, "checkout-start", { id: OTHER_ID });
  assert.equal(app.expire(), 2);
  assert.equal(app.expire(), 0);
  for (const name of APP_ROUTES.filter((n) => n !== "checkout-start")) assert.equal(result(route(app, name)), "no-such-account", name);
  assert.equal(result(route(app, "status", { credential: WRONG })), "no-such-account");
  assert.equal(result(route(app, "checkout-start", { credential: WRONG })), "session");
  assert.equal(result(route(app, "status", { credential: WRONG })), "pending");
  assert.equal(result(route(app, "status")), "credential-mismatch");
});

test("the armed not-finished answer waits for a delete that would delete", () => {
  const app = makeApp(vendored);
  app.armNotFinished();
  assert.equal(result(route(app, "account-delete")), "no-such-account");
  route(app, "checkout-start");
  assert.equal(result(route(app, "account-delete")), "pending");
  app.complete();
  assert.equal(result(route(app, "account-delete")), "not-finished");
  assert.equal(result(route(app, "account-delete")), "deleted");
});

test("the states of two accounts are kept apart", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start");
  app.complete();
  route(app, "checkout-start", { id: OTHER_ID, credential: WRONG });
  assert.equal(result(route(app, "status")), "active");
  assert.equal(result(route(app, "status", { id: OTHER_ID, credential: WRONG })), "pending");
  assert.equal(result(route(app, "account-delete")), "deleted");
  assert.equal(result(route(app, "status", { id: OTHER_ID, credential: WRONG })), "pending");
});

test("every answer in every state validates against its schema and agrees with results", () => {
  const states = {
    nothing: () => {},
    pending: (app) => route(app, "checkout-start"),
    active: (app) => (route(app, "checkout-start"), app.complete()),
    "payment-failed": (app) => (route(app, "checkout-start"), app.complete(), app.failPayment()),
    cancelled: (app) => (route(app, "checkout-start"), app.complete(), app.cancel()),
    "not-finished armed": (app) => (route(app, "checkout-start"), app.complete(), app.armNotFinished()),
  };
  const seen = new Map(APP_ROUTES.map((name) => [name, new Set()]));
  for (const [state, enter] of Object.entries(states)) {
    for (const name of APP_ROUTES) {
      for (const credential of [CRED, WRONG]) {
        const app = makeApp(vendored);
        enter(app);
        const answer = route(app, name, { credential });
        const label = `${state} ${name} ${credential === CRED ? "right" : "wrong"} credential`;
        assert.equal(answer.status, 200, label);
        assert.equal(answer.type, "application/json", label);
        const parsed = JSON.parse(answer.body);
        assert.equal(answer.body, JSON.stringify(parsed), label);
        assert.equal(schema.validate(ROUTE[name].answer, parsed), true, label);
        assert.equal(keysAgree(ROUTE[name].results, parsed), true, label);
        assert.equal(answer.result, parsed.result, label);
        seen.get(name).add(parsed.result);
      }
    }
  }
  const never = { "checkout-start": ["unavailable"], "cap-get": ["no-cap"], manage: ["unavailable"] };
  for (const name of APP_ROUTES) {
    const want = Object.keys(ROUTE[name].results).filter((value) => !(never[name] ?? []).includes(value));
    assert.deepEqual([...seen.get(name)].sort(), want.sort(), name);
  }
});

test("a credential that does not match changes nothing, also with a body", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start", { body: '{"cap":{"amount":7,"period":"month"}}' });
  assert.equal(result(route(app, "checkout-start", { credential: WRONG, body: '{"cap":{"amount":9,"period":"week"}}' })), "credential-mismatch");
  app.complete();
  assert.equal(result(route(app, "cap-set", { credential: WRONG, body: '{"cap":{"amount":9,"period":"week"}}' })), "credential-mismatch");
  assert.deepEqual(JSON.parse(route(app, "cap-get").body).cap, { amount: 7, period: "month" });
});

test("the hash covers every character of the credential", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start");
  for (const at of [0, 31, 63]) {
    const other = `${CRED.slice(0, at)}${CRED[at] === "f" ? "e" : "f"}${CRED.slice(at + 1)}`;
    assert.equal(result(route(app, "status", { credential: other })), "credential-mismatch", String(at));
  }
});

test("two content-type headers are joined and refused with 415", () => {
  const app = makeApp(vendored);
  const headers = [["halvo-account", ID], ["authorization", `Bearer ${CRED}`], ["content-type", "application/json"], ["content-type", "application/json"]];
  assertRejected(call(app, "POST", "/v1/checkout", { headers, body: CAP }), 415, "two content-type headers");
  assertNoState(app);
});

test("a new checkout keeps the cap of its body", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start", { body: '{"cap":{"amount":7,"period":"month"}}' });
  app.complete();
  assert.deepEqual(JSON.parse(route(app, "cap-get").body).cap, { amount: 7, period: "month" });
});

test("expire drops only pending checkouts, and complete moves only pending checkouts", () => {
  const app = makeApp(vendored);
  route(app, "checkout-start");
  app.complete();
  route(app, "checkout-start", { id: OTHER_ID });
  assert.equal(app.expire(), 1);
  assert.equal(result(route(app, "status")), "active");
  app.failPayment();
  route(app, "checkout-start", { id: OTHER_ID });
  assert.equal(app.complete(), 1);
  assert.equal(result(route(app, "status")), "payment-failed");
  app.cancel();
  assert.equal(app.complete(), 0);
  assert.equal(result(route(app, "status")), "cancelled");
  assert.equal(result(route(app, "status", { id: OTHER_ID })), "cancelled");
});

test("bytes that are no request get 400 and the samples' rejected body", () => {
  const app = makeApp(vendored);
  assertRejected(app.malformed(), 400, "malformed");
});
