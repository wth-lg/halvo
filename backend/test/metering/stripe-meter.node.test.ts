import { readFileSync } from "node:fs";
import { createServer as httpServer, type IncomingMessage, type ServerResponse } from "node:http";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { PROVIDER_CALL_TIMEOUT_MS, providerPort } from "../../src/billing/index.ts";
import { PROVIDER_CALL_TIMEOUT_MS as ADAPTER_TIMEOUT_MS } from "../../src/billing/stripe/timeout.ts";
import { API_VERSION } from "../../src/billing/stripe/adapter.ts";
import { REFUSAL_OF_CODE } from "../../src/metering/stripe-meter.ts";
import { type Env, MICRO_PER_MINOR } from "../../src/types.ts";
import { localValues } from "../../test-kit/provider-fake/main.mjs";
import { freePort, ROOT, type Recorded, startRecorder, startStripeMock } from "../billing/servers.ts";
import { fakeMeter, providerOver } from "./stripe-meter-provider.ts";

const VARS = localValues(`${ROOT}stack.d/vars.env`);
const LISTS = JSON.parse(readFileSync(`${ROOT}test/metering/stripe-meter-params.json`, "utf8")) as Record<string, string[]>;
const ACCOUNT = "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";
const CUSTOMER = "cus_Test_Meter_1";
const START = 1_794_000_000;
const IDENTIFIER = `${ACCOUNT}:${START}`;
const USAGE = { accountId: ACCOUNT, customerId: CUSTOMER, identifier: IDENTIFIER, amountMinor: 12, timestamp: START + 3_599 };

function env(baseUrl: string, over: Record<string, string> = {}): Env {
  return { ...VARS, PROVIDER_BASE_URL: baseUrl, ...over } as unknown as Env;
}

function absent(baseUrl: string, name: string): Env {
  return Object.fromEntries(Object.entries({ ...VARS, PROVIDER_BASE_URL: baseUrl }).filter(([key]) => key !== name)) as unknown as Env;
}

function endpoint(request: Recorded): string {
  return `${request.method} ${request.path.replace(/^\/v1\/billing\/meters\/[^/]+\//, "/v1/billing/meters/{id}/")}`;
}

function echo(request: Recorded, status = 200) {
  return { status, body: JSON.stringify({ object: "billing.meter_event", identifier: request.values.identifier, event_name: request.values.event_name }) };
}

describe("against the pinned stripe-mock", () => {
  let mock: { url: string; stop(): Promise<void> };
  beforeAll(async () => {
    mock = await startStripeMock();
  });
  afterAll(async () => {
    await mock.stop();
  });

  test("stripe-mock accepts each meter-event request that the mapping sends", async () => {
    const statuses: string[] = [];
    const real = globalThis.fetch;
    globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
      const response = await real(...args);
      statuses.push(`${args[1]?.method} ${String(args[0]).replace(mock.url, "")} ${response.status}`);
      return response;
    };
    const answers: string[] = [];
    try {
      const port = providerPort(env(mock.url));
      for (const amountMinor of [1, 12, 9_007_199_254_741, Number.MAX_SAFE_INTEGER]) {
        for (const timestamp of [START - 1, START + 3_599]) {
          answers.push((await port.sendUsage({ ...USAGE, amountMinor, timestamp, identifier: `${ACCOUNT}:${timestamp + 1}` })).kind);
        }
      }
    } finally {
      globalThis.fetch = real;
    }
    expect(statuses).toEqual(Array.from({ length: 8 }, () => "POST /v1/billing/meter_events 200"));
    expect(answers).toEqual(Array.from({ length: 8 }, () => "recorded"));
  });

  test("stripe-mock accepts each event-summary request that the mapping sends, and its fixture is no total of the range asked", async () => {
    const statuses: string[] = [];
    const real = globalThis.fetch;
    globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
      const response = await real(...args);
      statuses.push(`${args[1]?.method} ${String(args[0]).replace(mock.url, "").replace(/\?.*$/, "")} ${response.status}`);
      return response;
    };
    const answers: unknown[] = [];
    try {
      const port = providerPort(env(mock.url));
      for (const [from, to] of [
        [START + 3_599, START + 3_600],
        [START, START + 3_600],
        [START + 1, START + 61],
      ]) {
        answers.push(await port.readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from: from ?? 0, to: to ?? 0 }));
      }
    } finally {
      globalThis.fetch = real;
    }
    expect(statuses).toEqual(Array.from({ length: 3 }, () => `GET /v1/billing/meters/${VARS.PROVIDER_METER_ID}/event_summaries 200`));
    expect(answers).toEqual(Array.from({ length: 3 }, () => ({ kind: "failed" })));
  });

  test("stripe-mock refuses an event-summary request with a name that its spec does not name", async () => {
    const query = (extra: [string, string][]) =>
      new URLSearchParams([["customer", CUSTOMER], ["start_time", String(START)], ["end_time", String(START + 60)], ...extra]).toString();
    const read = (text: string) =>
      fetch(`${mock.url}/v1/billing/meters/${VARS.PROVIDER_METER_ID}/event_summaries?${text}`, { headers: { authorization: `Bearer ${VARS.PROVIDER_API_KEY}` } });
    expect((await read(query([]))).status).toBe(200);
    expect((await read(query([["meter", "x"]]))).status).toBe(400);
    expect((await read(query([["value_grouping_window", "minute"]]))).status).toBe(400);
  });

  test("stripe-mock refuses a meter-event request with a top-level name that its spec does not name", async () => {
    const names = LISTS["POST /v1/billing/meter_events"] ?? [];
    const values: Record<string, string> = {
      event_name: VARS.PROVIDER_METER_EVENT ?? "",
      "payload[stripe_customer_id]": CUSTOMER,
      "payload[value]": "12",
      identifier: IDENTIFIER,
      timestamp: String(START),
    };
    const body = (extra: [string, string][]) =>
      new URLSearchParams([...names.map((name): [string, string] => [name, values[name] ?? ""]), ...extra]).toString();
    const send = (text: string) =>
      fetch(`${mock.url}/v1/billing/meter_events`, {
        method: "POST",
        headers: { authorization: `Bearer ${VARS.PROVIDER_API_KEY}`, "content-type": "application/x-www-form-urlencoded" },
        body: text,
      });
    expect((await send(body([]))).status).toBe(200);
    expect((await send(body([["account", ACCOUNT]]))).status).toBe(400);
    expect((await send(body([["value", "12"]]))).status).toBe(400);
  });
});

describe("what the mapping sends", () => {
  test("every parameter name that the mapping sends stands on the committed list, in its order, and the account id travels only in the identifier", async () => {
    const server = await startRecorder((request) => echo(request));
    try {
      const port = providerPort(env(server.url));
      expect(await port.sendUsage(USAGE)).toEqual({ kind: "recorded" });
      expect(server.requests).toHaveLength(1);
      for (const request of server.requests) {
        expect({ endpoint: endpoint(request), params: request.params }).toEqual({ endpoint: endpoint(request), params: LISTS[endpoint(request)] });
        expect(request.headers.authorization).toBe(`Bearer ${VARS.PROVIDER_API_KEY}`);
        expect(request.headers["stripe-version"]).toBe(API_VERSION);
        expect(request.path.includes("?")).toBe(false);
        for (const name of request.params) expect(name.toLowerCase()).not.toContain("account");
      }
      const [send] = server.requests;
      expect(send?.headers["content-type"]).toBe("application/x-www-form-urlencoded");
      expect(send?.values).toEqual({
        event_name: VARS.PROVIDER_METER_EVENT,
        "payload[stripe_customer_id]": CUSTOMER,
        "payload[value]": "12",
        identifier: IDENTIFIER,
        timestamp: String(START + 3_599),
      });
      expect(Object.values(send?.values ?? {}).filter((value) => value.includes(ACCOUNT))).toEqual([IDENTIFIER]);
    } finally {
      await server.close();
    }
  });

  test("a 2xx answer is recorded only when it echoes the identifier, and every other answer is failed", async () => {
    const cases: [string, (request: Recorded) => { status: number; body: string } | "drop", string][] = [
      ["200 with the identifier", (request) => echo(request), "recorded"],
      ["201 with the identifier", (request) => echo(request, 201), "recorded"],
      ["200 with another identifier", () => ({ status: 200, body: JSON.stringify({ identifier: `${IDENTIFIER}0` }) }), "failed"],
      ["200 with no identifier", () => ({ status: 200, body: "{}" }), "failed"],
      ["200 that is no JSON", () => ({ status: 200, body: "recorded" }), "failed"],
      ["200 with an array", () => ({ status: 200, body: JSON.stringify([{ identifier: IDENTIFIER }]) }), "failed"],
      ["200 with an empty identifier", () => ({ status: 200, body: JSON.stringify({ object: "billing.meter_event", identifier: "" }) }), "failed"],
      ["200 with a prefix of the identifier", () => ({ status: 200, body: JSON.stringify({ object: "billing.meter_event", identifier: IDENTIFIER.slice(0, -1) }) }), "failed"],
      ["200 with the identifier and no object kind", () => ({ status: 200, body: JSON.stringify({ identifier: IDENTIFIER }) }), "failed"],
      ["200 with the identifier and another object kind", () => ({ status: 200, body: JSON.stringify({ object: "billing.meter", identifier: IDENTIFIER }) }), "failed"],
      ["200 with the identifier upper-cased", () => ({ status: 200, body: JSON.stringify({ object: "billing.meter_event", identifier: IDENTIFIER.toUpperCase() }) }), "failed"],
      ["300 with the identifier", (request) => echo(request, 300), "failed"],
      ["500 with the identifier", (request) => echo(request, 500), "failed"],
      ["a dropped connection", () => "drop", "failed"],
    ];
    for (const [about, answer, kind] of cases) {
      const server = await startRecorder(answer);
      try {
        expect({ about, answer: await providerPort(env(server.url)).sendUsage(USAGE) }).toEqual({ about, answer: { kind } });
        expect({ about, requests: server.requests.length }).toEqual({ about, requests: 1 });
      } finally {
        await server.close();
      }
    }
    const closed = await freePort();
    expect(await providerPort(env(`http://127.0.0.1:${closed}`)).sendUsage(USAGE)).toEqual({ kind: "failed" });
  });

  test("an absent or empty address, key, meter id or event name, or an empty customer or identifier, makes sendUsage answer failed and open no connection", async () => {
    const server = await startRecorder((request) => echo(request));
    try {
      for (const name of ["PROVIDER_BASE_URL", "PROVIDER_API_KEY", "PROVIDER_METER_ID", "PROVIDER_METER_EVENT"]) {
        expect({ name, answer: await providerPort(absent(server.url, name)).sendUsage(USAGE) }).toEqual({ name, answer: { kind: "failed" } });
      }
      const overs: Record<string, string>[] = [{ PROVIDER_BASE_URL: "" }, { PROVIDER_API_KEY: "" }, { PROVIDER_METER_ID: "" }, { PROVIDER_METER_EVENT: "" }];
      for (const over of overs) {
        expect({ over, answer: await providerPort(env(server.url, over)).sendUsage(USAGE) }).toEqual({ over, answer: { kind: "failed" } });
      }
      const port = providerPort(env(server.url));
      expect(await port.sendUsage({ ...USAGE, customerId: "" })).toEqual({ kind: "failed" });
      expect(await port.sendUsage({ ...USAGE, identifier: "" })).toEqual({ kind: "failed" });
      expect(server.requests).toHaveLength(0);
      expect(await providerPort(env(server.url)).sendUsage(USAGE)).toEqual({ kind: "recorded" });
      expect(server.requests).toHaveLength(1);
    } finally {
      await server.close();
    }
  });

  test("providerPort binds the mapping to the meter variables of its entry and keeps the adapter's other methods", async () => {
    const server = await startRecorder((request) =>
      request.path.startsWith("/v1/checkout/sessions/") ? { status: 200, body: JSON.stringify({ id: "cs_test_1", status: "open" }) } : echo(request),
    );
    try {
      const port = providerPort(env(server.url, { PROVIDER_METER_EVENT: "Halvo_Usage_Entry", PROVIDER_API_KEY: "sk_test_meterentry" }));
      expect(await port.readSession("cs_test_1")).toEqual({ kind: "open" });
      expect(await port.sendUsage(USAGE)).toEqual({ kind: "recorded" });
      expect(server.requests.map((request) => `${request.method} ${request.path}`)).toEqual(["GET /v1/checkout/sessions/cs_test_1", "POST /v1/billing/meter_events"]);
      expect(server.requests[1]?.values.event_name).toBe("Halvo_Usage_Entry");
      expect(server.requests.map((request) => request.headers.authorization)).toEqual(["Bearer sk_test_meterentry", "Bearer sk_test_meterentry"]);
    } finally {
      await server.close();
    }
  });
});

describe("values that never reach a request", () => {
  test("a customer id or an identifier that is no string or holds an unpaired surrogate is failed with no connection, for a send and for a read; a paired surrogate is sent whole", async () => {
    const high = String.fromCharCode(0xd800);
    const low = String.fromCharCode(0xdc00);
    const pair = String.fromCodePoint(0x1f600);
    const server = await startRecorder((request) => (request.method === "POST" ? echo(request) : summaries([summary(START, START + 60, 1)])));
    try {
      const port = providerPort(env(server.url));
      const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 };
      const broken: [string, unknown][] = [
        ["a lone high surrogate", `${CUSTOMER}${high}`],
        ["a lone low surrogate", `${CUSTOMER}${low}`],
        ["a reversed pair", `${CUSTOMER}${low}${high}`],
        ["a lone high surrogate after a pair", `${CUSTOMER}${pair}${high}`],
        ["undefined", undefined],
        ["null", null],
        ["a number", 123],
        ["an array", [CUSTOMER]],
        ["an object with toString", { toString: () => CUSTOMER }],
        ["an object whose toString throws", { toString: () => { throw new Error("boom"); } }],
        ["a symbol", Symbol(CUSTOMER)],
      ];
      for (const [about, value] of broken) {
        const id = value as string;
        expect({ about, as: "customer of a send", answer: await port.sendUsage({ ...USAGE, customerId: id }) }).toEqual({ about, as: "customer of a send", answer: { kind: "failed" } });
        expect({ about, as: "identifier", answer: await port.sendUsage({ ...USAGE, identifier: id }) }).toEqual({ about, as: "identifier", answer: { kind: "failed" } });
        expect({ about, as: "customer of a read", answer: await port.readUsageTotal({ ...read, customerId: id }) }).toEqual({ about, as: "customer of a read", answer: { kind: "failed" } });
      }
      expect(server.requests).toHaveLength(0);
      const customer = `${CUSTOMER}${pair}`;
      const identifier = `${IDENTIFIER}${pair}`;
      expect(await port.sendUsage({ ...USAGE, customerId: customer, identifier })).toEqual({ kind: "recorded" });
      expect(await port.readUsageTotal({ ...read, customerId: customer })).toEqual({ kind: "total", from: START, to: START + 60, totalMinor: 1 });
      expect(server.requests.map((request) => request.values["payload[stripe_customer_id]"] ?? request.values.customer)).toEqual([customer, customer]);
      expect(server.requests[0]?.values.identifier).toBe(identifier);
    } finally {
      await server.close();
    }
  });

  test("a meter id outside letters, digits and the underscore or longer than 100, and an identifier or an event name longer than 100, are failed with no connection", async () => {
    const server = await startRecorder((request) => (request.method === "POST" ? echo(request) : summaries([summary(START, START + 60, 1, request.path.split("/")[4])])));
    try {
      const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 };
      for (const meterId of [".", "..", "mtr_a/b", "mtr_a%2Eb", "mtr_a.b", "mtr a", "mtr-a", "", "m".repeat(101)]) {
        expect({ meterId, answer: await providerPort(env(server.url, { PROVIDER_METER_ID: meterId })).readUsageTotal(read) }).toEqual({ meterId, answer: { kind: "failed" } });
      }
      expect(await providerPort(env(server.url, { PROVIDER_METER_EVENT: "e".repeat(101) })).sendUsage(USAGE)).toEqual({ kind: "failed" });
      expect(await providerPort(env(server.url)).sendUsage({ ...USAGE, identifier: "i".repeat(101) })).toEqual({ kind: "failed" });
      expect(server.requests).toHaveLength(0);
      const longest = `mtr_${"Z9_".repeat(32)}`;
      expect(longest).toHaveLength(100);
      expect(await providerPort(env(server.url, { PROVIDER_METER_ID: longest })).readUsageTotal(read)).toEqual({ kind: "total", from: START, to: START + 60, totalMinor: 1 });
      expect(await providerPort(env(server.url, { PROVIDER_METER_EVENT: "e".repeat(100) })).sendUsage(USAGE)).toEqual({ kind: "recorded" });
      expect(await providerPort(env(server.url)).sendUsage({ ...USAGE, identifier: "i".repeat(100) })).toEqual({ kind: "recorded" });
      expect(server.requests.map((request) => request.path)).toEqual([`/v1/billing/meters/${longest}/event_summaries`, "/v1/billing/meter_events", "/v1/billing/meter_events"]);
    } finally {
      await server.close();
    }
  });

  test("a value that cannot be put into the form makes the send failed, and the call does not reject", async () => {
    const server = await startRecorder((request) => echo(request));
    try {
      const port = providerPort({ ...env(server.url), PROVIDER_METER_EVENT: Symbol("halvo_usage") } as unknown as Env);
      expect(await port.sendUsage(USAGE)).toEqual({ kind: "failed" });
      expect(server.requests).toHaveLength(0);
    } finally {
      await server.close();
    }
  });

  test("a meter id that holds a line break is refused before any connection, for a send and for a read", async () => {
    const server = await startRecorder((request) => (request.method === "POST" ? echo(request) : summaries([summary(START, START + 60, 1)])));
    const calls: string[] = [];
    const real = globalThis.fetch;
    globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
      calls.push(String(args[0]));
      return real(...args);
    };
    try {
      const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 };
      const meterId = VARS.PROVIDER_METER_ID ?? "";
      for (const id of [`${meterId}\n..`, `..\n${meterId}`, `${meterId}\nmtr_other`, `${meterId}\n`, `\n${meterId}`, `${meterId}\r\n..`]) {
        const port = providerPort(env(server.url, { PROVIDER_METER_ID: id }));
        expect({ id, answer: await port.readUsageTotal(read) }).toEqual({ id, answer: { kind: "failed" } });
        expect({ id, answer: await port.sendUsage(USAGE) }).toEqual({ id, answer: { kind: "failed" } });
      }
    } finally {
      globalThis.fetch = real;
      await server.close();
    }
    expect(calls).toEqual([]);
    expect(server.requests).toHaveLength(0);
  });

  test("with any of the four provider values absent or empty neither call reaches fetch", async () => {
    const calls: string[] = [];
    const real = globalThis.fetch;
    globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
      calls.push(String(args[0]));
      return real(...args);
    };
    try {
      const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 };
      for (const name of ["PROVIDER_BASE_URL", "PROVIDER_API_KEY", "PROVIDER_METER_ID", "PROVIDER_METER_EVENT"]) {
        for (const port of [providerPort(absent("http://127.0.0.1:1", name)), providerPort(env("http://127.0.0.1:1", { [name]: "" }))]) {
          expect({ name, answer: await port.sendUsage(USAGE) }).toEqual({ name, answer: { kind: "failed" } });
          expect({ name, answer: await port.readUsageTotal(read) }).toEqual({ name, answer: { kind: "failed" } });
        }
      }
    } finally {
      globalThis.fetch = real;
    }
    expect(calls).toEqual([]);
  });
});

function summaries(data: unknown[], over: Record<string, unknown> = {}) {
  return { status: 200, body: JSON.stringify({ object: "list", data, has_more: false, url: "/v1/billing/meters/x/event_summaries", ...over }) };
}

function summary(start: number, end: number, value: unknown, meter: unknown = VARS.PROVIDER_METER_ID) {
  return { id: "mtrusg_test_1", object: "billing.meter_event_summary", aggregated_value: value, start_time: start, end_time: end, livemode: false, meter };
}

describe("what the mapping reads", () => {
  test("the read asks for the event summaries of the meter PROVIDER_METER_ID and the customer, over the whole minutes that hold the range", async () => {
    const server = await startRecorder((request) => summaries([summary(Number(request.values.start_time), Number(request.values.end_time), 7)]));
    try {
      const port = providerPort(env(server.url));
      const cases: [number, number, number, number][] = [
        [START + 3_599, START + 3_600, START + 3_540, START + 3_600],
        [START, START + 3_600, START, START + 3_600],
        [START + 1, START + 61, START, START + 120],
        [START - 1, START, START - 60, START],
      ];
      for (const [from, to, start, end] of cases) {
        expect(await port.readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from, to })).toEqual({ kind: "total", from: start, to: end, totalMinor: 7 });
      }
      expect(server.requests).toHaveLength(cases.length);
      for (const [index, request] of server.requests.entries()) {
        const [, , start, end] = cases[index] ?? [];
        expect(request.method).toBe("GET");
        expect(request.path).toBe(`/v1/billing/meters/${VARS.PROVIDER_METER_ID}/event_summaries`);
        expect({ endpoint: endpoint(request), params: request.params }).toEqual({ endpoint: endpoint(request), params: LISTS[endpoint(request)] });
        expect(request.values).toEqual({ customer: CUSTOMER, start_time: String(start), end_time: String(end) });
        expect(request.body).toBe("");
        expect(request.headers.authorization).toBe(`Bearer ${VARS.PROVIDER_API_KEY}`);
        expect(request.headers["stripe-version"]).toBe(API_VERSION);
        for (const name of request.params) expect(name.toLowerCase()).not.toContain("account");
      }
      await providerPort(env(server.url, { PROVIDER_METER_ID: "mtr_Test_Other_9" })).readUsageTotal({ accountId: ACCOUNT, customerId: "cus_Test&x=1", from: START, to: START + 60 });
      expect(server.requests.at(-1)?.path).toBe("/v1/billing/meters/mtr_Test_Other_9/event_summaries");
      expect(server.requests.at(-1)?.values).toEqual({ customer: "cus_Test&x=1", start_time: String(START), end_time: String(START + 60) });
    } finally {
      await server.close();
    }
  });

  test("totals read through the mapping equal the fake meter's own read and its record of what the mapping sent", async () => {
    const meter = fakeMeter();
    const clock = { now: START + 7_200 };
    const names = { meterId: VARS.PROVIDER_METER_ID ?? "", eventName: VARS.PROVIDER_METER_EVENT ?? "" };
    const server = await startRecorder(providerOver(meter, clock, names));
    try {
      const other = "cus_Test_Meter_2";
      meter.subscribe(CUSTOMER, START);
      meter.subscribe(other, START);
      const port = providerPort(env(server.url));
      const sent: [string, number, number][] = [
        [CUSTOMER, START + 3_599, 5],
        [CUSTOMER, START + 3_540, 7],
        [CUSTOMER, START + 7_199, 11],
        [other, START + 3_599, 13],
        [CUSTOMER, START + 59, 17],
      ];
      for (const [customerId, timestamp, amountMinor] of sent) {
        const usage = { accountId: ACCOUNT, customerId, identifier: `${customerId}:${timestamp}`, amountMinor, timestamp };
        expect(await port.sendUsage(usage)).toEqual({ kind: "recorded" });
      }
      expect(meter.exactTotal(CUSTOMER, START, START + 7_200)).toBe(40);
      for (const [customerId, from, to] of [
        [CUSTOMER, START + 3_599, START + 3_600],
        [CUSTOMER, START + 3_540, START + 3_541],
        [CUSTOMER, START, START + 3_600],
        [CUSTOMER, START + 3_600, START + 7_200],
        [CUSTOMER, START + 1, START + 59],
        [CUSTOMER, START, START + 7_200],
        [other, START, START + 7_200],
        [other, START + 3_600, START + 7_200],
      ] as [string, number, number][]) {
        const expected = meter.read(customerId, from, to, clock.now);
        expect({ customerId, from, to, answer: await port.readUsageTotal({ accountId: ACCOUNT, customerId, from, to }) }).toEqual({
          customerId,
          from,
          to,
          answer: { kind: "total", from: expected.widenedFrom, to: expected.widenedTo, totalMinor: expected.totalMinor },
        });
      }
      expect(await port.readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from: START + 3_599, to: START + 3_600 })).toEqual({
        kind: "total",
        from: START + 3_540,
        to: START + 3_600,
        totalMinor: 12,
      });
    } finally {
      await server.close();
    }
  });

  test("an empty summary list is a total of 0 over the minutes asked, and only one summary of exactly those minutes is a total: one shifted by a minute, one twice as long or two for one minute are failed", async () => {
    const cases: [string, number, number, { status: number; body: string }, unknown][] = [
      ["the empty list", START + 30, START + 90, summaries([]), { kind: "total", from: START, to: START + 120, totalMinor: 0 }],
      ["a summary of exactly the minutes asked", START + 30, START + 90, summaries([summary(START, START + 120, 9)]), { kind: "total", from: START, to: START + 120, totalMinor: 9 }],
      ["a summary shifted one minute later", START + 30, START + 90, summaries([summary(START + 60, START + 180, 9)]), { kind: "failed" }],
      ["a summary shifted one minute earlier", START + 30, START + 90, summaries([summary(START - 60, START + 60, 9)]), { kind: "failed" }],
      ["a summary twice as long, ending at the minutes asked", START + 30, START + 90, summaries([summary(START - 120, START + 120, 9)]), { kind: "failed" }],
      ["a summary twice as long, starting at the minutes asked", START + 30, START + 90, summaries([summary(START, START + 240, 9)]), { kind: "failed" }],
      ["a summary that holds the range but not its minutes", START, START + 60, summaries([summary(START - 3_600, START + 7_200, 9)]), { kind: "failed" }],
      ["two summaries for one minute", START + 30, START + 59, summaries([summary(START, START + 60, 4), summary(START, START + 60, 5)]), { kind: "failed" }],
    ];
    for (const [about, from, to, answer, expected] of cases) {
      const server = await startRecorder(() => answer);
      try {
        expect({ about, answer: await providerPort(env(server.url)).readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from, to }) }).toEqual({ about, answer: expected });
        expect({ about, requests: server.requests.length }).toEqual({ about, requests: 1 });
      } finally {
        await server.close();
      }
    }
  });

  test("a summary of other minutes than the asked ones is failed also when its total is 0, and one of exactly those minutes with a total of 0 is the total 0", async () => {
    const cases: [string, { status: number; body: string }, unknown][] = [
      ["exactly the minutes asked with a total of 0", summaries([summary(START, START + 120, 0)]), { kind: "total", from: START, to: START + 120, totalMinor: 0 }],
      ["shifted one minute later with a total of 0", summaries([summary(START + 60, START + 180, 0)]), { kind: "failed" }],
      ["shifted one minute earlier with a total of 0", summaries([summary(START - 60, START + 60, 0)]), { kind: "failed" }],
      ["twice as long with a total of 0", summaries([summary(START - 120, START + 120, 0)]), { kind: "failed" }],
      ["other minutes altogether with a total of 0", summaries([summary(START + 3_600, START + 3_720, 0)]), { kind: "failed" }],
    ];
    for (const [about, answer, expected] of cases) {
      const server = await startRecorder(() => answer);
      try {
        const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START + 30, to: START + 90 };
        expect({ about, answer: await providerPort(env(server.url)).readUsageTotal(read) }).toEqual({ about, answer: expected });
      } finally {
        await server.close();
      }
    }
  });

  test("a summary answer that the mapping does not know is failed: more pages, two summaries, another meter, a range that misses the minutes asked, or a total that is no whole number", async () => {
    const from = START + 30;
    const to = START + 90;
    const cases: [string, { status: number; body: string } | "drop"][] = [
      ["has_more", summaries([summary(START, START + 120, 1)], { has_more: true })],
      ["no has_more", summaries([summary(START, START + 120, 1)], { has_more: undefined })],
      ["two summaries", summaries([summary(START, START + 60, 1), summary(START + 60, START + 120, 1)])],
      ["two summaries that each hold the range", summaries([summary(START, START + 120, 1), summary(START, START + 120, 5)])],
      ["another meter", summaries([summary(START, START + 120, 1, "mtr_test_other")])],
      ["no meter", summaries([summary(START, START + 120, 1, null)])],
      ["a later start", summaries([summary(START + 31, START + 120, 1)])],
      ["an earlier end", summaries([summary(START, START + 89, 1)])],
      ["a start that is no integer", summaries([summary(START + 0.5, START + 120, 1)])],
      ["an end that is no integer", summaries([summary(START, START + 120.5, 1)])],
      ["an end as a string", summaries([{ ...summary(START, START + 120, 1), end_time: String(START + 120) }])],
      ["a start as a string", summaries([{ ...summary(START, START + 120, 1), start_time: String(START) }])],
      ["a fraction", summaries([summary(START, START + 120, 2.5)])],
      ["a negative total", summaries([summary(START, START + 120, -1)])],
      ["a total above the exact range", summaries([summary(START, START + 120, 2 ** 53)])],
      ["a total as a string", summaries([summary(START, START + 120, "1")])],
      ["no total", summaries([summary(START, START + 120, null)])],
      ["data that is no list", summaries([], { data: { 0: summary(START, START + 120, 1) } })],
      ["a summary that is no object", summaries([1])],
      ["a summary that is null", summaries([null])],
      ["a list with no object kind", summaries([summary(START, START + 120, 1)], { object: undefined })],
      ["a list of another object kind", summaries([summary(START, START + 120, 1)], { object: "billing.meter_event_summary" })],
      ["a list with an error key", summaries([summary(START, START + 120, 1)], { error: { type: "invalid_request_error" } })],
      ["an empty list with an error key that is null", summaries([], { error: null })],
      ["a summary that is a string", summaries(["mtrusg_test_1"])],
      ["a body that is no JSON", { status: 200, body: "total" }],
      ["status 404", { status: 404, body: JSON.stringify({ error: { type: "invalid_request_error", code: "resource_missing" } }) }],
      ["status 300 with a summary", { status: 300, body: summaries([summary(START, START + 120, 1)]).body }],
      ["status 400 with a summary", { status: 400, body: summaries([summary(START, START + 120, 1)]).body }],
      ["status 500", { status: 500, body: summaries([summary(START, START + 120, 1)]).body }],
      ["a dropped connection", "drop"],
    ];
    for (const [about, answer] of cases) {
      const server = await startRecorder(() => answer);
      try {
        expect({ about, answer: await providerPort(env(server.url)).readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from, to }) }).toEqual({ about, answer: { kind: "failed" } });
        expect({ about, requests: server.requests.length }).toEqual({ about, requests: 1 });
      } finally {
        await server.close();
      }
    }
    const closed = await freePort();
    expect(await providerPort(env(`http://127.0.0.1:${closed}`)).readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from, to })).toEqual({ kind: "failed" });
  });

  test("an absent or empty address, key, meter id or event name, an empty customer, or a range that is no range makes readUsageTotal answer failed and open no connection", async () => {
    const server = await startRecorder((request) => summaries([summary(Number(request.values.start_time), Number(request.values.end_time), 3)]));
    try {
      const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 };
      for (const name of ["PROVIDER_BASE_URL", "PROVIDER_API_KEY", "PROVIDER_METER_ID", "PROVIDER_METER_EVENT"]) {
        expect({ name, answer: await providerPort(absent(server.url, name)).readUsageTotal(read) }).toEqual({ name, answer: { kind: "failed" } });
      }
      const overs: Record<string, string>[] = [{ PROVIDER_BASE_URL: "" }, { PROVIDER_API_KEY: "" }, { PROVIDER_METER_ID: "" }, { PROVIDER_METER_EVENT: "" }];
      for (const over of overs) {
        expect({ over, answer: await providerPort(env(server.url, over)).readUsageTotal(read) }).toEqual({ over, answer: { kind: "failed" } });
      }
      const port = providerPort(env(server.url));
      const ranges: [string, Partial<typeof read>][] = [
        ["no customer", { customerId: "" }],
        ["from equal to to", { to: START }],
        ["from after to", { from: START + 61 }],
        ["a from that is no integer", { from: START + 0.5 }],
        ["a to that is no integer", { to: Number.NaN }],
        ["a to above the exact range", { to: 2 ** 53 }],
        ["a to whose minute leaves the exact range", { to: Number.MAX_SAFE_INTEGER }],
        ["a to that is no whole second", { to: START + 59.5 }],
        ["a from whose minute leaves the exact range", { from: Number.MIN_SAFE_INTEGER }],
      ];
      for (const [about, over] of ranges) {
        expect({ about, answer: await port.readUsageTotal({ ...read, ...over }) }).toEqual({ about, answer: { kind: "failed" } });
      }
      expect(server.requests).toHaveLength(0);
      expect(await providerPort(env(server.url)).readUsageTotal(read)).toEqual({ kind: "total", from: START, to: START + 60, totalMinor: 3 });
      expect(server.requests).toHaveLength(1);
    } finally {
      await server.close();
    }
  });
});

const LARGEST_REPORTED_MINOR = Math.floor(Number.MAX_SAFE_INTEGER / MICRO_PER_MINOR);

describe("the amounts and the times that the mapping sends", () => {
  test("every amount from 1 to the largest that the ledger can report is sent as plain decimal digits, never rounded or in exponent form, and the fake meter counts it whole", async () => {
    expect(LARGEST_REPORTED_MINOR).toBe(900_719_925_474);
    expect(9_007_199_254_741).toBeGreaterThan(2 ** 53 / 1_000);
    const amounts = [1, 2, 10, 1_000_000, LARGEST_REPORTED_MINOR, LARGEST_REPORTED_MINOR + 1, 9_007_199_254_741, 10 ** 15, Number.MAX_SAFE_INTEGER];
    const recorder = await startRecorder((request) => echo(request));
    const meter = fakeMeter();
    const clock = { now: START + 3_600 };
    const provider = await startRecorder(providerOver(meter, clock, { meterId: VARS.PROVIDER_METER_ID ?? "", eventName: VARS.PROVIDER_METER_EVENT ?? "" }));
    try {
      const recorded = providerPort(env(recorder.url));
      const counted = providerPort(env(provider.url));
      for (const [index, amountMinor] of amounts.entries()) {
        const customerId = `cus_Test_Amount_${index}`;
        meter.subscribe(customerId, START);
        const usage = { ...USAGE, customerId, amountMinor, identifier: `${customerId}:${START}` };
        expect(await recorded.sendUsage(usage)).toEqual({ kind: "recorded" });
        expect(await counted.sendUsage(usage)).toEqual({ kind: "recorded" });
        const value = recorder.requests.at(-1)?.values["payload[value]"];
        expect({ amountMinor, value }).toEqual({ amountMinor, value: BigInt(amountMinor).toString() });
        expect(value).toMatch(/^[1-9][0-9]*$/);
        expect({ amountMinor, total: meter.exactTotal(customerId, START, START + 3_600) }).toEqual({ amountMinor, total: amountMinor });
        expect(await counted.readUsageTotal({ accountId: ACCOUNT, customerId, from: USAGE.timestamp, to: USAGE.timestamp + 1 })).toEqual({
          kind: "total",
          from: START + 3_540,
          to: START + 3_600,
          totalMinor: amountMinor,
        });
      }
    } finally {
      await recorder.close();
      await provider.close();
    }
  });

  test("an amount that the meter's field cannot hold exactly, or that is under 1, is refused as amount_not_whole_minor_units and sent nowhere", async () => {
    const server = await startRecorder((request) => echo(request));
    try {
      const port = providerPort(env(server.url));
      for (const amountMinor of [0, -0, -1, 0.5, 1.5, LARGEST_REPORTED_MINOR + 0.5, 2 ** 53, 2 ** 53 + 2, 1e21, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
        expect({ amountMinor, answer: await port.sendUsage({ ...USAGE, amountMinor }) }).toEqual({ amountMinor, answer: { kind: "refused", reason: "amount_not_whole_minor_units" } });
      }
      expect(await port.sendUsage({ ...USAGE, amountMinor: 0, timestamp: 0.5 })).toEqual({ kind: "refused", reason: "amount_not_whole_minor_units" });
      expect(server.requests).toHaveLength(0);
    } finally {
      await server.close();
    }
  });

  test("a timestamp that is no whole second is refused as timestamp_too_old and sent nowhere, as the fake meter refuses it; a whole second is sent as digits", async () => {
    const server = await startRecorder((request) => echo(request));
    try {
      const port = providerPort(env(server.url));
      for (const timestamp of [START + 0.5, Number.NaN, Number.POSITIVE_INFINITY, 2 ** 53, -(2 ** 53) - 2]) {
        expect({ timestamp, answer: await port.sendUsage({ ...USAGE, timestamp }) }).toEqual({ timestamp, answer: { kind: "refused", reason: "timestamp_too_old" } });
      }
      expect(server.requests).toHaveLength(0);
      for (const timestamp of [0, -1, -START, START + 3_599, 10 ** 12]) {
        expect(await port.sendUsage({ ...USAGE, timestamp })).toEqual({ kind: "recorded" });
        expect(server.requests.at(-1)?.values.timestamp).toBe(BigInt(timestamp).toString());
      }
    } finally {
      await server.close();
    }
  });
});

async function startHalfAnswer(): Promise<{ url: string; requests: number; close(): Promise<void> }> {
  const state = { requests: 0 };
  const server = httpServer((request: IncomingMessage, response: ServerResponse) => {
    request.on("data", () => undefined);
    request.on("end", () => {
      state.requests += 1;
      response.writeHead(200, { "content-type": "application/json", "content-length": "4096" });
      response.write('{"identifier":"');
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    get requests() {
      return state.requests;
    },
    async close() {
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

async function startRaw(answer: (url: string) => { status: number; headers: Record<string, string>; body: Uint8Array }) {
  const seen: string[] = [];
  const server = httpServer((request: IncomingMessage, response: ServerResponse) => {
    request.on("data", () => undefined);
    request.on("end", () => {
      seen.push(`${request.method} ${request.url}`);
      const reply = answer(request.url ?? "/");
      response.writeHead(reply.status, reply.headers);
      response.end(reply.body);
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    seen,
    async close() {
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

async function startEndless(status = 200): Promise<{ url: string; requests: number; closed: number; close(): Promise<void> }> {
  const state = { requests: 0, closed: 0 };
  const timers: ReturnType<typeof setInterval>[] = [];
  const chunk = " ".repeat(65_536);
  const server = httpServer((request: IncomingMessage, response: ServerResponse) => {
    request.on("data", () => undefined);
    request.on("end", () => {
      state.requests += 1;
      response.writeHead(status, { "content-type": "application/json" });
      response.write('{"object":"list","data":[],"has_more":false,"pad":"');
      const timer = setInterval(() => response.write(chunk), 5);
      timers.push(timer);
      response.on("close", () => {
        clearInterval(timer);
        state.closed += 1;
      });
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    get requests() {
      return state.requests;
    },
    get closed() {
      return state.closed;
    },
    async close() {
      for (const timer of timers) clearInterval(timer);
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

describe("answers that the mapping does not take", () => {
  test("an answer above 1 MiB with a status of 400 or above is failed by the size rule, as a 2xx is", async () => {
    const limit = 1_048_576;
    const encoder = new TextEncoder();
    const padded = (text: string, size: number) => encoder.encode(text + " ".repeat(size - encoder.encode(text).byteLength));
    const refusal = providerError("timestamp_too_far_in_past");
    const json = { "content-type": "application/json" };
    const cases: [string, number, number, unknown][] = [
      ["a refusal of exactly 1 MiB", 400, limit, { kind: "refused", reason: "timestamp_too_old" }],
      ["a refusal of 1 MiB and one byte", 400, limit + 1, { kind: "failed" }],
      ["a 404 of 1 MiB and one byte", 404, limit + 1, { kind: "failed" }],
      ["a 500 of 1 MiB and one byte", 500, limit + 1, { kind: "failed" }],
    ];
    for (const [about, status, size, expected] of cases) {
      const server = await startRaw(() => ({ status, headers: json, body: padded(refusal, size) }));
      try {
        expect({ about, answer: await providerPort(env(server.url)).sendUsage(USAGE) }).toEqual({ about, answer: expected });
      } finally {
        await server.close();
      }
    }
    const endless = await startEndless(400);
    try {
      const port = providerPort(env(endless.url));
      const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START + 30, to: START + 90 };
      for (const method of ["send", "read"] as const) {
        const began = Date.now();
        const answer = method === "send" ? await port.sendUsage(USAGE) : await port.readUsageTotal(read);
        const early = Date.now() - began < PROVIDER_CALL_TIMEOUT_MS / 2;
        expect({ method, answer, early }).toEqual({ method, answer: { kind: "failed" }, early: true });
      }
    } finally {
      await endless.close();
    }
  }, PROVIDER_CALL_TIMEOUT_MS * 3);

  test("an answer above 1 MiB is failed for a send and a read, one of exactly 1 MiB is read, and one that never ends is left at 1 MiB without waiting for the deadline", async () => {
    const limit = 1_048_576;
    const encoder = new TextEncoder();
    const padded = (text: string, size: number) => encoder.encode(text + " ".repeat(size - encoder.encode(text).byteLength));
    const echoText = JSON.stringify({ object: "billing.meter_event", identifier: IDENTIFIER });
    const listText = summaries([summary(START, START + 120, 7)]).body;
    const json = { "content-type": "application/json" };
    const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START + 30, to: START + 90 };
    const cases: [string, "send" | "read", number, unknown][] = [
      ["an echo of exactly 1 MiB", "send", limit, { kind: "recorded" }],
      ["an echo of 1 MiB and one byte", "send", limit + 1, { kind: "failed" }],
      ["a list of exactly 1 MiB", "read", limit, { kind: "total", from: START, to: START + 120, totalMinor: 7 }],
      ["a list of 1 MiB and one byte", "read", limit + 1, { kind: "failed" }],
    ];
    for (const [about, method, size, expected] of cases) {
      const server = await startRaw(() => ({ status: 200, headers: json, body: padded(method === "send" ? echoText : listText, size) }));
      try {
        const port = providerPort(env(server.url));
        const answer = method === "send" ? await port.sendUsage(USAGE) : await port.readUsageTotal(read);
        expect({ about, answer }).toEqual({ about, answer: expected });
      } finally {
        await server.close();
      }
    }
    const endless = await startEndless();
    try {
      const port = providerPort(env(endless.url));
      for (const method of ["send", "read"] as const) {
        const began = Date.now();
        const answer = method === "send" ? await port.sendUsage(USAGE) : await port.readUsageTotal(read);
        const took = Date.now() - began;
        expect({ method, answer }).toEqual({ method, answer: { kind: "failed" } });
        expect({ method, early: took < PROVIDER_CALL_TIMEOUT_MS / 2 }).toEqual({ method, early: true });
        const closed = method === "send" ? 1 : 2;
        const until = Date.now() + 2_000;
        while (endless.closed < closed && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 20));
        expect({ method, closed: endless.closed }).toEqual({ method, closed });
      }
      expect(endless.requests).toBe(2);
    } finally {
      await endless.close();
    }
  }, PROVIDER_CALL_TIMEOUT_MS * 3);

  test("a redirect is not followed: a 3xx with a location is failed for a send and a read, and the target hears nothing", async () => {
    const target = await startRecorder((request) => (request.method === "POST" ? echo(request) : summaries([summary(START, START + 120, 1)])));
    const state = { status: 302 };
    const redirect = await startRaw((url) => ({ status: state.status, headers: { location: `${target.url}${url}` }, body: new Uint8Array() }));
    try {
      const port = providerPort(env(redirect.url));
      for (const status of [301, 302, 303, 307, 308]) {
        state.status = status;
        expect({ status, answer: await port.sendUsage(USAGE) }).toEqual({ status, answer: { kind: "failed" } });
        expect({ status, answer: await port.readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from: START + 30, to: START + 90 }) }).toEqual({ status, answer: { kind: "failed" } });
      }
      expect(redirect.seen).toHaveLength(10);
      expect(target.requests).toHaveLength(0);
    } finally {
      await redirect.close();
      await target.close();
    }
  });

  test("a body with a byte-order mark or a byte that is no UTF-8 is failed, as the adapter decodes it, and never a final refusal", async () => {
    const encoder = new TextEncoder();
    const bom = (text: string) => Uint8Array.from([0xef, 0xbb, 0xbf, ...encoder.encode(text)]);
    const badByte = (text: string) => {
      const at = text.indexOf("refused") + 3;
      return Uint8Array.from([...encoder.encode(text.slice(0, at)), 0xff, ...encoder.encode(text.slice(at))]);
    };
    const json = { "content-type": "application/json" };
    const cases: [string, { status: number; headers: Record<string, string>; body: Uint8Array }, "send" | "read"][] = [
      ["an echo after a byte-order mark", { status: 200, headers: json, body: bom(JSON.stringify({ identifier: IDENTIFIER })) }, "send"],
      ["a refusal after a byte-order mark", { status: 400, headers: json, body: bom(providerError("timestamp_too_far_in_past")) }, "send"],
      ["a refusal with a byte that is no UTF-8", { status: 400, headers: json, body: badByte(providerError("timestamp_too_far_in_past")) }, "send"],
      ["a summary after a byte-order mark", { status: 200, headers: json, body: bom(summaries([summary(START, START + 120, 1)]).body) }, "read"],
    ];
    for (const [about, reply, method] of cases) {
      const server = await startRaw(() => reply);
      try {
        const port = providerPort(env(server.url));
        const answer = method === "send" ? await port.sendUsage(USAGE) : await port.readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from: START + 30, to: START + 90 });
        expect({ about, answer }).toEqual({ about, answer: { kind: "failed" } });
        expect({ about, requests: server.seen.length }).toEqual({ about, requests: 1 });
      } finally {
        await server.close();
      }
    }
  });
});

describe("how long a call may take", () => {
  test("each meter call sets its deadline to the adapter's own PROVIDER_CALL_TIMEOUT_MS, which the port module exports again", async () => {
    expect(PROVIDER_CALL_TIMEOUT_MS).toBe(ADAPTER_TIMEOUT_MS);
    const server = await startRecorder((request) => (request.method === "POST" ? echo(request) : summaries([summary(START, START + 60, 1)])));
    const real = AbortSignal.timeout;
    const asked: number[] = [];
    AbortSignal.timeout = (delay: number) => {
      asked.push(delay);
      return real.call(AbortSignal, delay);
    };
    try {
      const port = providerPort(env(server.url));
      expect(await port.sendUsage(USAGE)).toEqual({ kind: "recorded" });
      expect(await port.readUsageTotal({ accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 })).toEqual({ kind: "total", from: START, to: START + 60, totalMinor: 1 });
    } finally {
      AbortSignal.timeout = real;
      await server.close();
    }
    expect(asked).toEqual([ADAPTER_TIMEOUT_MS, ADAPTER_TIMEOUT_MS]);
  });

  test(
    "a send or a read that gets no answer, half an answer or an answer after PROVIDER_CALL_TIMEOUT_MS ends within it and is failed, so a late answer is never taken",
    async () => {
      const hang = await startRecorder(() => "hang");
      const late = await startRecorder((request) =>
        request.method === "POST" ? { ...echo(request), afterMs: PROVIDER_CALL_TIMEOUT_MS + 3_000 } : { ...summaries([summary(START, START + 60, 1)]), afterMs: PROVIDER_CALL_TIMEOUT_MS + 3_000 },
      );
      const half = await startHalfAnswer();
      try {
        const timed = async (run: () => Promise<{ kind: string }>) => {
          const began = Date.now();
          const answer = await run();
          return { kind: answer.kind, took: Date.now() - began };
        };
        const read = { accountId: ACCOUNT, customerId: CUSTOMER, from: START, to: START + 60 };
        const results = await Promise.all(
          [hang.url, late.url, half.url].flatMap((url) => [timed(() => providerPort(env(url)).sendUsage(USAGE)), timed(() => providerPort(env(url)).readUsageTotal(read))]),
        );
        for (const [index, result] of results.entries()) {
          expect({ index, kind: result.kind }).toEqual({ index, kind: "failed" });
          expect(result.took).toBeGreaterThanOrEqual(PROVIDER_CALL_TIMEOUT_MS - 50);
          expect(result.took).toBeLessThan(PROVIDER_CALL_TIMEOUT_MS + 2_000);
        }
        expect([hang.requests.length, late.requests.length, half.requests]).toEqual([2, 2, 2]);
        const margin = /"SYNC_MARGIN_SECONDS": "([0-9]+)"/.exec(readFileSync(`${ROOT}wrangler.jsonc`, "utf8"))?.[1];
        expect(PROVIDER_CALL_TIMEOUT_MS).toBeLessThan(Number(margin) * 1000);
      } finally {
        await hang.close();
        await late.close();
        await half.close();
      }
    },
    PROVIDER_CALL_TIMEOUT_MS + 10_000,
  );
});

function providerError(code: string | undefined, type = "invalid_request_error") {
  return JSON.stringify({ error: { type, message: "refused", ...(code === undefined ? {} : { code }) } });
}

describe("what the mapping makes of a refusal", () => {
  test("each refusal of the provider maps to the UsageRefusal that contract 2 names, and every other refusal is failed", async () => {
    const cases: [string, { status: number; body: string }, unknown][] = [
      ["timestamp_too_far_in_past", { status: 400, body: providerError("timestamp_too_far_in_past") }, { kind: "refused", reason: "timestamp_too_old" }],
      ["timestamp_in_future", { status: 400, body: providerError("timestamp_in_future") }, { kind: "refused", reason: "timestamp_in_future" }],
      ["meter_event_customer_not_found", { status: 400, body: providerError("meter_event_customer_not_found") }, { kind: "refused", reason: "unknown_account" }],
      ["meter_event_invalid_value", { status: 400, body: providerError("meter_event_invalid_value") }, { kind: "refused", reason: "amount_not_whole_minor_units" }],
      ["meter_event_value_too_many_digits", { status: 400, body: providerError("meter_event_value_too_many_digits") }, { kind: "refused", reason: "amount_not_whole_minor_units" }],
      ["meter_event_no_customer_defined", { status: 400, body: providerError("meter_event_no_customer_defined") }, { kind: "failed" }],
      ["archived_meter", { status: 400, body: providerError("archived_meter") }, { kind: "failed" }],
      ["no_meter", { status: 400, body: providerError("no_meter") }, { kind: "failed" }],
      ["missing_dimension_payload_keys", { status: 400, body: providerError("missing_dimension_payload_keys") }, { kind: "failed" }],
      ["a refusal with no code", { status: 400, body: providerError(undefined) }, { kind: "failed" }],
      ["a code that is a prototype name", { status: 400, body: providerError("constructor") }, { kind: "failed" }],
      ["a known code with an error type that the spec does not list", { status: 400, body: providerError("timestamp_too_far_in_past", "meter_error") }, { kind: "failed" }],
      ["a known code in an idempotency_error", { status: 400, body: providerError("timestamp_too_far_in_past", "idempotency_error") }, { kind: "failed" }],
      ["a known code in an api_error", { status: 400, body: providerError("meter_event_customer_not_found", "api_error") }, { kind: "failed" }],
      ["a known code in a card_error", { status: 400, body: providerError("meter_event_invalid_value", "card_error") }, { kind: "failed" }],
      ["a 400 with no error", { status: 400, body: "{}" }, { kind: "failed" }],
      ["a 400 whose error is null", { status: 400, body: JSON.stringify({ error: null }) }, { kind: "failed" }],
      ["a 400 whose error is a string", { status: 400, body: JSON.stringify({ error: "timestamp_too_far_in_past" }) }, { kind: "failed" }],
      ["a known code outside error", { status: 400, body: JSON.stringify({ code: "timestamp_too_far_in_past", error: { type: "invalid_request_error" } }) }, { kind: "failed" }],
      ["a known code with status 402", { status: 402, body: providerError("timestamp_too_far_in_past") }, { kind: "failed" }],
      ["a known code with status 404", { status: 404, body: providerError("meter_event_customer_not_found") }, { kind: "failed" }],
      ["a known code with status 429", { status: 429, body: providerError("timestamp_in_future") }, { kind: "failed" }],
      ["a known code with status 500", { status: 500, body: providerError("meter_event_invalid_value") }, { kind: "failed" }],
      ["a known code with status 200", { status: 200, body: providerError("timestamp_too_far_in_past") }, { kind: "failed" }],
    ];
    for (const [about, answer, expected] of cases) {
      const server = await startRecorder(() => answer);
      try {
        expect({ about, answer: await providerPort(env(server.url)).sendUsage(USAGE) }).toEqual({ about, answer: expected });
        expect({ about, requests: server.requests.length }).toEqual({ about, requests: 1 });
      } finally {
        await server.close();
      }
    }
  });

  test("every provider code that the mapping reads stands in the list of meter error codes that the vendored stripe package publishes", () => {
    const types = readFileSync(`${ROOT}node_modules/stripe/esm/resources/V2/Core/Events.d.ts`, "utf8");
    const lists = [...types.matchAll(/type Code = ('archived_meter'[^;]*);/g)].map((match) => [...(match[1] ?? "").matchAll(/'([a-z_]+)'/g)].map((name) => name[1]));
    expect(lists).toHaveLength(2);
    for (const code of REFUSAL_OF_CODE.keys()) {
      for (const list of lists) expect({ code, published: list.includes(code) }).toEqual({ code, published: true });
    }
    expect([...REFUSAL_OF_CODE.entries()].sort()).toEqual([
      ["meter_event_customer_not_found", "unknown_account"],
      ["meter_event_invalid_value", "amount_not_whole_minor_units"],
      ["meter_event_value_too_many_digits", "amount_not_whole_minor_units"],
      ["timestamp_in_future", "timestamp_in_future"],
      ["timestamp_too_far_in_past", "timestamp_too_old"],
    ]);
  });

  test("each refusal of the fake meter comes back through the mapping as the same UsageRefusal, and a repeat inside the window adds nothing", async () => {
    for (const duplicateAnswer of ["same-as-recorded", "error"] as const) {
      const meter = fakeMeter({ duplicateAnswer });
      const clock = { now: START + 40 * 24 * 3600 };
      const server = await startRecorder(providerOver(meter, clock, { meterId: VARS.PROVIDER_METER_ID ?? "", eventName: VARS.PROVIDER_METER_EVENT ?? "" }));
      try {
        meter.subscribe(CUSTOMER, START);
        const port = providerPort(env(server.url));
        const now = clock.now;
        const usage = { ...USAGE, timestamp: now - 3_600, identifier: `${ACCOUNT}:${now - 7_200}` };
        expect(await port.sendUsage({ ...usage, customerId: "cus_test_unknown" })).toEqual({ kind: "refused", reason: "unknown_account" });
        expect(await port.sendUsage({ ...usage, timestamp: now - 35 * 24 * 3600 - 1 })).toEqual({ kind: "refused", reason: "timestamp_too_old" });
        expect(await port.sendUsage({ ...usage, timestamp: now + 301 })).toEqual({ kind: "refused", reason: "timestamp_in_future" });
        expect(await port.sendUsage({ ...usage, amountMinor: 0 })).toEqual({ kind: "refused", reason: "amount_not_whole_minor_units" });
        expect(meter.exactTotal(CUSTOMER, 0, now + 3_600)).toBe(0);
        expect(await port.sendUsage(usage)).toEqual({ kind: "recorded" });
        expect(await port.sendUsage(usage)).toEqual(duplicateAnswer === "error" ? { kind: "failed" } : { kind: "recorded" });
        expect(meter.exactTotal(CUSTOMER, 0, now + 3_600)).toBe(12);
        expect(meter.stats().duplicates).toBe(1);
      } finally {
        await server.close();
      }
    }
  });
});
