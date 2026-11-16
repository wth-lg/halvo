import { describe, expect, test } from "vitest";
import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import { driven, runs } from "./u56p2-provider-world.ts";

const HOUR = 3_600_000;
const BAD_ID = "sub_\ud800bad";

describe("X1: the stored id goes first until a call settles it; then no try asks for it (P7-119 Addition 1)", () => {
  test("a stored id whose first try gets no answer goes first again; the try that ends it says so, the column holds it, and no later try makes a request for it", async () => {
    const { ending, w } = await driven({ plans: 30, stored: true, delayMs: 1000, down: (attempt) => attempt === 1 });
    w.close();
    expect({
      storedRequests: ending.asked.filter((each) => each.path === "/v1/subscriptions/sub_1").map((each) => `${each.attempt} ${each.method}`),
      settledFor: ending.storedSettledFor,
      runs: runs(ending.answers),
      gone: ending.goneAfterMs !== null,
      live: ending.live,
    }).toEqual({
      storedRequests: ["1 DELETE", "2 DELETE"],
      settledFor: "sub_1",
      runs: ["failed 0/0 x1", "failed 7/0 x1", "failed 8/0 x2", "cancelled x1"],
      gone: true,
      live: [],
    });
  });

  test("a stored id that the provider does not know is settled by its read in the second try whose customer read shows the key, with one cancel-stored-unknown line; no later try asks for it (P7-120 N2, the two-try form)", async () => {
    const { ending, w } = await driven({ plans: 30, stored: "sub_gone", delayMs: 1000 });
    w.close();
    expect({
      storedRequests: ending.asked.filter((each) => each.path === "/v1/subscriptions/sub_gone").map((each) => `${each.attempt} ${each.method}`),
      settledFor: ending.storedSettledFor,
      lines: ending.lines["cancel-stored-unknown"] ?? 0,
      gone: ending.goneAfterMs !== null,
    }).toEqual({ storedRequests: ["1 DELETE", "1 GET", "2 DELETE", "2 GET"], settledFor: "sub_gone", lines: 1, gone: true });
  });
});

describe("X3: an id that cannot be sent unchanged makes no request and is never a throw", () => {
  test("a listed subscription whose id cannot be sent: no request for it, the walk ends the other one, the delete stays open with cancel-unanswered at most once an hour, and no destroy-error or alarm-error", async () => {
    const { ending, w } = await driven({ plans: 1, stored: true, delayMs: 1000, extra: [{ id: BAD_ID, status: "active" }], days: 2 });
    w.close();
    const unanswered = ending.lines["cancel-unanswered"] ?? 0;
    expect({
      badRequests: ending.asked.filter((each) => each.path.includes("bad")).length,
      live: ending.live,
      gone: ending.goneAfterMs,
      errors: (ending.lines["destroy-error"] ?? 0) + (ending.lines["alarm-error"] ?? 0),
      linesHourly: unanswered >= 1 && unanswered <= 49,
      first: ending.answers[0],
    }).toEqual({ badRequests: 0, live: [BAD_ID], gone: null, errors: 0, linesHourly: true, first: "failed 1/1" });
  });
});

describe("X3's cursor with no server (pr-review-35 finding 1, its case f1-04)", () => {
  test("a live id that cannot be sent, last on a full first page behind entries that ended: the cursor is the page's last sendable id, page 2's live subscription is cancelled, no request carries the bad id, and the call answers failed 1/1", async () => {
    const bad = "sub_\uD800";
    const order = [...Array.from({ length: 99 }, (_, k) => ({ id: `sub_x${String(k).padStart(3, "0")}`, status: "incomplete_expired" })), { id: bad, status: "active" }, { id: "sub_live2", status: "active" }];
    const ended = new Set<string>();
    const asked: string[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
      const url = new URL(String(input));
      const method = init?.method ?? "GET";
      asked.push(`${method} ${url.pathname} ${url.searchParams.get("starting_after") ?? "-"}`);
      if (url.pathname === "/v1/customers/cus_1") return new Response(JSON.stringify({ id: "cus_1", object: "customer", livemode: false }), { status: 200 });
      if (url.pathname === "/v1/subscriptions" && method === "GET") {
        const shown = order.filter((e) => !ended.has(e.id));
        const after = url.searchParams.get("starting_after");
        const rest = shown.slice(after === null ? 0 : shown.findIndex((e) => e.id === after) + 1);
        const limit = Number(url.searchParams.get("limit"));
        const data = rest.slice(0, limit).map((e) => ({ id: e.id, object: "subscription", status: e.status, customer: "cus_1" }));
        return new Response(JSON.stringify({ object: "list", has_more: rest.length > limit, data }), { status: 200 });
      }
      if (url.pathname === "/v1/subscriptions/sub_live2" && method === "DELETE") {
        ended.add("sub_live2");
        return new Response(JSON.stringify({ id: "sub_live2", object: "subscription", status: "canceled", customer: "cus_1" }), { status: 200 });
      }
      return new Response("{}", { status: 500 });
    }) as typeof fetch;
    try {
      const port = makeStripePort({ baseUrl: "https://provider.invalid", apiKey: "sk_test_x", signingSecret: "whsec_x", priceId: "price_x" }, () => 0);
      const got = await port.cancelSubscription(null, "cus_1", "unsettled");
      expect({ got, asked, ended: [...ended] }).toEqual({
        got: { kind: "failed", cancelled: 1, unended: 1, stored: "unsettled" },
        asked: ["GET /v1/customers/cus_1 -", "GET /v1/subscriptions -", "GET /v1/subscriptions sub_x098", "DELETE /v1/subscriptions/sub_live2 -", "GET /v1/subscriptions -"],
        ended: ["sub_live2"],
      });
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe("X4 and X5: what the call counts and what the object writes", () => {
  test("R4 (a): a listed cancel answered 2xx canceled that the same call's last whole list still shows counts for nothing; the pause doubles to the cap and cancel-unanswered is written", async () => {
    const { ending, w } = await driven({ plans: 1, stored: true, delayMs: 1000, stale: ["sub_00002"], days: 1 });
    w.close();
    const unanswered = ending.lines["cancel-unanswered"] ?? 0;
    expect({
      first: ending.answers.slice(0, 2),
      capped: ending.waits.includes(HOUR),
      lines: unanswered >= 1 && unanswered <= 25,
      gone: ending.goneAfterMs,
    }).toEqual({ first: ["failed 0/1", "failed 0/0"], capped: true, lines: true, gone: null });
  });

  test("R4 (c): a subscription refused on every try writes cancel-unanswered once an hour although each try ends a new subscription (6 hours of tries at 60 s)", async () => {
    const { ending, w } = await driven({
      plans: 0,
      stored: false,
      noFirstPlan: true,
      delayMs: 1000,
      refused: ["sub_refused"],
      extra: [{ id: "sub_refused", status: "active" }],
      beforeTry: (attempt, made) => {
        if (attempt > 1) made(`sub_new_${attempt}`);
      },
      days: 0.25,
    });
    w.close();
    const unanswered = ending.lines["cancel-unanswered"] ?? 0;
    expect({ waits: [...new Set(ending.waits)], lines: unanswered, live: ending.live, gone: ending.goneAfterMs }).toEqual({
      waits: [60_000],
      lines: 7,
      live: ["sub_refused"],
      gone: null,
    });
  });
});

describe("X6: after a last whole list of more than one page the first page is read again", () => {
  test("R1: a subscription made after the last whole list's first page is answered, with a cancel in the call: the call answers failed, the storage stays, and the next try ends it", async () => {
    const { ending, w } = await driven({
      plans: 150,
      status: "incomplete_expired",
      extra: [{ id: "sub_live", status: "active" }],
      stored: false,
      delayMs: 100,
      beforeList: (attempt, n, made) => {
        if (attempt === 1 && n === 4) made("sub_made_during_the_last_list");
      },
    });
    w.close();
    expect({ answers: ending.answers, live: ending.live, gone: ending.goneAfterMs !== null }).toEqual({
      answers: ["failed 2/0", "cancelled"],
      live: [],
      gone: true,
    });
  });

  test("R1 with no cancel in the call: 150 entries that ended and a subscription made between the two pages of the walk: failed, then the next try ends it", async () => {
    const { ending, w } = await driven({
      plans: 150,
      status: "incomplete_expired",
      stored: false,
      noFirstPlan: true,
      delayMs: 100,
      beforeList: (attempt, n, made) => {
        if (attempt === 1 && n === 2) made("sub_made_between_the_pages");
      },
    });
    w.close();
    expect({ answers: ending.answers, live: ending.live, gone: ending.goneAfterMs !== null }).toEqual({
      answers: ["failed 0/0", "cancelled"],
      live: [],
      gone: true,
    });
  });

  test("the pages one call can read: with the first page read again, 1,800 entries that ended are read whole in one try (18 pages, the customer's read and the re-read: 20 requests), and 1,801 are never read whole", async () => {
    const whole = await driven({ plans: 1800, status: "incomplete_expired", stored: false, noFirstPlan: true, delayMs: 10 });
    whole.w.close();
    const over = await driven({ plans: 1801, status: "incomplete_expired", stored: false, noFirstPlan: true, delayMs: 10, days: 1 });
    over.w.close();
    expect({ whole: whole.ending.answers, overGone: over.ending.goneAfterMs, overFirst: over.ending.answers.slice(0, 2) }).toEqual({
      whole: ["nothing-to-cancel"],
      overGone: null,
      overFirst: ["failed 0/0", "failed 0/0"],
    });
  });
});
