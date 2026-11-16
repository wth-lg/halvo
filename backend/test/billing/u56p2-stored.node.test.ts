import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { StoredIdState } from "../../src/billing/port.ts";
import { makeStripePort, type StripeConfig } from "../../src/billing/stripe/adapter.ts";

// Round three (ruling P7-120 with its Addition 1, N1, N2 and N5; R2's copies a35 and a16): the stored id's three states
// in the adapter, with a stand-in fetch that answers each request by a function of its method and path.

const CUSTOMER = "cus_1";
const CONFIG: StripeConfig = { baseUrl: "https://provider.invalid", apiKey: "sk_test_stored", signingSecret: "whsec_stored", priceId: "price_stored" };
const MISSING = { error: { type: "invalid_request_error", code: "resource_missing", message: "No such object", param: "id" } };
const REFUSED = { error: { type: "invalid_request_error", message: "refused" } };
const GOES_ON = { id: CUSTOMER, object: "customer", livemode: false };
const sub = (id: string, status: string) => ({ id, object: "subscription", status, customer: CUSTOMER });
const page = (data: unknown[], hasMore = false) => ({ object: "list", has_more: hasMore, data });

type Reply = { status: number; body: unknown } | "drop";
type Provider = (method: string, path: string, query: URLSearchParams) => Reply;

let lines: string[] = [];
beforeEach(() => {
  lines = [];
  vi.spyOn(console, "log").mockImplementation((text: string) => {
    lines.push(text);
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

/** One call of the real adapter; `msPerRequest` moves the adapter's clock by that much for each request it makes. */
async function call(provider: Provider, stored: StoredIdState, o: { subscriptionId?: string | null; customerId?: string | null; msPerRequest?: number; config?: StripeConfig } = {}) {
  const asked: string[] = [];
  let now = 0;
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input));
    const method = init?.method ?? "GET";
    asked.push(`${method} ${url.pathname}`);
    now += o.msPerRequest ?? 0;
    const reply = provider(method, url.pathname, url.searchParams);
    if (reply === "drop") throw new TypeError("fetch failed");
    return new Response(JSON.stringify(reply.body), { status: reply.status });
  }) as typeof fetch;
  try {
    const port = makeStripePort(o.config ?? CONFIG, () => now);
    const answer = await port.cancelSubscription(o.subscriptionId === undefined ? "sub_1" : o.subscriptionId, o.customerId === undefined ? CUSTOMER : o.customerId, stored);
    return { answer, asked, lines: [...lines] };
  } finally {
    globalThis.fetch = original;
  }
}

/** A provider that does not know the stored id, with a customer read and an empty list that can be replaced. */
function unknownStored(customer: Reply = { status: 200, body: GOES_ON }): Provider {
  return (_method, path) => {
    if (path === "/v1/subscriptions/sub_1") return { status: 404, body: MISSING };
    if (path === `/v1/customers/${CUSTOMER}`) return customer;
    if (path === "/v1/subscriptions") return { status: 200, body: page([]) };
    return { status: 500, body: "" };
  };
}

const UNKNOWN_LINE = '{"event":"cancel-stored-unknown"}';

describe("N2: an unknown answer counts only after a customer read that shows the key", () => {
  test("the customer goes on and the list is empty: the first counted unknown is kept and the call answers failed, with no line", async () => {
    const run = await call(unknownStored(), "unsettled");
    expect(run).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unknown-once" },
      asked: ["DELETE /v1/subscriptions/sub_1", "GET /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"],
      lines: [],
    });
  });

  test("the second counted unknown, in a later call: the id is settled, one cancel-stored-unknown line, and the call ends as the list says", async () => {
    const run = await call(unknownStored(), "unknown-once");
    expect({ answer: run.answer, lines: run.lines }).toEqual({ answer: { kind: "nothing-to-cancel" }, lines: [UNKNOWN_LINE] });
  });

  test.each([
    ["the customer answers 404 (a key of the other mode)", { status: 404, body: MISSING }, ['{"event":"cancel-customer-unknown"}']],
    ["the customer is of the other mode", { status: 200, body: { ...GOES_ON, livemode: true } }, ['{"event":"cancel-customer-unknown"}']],
    ["the customer's read answers 500", { status: 500, body: { error: { type: "api_error", message: "down" } } }, []],
    ["the customer's read gets no answer", "drop", []],
  ] as const)("%s: the unknown answer counts for nothing, the given state stays, and the call answers failed", async (_, customer, customerLines) => {
    const first = await call(unknownStored(customer as Reply), "unsettled");
    const kept = await call(unknownStored(customer as Reply), "unknown-once");
    expect({ first: first.answer, kept: kept.answer, lines: first.lines }).toEqual({
      first: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      kept: { kind: "failed", cancelled: 0, unended: 0, stored: "unknown-once" },
      lines: customerLines,
    });
  });

  test("a call stopped by the deadline before the customer's read: the unknown answer counts for nothing", async () => {
    const run = await call(unknownStored(), "unsettled", { msPerRequest: 4_800 });
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      asked: ["DELETE /v1/subscriptions/sub_1", "GET /v1/subscriptions/sub_1"],
    });
  });

  test("a customer shown deleted shows the key: the first counted unknown answers failed, and the second ends the call as T5 says, with the line", async () => {
    const deleted: Reply = { status: 200, body: { id: CUSTOMER, object: "customer", deleted: true } };
    const first = await call(unknownStored(deleted), "unsettled");
    const second = await call(unknownStored(deleted), "unknown-once");
    expect({ first: first.answer, second: second.answer, lines: [...first.lines, ...second.lines] }).toEqual({
      first: { kind: "failed", cancelled: 0, unended: 0, stored: "unknown-once" },
      second: { kind: "nothing-to-cancel" },
      lines: [UNKNOWN_LINE],
    });
  });

  test("an account with no customer id: an unknown answer never counts, and the call answers failed after the stored id's requests alone (N4)", async () => {
    const run = await call(unknownStored(), "unsettled", { customerId: null });
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      asked: ["DELETE /v1/subscriptions/sub_1", "GET /v1/subscriptions/sub_1"],
    });
  });
});

describe("N1: ended and not answered", () => {
  test("an ended answer after a kept first unknown settles the id as ended, with no line", async () => {
    const run = await call(
      (method, path) => {
        if (path === "/v1/subscriptions/sub_1" && method === "DELETE") return { status: 200, body: sub("sub_1", "canceled") };
        if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: GOES_ON };
        return { status: 200, body: page([]) };
      },
      "unknown-once",
    );
    expect({ answer: run.answer, lines: run.lines }).toEqual({ answer: { kind: "cancelled" }, lines: [] });
  });

  test("a refusal read as live after a kept first unknown starts the count again: failed with the stored id unsettled and unended", async () => {
    const run = await call(
      (method, path) => {
        if (path === "/v1/subscriptions/sub_1") return method === "DELETE" ? { status: 400, body: REFUSED } : { status: 200, body: sub("sub_1", "active") };
        if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: GOES_ON };
        return { status: 200, body: page([]) };
      },
      "unknown-once",
    );
    expect(run.answer).toEqual({ kind: "failed", cancelled: 0, unended: 1, stored: "unsettled" });
  });

  test("a call that stops during the stored id's own requests after a kept first unknown starts the count again", async () => {
    const run = await call(
      (method, path) => (path === "/v1/subscriptions/sub_1" && method === "DELETE" ? { status: 400, body: REFUSED } : { status: 200, body: GOES_ON }),
      "unknown-once",
      { msPerRequest: 9_600 },
    );
    expect({ answer: run.answer, asked: run.asked }).toEqual({ answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" }, asked: ["DELETE /v1/subscriptions/sub_1"] });
  });

  test("with no provider key or address the call answers failed with no request, keeps settled, and starts a kept first unknown again", async () => {
    const keyless = { ...CONFIG, apiKey: "" };
    const kept = await call(unknownStored(), "unknown-once", { config: keyless });
    const settled = await call(unknownStored(), "settled", { config: keyless });
    expect({ kept: kept.answer, settled: settled.answer, asked: [...kept.asked, ...settled.asked] }).toEqual({
      kept: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      settled: { kind: "failed", cancelled: 0, unended: 0, stored: "settled" },
      asked: [],
    });
  });
});

describe("R2: the verifier's copies a35 and a16", () => {
  test("a35: a refused cancel of the stored id whose read gets no answer leaves it unsettled, and the call answers failed", async () => {
    const run = await call(
      (method, path) => {
        if (path === "/v1/subscriptions/sub_1") return method === "DELETE" ? { status: 400, body: REFUSED } : "drop";
        if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: GOES_ON };
        return { status: 200, body: page([]) };
      },
      "unsettled",
    );
    expect(run.answer).toEqual({ kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" });
  });

  test("a16: a listed entry whose status is canceled has ended: no cancel for it, and the call ends (the provider's default list hides such entries; this holds the rule if one shows)", async () => {
    const run = await call(
      (_method, path) => {
        if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: GOES_ON };
        if (path === "/v1/subscriptions") return { status: 200, body: page([sub("sub_old", "canceled")]) };
        return { status: 400, body: REFUSED };
      },
      "unsettled",
      { subscriptionId: null },
    );
    expect({ answer: run.answer, asked: run.asked }).toEqual({
      answer: { kind: "nothing-to-cancel" },
      asked: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"],
    });
  });
});

describe("N5: a stored id that its own requests left unanswered and that the walk ends", () => {
  function walkEnds(storedDelete: Reply, readAfter: Reply, laterShows: boolean): Provider {
    let ended = false;
    let deletes = 0;
    return (method, path) => {
      if (path === "/v1/subscriptions/sub_1" && method === "DELETE") {
        deletes += 1;
        if (deletes === 1) return storedDelete;
        ended = true;
        return { status: 200, body: sub("sub_1", "canceled") };
      }
      if (path === "/v1/subscriptions/sub_1") return readAfter;
      if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: GOES_ON };
      if (path === "/v1/subscriptions") return { status: 200, body: page(ended && !laterShows ? [] : [sub("sub_1", "active")]) };
      return { status: 500, body: "" };
    };
  }

  test.each([
    ["refused and read as live", { status: 400, body: REFUSED }, { status: 200, body: sub("sub_1", "active") }],
    ["unanswered", "drop", { status: 500, body: "" }],
  ] as const)("its own cancel %s, then the walk ends it and the last whole list does not show it: settled, not unended, and the call still failed", async (_, storedDelete, readAfter) => {
    const run = await call(walkEnds(storedDelete as Reply, readAfter as Reply, false), "unsettled");
    expect(run.answer).toEqual({ kind: "failed", cancelled: 1, unended: 0, stored: "settled" });
  });

  test("the walk ends it on a list of two pages, and the first page read again after the last whole list faults: not settled, and unended (X6's read decides)", async () => {
    const entries = [sub("sub_1", "active"), ...Array.from({ length: 150 }, (_, k) => sub(`sub_e${String(k).padStart(3, "0")}`, "incomplete_expired"))];
    let ended = false;
    let deletes = 0;
    let firstPages = 0;
    const run = await call((method, path, query) => {
      if (path === "/v1/subscriptions/sub_1" && method === "DELETE") {
        deletes += 1;
        if (deletes === 1) return { status: 400, body: REFUSED };
        ended = true;
        return { status: 200, body: sub("sub_1", "canceled") };
      }
      if (path === "/v1/subscriptions/sub_1") return { status: 200, body: sub("sub_1", ended ? "canceled" : "active") };
      if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: GOES_ON };
      if (path === "/v1/subscriptions") {
        const shown = entries.filter((each) => !(ended && each.id === "sub_1"));
        const after = query.get("starting_after");
        if (after === null) {
          firstPages += 1;
          if (firstPages === 3) return { status: 500, body: { error: { type: "api_error", message: "down" } } };
        }
        const rest = after === null ? shown : shown.slice(shown.findIndex((each) => each.id === after) + 1);
        return { status: 200, body: page(rest.slice(0, 100), rest.length > 100) };
      }
      return { status: 500, body: "" };
    }, "unsettled");
    expect(run.answer).toEqual({ kind: "failed", cancelled: 1, unended: 1, stored: "unsettled" });
  });

  test("the walk ends it but the last whole list still shows it: not settled, and unended", async () => {
    const run = await call(walkEnds({ status: 400, body: REFUSED }, { status: 200, body: sub("sub_1", "active") }, true), "unsettled");
    expect(run.answer).toEqual({ kind: "failed", cancelled: 0, unended: 1, stored: "unsettled" });
  });
});
