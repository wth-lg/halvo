import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CALL_REQUEST_LIMIT, makeStripePort, REQUEST_START_MARGIN_MS, type StripeConfig } from "../../src/billing/stripe/adapter.ts";
import { PROVIDER_CALL_TIMEOUT_MS } from "../../src/billing/stripe/timeout.ts";
import { startRecorder } from "./servers.ts";
import { config, CUSTOMER, json, line, live, MISSING, type Model, provider, REFUSED, type Reply } from "./u56p2-provider.ts";

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

async function cancel(model: Model, subscriptionId: string | null, customerId: string | null = CUSTOMER, over: Partial<StripeConfig> = {}, clock?: () => number) {
  const server = await startRecorder(provider(model));
  try {
    const port = makeStripePort(config(server.url, over), clock);
    const answer = await port.cancelSubscription(subscriptionId, customerId, "unsettled");
    return { answer, requests: server.requests.map(line), recorded: [...server.requests], lines: [...logged] };
  } finally {
    await server.close();
  }
}

describe("the walk of the customer's list (contract 2 from unit 5.6 part 2; P7-86 E, P7-117)", () => {
  test("a second subscription on the customer: the stored id's cancel, the customer's read, the list, the listed cancel with the same request, and a whole list read after it: cancelled", async () => {
    const run = await cancel({ subs: [live("sub_1"), live("sub_2")] }, "sub_1");
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "cancelled" },
      requests: ["DELETE /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
    });
    for (const request of run.recorded.filter((each) => each.method === "DELETE")) {
      expect({ params: request.params, values: request.values }).toEqual({ params: ["invoice_now"], values: { invoice_now: "true" } });
    }
    for (const request of run.recorded.filter((each) => each.path === "/v1/subscriptions")) {
      expect({ params: request.params, values: request.values }).toEqual({ params: ["customer", "limit"], values: { customer: CUSTOMER, limit: "100" } });
    }
  });

  test("an account with a customer and no stored id: the customer's read, the list, the cancel of its subscription and a whole list after it: cancelled", async () => {
    const run = await cancel({ subs: [live("sub_1")] }, null);
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "cancelled" },
      requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_1", "GET /v1/subscriptions"],
    });
  });

  test("a list in two pages: the first page's cancels come before the second page is asked, which starts after the first page's last entry, and the whole list is read again after the last cancel", async () => {
    const ended = Array.from({ length: 99 }, (_, index) => live(`sub_ie_${index}`, "incomplete_expired"));
    const run = await cancel({ subs: [...ended, live("sub_a"), live("sub_b")] }, null);
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "cancelled" },
      requests: [
        `GET /v1/customers/${CUSTOMER}`,
        "GET /v1/subscriptions",
        "DELETE /v1/subscriptions/sub_a",
        "GET /v1/subscriptions after sub_a",
        "DELETE /v1/subscriptions/sub_b",
        "GET /v1/subscriptions",
      ],
    });
    expect(run.recorded.filter((each) => each.path === "/v1/subscriptions").map((each) => each.params)).toEqual([
      ["customer", "limit"],
      ["customer", "limit", "starting_after"],
      ["customer", "limit"],
    ]);
  });

  test("T1: a customer with one incomplete_expired subscription: the list is asked with the customer, limit=100 and no status, nothing is cancelled, and the call ends: nothing-to-cancel", async () => {
    const run = await cancel({ subs: [live("sub_ie", "incomplete_expired")] }, null);
    expect({ answer: run.answer, requests: run.requests, list: run.recorded[1]?.values }).toEqual({
      answer: { kind: "nothing-to-cancel" },
      requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"],
      list: { customer: CUSTOMER, limit: "100" },
    });
  });

  test("an entry whose status is outside the spec's list has not ended and is cancelled", async () => {
    const run = await cancel({ subs: [live("sub_odd", "a_status_of_tomorrow")] }, null);
    expect({ answer: run.answer, requests: run.requests.slice(2, 3) }).toEqual({ answer: { kind: "cancelled" }, requests: ["DELETE /v1/subscriptions/sub_odd"] });
  });

  test("S2: a refused cancel of a listed subscription whose read shows it ended: the read follows the refused cancel, nothing counts, and the call ends on the whole list after it", async () => {
    const run = await cancel({ subs: [live("sub_2")], endsOnRefusal: ["sub_2"] }, null);
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "nothing-to-cancel" },
      requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
    });
  });

  test("T8: a listed subscription refused on every try while its read shows it live: the walk goes past it and ends the next one, and the last whole list keeps the answer failed with the count 1", async () => {
    const run = await cancel({ subs: [live("sub_2"), live("sub_3")], refused: ["sub_2"] }, null);
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "failed", cancelled: 1, unended: 1, stored: "unsettled" },
      requests: [
        `GET /v1/customers/${CUSTOMER}`,
        "GET /v1/subscriptions",
        "DELETE /v1/subscriptions/sub_2",
        "GET /v1/subscriptions/sub_2",
        "DELETE /v1/subscriptions/sub_3",
        "GET /v1/subscriptions",
      ],
    });
  });

  test("a subscription made at the provider during the call, before the last whole list, is caught by that list: failed with the count of what this call ended", async () => {
    const run = await cancel(
      {
        subs: [live("sub_2")],
        onCancel: (id, subs) => {
          if (id === "sub_2") subs.push(live("sub_new"));
        },
      },
      null,
    );
    expect({ answer: run.answer, last: run.requests.at(-1) }).toEqual({ answer: { kind: "failed", cancelled: 1, unended: 0, stored: "unsettled" }, last: "GET /v1/subscriptions" });
  });
});

const PAGE_FAULTS: [string, Reply][] = [
  ["a 404", { status: 404, body: MISSING }],
  ["a 500", { status: 500, body: REFUSED }],
  ["no answer", "drop"],
  ["a body that is no JSON", { status: 200, body: "not json" }],
  ["an object that is not a list", json({ object: "search_result", has_more: false, data: [] })],
  ["no data array", json({ object: "list", has_more: false })],
  ["has_more that is not a boolean", json({ object: "list", has_more: "false", data: [] })],
  ["an entry of another customer", json({ object: "list", has_more: false, data: [live("sub_x", "active", "cus_other")] })],
  ["an entry with no string id", json({ object: "list", has_more: false, data: [{ id: 7, status: "active", customer: CUSTOMER }] })],
  ["an entry with no string status", json({ object: "list", has_more: false, data: [{ id: "sub_x", customer: CUSTOMER }] })],
  ["has_more true on an empty page", json({ object: "list", has_more: true, data: [] })],
];

describe("T3: a page that is a fault is the answer failed, never the end of the list", () => {
  test.each(PAGE_FAULTS)("the first page answers %s: failed with the count 0, one cancel-list-fault line and no cancel", async (_, fault) => {
    const run = await cancel({ subs: [live("sub_1")], page: (index, reply) => (index === 0 ? fault : reply) }, null);
    expect({ answer: run.answer, requests: run.requests, lines: run.lines }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"],
      lines: ['{"event":"cancel-list-fault"}'],
    });
  });

  test.each(PAGE_FAULTS)("the second page answers %s: failed with the count of the first page's cancel, and no whole list is taken", async (_, fault) => {
    const ended = Array.from({ length: 99 }, (_, index) => live(`sub_ie_${index}`, "incomplete_expired"));
    const run = await cancel({ subs: [...ended, live("sub_a"), live("sub_b")], page: (index, reply) => (index === 1 ? fault : reply) }, null);
    expect({ answer: run.answer, requests: run.requests, lines: run.lines }).toEqual({
      answer: { kind: "failed", cancelled: 1, unended: 0, stored: "unsettled" },
      requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_a", "GET /v1/subscriptions after sub_a"],
      lines: ['{"event":"cancel-list-fault"}'],
    });
  });

  test("the whole list after the last cancel answers a fault: failed with the count, and one cancel-list-fault line", async () => {
    const run = await cancel({ subs: [live("sub_1")], page: (index, reply) => (index === 1 ? { status: 500, body: REFUSED } : reply) }, null);
    expect({ answer: run.answer, lines: run.lines }).toEqual({ answer: { kind: "failed", cancelled: 1, unended: 0, stored: "unsettled" }, lines: ['{"event":"cancel-list-fault"}'] });
  });
});

describe("T5 and U2: the read of the customer", () => {
  test("a customer that the provider shows as deleted: the read of the customer, no list, and nothing-to-cancel", async () => {
    const run = await cancel({ subs: [live("sub_1")], customer: json({ id: CUSTOMER, object: "customer", deleted: true }) }, null);
    expect({ answer: run.answer, requests: run.requests, lines: run.lines }).toEqual({ answer: { kind: "nothing-to-cancel" }, requests: [`GET /v1/customers/${CUSTOMER}`], lines: [] });
  });

  test("U4: an unanswered cancel of the stored id and a customer shown as deleted: failed, and the next try decides", async () => {
    const run = await cancel({ subs: [live("sub_1")], dropped: ["sub_1"], customer: json({ id: CUSTOMER, object: "customer", deleted: true }) }, "sub_1");
    expect({ answer: run.answer, requests: run.requests }).toEqual({ answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" }, requests: ["DELETE /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`] });
  });

  test("the stored id ended by this call and a customer shown as deleted: cancelled", async () => {
    const run = await cancel({ subs: [live("sub_1")], customer: json({ id: CUSTOMER, object: "customer", deleted: true }) }, "sub_1");
    expect({ answer: run.answer, requests: run.requests }).toEqual({ answer: { kind: "cancelled" }, requests: ["DELETE /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`] });
  });

  test.each([
    ["a 404 with the provider's error object", { status: 404, body: MISSING }],
    ["a 404 with no error object", { status: 404, body: "gone" }],
    ["a customer of live mode under a key of test mode", json({ id: CUSTOMER, object: "customer", livemode: true })],
  ] as [string, Reply][])("a customer that the provider does not know or shows in the other mode (%s): failed, one cancel-customer-unknown line, no list", async (_, reply) => {
    const run = await cancel({ subs: [live("sub_1")], customer: reply }, null);
    expect({ answer: run.answer, requests: run.requests, lines: run.lines }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      requests: [`GET /v1/customers/${CUSTOMER}`],
      lines: ['{"event":"cancel-customer-unknown"}'],
    });
  });

  test("a key that names no mode shows no customer as its own: failed, one cancel-customer-unknown line", async () => {
    const run = await cancel({ subs: [live("sub_1")] }, null, CUSTOMER, { apiKey: "halvo_key_of_no_mode" });
    expect({ answer: run.answer, lines: run.lines }).toEqual({ answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" }, lines: ['{"event":"cancel-customer-unknown"}'] });
  });

  test("a key of live mode goes on with a customer of live mode to the list", async () => {
    const run = await cancel({ subs: [], customer: json({ id: CUSTOMER, object: "customer", livemode: true }) }, null, CUSTOMER, { apiKey: "rk_live_halvo" });
    expect({ answer: run.answer, requests: run.requests }).toEqual({ answer: { kind: "nothing-to-cancel" }, requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"] });
  });

  test.each([
    ["no answer", "drop"],
    ["a 500", { status: 500, body: REFUSED }],
    ["a body that is no JSON", { status: 200, body: "not json" }],
    ["a body that is a subscription", json({ id: CUSTOMER, object: "subscription", livemode: false })],
    ["a customer of another id", json({ id: "cus_other", object: "customer", livemode: false })],
    ["a customer with no livemode", json({ id: CUSTOMER, object: "customer" })],
    ["a deleted customer of another id", json({ id: "cus_other", object: "customer", deleted: true })],
  ] as [string, Reply][])("U2: a read of the customer that answers %s is failed, with no line and no list", async (_, reply) => {
    const run = await cancel({ subs: [live("sub_1")], customer: reply }, null);
    expect({ answer: run.answer, requests: run.requests, lines: run.lines }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      requests: [`GET /v1/customers/${CUSTOMER}`],
      lines: [],
    });
  });
});

describe("U1, U3, U4 and U6: the stored id and the count", () => {
  test("U1: a stored id that the provider does not know and a customer that it does not know: failed, after the read of the customer, and the unknown answer does not count (P7-120 N2: the stored id stays unsettled)", async () => {
    const run = await cancel({ subs: [], customer: { status: 404, body: MISSING } }, "sub_gone");
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      requests: ["DELETE /v1/subscriptions/sub_gone", "GET /v1/subscriptions/sub_gone", `GET /v1/customers/${CUSTOMER}`],
    });
  });

  test("U1: the stored id refused and read as ended, with a second subscription that has not ended on the list: the second is cancelled", async () => {
    const run = await cancel({ subs: [live("sub_1", "canceled"), live("sub_2")] }, "sub_1");
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "cancelled" },
      requests: [
        "DELETE /v1/subscriptions/sub_1",
        "GET /v1/subscriptions/sub_1",
        `GET /v1/customers/${CUSTOMER}`,
        "GET /v1/subscriptions",
        "DELETE /v1/subscriptions/sub_2",
        "GET /v1/subscriptions",
      ],
    });
  });

  test("U6: a customer whose only subscription is the stored id, ended by this call: cancelled", async () => {
    const run = await cancel({ subs: [live("sub_1")] }, "sub_1");
    expect(run.answer).toEqual({ kind: "cancelled" });
  });

  test("U3: the stored id ended by this call and a fault of the list: failed with the count 0", async () => {
    const run = await cancel({ subs: [live("sub_1")], page: () => ({ status: 500, body: REFUSED }) }, "sub_1");
    expect(run.answer).toEqual({ kind: "failed", cancelled: 0, unended: 0, stored: "settled" });
  });

  test("U4: an unanswered cancel of the stored id is failed whatever the list shows, and the walk still ends the listed subscription", async () => {
    const run = await cancel({ subs: [live("sub_1", "canceled"), live("sub_2")], dropped: ["sub_1"] }, "sub_1");
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "failed", cancelled: 1, unended: 0, stored: "unsettled" },
      requests: ["DELETE /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
    });
  });

  const UNREAD_BODIES = [
    ["a body that is no JSON", "not json"],
    ["an empty body", ""],
    ["a JSON array", "[]"],
    ["JSON null", "null"],
  ] as const;
  const twoHundredWith = (model: Model, id: string, body: string) => {
    const inner = provider(model);
    return (request: Parameters<typeof inner>[0]): Reply => {
      if (request.method === "DELETE" && request.path === `/v1/subscriptions/${id}`) {
        const found = model.subs.find((each) => each.id === id);
        if (found !== undefined) found.status = "canceled";
        return { status: 200, body };
      }
      return inner(request);
    };
  };

  test.each(UNREAD_BODIES)("the stored id's cancel answers 2xx with %s: an answer the adapter does not know, unanswered with no read of the id, and failed whatever the list shows", async (_, body) => {
    const model: Model = { subs: [live("sub_1")], customer: json({ id: CUSTOMER, object: "customer", deleted: true }) };
    const server = await startRecorder(twoHundredWith(model, "sub_1", body));
    try {
      const answer = await makeStripePort(config(server.url)).cancelSubscription("sub_1", CUSTOMER, "unsettled");
      expect({ answer, requests: server.requests.map(line) }).toEqual({
        answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
        requests: ["DELETE /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`],
      });
    } finally {
      await server.close();
    }
  });

  test.each(UNREAD_BODIES)("a listed cancel answers 2xx with %s: no read of the id follows a 2xx, it counts nothing, and the last whole list decides", async (_, body) => {
    const model: Model = { subs: [live("sub_2")] };
    const server = await startRecorder(twoHundredWith(model, "sub_2", body));
    try {
      const answer = await makeStripePort(config(server.url)).cancelSubscription(null, CUSTOMER, "unsettled");
      expect({ answer, requests: server.requests.map(line) }).toEqual({
        answer: { kind: "nothing-to-cancel" },
        requests: [`GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions", "DELETE /v1/subscriptions/sub_2", "GET /v1/subscriptions"],
      });
    } finally {
      await server.close();
    }
  });

  test("U4: an unanswered cancel of the stored id, with an empty list: failed with the count 0", async () => {
    const run = await cancel({ subs: [live("sub_1", "canceled")], dropped: ["sub_1"] }, "sub_1");
    expect({ answer: run.answer, requests: run.requests }).toEqual({
      answer: { kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" },
      requests: ["DELETE /v1/subscriptions/sub_1", `GET /v1/customers/${CUSTOMER}`, "GET /v1/subscriptions"],
    });
  });
});

describe("the bound of one call (contract 3.6; P7-117 (d), S7, U5)", () => {
  const many = (count: number) => Array.from({ length: count }, (_, index) => live(`sub_${String(index).padStart(3, "0")}`));

  test("CALL_REQUEST_LIMIT is 20 and REQUEST_START_MARGIN_MS is 500, 20 x 500 ms = PROVIDER_CALL_TIMEOUT_MS", () => {
    expect([CALL_REQUEST_LIMIT, REQUEST_START_MARGIN_MS, CALL_REQUEST_LIMIT * REQUEST_START_MARGIN_MS]).toEqual([20, 500, PROVIDER_CALL_TIMEOUT_MS]);
  });

  test.each([
    ["no stored id", null, [], 18],
    ["a stored id whose cancel goes through", "sub_000", [], 17],
    ["a stored id whose cancel is refused and read", "sub_000", ["sub_000"], 16],
  ] as [string, string | null, string[], number][])("one page of 30 subscriptions with %s: 20 requests, then failed with the count of the listed cancels", async (_, stored, endedBefore, listed) => {
    const subs = many(30);
    for (const each of subs) if (endedBefore.includes(each.id)) each.status = "canceled";
    const run = await cancel({ subs }, stored);
    expect({ requests: run.requests.length, deletes: run.requests.filter((each) => each.startsWith("DELETE")).length - (stored === null ? 0 : 1), answer: run.answer }).toEqual({
      requests: 20,
      deletes: listed,
      answer: { kind: "failed", cancelled: listed, unended: 0, stored: stored !== null ? "settled" : "unsettled" },
    });
  });

  test("REQUEST_START_MARGIN_MS: no request starts with less than 500 ms of the call's deadline left (980 ms a request: the eleventh would start with 200 ms left)", async () => {
    let now = 0;
    const run = await cancel({ subs: many(30), tick: () => (now += 980) }, null, CUSTOMER, {}, () => now);
    expect({ requests: run.requests.length, answer: run.answer }).toEqual({ requests: 10, answer: { kind: "failed", cancelled: 8, unended: 0, stored: "unsettled" } });
  });

  test.each([
    ["with a stored id among them", "sub_000", 16, 13],
    ["with no stored id", null, 14, 19],
  ] as [string, string | null, number, number][])("a customer of 250 subscriptions that have not ended, %s, ends in the counted number of calls", async (_, stored, calls, lastRequests) => {
    const model: Model = { subs: many(250) };
    const server = await startRecorder(provider(model));
    try {
      const port = makeStripePort(config(server.url));
      const answers: string[] = [];
      let before = 0;
      const requestsOfEach: number[] = [];
      for (let each = 0; each < 40; each += 1) {
        const answer = await port.cancelSubscription(stored, CUSTOMER, "unsettled");
        answers.push(answer.kind === "failed" ? `failed ${answer.cancelled}` : answer.kind);
        requestsOfEach.push(server.requests.length - before);
        before = server.requests.length;
        if (answer.kind !== "failed") break;
      }
      expect({
        calls: answers.length,
        last: answers.at(-1),
        lastRequests: requestsOfEach.at(-1),
        most: Math.max(...requestsOfEach),
        live: model.subs.filter((each) => each.status !== "canceled").length,
      }).toEqual({ calls, last: "cancelled", lastRequests, most: 20, live: 0 });
    } finally {
      await server.close();
    }
  });
});

describe("T4: no address or no key", () => {
  test("with a customer id every answer is failed and no connection opens; only an account with neither id gets nothing-to-cancel", async () => {
    const server = await startRecorder(provider({ subs: [live("sub_1")] }));
    try {
      for (const over of [{ baseUrl: "" }, { apiKey: "" }]) {
        const port = makeStripePort(config(server.url, over));
        expect(await port.cancelSubscription(null, CUSTOMER, "unsettled")).toEqual({ kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" });
        expect(await port.cancelSubscription("sub_1", CUSTOMER, "unsettled")).toEqual({ kind: "failed", cancelled: 0, unended: 0, stored: "unsettled" });
        expect(await port.cancelSubscription(null, null, "unsettled")).toEqual({ kind: "nothing-to-cancel" });
      }
      expect(server.requests).toEqual([]);
    } finally {
      await server.close();
    }
  });
});
