import { env, runInDurableObject } from "cloudflare:test";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { credentialHash } from "../../src/app.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { type AccountObject, stepOf } from "../../src/do/account-object.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { chat } from "../../src/gateway/chat.ts";
import { costOf } from "../../src/ledger/cost.ts";
import { makeLedger, type Price, type Usage } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Env, Tier } from "../../src/types.ts";
import prices from "../../test-kit/prices/v1.json";

type SetAlarm = (this: DurableObjectStorage, at: unknown, options?: unknown) => Promise<void>;
type DeleteAlarm = (this: DurableObjectStorage, options?: unknown) => Promise<void>;
type AlarmMethods = { setAlarm: SetAlarm; deleteAlarm: DeleteAlarm };
type Row = Record<string, string | number | null>;

const bindings = env as unknown as Env;
const CREDENTIAL = "4e".repeat(32);
const HOLD_LIFE_MS = 3000;
const TIMER_TURN_MS = 50;
const EXPECTED_CALLS = ["setAlarm reserve", "setAlarm dispatch", "deleteAlarm settle", "setAlarm reserve", "setAlarm dispatch", "deleteAlarm settle"];
const timerTurn = () => new Promise((resolve) => setTimeout(resolve, TIMER_TURN_MS));
const USAGE: Usage = { inputTokens: 7, outputTokens: 9 };
const LOW: Price = { input: prices.low_cost_hosted.input, output: prices.low_cost_hosted.output };
const BODY = JSON.stringify({ tier: "low_cost_hosted", max_tokens: 100, messages: [{ role: "user", content: "Synthetic alarm rejection test." }] });
const encoder = new TextEncoder();
const chunk = (data: string) => encoder.encode(`data: ${data}\n\n`);
const UPSTREAM = [chunk(JSON.stringify({ choices: [], usage: { prompt_tokens: USAGE.inputTokens, completion_tokens: USAGE.outputTokens } })), chunk("[DONE]")];
const port: ProviderPort = {
  startCheckout: async () => ({ kind: "failed" }),
  verifyWebhook: async () => ({ kind: "rejected" }),
  readSession: async () => ({ kind: "failed" }),
  openManagePage: async () => ({ kind: "failed" }),
  cancelSubscription: async () => ({ kind: "nothing-to-cancel" }),
  sendUsage: async () => ({ kind: "failed" }),
  readUsageTotal: async () => ({ kind: "failed" }),
};

function lateRejection(): Error {
  const error = new Error("the alarm write was not kept");
  error.name = "AlarmRejectionError";
  return error;
}

const faults: { name: string; account: string; reason: string; call(real: SetAlarm, storage: DurableObjectStorage, at: unknown, options: unknown): Promise<void> }[] = [
  {
    name: "the host refuses NaN, which the test passes in place of the step's time",
    account: "6f1e2d3c-4b5a-4968-8776-5a4b3c2d1e0f",
    reason: "TypeError",
    call: (real, storage) => real.call(storage, Number.NaN),
  },
  {
    name: "the host's write of the step's time resolves and the promise that the step gets then rejects",
    account: "7a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
    reason: "AlarmRejectionError",
    call: (real, storage, at, options) =>
      real.call(storage, at, options).then(() => {
        throw lateRejection();
      }),
  },
];

let lines: string[] = [];
let rejections: { name: string; promise: unknown }[] = [];
const onRejection = (reason: unknown, promise: unknown) => {
  rejections.push({ name: String((reason as Error | null)?.name), promise });
};
const alarm: { during: string; calls: string[]; fault: (typeof faults)[number] | null; faulted: { during: string; promise: Promise<void> }[] } = {
  during: "",
  calls: [],
  fault: null,
  faulted: [],
};
let storagePrototype: AlarmMethods | null = null;
let hostMethods: AlarmMethods | null = null;
beforeAll(async () => {
  await runInDurableObject(bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName("alarm-rejection-prototype")), async (_instance: AccountObject, state) => {
    storagePrototype = Object.getPrototypeOf(state.storage) as AlarmMethods;
    hostMethods = { setAlarm: storagePrototype.setAlarm, deleteAlarm: storagePrototype.deleteAlarm };
  });
});
beforeEach(() => {
  lines = [];
  rejections = [];
  alarm.during = "";
  alarm.calls = [];
  alarm.fault = null;
  alarm.faulted = [];
  const proto = storagePrototype as AlarmMethods;
  const host = hostMethods as AlarmMethods;
  proto.setAlarm = function (this: DurableObjectStorage, at: unknown, options?: unknown) {
    alarm.calls.push(`setAlarm ${alarm.during}`);
    if (alarm.fault === null || alarm.during !== "dispatch" || alarm.faulted.length > 0) return host.setAlarm.call(this, at, options);
    const promise = alarm.fault.call(host.setAlarm, this, at, options);
    alarm.faulted.push({ during: alarm.during, promise });
    return promise;
  };
  proto.deleteAlarm = function (this: DurableObjectStorage, options?: unknown) {
    alarm.calls.push(`deleteAlarm ${alarm.during}`);
    return host.deleteAlarm.call(this, options);
  };
  process.on("unhandledRejection", onRejection);
  for (const method of ["log", "error", "warn", "info", "debug"] as const) {
    vi.spyOn(console, method).mockImplementation((line: unknown) => {
      lines.push(`${method} ${String(line)}`);
    });
  }
});
afterEach(() => {
  const proto = storagePrototype as AlarmMethods;
  const host = hostMethods as AlarmMethods;
  proto.setAlarm = host.setAlarm;
  proto.deleteAlarm = host.deleteAlarm;
  process.off("unhandledRejection", onRejection);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

for (const fault of faults) {
  test(`unit 4.7, ruling B-51, on workerd: when the promise of the host's alarm call rejects after the dispatch step has committed (${fault.name}), the gateway gets go, the upstream is called once and the hold settles at the reported usage with the period totals to match, and the next request on the object gets go and settles; up to ${TIMER_TURN_MS} ms after the first request and again after the second (the object's timers get that turn and no later one), the hold keeps the mark, the object is not reset, the host's alarm calls are, for each of the two requests in order, setAlarm for the reserve, setAlarm for the dispatch and deleteAlarm for the settle, and no other call of setAlarm or deleteAlarm, the rejection stays unhandled and the app writes no console line`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(fault.account));
    const hash = await credentialHash(CREDENTIAL);
    alarm.fault = fault;
    let before: AccountObject | null = null;
    await runInDurableObject(stub, async (instance: AccountObject, state) => {
      before = instance;
      const entries = makeEntries(
        {
          step: stepOf(state.storage),
          clock: () => Date.now(),
          setAlarm() {},
          getAlarm: () => state.storage.getAlarm(),
          deleteAll: async () => {},
          port,
          config: { holdLifeMs: HOLD_LIFE_MS, intervalSeconds: 3600, marginSeconds: 300, publicOrigin: "", testClock: true },
        },
        { accounts: makeAccounts(), ledger: makeLedger({ holdLifeMs: HOLD_LIFE_MS }), metering: makeMetering({ intervalSeconds: 3600, marginSeconds: 300 }) },
      );
      expect(await entries.testSeed({ accountId: fault.account, hash, subscription: "active", cap: { amount: 10_000, period: "week" } })).toEqual({ result: "seeded" });
    });

    const calls: string[] = [];
    const dispatched: string[] = [];
    const call = async (name: string, run: () => Promise<unknown>): Promise<unknown> => {
      calls.push(name);
      alarm.during = name;
      try {
        return await run();
      } finally {
        alarm.during = "";
      }
    };
    const object = {
      reserve: (h: string, req: { requestId: string; tier: Tier; amount: number; price: Price }) => call("reserve", () => stub.reserve(h, req)),
      dispatch: (requestId: string) =>
        call("dispatch", async () => {
          const answer = await stub.dispatch(requestId);
          dispatched.push(answer);
          return answer;
        }),
      settle: (requestId: string, usage: Usage) => call("settle", () => stub.settle(requestId, usage)),
      void: (requestId: string) => call("void", () => stub.void(requestId)),
    };
    let upstreamCalls = 0;
    vi.stubGlobal("fetch", async () => {
      upstreamCalls += 1;
      let at = 0;
      const body = new ReadableStream<Uint8Array>({
        pull(controller) {
          const next = UPSTREAM[at];
          at += 1;
          if (next === undefined) controller.close();
          else controller.enqueue(next);
        },
      });
      return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
    });
    const gatewayEnv = {
      ACCOUNT: { idFromName: (name: string) => bindings.ACCOUNT.idFromName(name), get: () => object },
      UPSTREAM_BASE_URL: "http://127.0.0.1:9",
      UPSTREAM_DEADLINE_MS: "100",
      DISPATCH_MARGIN_MS: "400",
    } as unknown as Env;
    const send = async (requestId: string) => {
      const waits: Promise<unknown>[] = [];
      const ctx = { waitUntil: (promise: Promise<unknown>) => waits.push(promise) } as unknown as ExecutionContext;
      const response = await chat(
        new Request("http://backend.test/v1/chat", {
          method: "POST",
          headers: { "halvo-account": fault.account, authorization: `Bearer ${CREDENTIAL}`, "content-type": "application/json", "x-request-id": requestId },
          body: BODY,
        }),
        gatewayEnv,
        ctx,
      );
      const text = await response.text();
      await Promise.all(waits);
      return { status: response.status, text };
    };
    const requestId = `alarm-rejection-${fault.reason}`;
    const nextId = `after-rejection-${fault.reason}`;
    const sent = await send(requestId);
    await vi.waitFor(() => expect(rejections).toHaveLength(1));
    await timerTurn();
    const next = await send(nextId);
    await timerTurn();

    expect(sent.status).toBe(200);
    expect(sent.text).toContain("[DONE]");
    expect(next.status).toBe(200);
    expect(next.text).toContain("[DONE]");
    expect(calls).toEqual(["reserve", "dispatch", "settle", "reserve", "dispatch", "settle"]);
    expect(dispatched).toEqual(["go", "go"]);
    expect(upstreamCalls).toBe(2);
    expect(alarm.calls).toEqual(EXPECTED_CALLS);
    expect(alarm.faulted.map((each) => each.during)).toEqual(["dispatch"]);
    expect(rejections.map((each) => each.name)).toEqual([fault.reason]);
    expect(rejections[0]?.promise).toBe(alarm.faulted[0]?.promise);
    expect(lines).toEqual([]);
    let after: AccountObject | null = null;
    let rows: Row[] = [];
    let periods: Row[] = [];
    await runInDurableObject(stub, async (instance: AccountObject, state) => {
      after = instance;
      rows = state.storage.sql.exec("SELECT request_id, state, dispatched, cost FROM ledger_holds ORDER BY created_seq").toArray() as Row[];
      periods = state.storage.sql.exec("SELECT reported, unreported, open FROM ledger_periods").toArray() as Row[];
    });
    const cost = costOf(LOW, USAGE);
    expect(after).not.toBeNull();
    expect(after).toBe(before);
    expect(rows).toEqual([
      { request_id: requestId, state: "settled", dispatched: 1, cost },
      { request_id: nextId, state: "settled", dispatched: 1, cost },
    ]);
    expect(periods).toEqual([{ reported: 2 * cost, unreported: 0, open: 0 }]);
  });
}
