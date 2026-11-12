import { readFileSync } from "node:fs";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import type { ProviderEvent, ProviderPort } from "../../src/billing/port.ts";
import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import { WEBHOOK_BODY_LIMIT } from "../../src/contract/routes.ts";
import { PERIOD_SECONDS, type ProviderFake, startProviderFake } from "../../test-kit/provider-fake/fake.mjs";
import { sign } from "../../test-kit/provider-fake/sign.mjs";
import { PROVIDER_API_KEY, ROOT, type Recorded, startRecorder, startStripeMock, WEBHOOK_SIGNING_SECRET } from "./servers.ts";

const KEY = PROVIDER_API_KEY;
const SECRET = WEBHOOK_SIGNING_SECRET;
const ACCOUNT = "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";
const BUYER = JSON.parse(readFileSync(`${ROOT}test-kit/provider-fake/buyer.json`, "utf8")) as Record<string, string>;
const BUYER_VALUES = ["name", "email", "phone", "line1", "line2", "city", "state", "postal_code"].map((key) => BUYER[key] as string);

type Json = Record<string, unknown>;

let mock: { url: string; stop(): Promise<void> };
let entry: { url: string; requests: Recorded[]; close(): Promise<void> };
let fake: ProviderFake;
let port: ProviderPort;

beforeAll(async () => {
  mock = await startStripeMock();
  entry = await startRecorder(() => ({ status: 200, body: "" }));
  fake = await startProviderFake({ mockUrl: mock.url, apiKey: KEY, signingSecret: SECRET, entries: [entry.url] });
  port = makeStripePort({ baseUrl: fake.origin, apiKey: KEY, signingSecret: SECRET, priceId: "price_test_halvo_local" });
});

afterAll(async () => {
  await fake.close();
  await entry.close();
  await mock.stop();
});

async function call(path: string, body?: Json): Promise<Json> {
  const response = await fetch(`${fake.origin}${path}`, body === undefined ? {} : { method: "POST", body: JSON.stringify(body) });
  return (await response.json()) as Json;
}

async function newSession(): Promise<{ id: string; url: string }> {
  const started = await port.startCheckout({ accountId: ACCOUNT, successUrl: `${entry.url}/v1/done`, expiresAt: Date.now() + 1_860_000 });
  if (started.kind !== "session") throw new Error("the fake made no session");
  const found = await call("/test/lookup", { url: started.url });
  expect(found).toEqual({ result: "session", session: started.sessionId });
  return { id: started.sessionId, url: started.url };
}

function deliveredSince(count: number): Recorded[] {
  return entry.requests.slice(count);
}

async function verified(request: Recorded): Promise<ProviderEvent | "rejected"> {
  const answer = await port.verifyWebhook(new TextEncoder().encode(request.body), new Headers({ "stripe-signature": String(request.headers["stripe-signature"]) }), Date.now());
  return answer.kind === "event" ? answer.event : "rejected";
}

describe("the v1 API", () => {
  test("a call with another key gets 401, is recorded without the key and is not forwarded", async () => {
    const before = ((await call("/test/requests")).requests as unknown[]).length;
    const refused = await fetch(`${fake.origin}/v1/checkout/sessions/cs_x`, { headers: { authorization: `Bearer ${KEY}_other` } });
    expect(refused.status).toBe(401);
    const recorded = ((await call("/test/requests")).requests as Json[]).slice(before);
    expect(recorded).toEqual([{ method: "GET", path: "/v1/checkout/sessions/cs_x", params: [], mock_status: 0, fake_status: 401 }]);
    expect(JSON.stringify(recorded)).not.toContain("sk_test");
  });

  test("a call that stripe-mock rejects is passed back unchanged and changes no state", async () => {
    const body = "mode=subscription&no_such_parameter=1";
    const headers = { authorization: `Bearer ${KEY}`, "content-type": "application/x-www-form-urlencoded" };
    const direct = await fetch(`${mock.url}/v1/checkout/sessions`, { method: "POST", headers, body });
    const passed = await fetch(`${fake.origin}/v1/checkout/sessions`, { method: "POST", headers, body });
    expect(passed.status).toBe(direct.status);
    expect(passed.status).toBe(400);
    expect(await passed.text()).toBe(await direct.text());
    const last = ((await call("/test/requests")).requests as Json[]).at(-1);
    expect(last).toEqual({ method: "POST", path: "/v1/checkout/sessions", params: ["mode", "no_such_parameter"], mock_status: 400, fake_status: 400 });
  });

  test("a session create with a success URL of another origin is refused", async () => {
    expect(await port.startCheckout({ accountId: ACCOUNT, successUrl: "http://halvo.test/v1/done", expiresAt: Date.now() })).toEqual({ kind: "failed" });
    expect(((await call("/test/requests")).requests as Json[]).at(-1)).toEqual(expect.objectContaining({ mock_status: 200, fake_status: 400 }));
  });

  test("a session create with a trial, a billing cycle anchor, automatic tax or an expiry outside 30 minutes to 24 hours gets the mock's answer for an invalid parameter", async () => {
    const now = Math.floor(Date.now() / 1000);
    const params = (expiresAt: number, extra: [string, string][] = []) =>
      new URLSearchParams([
        ["mode", "subscription"],
        ["line_items[0][price]", "price_test_halvo_local"],
        ["success_url", `${entry.url}/v1/done`],
        ["client_reference_id", ACCOUNT],
        ["subscription_data[metadata][halvo_account]", ACCOUNT],
        ["expires_at", String(expiresAt)],
        ["allowed_payment_method_types[0]", "card"],
        ...extra,
      ]).toString();
    const create = async (body: string) => {
      const before = ((await call("/test/requests")).requests as Json[]).length;
      const response = await fetch(`${fake.origin}/v1/checkout/sessions`, {
        method: "POST",
        headers: { authorization: `Bearer ${KEY}`, "content-type": "application/x-www-form-urlencoded" },
        body,
      });
      const recorded = ((await call("/test/requests")).requests as Json[]).slice(before);
      return { status: response.status, body: (await response.json()) as Json, recorded };
    };
    const refused = [
      ["a trial in days", params(now + 1_860, [["subscription_data[trial_period_days]", "14"]])],
      ["a trial end", params(now + 1_860, [["subscription_data[trial_end]", String(now + 864_000)]])],
      ["a billing cycle anchor", params(now + 1_860, [["subscription_data[billing_cycle_anchor]", String(now + 864_000)]])],
      ["automatic tax", params(now + 1_860, [["automatic_tax[enabled]", "true"]])],
      ["an expiry of 48 hours", params(now + 172_800)],
      ["an expiry of 29 minutes", params(now + 1_740)],
    ] as const;
    for (const [name, body] of refused) {
      const answer = await create(body);
      expect({ name, status: answer.status, body: answer.body }).toEqual({
        name,
        status: 400,
        body: { error: { message: expect.any(String), type: "invalid_request_error" } },
      });
      expect({ name, recorded: answer.recorded }).toEqual({ name, recorded: [expect.objectContaining({ mock_status: 200, fake_status: 400 })] });
    }
    const accepted = [
      ["an expiry of 31 minutes", params(now + 1_860)],
      ["an expiry just under 24 hours", params(now + 86_340)],
      ["automatic tax turned off", params(now + 1_860, [["automatic_tax[enabled]", "false"]])],
    ] as const;
    for (const [name, body] of accepted) {
      const answer = await create(body);
      expect({ name, status: answer.status, object: answer.body.object }).toEqual({ name, status: 200, object: "checkout.session" });
    }
  });

  test("the manage page is a path of the fake's own address", async () => {
    const page = await port.openManagePage({ customerId: "cus_test_1", returnUrl: `${entry.url}/v1/done` });
    if (page.kind !== "page") throw new Error("no manage page");
    expect(page.url.startsWith(`${fake.origin}/manage/`)).toBe(true);
    expect((await fetch(page.url)).status).toBe(200);
  });
});

describe("events", () => {
  test("pay completes the session and delivers three signed events with a two-space indent, in the order made, to the entry of the success URL", async () => {
    const session = await newSession();
    const count = entry.requests.length;
    const paid = await call("/test/pay", { session: session.id, created: 1_794_000_000 });
    expect(paid).toEqual({ result: "paid", subscription: expect.stringMatching(/^sub_test_/), events: [expect.any(String), expect.any(String), expect.any(String)] });
    const sent = deliveredSince(count);
    expect(sent.map((each) => `${each.method} ${each.path}`)).toEqual(Array(3).fill("POST /v1/webhook"));
    const reduced = await Promise.all(sent.map(verified));
    expect(reduced.map((each) => (each === "rejected" ? each : each.kind))).toEqual(["checkout-completed", "subscription", "ignored"]);
    expect(reduced[0]).toEqual(
      expect.objectContaining({ accountId: ACCOUNT, sessionId: session.id, payment: "paid", subscriptionId: paid.subscription, providerTime: 1_794_000_000_000 }),
    );
    expect(reduced[1]).toEqual(
      expect.objectContaining({ accountId: ACCOUNT, state: "active", period: { start: 1_794_000_000_000, end: (1_794_000_000 + PERIOD_SECONDS) * 1000 } }),
    );
    const types = sent.map((each) => (JSON.parse(each.body) as Json).type);
    expect(types).toEqual(["checkout.session.completed", "customer.subscription.created", "invoice.paid"]);
    for (const each of sent) {
      expect(each.body).not.toBe(JSON.stringify(JSON.parse(each.body)));
      expect(each.body).toBe(JSON.stringify(JSON.parse(each.body), null, 2));
      expect(new TextEncoder().encode(each.body).byteLength).toBeLessThanOrEqual(WEBHOOK_BODY_LIMIT);
    }
    for (const value of BUYER_VALUES) {
      expect(sent[0]?.body).toContain(value);
      expect(sent[2]?.body).toContain(value);
      expect(sent[1]?.body).not.toContain(value);
    }
    const read = await port.readSession(session.id);
    expect(read).toEqual({
      kind: "complete",
      session: { sessionId: session.id, customerId: (reduced[0] as { customerId: string }).customerId, subscriptionId: paid.subscription, payment: "paid" },
    });
    expect(await call("/test/pay", { session: session.id })).toEqual({ result: "not-open" });
    const event = await call(`/test/events/${(paid.events as string[])[0]}`);
    expect(event).toEqual(expect.objectContaining({ result: "event", type: "checkout.session.completed", created: 1_794_000_000, body: sent[0]?.body }));
    expect(event.deliveries).toEqual([{ event: (paid.events as string[])[0], status: 200 }]);
  });

  test("hold queues the events of the session and its subscription until release, and deliver sends chosen events in a chosen order", async () => {
    const session = await newSession();
    expect(await call("/test/hold", { session: session.id })).toEqual({ result: "held" });
    const count = entry.requests.length;
    const paid = await call("/test/pay", { session: session.id });
    const failed = await call("/test/fail-payment", { subscription: paid.subscription });
    expect(failed).toEqual({ result: "payment-failed", events: [expect.any(String), expect.any(String)] });
    expect(deliveredSince(count)).toEqual([]);
    const all = [...(paid.events as string[]), ...(failed.events as string[])];
    const released = await call("/test/release", { session: session.id });
    expect(released).toEqual({ result: "released", deliveries: all.map((event) => ({ event, status: 200 })) });
    const reversed = [all[1], all[0]] as string[];
    expect(await call("/test/deliver", { events: reversed })).toEqual({ result: "delivered", deliveries: reversed.map((event) => ({ event, status: 200 })) });
    const sentIds = deliveredSince(count).map((each) => (JSON.parse(each.body) as Json).id);
    expect(sentIds).toEqual([...all, ...reversed]);
    const state = await call(`/test/subscriptions/${String(paid.subscription)}`);
    expect(state).toEqual(expect.objectContaining({ result: "subscription", status: "past_due", session: session.id }));
  });

  test("complete-unpaid makes a customer and no subscription, and expire makes a read say expired", async () => {
    const unpaid = await newSession();
    const count = entry.requests.length;
    const completed = await call("/test/complete-unpaid", { session: unpaid.id });
    expect(completed).toEqual({ result: "completed", events: [expect.any(String)] });
    const [event] = await Promise.all(deliveredSince(count).map(verified));
    expect(event).toEqual(expect.objectContaining({ kind: "checkout-completed", payment: "unpaid", subscriptionId: null, customerId: expect.stringMatching(/^cus_test_/) }));
    expect(await port.readSession(unpaid.id)).toEqual(expect.objectContaining({ kind: "complete", session: expect.objectContaining({ payment: "unpaid", subscriptionId: null }) }));
    const expiring = await newSession();
    expect(await call("/test/expire", { session: expiring.id })).toEqual({ result: "expired", events: [expect.any(String)] });
    expect(await port.readSession(expiring.id)).toEqual({ kind: "expired" });
    expect(await call("/test/expire", { session: expiring.id })).toEqual({ result: "not-open" });
  });

  test("recover and cancel move the subscription; the cancel of an ended subscription is refused and the adapter reads nothing-to-cancel", async () => {
    const session = await newSession();
    const paid = await call("/test/pay", { session: session.id });
    const subscription = String(paid.subscription);
    expect(await call("/test/recover", { subscription })).toEqual({ result: "not-failed" });
    await call("/test/fail-payment", { subscription });
    expect(await call("/test/recover", { subscription, created: 1_794_000_500 })).toEqual({ result: "recovered", events: [expect.any(String), expect.any(String)] });
    const cancelled = await call("/test/cancel", { subscription });
    expect(cancelled).toEqual({ result: "cancelled", events: [expect.any(String)] });
    expect((await call(`/test/events/${(cancelled.events as string[])[0]}`)).type).toBe("customer.subscription.deleted");
    expect(await call("/test/cancel", { subscription })).toEqual({ result: "already-ended" });
    expect(await port.cancelSubscription(subscription)).toEqual({ kind: "nothing-to-cancel" });
    expect(await port.cancelSubscription("sub_test_not_held")).toEqual({ kind: "nothing-to-cancel" });

    const other = await newSession();
    const otherPaid = await call("/test/pay", { session: other.id });
    const count = entry.requests.length;
    expect(await port.cancelSubscription(String(otherPaid.subscription))).toEqual({ kind: "cancelled" });
    expect(deliveredSince(count)).toEqual([]);
    expect((await call(`/test/subscriptions/${String(otherPaid.subscription)}`)).status).toBe("canceled");
  });

  test("the stand-in page's pay button does what pay does, and the session state shows the page's own count of inputs", async () => {
    const session = await newSession();
    const count = entry.requests.length;
    const values = {
      email: "page.buyer@example.test",
      "card-number": "4242424242424242",
      "card-expiry": "12/34",
      "card-cvc": "123",
      "cardholder-name": "Page Buyer",
      country: "US",
      "postal-code": "10001",
    };
    const ids = [...Object.keys(values), "pay"];
    const response = await fetch(`${session.url}/pay`, { method: "POST", body: JSON.stringify({ values, inputs: { count: ids.length, ids } }) });
    expect(await response.json()).toEqual({ ok: true });
    expect(deliveredSince(count)).toHaveLength(3);
    const state = await call(`/test/sessions/${session.id}`);
    expect(state).toEqual(expect.objectContaining({ result: "session", status: "complete", payment_status: "paid", inputs: { count: 8, ids } }));
    for (const each of deliveredSince(count)) expect(each.body).not.toContain("page.buyer@example.test");
  });

  test("each event call refuses a state it does not start from, and deliver refuses a list with one unknown id", async () => {
    const session = await newSession();
    const paid = await call("/test/pay", { session: session.id });
    const subscription = String(paid.subscription);
    expect(await call("/test/complete-unpaid", { session: session.id })).toEqual({ result: "not-open" });
    await call("/test/fail-payment", { subscription });
    expect(await call("/test/fail-payment", { subscription })).toEqual({ result: "not-active" });
    const count = entry.requests.length;
    expect(await call("/test/deliver", { events: [(paid.events as string[])[0], "evt_x"] })).toEqual({ result: "unknown" });
    expect(deliveredSince(count)).toEqual([]);
    expect(await call("/test/release", { session: "cs_x" })).toEqual({ result: "unknown" });
    const unpaid = await newSession();
    await call("/test/complete-unpaid", { session: unpaid.id });
    expect(await call("/test/complete-unpaid", { session: unpaid.id })).toEqual({ result: "not-open" });
  });

  test("a session keeps the expiry it was sent through reads and payment, and a read during the payment never shows complete without its subscription", async () => {
    const expiresAt = Math.ceil((Date.now() + 1_860_000) / 1000);
    const started = await port.startCheckout({ accountId: ACCOUNT, successUrl: `${entry.url}/v1/done`, expiresAt: expiresAt * 1000 });
    if (started.kind !== "session") throw new Error("no session");
    expect(started.expiresAt).toBe(expiresAt * 1000);
    const paying = call("/test/pay", { session: started.sessionId });
    const reads = [];
    for (let each = 0; each < 20; each += 1) reads.push(await call(`/test/sessions/${started.sessionId}`));
    await paying;
    reads.push(await call(`/test/sessions/${started.sessionId}`));
    for (const read of reads) {
      expect(read.expires_at).toBe(expiresAt);
      if (read.status === "complete") expect(read.subscription).toEqual(expect.stringMatching(/^sub_test_/));
    }
  });

  test("a session create with a success URL of a loopback origin that is no entry is refused", async () => {
    const other = entry.url.replace(/:(\d+)$/, (_, port: string) => `:${Number(port) + 1}`);
    expect(await port.startCheckout({ accountId: ACCOUNT, successUrl: `${other}/v1/done`, expiresAt: Date.now() })).toEqual({ kind: "failed" });
  });

  test("the right key sent as Basic auth is taken, and a body over the fake's limit is recorded and refused", async () => {
    const basic = `Basic ${btoa(`${KEY}:`)}`;
    const read = await fetch(`${fake.origin}/v1/checkout/sessions/cs_x`, { headers: { authorization: basic } });
    expect(read.status).toBe(404);
    const big = await fetch(`${fake.origin}/v1/checkout/sessions`, {
      method: "POST",
      headers: { authorization: `Bearer ${KEY}`, "content-type": "application/x-www-form-urlencoded" },
      body: `mode=${"x".repeat(70_000)}`,
    });
    expect(big.status).toBe(400);
    expect(((await call("/test/requests")).requests as Json[]).at(-1)).toEqual({ method: "POST", path: "/v1/checkout/sessions", params: [], mock_status: 0, fake_status: 400 });
  });

  test("a test call that names an id the fake does not hold answers unknown", async () => {
    expect(await call("/test/lookup", { url: "http://127.0.0.1:1/c/x" })).toEqual({ result: "unknown" });
    expect(await call("/test/hold", { session: "cs_x" })).toEqual({ result: "unknown" });
    expect(await call("/test/pay", { session: "cs_x" })).toEqual({ result: "unknown" });
    expect(await call("/test/cancel", { subscription: "sub_x" })).toEqual({ result: "unknown" });
    expect(await call("/test/deliver", { events: ["evt_x"] })).toEqual({ result: "unknown" });
    expect(await call("/test/events/evt_x")).toEqual({ result: "unknown" });
    expect(await call("/test/sessions/cs_x")).toEqual({ result: "unknown" });
  });

  test("sign writes the provider's header, which the adapter accepts", async () => {
    const body = JSON.stringify({ id: "evt_1", type: "invoice.paid", created: 1, data: { object: {} } }, null, 2);
    const t = Math.floor(Date.now() / 1000);
    expect(sign(SECRET, body, t)).toMatch(new RegExp(`^t=${t},v1=[0-9a-f]{64}$`));
    const answer = await port.verifyWebhook(new TextEncoder().encode(body), new Headers({ "stripe-signature": sign(SECRET, body, t) }), Date.now());
    expect(answer).toEqual({ kind: "event", event: { kind: "ignored" } });
  });
});
