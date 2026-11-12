import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import app, { credentialHash } from "../../src/app.ts";
import { type Entries, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Env } from "../../src/types.ts";
import { sign } from "../../test-kit/provider-fake/sign.mjs";
import { makeTestHost } from "../../test-kit/sql/node.ts";
import { WEBHOOK_SIGNING_SECRET } from "./servers.ts";
import { ACCOUNT, HASH, sessionPort } from "./support.ts";

const SECRET = WEBHOOK_SIGNING_SECRET;
const CREDENTIAL = "c".repeat(64);
const HEADERS = { "halvo-account": ACCOUNT, authorization: `Bearer ${CREDENTIAL}` };

let objects: Map<string, Entries>;
let gets: string[];
let lines: unknown[];
let closers: (() => void)[];

function env(): Env {
  const namespace = {
    idFromName: (name: string) => name,
    get(id: string) {
      gets.push(id);
      let entries = objects.get(id);
      if (entries === undefined) {
        const made = makeTestHost({ port: sessionPort(10_000), clock: () => 1000 });
        closers.push(made.close);
        entries = makeEntries(made.host, {
          accounts: makeAccounts(),
          ledger: makeLedger({ holdLifeMs: 3000 }),
          metering: makeMetering({ intervalSeconds: 3600, marginSeconds: 300 }),
        });
        objects.set(id, entries);
      }
      return entries;
    },
  };
  return { ACCOUNT: namespace as unknown as Env["ACCOUNT"], WEBHOOK_SIGNING_SECRET: SECRET, PROVIDER_BASE_URL: "" } as Env;
}

beforeEach(() => {
  objects = new Map();
  gets = [];
  lines = [];
  closers = [];
  vi.spyOn(console, "log").mockImplementation((line: unknown) => {
    lines.push(JSON.parse(String(line)));
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const close of closers) close();
});

function event(type: string, object: Record<string, unknown>): string {
  return JSON.stringify({ id: `evt_${type}`, object: "event", created: Math.floor(Date.now() / 1000), type, data: { object } }, null, 2);
}

async function hook(body: string, headers: Record<string, string>, e: Env) {
  return app(new Request("http://backend.test/v1/webhook", { method: "POST", headers, body }), e);
}

async function answer(response: Response) {
  return { status: response.status, type: response.headers.get("content-type"), body: await response.text() };
}

test("the default app serves checkout start, status, manage, account delete and the webhook from the account's object", async () => {
  const e = env();
  const call = (method: string, path: string, body?: string) =>
    app(new Request(`http://backend.test${path}`, { method, headers: body ? { ...HEADERS, "content-type": "application/json" } : HEADERS, body }), e);
  expect(await (await call("POST", "/v1/checkout", '{"cap":{"amount":731,"period":"week"}}')).json()).toEqual({
    result: "session",
    url: "http://127.0.0.1:1/c/1",
  });
  expect(await (await call("GET", "/v1/subscription")).json()).toEqual({ result: "pending" });
  expect(await (await call("POST", "/v1/manage")).json()).toEqual({ result: "pending" });
  expect(await (await call("DELETE", "/v1/account")).json()).toEqual({ result: "pending" });

  const completed = event("checkout.session.completed", {
    id: "cs_1",
    client_reference_id: ACCOUNT,
    customer: "cus_1",
    subscription: "sub_1",
    payment_status: "paid",
    status: "complete",
  });
  const t = Math.floor(Date.now() / 1000);
  expect(await answer(await hook(completed, { "stripe-signature": sign(SECRET, completed, t) }, e))).toEqual({ status: 200, type: null, body: "" });
  expect(await (await call("GET", "/v1/subscription")).json()).toEqual({ result: "active" });
  expect(await objects.get(ACCOUNT)?.capGet(await credentialHash(CREDENTIAL))).toEqual(
    expect.objectContaining({ result: "cap", cap: { amount: 731, period: "week" } }),
  );
  expect(await (await call("POST", "/v1/manage")).json()).toEqual({ result: "unavailable" });
  expect(new Set(gets)).toEqual(new Set([ACCOUNT]));
  expect(HASH).not.toBe(CREDENTIAL);
});

test("the webhook route answers 400 with the rejection body and logs it for a refused signature, and 200 with no object call for an ignored event", async () => {
  const e = env();
  const body = event("invoice.paid", { id: "in_1", customer: "cus_1" });
  expect(await answer(await hook(body, {}, e))).toEqual({ status: 400, type: "application/json", body: '{"result":"rejected"}' });
  expect(await answer(await hook(body, { "stripe-signature": sign(`${SECRET}_other`, body, Math.floor(Date.now() / 1000)) }, e))).toEqual({
    status: 400,
    type: "application/json",
    body: '{"result":"rejected"}',
  });
  expect(lines).toEqual([
    { event: "rejected", route: "webhook", status: 400 },
    { event: "rejected", route: "webhook", status: 400 },
  ]);
  expect(await answer(await hook(body, { "stripe-signature": sign(SECRET, body, Math.floor(Date.now() / 1000)) }, e))).toEqual({
    status: 200,
    type: null,
    body: "",
  });
  expect(gets).toEqual([]);
  expect(lines).toHaveLength(2);
});

test("the webhook route answers only after webhookEvent has returned, and a throw of it reaches the router as 500 with handler-error", async () => {
  let release: () => void = () => {};
  let throws = false;
  const calls: string[] = [];
  const object = {
    async webhookEvent() {
      calls.push("webhookEvent");
      if (throws) throw new Error("boom");
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      calls.push("returned");
    },
  };
  const e = {
    ACCOUNT: { idFromName: (name: string) => name, get: () => object } as unknown as Env["ACCOUNT"],
    WEBHOOK_SIGNING_SECRET: SECRET,
    PROVIDER_BASE_URL: "",
  } as Env;
  const body = event("customer.subscription.updated", {
    id: "sub_1",
    customer: "cus_1",
    status: "active",
    metadata: { halvo_account: ACCOUNT },
    items: { data: [] },
  });
  const pending = hook(body, { "stripe-signature": sign(SECRET, body, Math.floor(Date.now() / 1000)) }, e);
  const early = await Promise.race([pending.then(() => "answered"), new Promise((resolve) => setTimeout(() => resolve("waiting"), 100))]);
  expect(early).toBe("waiting");
  release();
  expect(await answer(await pending)).toEqual({ status: 200, type: null, body: "" });
  expect(calls).toEqual(["webhookEvent", "returned"]);
  throws = true;
  expect(await answer(await hook(body, { "stripe-signature": sign(SECRET, body, Math.floor(Date.now() / 1000)) }, e))).toEqual({
    status: 500,
    type: null,
    body: "",
  });
  expect(lines).toEqual([{ event: "handler-error", route: "webhook", status: 500, error: "Error" }]);
});

test("the done page is fixed HTML with no script, no remote reference and no value from the request", async () => {
  const first = await answer(await app(new Request("http://backend.test/v1/done"), env()));
  const second = await answer(await app(new Request("http://backend.test/v1/done", { headers: { "x-halvo-sentinel": "hv-done-sentinel" } }), env()));
  expect(first.status).toBe(200);
  expect(first.type).toBe("text/html; charset=utf-8");
  expect(second).toEqual(first);
  expect(first.body).not.toMatch(/<script|https?:|src=|href=/i);
});
