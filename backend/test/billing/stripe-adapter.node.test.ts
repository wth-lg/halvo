import { readFileSync } from "node:fs";
import Stripe from "stripe";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { API_VERSION, makeStripePort, type StripeConfig } from "../../src/billing/stripe/adapter.ts";
import { PROVIDER_CALL_TIMEOUT_MS } from "../../src/billing/stripe/timeout.ts";
import { URL_MAX_LENGTH } from "../../src/contract/schemas.ts";
import { freePort, PROVIDER_API_KEY, paramsOf, ROOT, type Recorded, startRecorder, startStripeMock, WEBHOOK_SIGNING_SECRET } from "./servers.ts";

const ACCOUNT = "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";
const SECRET = WEBHOOK_SIGNING_SECRET;
const KEY = PROVIDER_API_KEY;
const NOW = 1_794_000_000_000;
const LISTS = JSON.parse(readFileSync(`${ROOT}test/billing/stripe-params.json`, "utf8")) as Record<string, string[]>;
const BUYER = { name: "Quill Testbuyer", email: "quill.testbuyer@example.test", phone: "+15555550123", line1: "742 Evergreen Terrace" };
const ERROR = (type = "invalid_request_error") => JSON.stringify({ error: { type, message: "refused" } });
const MISSING = JSON.stringify({ error: { type: "invalid_request_error", code: "resource_missing", message: "No such subscription", param: "id" } });
const sdk = new Stripe(KEY);
const PERSONAL = [
  "email",
  "customer_email",
  "name",
  "address",
  "phone",
  "ip",
  "ip_address",
  "description",
  "tax_ids",
  "customer_details",
  "billing_address_collection",
  "shipping_address_collection",
  "phone_number_collection",
  "name_collection",
  "tax_id_collection",
];

function config(baseUrl: string, over: Partial<StripeConfig> = {}): StripeConfig {
  return { baseUrl, apiKey: KEY, signingSecret: SECRET, priceId: "price_test_halvo_local", ...over };
}

function endpoint(request: Recorded): string {
  return `${request.method} ${request.path.replace(/\/(cs|sub)_[A-Za-z0-9_]+$/, "/{id}")}`;
}

function completedEvent(over: Record<string, unknown> = {}) {
  return {
    id: "evt_test_1",
    object: "event",
    api_version: API_VERSION,
    created: 1_794_000_000,
    type: "checkout.session.completed",
    data: {
      object: {
        id: "cs_test_1",
        object: "checkout.session",
        client_reference_id: ACCOUNT,
        customer: "cus_test_1",
        subscription: "sub_test_1",
        payment_status: "paid",
        status: "complete",
        customer_email: BUYER.email,
        customer_details: { name: BUYER.name, email: BUYER.email, phone: BUYER.phone, address: { line1: BUYER.line1 } },
        collected_information: { individual_name: BUYER.name },
        metadata: {},
        ...over,
      },
    },
  };
}

function subscriptionEvent(status: string, over: Record<string, unknown> = {}) {
  return {
    id: "evt_test_2",
    object: "event",
    created: 1_794_000_100,
    type: "customer.subscription.updated",
    data: {
      object: {
        id: "sub_test_1",
        object: "subscription",
        customer: "cus_test_1",
        status,
        metadata: { halvo_account: ACCOUNT },
        items: { object: "list", data: [{ id: "si_1", current_period_start: 1_794_000_000, current_period_end: 1_796_592_000 }] },
        ...over,
      },
    },
  };
}

const encoder = new TextEncoder();

async function verify(body: string, header: string | null, now = NOW, secret = SECRET) {
  const port = makeStripePort(config("http://127.0.0.1:1", { signingSecret: secret }));
  return port.verifyWebhook(encoder.encode(body), new Headers(header === null ? {} : { "stripe-signature": header }), now);
}

function header(body: string, over: { secret?: string; timestamp?: number } = {}) {
  return sdk.webhooks.generateTestHeaderString({ payload: body, secret: over.secret ?? SECRET, timestamp: over.timestamp ?? NOW / 1000 });
}

describe("against the pinned stripe-mock", () => {
  let mock: { url: string; stop(): Promise<void> };
  beforeAll(async () => {
    mock = await startStripeMock();
  });
  afterAll(async () => {
    await mock.stop();
  });

  test("stripe-mock accepts every request that the adapter sends", async () => {
    const statuses: string[] = [];
    const real = globalThis.fetch;
    globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
      const response = await real(...args);
      statuses.push(`${args[1]?.method} ${String(args[0]).replace(mock.url, "")} ${response.status}`);
      return response;
    };
    try {
      const port = makeStripePort(config(mock.url));
      const session = await port.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:8787/v1/done", expiresAt: NOW + 1_860_000 });
      expect(session.kind).toBe("session");
      expect(["open", "complete", "expired", "unknown", "failed"]).toContain((await port.readSession("cs_test_a1")).kind);
      expect((await port.openManagePage({ customerId: "cus_test_1", returnUrl: "http://127.0.0.1:8787/v1/done" })).kind).toBe("page");
      expect(["cancelled", "failed"]).toContain((await port.cancelSubscription("sub_test_1")).kind);
    } finally {
      globalThis.fetch = real;
    }
    expect(statuses.map((each) => each.replace(/(cs|sub)_test_\w+/, "{id}"))).toEqual([
      "POST /v1/checkout/sessions 200",
      "GET /v1/checkout/sessions/{id} 200",
      "POST /v1/billing_portal/sessions 200",
      "DELETE /v1/subscriptions/{id} 200",
    ]);
  });

  test("stripe-mock reads the form body of a cancel and refuses a parameter that its spec does not name", async () => {
    const refused = await fetch(`${mock.url}/v1/subscriptions/sub_test_1`, {
      method: "DELETE",
      headers: { authorization: `Bearer ${KEY}`, "content-type": "application/x-www-form-urlencoded" },
      body: "invoice_now=true&no_such_parameter=1",
    });
    expect(refused.status).toBe(400);
  });
});

describe("what the adapter sends", () => {
  test("each request carries exactly the committed parameter names, the card filter, the invoice request and the account id alone", async () => {
    const server = await startRecorder((request) => {
      if (request.path === "/v1/checkout/sessions") {
        return { status: 200, body: JSON.stringify({ id: "cs_test_1", url: "http://127.0.0.1:2/c/1", expires_at: 1_794_001_860 }) };
      }
      if (request.path.startsWith("/v1/checkout/sessions/")) return { status: 200, body: JSON.stringify({ id: "cs_test_1", status: "open" }) };
      if (request.path === "/v1/billing_portal/sessions") return { status: 200, body: JSON.stringify({ url: "http://127.0.0.1:2/manage" }) };
      if (request.method === "DELETE") return { status: 400, body: ERROR() };
      return { status: 200, body: JSON.stringify({ id: "sub_test_1", status: "canceled" }) };
    });
    try {
      const port = makeStripePort(config(server.url));
      expect(await port.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:8787/v1/done", expiresAt: NOW + 1_860_000 })).toEqual({
        kind: "session",
        sessionId: "cs_test_1",
        url: "http://127.0.0.1:2/c/1",
        expiresAt: 1_794_001_860_000,
      });
      expect(await port.readSession("cs_test_1")).toEqual({ kind: "open" });
      expect(await port.openManagePage({ customerId: "cus_test_1", returnUrl: "http://127.0.0.1:8787/v1/done" })).toEqual({
        kind: "page",
        url: "http://127.0.0.1:2/manage",
      });
      expect(await port.cancelSubscription("sub_test_1")).toEqual({ kind: "nothing-to-cancel" });
      for (const request of server.requests) {
        expect({ endpoint: endpoint(request), params: request.params }).toEqual({ endpoint: endpoint(request), params: LISTS[endpoint(request)] });
        expect(request.headers.authorization).toBe(`Bearer ${KEY}`);
        expect(request.headers["stripe-version"]).toBe(API_VERSION);
        for (const name of request.params) {
          for (const segment of name.split(/[[\]]+/).filter(Boolean)) expect(PERSONAL).not.toContain(segment);
        }
      }
      const [create, , , cancel] = server.requests;
      expect(create?.values).toEqual({
        mode: "subscription",
        "line_items[0][price]": "price_test_halvo_local",
        success_url: "http://127.0.0.1:8787/v1/done",
        client_reference_id: ACCOUNT,
        "subscription_data[metadata][halvo_account]": ACCOUNT,
        expires_at: String(Math.ceil((NOW + 1_860_000) / 1000)),
        "allowed_payment_method_types[0]": "card",
      });
      expect(create?.params.filter((name) => name.includes("metadata"))).toEqual(["subscription_data[metadata][halvo_account]"]);
      expect(create?.params.some((name) => /trial|billing_cycle_anchor|payment_method_types\]?$|automatic_tax/.test(name.replace("allowed_", "")))).toBe(false);
      expect(cancel?.values).toEqual({ invoice_now: "true" });
      expect(cancel?.params).not.toContain("prorate");
    } finally {
      await server.close();
    }
  });

  test("an empty base URL, key or price makes each method answer failed and opens no connection", async () => {
    const server = await startRecorder(() => ({ status: 200, body: "{}" }));
    try {
      for (const over of [{ baseUrl: "" }, { apiKey: "" }]) {
        const port = makeStripePort(config(server.url, over));
        expect(await port.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:1/v1/done", expiresAt: NOW })).toEqual({ kind: "failed" });
        expect(await port.readSession("cs_1")).toEqual({ kind: "failed" });
        expect(await port.openManagePage({ customerId: "cus_1", returnUrl: "http://127.0.0.1:1/v1/done" })).toEqual({ kind: "failed" });
        expect(await port.cancelSubscription("sub_1")).toEqual({ kind: "failed" });
        expect(await port.cancelSubscription(null)).toEqual({ kind: "nothing-to-cancel" });
        expect(await port.sendUsage({ accountId: ACCOUNT, customerId: "c", identifier: "a:0", amountMinor: 1, timestamp: 0 })).toEqual({ kind: "failed" });
        expect(await port.readUsageTotal({ accountId: ACCOUNT, customerId: "c", from: 0, to: 1 })).toEqual({ kind: "failed" });
      }
      const noPrice = makeStripePort(config(server.url, { priceId: "" }));
      expect(await noPrice.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:1/v1/done", expiresAt: NOW })).toEqual({ kind: "failed" });
      expect(await makeStripePort(config(server.url, { signingSecret: "" })).verifyWebhook(encoder.encode("{}"), new Headers({ "stripe-signature": header("{}") }), NOW)).toEqual({
        kind: "rejected",
      });
      expect(server.requests).toEqual([]);
    } finally {
      await server.close();
    }
  });

  test("a URL longer than URL_MAX_LENGTH, or a session answer that lacks a field, is failed", async () => {
    const long = `http://127.0.0.1:2/${"x".repeat(URL_MAX_LENGTH)}`;
    let reply = JSON.stringify({ id: "cs_1", url: long, expires_at: 1 });
    const server = await startRecorder(() => ({ status: 200, body: reply }));
    try {
      const port = makeStripePort(config(server.url));
      const start = () => port.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:1/v1/done", expiresAt: NOW });
      expect(await start()).toEqual({ kind: "failed" });
      reply = JSON.stringify({ url: long });
      expect(await port.openManagePage({ customerId: "cus_1", returnUrl: "http://127.0.0.1:1/v1/done" })).toEqual({ kind: "failed" });
      reply = JSON.stringify({ id: "cs_1", url: "http://127.0.0.1:2/c" });
      expect(await start()).toEqual({ kind: "failed" });
      reply = JSON.stringify({ id: "cs_other", status: "open" });
      expect(await port.readSession("cs_1")).toEqual({ kind: "failed" });
      reply = JSON.stringify({ id: "cs_1", status: "complete", customer: null, subscription: null, payment_status: "paid" });
      expect(await port.readSession("cs_1")).toEqual({ kind: "failed" });
      reply = JSON.stringify({ id: "cs_1", status: "complete", customer: "cus_1", subscription: null, payment_status: "unpaid", customer_email: BUYER.email });
      expect(await port.readSession("cs_1")).toEqual({
        kind: "complete",
        session: { sessionId: "cs_1", customerId: "cus_1", subscriptionId: null, payment: "unpaid" },
      });
      reply = JSON.stringify({ id: "cs_1", status: "expired" });
      expect(await port.readSession("cs_1")).toEqual({ kind: "expired" });
    } finally {
      await server.close();
    }
  });

  test.each([
    ["the cancel succeeds", [{ status: 200, body: JSON.stringify({ id: "sub_1", status: "canceled" }) }], "cancelled"],
    ["a 2xx cancel whose body shows the subscription active", [{ status: 200, body: JSON.stringify({ id: "sub_1", status: "active" }) }], "failed"],
    ["a 2xx cancel with an empty object", [{ status: 200, body: "{}" }], "failed"],
    ["a 2xx cancel of another subscription", [{ status: 200, body: JSON.stringify({ id: "sub_2", status: "canceled" }) }], "failed"],
    ["a refusal, then a read that says canceled", [{ status: 400, body: ERROR() }, { status: 200, body: JSON.stringify({ id: "sub_1", status: "canceled" }) }], "nothing-to-cancel"],
    [
      "a refusal, then a read that says incomplete_expired",
      [{ status: 400, body: ERROR() }, { status: 200, body: JSON.stringify({ id: "sub_1", status: "incomplete_expired" }) }],
      "nothing-to-cancel",
    ],
    ["a refusal, then a read of another subscription", [{ status: 400, body: ERROR() }, { status: 200, body: JSON.stringify({ id: "sub_2", status: "canceled" }) }], "failed"],
    ["a refusal, then a read of an unknown id", [{ status: 404, body: MISSING }, { status: 404, body: MISSING }], "nothing-to-cancel"],
    ["a refusal, then a 404 with a known error type and no code", [{ status: 404, body: ERROR() }, { status: 404, body: ERROR() }], "failed"],
    [
      "a refusal, then a 404 with another code",
      [{ status: 404, body: ERROR() }, { status: 404, body: JSON.stringify({ error: { type: "invalid_request_error", code: "parameter_unknown", message: "refused" } }) }],
      "failed",
    ],
    ["a refusal, then a read that says active", [{ status: 400, body: ERROR() }, { status: 200, body: JSON.stringify({ id: "sub_1", status: "active" }) }], "failed"],
    ["a refusal, then a read that fails", [{ status: 400, body: ERROR() }, { status: 500, body: ERROR("api_error") }], "failed"],
    ["a refusal, then a 404 without the error object", [{ status: 400, body: ERROR() }, { status: 404, body: "gone" }], "failed"],
  ] as const)("cancelSubscription: %s answers %s", async (_, replies, want) => {
    let at = 0;
    const server = await startRecorder(() => replies[Math.min(at++, replies.length - 1)] ?? { status: 500, body: "" });
    try {
      expect(await makeStripePort(config(server.url)).cancelSubscription("sub_1")).toEqual({ kind: want });
      expect(server.requests.map((each) => `${each.method} ${each.path}`)).toEqual(
        replies.length === 1 ? ["DELETE /v1/subscriptions/sub_1"] : ["DELETE /v1/subscriptions/sub_1", "GET /v1/subscriptions/sub_1"],
      );
    } finally {
      await server.close();
    }
  });

  test("a cancel that meets no answer is failed and reads nothing", async () => {
    const closed = await freePort();
    expect(await makeStripePort(config(`http://127.0.0.1:${closed}`)).cancelSubscription("sub_1")).toEqual({ kind: "failed" });
    const server = await startRecorder((request) => (request.method === "DELETE" ? "drop" : { status: 404, body: ERROR() }));
    try {
      expect(await makeStripePort(config(server.url)).cancelSubscription("sub_1")).toEqual({ kind: "failed" });
      expect(server.requests.map((each) => each.method)).toEqual(["DELETE"]);
    } finally {
      await server.close();
    }
  });

  test(
    "every call that gets no answer ends within PROVIDER_CALL_TIMEOUT_MS, counted over all requests of the method, and is failed",
    async () => {
      const hang = await startRecorder(() => "hang");
      const slowRefusal = await startRecorder((request) =>
        request.method === "DELETE" ? { status: 400, body: ERROR(), afterMs: PROVIDER_CALL_TIMEOUT_MS * 0.6 } : "hang",
      );
      try {
        const port = makeStripePort(config(hang.url));
        const timed = async (run: () => Promise<{ kind: string }>) => {
          const began = Date.now();
          const answer = await run();
          return { kind: answer.kind, took: Date.now() - began };
        };
        const results = await Promise.all([
          timed(() => port.readSession("cs_1")),
          timed(() => port.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:1/v1/done", expiresAt: NOW })),
          timed(() => port.openManagePage({ customerId: "cus_1", returnUrl: "http://127.0.0.1:1/v1/done" })),
          timed(() => port.cancelSubscription("sub_1")),
          timed(() => makeStripePort(config(slowRefusal.url)).cancelSubscription("sub_1")),
        ]);
        for (const result of results) {
          expect(result.kind).toBe("failed");
          expect(result.took).toBeGreaterThanOrEqual(PROVIDER_CALL_TIMEOUT_MS - 50);
          expect(result.took).toBeLessThan(PROVIDER_CALL_TIMEOUT_MS + 2_000);
        }
        expect(slowRefusal.requests.map((each) => each.method)).toEqual(["DELETE", "GET"]);
        expect(PROVIDER_CALL_TIMEOUT_MS).toBeLessThan(300 * 1000);
      } finally {
        await hang.close();
        await slowRefusal.close();
      }
    },
    PROVIDER_CALL_TIMEOUT_MS + 10_000,
  );

  test("a URL of exactly URL_MAX_LENGTH characters is kept, and an expiry that is no whole number of seconds is failed", async () => {
    const exact = `http://127.0.0.1:2/${"x".repeat(URL_MAX_LENGTH - "http://127.0.0.1:2/".length)}`;
    expect(exact).toHaveLength(URL_MAX_LENGTH);
    let reply = JSON.stringify({ id: "cs_1", url: exact, expires_at: 1_794_001_860 });
    const server = await startRecorder(() => ({ status: 200, body: reply }));
    try {
      const port = makeStripePort(config(server.url));
      const start = () => port.startCheckout({ accountId: ACCOUNT, successUrl: "http://127.0.0.1:1/v1/done", expiresAt: NOW });
      expect(await start()).toEqual({ kind: "session", sessionId: "cs_1", url: exact, expiresAt: 1_794_001_860_000 });
      reply = JSON.stringify({ url: exact });
      expect(await port.openManagePage({ customerId: "cus_1", returnUrl: "http://127.0.0.1:1/v1/done" })).toEqual({ kind: "page", url: exact });
      reply = JSON.stringify({ url: `${exact}x` });
      expect(await port.openManagePage({ customerId: "cus_1", returnUrl: "http://127.0.0.1:1/v1/done" })).toEqual({ kind: "failed" });
      reply = JSON.stringify({ id: "cs_1", url: exact, expires_at: 1_794_001_860.5 });
      expect(await start()).toEqual({ kind: "failed" });
    } finally {
      await server.close();
    }
  });
});

describe("the webhook signature and the reduction", () => {
  const body = JSON.stringify(completedEvent(), null, 2);

  test("the header that the SDK's test helper builds is accepted, and the event keeps ids and state only", async () => {
    const verified = await verify(body, header(body));
    expect(verified).toEqual({
      kind: "event",
      event: {
        kind: "checkout-completed",
        eventId: "evt_test_1",
        providerTime: 1_794_000_000_000,
        accountId: ACCOUNT,
        sessionId: "cs_test_1",
        customerId: "cus_test_1",
        subscriptionId: "sub_test_1",
        payment: "paid",
      },
    });
    for (const value of Object.values(BUYER)) expect(JSON.stringify(verified)).not.toContain(value);
  });

  test("a tampered, re-serialized, foreign, stale, early, missing or malformed signature is rejected", async () => {
    const tampered = body.replace('"created": 1794000000', '"created": 1794000001');
    expect(tampered).not.toBe(body);
    expect(await verify(tampered, header(body))).toEqual({ kind: "rejected" });
    expect(await verify(JSON.stringify(JSON.parse(body)), header(body))).toEqual({ kind: "rejected" });
    expect(await verify(body, header(body, { secret: `${SECRET}_other` }))).toEqual({ kind: "rejected" });
    expect(await verify(body, header(body, { timestamp: NOW / 1000 - 301 }))).toEqual({ kind: "rejected" });
    expect(await verify(body, header(body, { timestamp: NOW / 1000 + 301 }))).toEqual({ kind: "rejected" });
    expect((await verify(body, header(body, { timestamp: NOW / 1000 - 300 }))).kind).toBe("event");
    expect(await verify(body, null)).toEqual({ kind: "rejected" });
    expect(await verify(body, header(body).replace(/,v1=.*/, ""))).toEqual({ kind: "rejected" });
    expect(await verify(body, `t=${NOW / 1000},v1=nothex`)).toEqual({ kind: "rejected" });
    expect(await verify(body, header(body), NOW, "")).toEqual({ kind: "rejected" });
  });

  test("a signed body that is no event is rejected", async () => {
    for (const text of ["not json", "[]", JSON.stringify({ id: "evt_1", type: "x" }), JSON.stringify({ id: "evt_1", type: "x", created: 1, data: {} })]) {
      expect(await verify(text, header(text))).toEqual({ kind: "rejected" });
    }
  });

  test.each(["customer.subscription.created", "customer.subscription.updated", "customer.subscription.deleted"])(
    "the event type %s reduces to a subscription event",
    async (type) => {
      const text = JSON.stringify({ ...subscriptionEvent("canceled"), type }, null, 2);
      expect(await verify(text, header(text))).toEqual(expect.objectContaining({ event: expect.objectContaining({ kind: "subscription", state: "cancelled" }) }));
    },
  );

  test("an event without a creation time is no event, and a subscription without a customer or a whole period is reduced with care", async () => {
    const { created: _, ...withoutCreated } = subscriptionEvent("active");
    const noTime = JSON.stringify(withoutCreated, null, 2);
    expect(await verify(noTime, header(noTime))).toEqual({ kind: "rejected" });
    const noCustomer = JSON.stringify(subscriptionEvent("active", { customer: null }), null, 2);
    expect(await verify(noCustomer, header(noCustomer))).toEqual({ kind: "event", event: { kind: "ignored" } });
    const halfPeriod = JSON.stringify(subscriptionEvent("active", { items: { object: "list", data: [{ id: "si_1", current_period_start: 1_794_000_000 }] } }), null, 2);
    expect(await verify(halfPeriod, header(halfPeriod))).toEqual(expect.objectContaining({ event: expect.objectContaining({ period: null }) }));
    const shapeless = JSON.stringify(completedEvent({ subscription: { object: "subscription" } }), null, 2);
    expect(await verify(shapeless, header(shapeless))).toEqual({ kind: "event", event: { kind: "ignored" } });
    const server = await startRecorder(() => ({
      status: 200,
      body: JSON.stringify({ id: "cs_1", status: "complete", customer: "cus_1", subscription: { object: "subscription" }, payment_status: "paid" }),
    }));
    try {
      expect(await makeStripePort(config(server.url)).readSession("cs_1")).toEqual({ kind: "failed" });
    } finally {
      await server.close();
    }
  });

  test("events of other types, and events with no usable account id, are ignored", async () => {
    const ignored = [
      { ...completedEvent(), type: "checkout.session.expired" },
      { ...completedEvent(), type: "invoice.paid" },
      { ...completedEvent(), type: "invoice.payment_failed" },
      completedEvent({ client_reference_id: "not-an-account-id" }),
      completedEvent({ client_reference_id: ACCOUNT.toUpperCase() }),
      completedEvent({ customer: null }),
      subscriptionEvent("active", { metadata: {} }),
      subscriptionEvent("active", { metadata: { halvo_account: "x" } }),
    ];
    for (const each of ignored) {
      const text = JSON.stringify(each, null, 2);
      expect(await verify(text, header(text))).toEqual({ kind: "event", event: { kind: "ignored" } });
    }
  });

  test.each([
    ["active", "active"],
    ["canceled", "cancelled"],
    ["incomplete_expired", "cancelled"],
    ["past_due", "payment-failed"],
    ["unpaid", "payment-failed"],
    ["incomplete", "payment-failed"],
    ["trialing", "payment-failed"],
    ["paused", "payment-failed"],
  ])("a subscription with the status %s reduces to %s with its period", async (status, state) => {
    const text = JSON.stringify(subscriptionEvent(status), null, 2);
    expect(await verify(text, header(text))).toEqual({
      kind: "event",
      event: {
        kind: "subscription",
        eventId: "evt_test_2",
        providerTime: 1_794_000_100_000,
        accountId: ACCOUNT,
        subscriptionId: "sub_test_1",
        customerId: "cus_test_1",
        state,
        period: { start: 1_794_000_000_000, end: 1_796_592_000_000 },
        endedAt: null,
      },
    });
  });

  test("a subscription whose items carry no period reduces with a null period, and the session's payment values map one to one", async () => {
    const text = JSON.stringify(subscriptionEvent("active", { items: { object: "list", data: [] } }), null, 2);
    expect(await verify(text, header(text))).toEqual(expect.objectContaining({ event: expect.objectContaining({ period: null }) }));
    for (const [given, want] of [
      ["unpaid", "unpaid"],
      ["no_payment_required", "no-payment-required"],
    ]) {
      const each = JSON.stringify(completedEvent({ payment_status: given, subscription: null }), null, 2);
      expect(await verify(each, header(each))).toEqual(
        expect.objectContaining({ event: expect.objectContaining({ payment: want, subscriptionId: null }) }),
      );
    }
  });

  test("the recorder reads the names of the query and of the form body in their order", () => {
    expect(paramsOf("/x?b=1&a=2", "c=3&a=4").names).toEqual(["b", "a", "c", "a"]);
  });
});
