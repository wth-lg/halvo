import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering, SYNC_RETRY_MS } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import { type ProviderFake, startProviderFake } from "../../test-kit/provider-fake/fake.mjs";
import { makeTestHost } from "../../test-kit/sql/node.ts";
import { PROVIDER_API_KEY, type Recorded, startRecorder, startStripeMock, WEBHOOK_SIGNING_SECRET } from "./servers.ts";

const ACCOUNT = "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";
const HASH = "a".repeat(64);

type Json = Record<string, unknown>;

let mock: { url: string; stop(): Promise<void> };
let entry: { url: string; requests: Recorded[]; close(): Promise<void> };
let fake: ProviderFake;
let port: ProviderPort;

beforeAll(async () => {
  mock = await startStripeMock();
  entry = await startRecorder(() => ({ status: 200, body: "" }));
  fake = await startProviderFake({ mockUrl: mock.url, apiKey: PROVIDER_API_KEY, signingSecret: WEBHOOK_SIGNING_SECRET, entries: [entry.url] });
  port = makeStripePort({ baseUrl: fake.origin, apiKey: PROVIDER_API_KEY, signingSecret: WEBHOOK_SIGNING_SECRET, priceId: "price_test_halvo_local" });
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

async function paidCustomer(): Promise<{ customer: string; subscription: string }> {
  const started = await port.startCheckout({ accountId: ACCOUNT, successUrl: `${entry.url}/v1/done`, expiresAt: Date.now() + 1_860_000 });
  if (started.kind !== "session") throw new Error("the fake made no session");
  const paid = await call("/test/pay", { session: started.sessionId });
  const subscription = String(paid.subscription);
  return { customer: String((await call(`/test/subscriptions/${subscription}`)).customer), subscription };
}

async function statuses(customer: string): Promise<Record<string, number>> {
  const state = await call(`/test/customers/${customer}`);
  const counted: Record<string, number> = {};
  for (const each of state.subscriptions as { status: string }[]) counted[each.status] = (counted[each.status] ?? 0) + 1;
  return counted;
}

async function requestsSince(count: number): Promise<{ method: string; path: string; params: string[] }[]> {
  const all = (await call("/test/requests")).requests as { method: string; path: string; params: string[] }[];
  return all.slice(count).map((each) => ({ method: each.method, path: each.path.replace(/(cus|sub)_test_[0-9a-f]+/g, "{id}"), params: each.params }));
}

async function requestCount(): Promise<number> {
  return ((await call("/test/requests")).requests as unknown[]).length;
}

describe("the provider fake's customer and list (contract 7 from unit 5.6 part 2), through the adapter", () => {
  test("a second subscription on one customer: the call ends both at the fake", async () => {
    const paid = await paidCustomer();
    expect((await call("/test/subscribe", { customer: paid.customer })).result).toBe("subscribed");
    const answer = await port.cancelSubscription(paid.subscription, paid.customer, false);
    expect({ answer, statuses: await statuses(paid.customer) }).toEqual({ answer: { kind: "cancelled" }, statuses: { canceled: 2 } });
  });

  test("a list in two pages: the first page's cancel comes before the second page, which starts after the first page's last entry", async () => {
    const paid = await paidCustomer();
    await call("/test/subscribe", { customer: paid.customer });
    await call("/test/subscribe", { customer: paid.customer, status: "incomplete_expired", count: 100 });
    await call("/test/subscribe", { customer: paid.customer });
    const before = await requestCount();
    const answer = await port.cancelSubscription(null, paid.customer, false);
    expect({ answer, statuses: await statuses(paid.customer), requests: await requestsSince(before) }).toEqual({
      answer: { kind: "cancelled" },
      statuses: { canceled: 3, incomplete_expired: 100 },
      requests: [
        { method: "GET", path: "/v1/customers/{id}", params: [] },
        { method: "GET", path: "/v1/subscriptions", params: ["customer", "limit"] },
        { method: "DELETE", path: "/v1/subscriptions/{id}", params: ["invoice_now"] },
        { method: "GET", path: "/v1/subscriptions", params: ["customer", "limit", "starting_after"] },
        { method: "DELETE", path: "/v1/subscriptions/{id}", params: ["invoice_now"] },
        { method: "DELETE", path: "/v1/subscriptions/{id}", params: ["invoice_now"] },
        { method: "GET", path: "/v1/subscriptions", params: ["customer", "limit"] },
      ],
    });
  });

  test("T1: a customer with one incomplete_expired subscription: the list is asked with no status and the call ends with nothing to cancel", async () => {
    const started = await port.startCheckout({ accountId: ACCOUNT, successUrl: `${entry.url}/v1/done`, expiresAt: Date.now() + 1_860_000 });
    if (started.kind !== "session") throw new Error("the fake made no session");
    await call("/test/complete-unpaid", { session: started.sessionId });
    const customer = String((await call(`/test/sessions/${started.sessionId}`)).customer);
    await call("/test/subscribe", { customer, status: "incomplete_expired" });
    const before = await requestCount();
    const answer = await port.cancelSubscription(null, customer, false);
    expect({ answer, requests: await requestsSince(before) }).toEqual({
      answer: { kind: "nothing-to-cancel" },
      requests: [
        { method: "GET", path: "/v1/customers/{id}", params: [] },
        { method: "GET", path: "/v1/subscriptions", params: ["customer", "limit"] },
      ],
    });
  });

  test("a customer that the provider shows as deleted: the read of the customer, and nothing to cancel", async () => {
    const paid = await paidCustomer();
    expect((await call("/test/delete-customer", { customer: paid.customer })).result).toBe("deleted");
    const before = await requestCount();
    const answer = await port.cancelSubscription(null, paid.customer, false);
    expect({ answer, requests: await requestsSince(before) }).toEqual({
      answer: { kind: "nothing-to-cancel" },
      requests: [{ method: "GET", path: "/v1/customers/{id}", params: [] }],
    });
  });

  test("a customer that the provider does not know, with no stored id: the read of the customer is made and the answer is failed", async () => {
    const before = await requestCount();
    const answer = await port.cancelSubscription(null, "cus_test_000000000000000000000000", false);
    expect({ answer, requests: await requestsSince(before) }).toEqual({
      answer: { kind: "failed", cancelled: 0 },
      requests: [{ method: "GET", path: "/v1/customers/{id}", params: [] }],
    });
  });

  test("T8: a listed subscription whose cancel the fake refuses on every try keeps the answer failed, while the stored id is ended", async () => {
    const paid = await paidCustomer();
    const second = String(((await call("/test/subscribe", { customer: paid.customer })).subscriptions as string[])[0]);
    expect((await call("/test/refuse-cancel", { subscription: second })).result).toBe("refusing");
    const answer = await port.cancelSubscription(paid.subscription, paid.customer, false);
    expect({ answer, statuses: await statuses(paid.customer) }).toEqual({ answer: { kind: "failed", cancelled: 0 }, statuses: { active: 1, canceled: 1 } });
  });
});

describe("a customer of 250 subscriptions that have not ended, deleted through the object (contract 3.6, P7-117 (d) and U5)", () => {
  test.each([
    ["with the stored id among them", true, 16],
    ["with no stored id", false, 14],
  ] as const)("%s: the delete ends in the counted number of runs, each after a failed run with progress at the first pause", async (_, stored, runs) => {
    const paid = await paidCustomer();
    await call("/test/subscribe", { customer: paid.customer, count: 249 });
    let now: Ms = 1_800_000_000_000;
    const calls: string[] = [];
    const counted: ProviderPort = {
      ...port,
      async cancelSubscription(id, customerId, settled) {
        const answer = await port.cancelSubscription(id, customerId, settled);
        calls.push(answer.kind === "failed" ? `failed ${answer.cancelled}` : answer.kind);
        return answer;
      },
    };
    const test = makeTestHost({ port: counted, clock: () => now });
    const entries = makeEntries(test.host, {
      accounts: makeAccounts(),
      ledger: makeLedger({ holdLifeMs: test.host.config.holdLifeMs }),
      metering: makeMetering({ intervalSeconds: test.host.config.intervalSeconds, marginSeconds: test.host.config.marginSeconds }),
    });
    const lines: string[] = [];
    const original = console.log;
    console.log = (line: string) => lines.push(line);
    try {
      expect((await entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", customerId: paid.customer })).result).toBe("seeded");
      if (stored) {
        await entries.webhookEvent({
          kind: "subscription",
          eventId: "evt_stored",
          providerTime: now - 1000,
          accountId: ACCOUNT,
          subscriptionId: paid.subscription,
          customerId: paid.customer,
          state: "active",
          period: null,
          endedAt: null,
        });
      }
      const waits: Ms[] = [];
      let answer = await entries.destroy(HASH);
      for (let each = 0; each < 40 && answer.result !== "deleted"; each += 1) {
        const snapshot = await entries.testInspect();
        if (snapshot.result !== "snapshot") break;
        const next = Number(snapshot.tables.skeleton?.[0]?.cancel_next_at);
        waits.push(next - now);
        now = next;
        await entries.alarm();
        answer = (await entries.testInspect()).result === "snapshot" ? { result: "not-finished" } : { result: "deleted" };
      }
      expect({
        runs: calls.length,
        last: calls.at(-1),
        waits: [...new Set(waits)],
        storage: (await entries.testInspect()).result,
        statuses: await statuses(paid.customer),
        unanswered: lines.filter((line) => line.includes("cancel-unanswered")).length,
      }).toEqual({ runs, last: "cancelled", waits: [SYNC_RETRY_MS], storage: "no-such-account", statuses: { canceled: 250 }, unanswered: 0 });
    } finally {
      console.log = original;
      test.close();
    }
  });
});
