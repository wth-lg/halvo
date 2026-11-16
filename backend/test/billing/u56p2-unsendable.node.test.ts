import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import type { Recorded } from "./servers.ts";
import { config, CUSTOMER, line, live, type Model, provider, type Sub } from "./u56p2-provider.ts";

const BAD = "sub_\ud800bad";

let logged: string[] = [];
beforeEach(() => {
  logged = [];
  vi.spyOn(console, "log").mockImplementation((text: string) => {
    logged.push(text);
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

async function call(model: Model, subscriptionId: string | null, customerId: string, storedSettled = false) {
  const answer = provider(model);
  const asked: string[] = [];
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input));
    const params: string[] = [];
    const values: Record<string, string> = {};
    const body = typeof init?.body === "string" ? init.body : "";
    for (const [name, value] of [...url.searchParams, ...new URLSearchParams(body)]) {
      params.push(name);
      values[name] = value;
    }
    const request: Recorded = { method: init?.method ?? "GET", path: url.pathname, params, values, headers: {}, body };
    asked.push(line(request));
    const reply = answer(request);
    if (reply === "drop" || reply === "hang") throw new TypeError("fetch failed");
    return new Response(reply.body, { status: reply.status });
  }) as typeof fetch;
  try {
    const got = await makeStripePort(config("https://provider.invalid")).cancelSubscription(subscriptionId, customerId, storedSettled);
    return { answer: got, asked, lines: [...logged] };
  } finally {
    globalThis.fetch = original;
  }
}

const ended = (count: number): Sub[] => Array.from({ length: count }, (_, index) => live(`sub_ie_${String(index).padStart(2, "0")}`, "incomplete_expired"));

describe("X1 in the adapter: a stored id that an earlier call settled gets no request", () => {
  test("called with the stored id settled: no request for it, the customer's read, the list, the listed cancel and the last list", async () => {
    const run = await call({ subs: [live("sub_1", "canceled"), live("sub_2")] }, "sub_1", CUSTOMER, true);
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "cancelled" },
      asked: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
    });
  });

  test("a call that ends the stored id says so in its failed answer, and a call that finds it unknown says so too", async () => {
    const endedHere = await call({ subs: [live("sub_1")], page: () => ({ status: 500, body: "" }) }, "sub_1", CUSTOMER);
    const unknown = await call({ subs: [], page: () => ({ status: 500, body: "" }) }, "sub_gone", CUSTOMER);
    expect({ endedHere: endedHere.answer, unknown: unknown.answer }).toEqual({
      endedHere: { kind: "failed", cancelled: 0, unended: 0, storedSettled: true },
      unknown: { kind: "failed", cancelled: 0, unended: 0, storedSettled: true },
    });
  });
});

describe("X3: an id that cannot be sent unchanged", () => {
  test("the bad id last on a page of 100 behind entries that ended, with live entries on page 2: no request for it, the cursor is the page's last id that can be sent, the live entries are cancelled, and the call answers failed with its counts", async () => {
    const run = await call({ subs: [...ended(99), live(BAD), live("sub_a"), live("sub_b")] }, null, CUSTOMER);
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "failed", cancelled: 2, unended: 1, storedSettled: false },
      asked: [
        `GET /v1/customers/${CUSTOMER}`,
        "GET /v1/subscriptions",
        "GET /v1/subscriptions after sub_ie_98",
        "DELETE /v1/subscriptions/sub_a",
        "DELETE /v1/subscriptions/sub_b",
        "GET /v1/subscriptions",
      ],
    });
  });

  test("a page that says more follows and holds no id that can be sent is a page fault", async () => {
    const bad = Array.from({ length: 101 }, (_, index) => live(`sub_\ud800${index}`));
    const run = await call({ subs: bad }, null, CUSTOMER);
    expect({ answer: run.answer, asked: run.asked, lines: run.lines }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 100, storedSettled: false },
      asked: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"],
      lines: ['{"event":"cancel-list-fault"}'],
    });
  });

  test("a stored id that cannot be sent: no request for it, never a throw, never settled; the call goes on and answers failed", async () => {
    const run = await call({ subs: [live("sub_2")] }, BAD, CUSTOMER);
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "failed", cancelled: 1, unended: 0, storedSettled: false },
      asked: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
    });
  });

  test("a customer id that cannot be sent: the stored id's cancel, then failed with no request for the customer, never a throw", async () => {
    const run = await call({ subs: [live("sub_1")] }, "sub_1", "cus_\udc00");
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, storedSettled: true },
      asked: ["DELETE /v1/subscriptions/sub_1"],
    });
  });
});

describe("X4 and X6 in the adapter", () => {
  test("X4: a listed cancel answered 2xx canceled that the same call's last whole list still shows: the count drops it, and the second count holds it", async () => {
    const model: Model = {
      subs: [live("sub_2")],
      onCancel: (_, subs) => {
        // A new object in the list, so the DELETE's own reply still shows the subscription canceled.
        const index = subs.findIndex((each) => each.id === "sub_2");
        const shown = subs[index];
        if (shown !== undefined) subs[index] = { ...shown, status: "active" };
      },
    };
    const run = await call(model, null, CUSTOMER);
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 1, storedSettled: false },
      asked: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
    });
  });

  test("X6: after a last whole list of two pages the first page is read again, and the call ends when that page holds nothing that has not ended", async () => {
    const run = await call({ subs: [live("sub_live"), ...ended(150)] }, null, CUSTOMER);
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "cancelled" },
      asked: [
        `GET /v1/customers/${CUSTOMER}`,
        "GET /v1/subscriptions",
        "DELETE /v1/subscriptions/sub_live",
        "GET /v1/subscriptions after sub_ie_98",
        "GET /v1/subscriptions",
        "GET /v1/subscriptions after sub_ie_99",
        "GET /v1/subscriptions",
      ],
    });
  });
});
