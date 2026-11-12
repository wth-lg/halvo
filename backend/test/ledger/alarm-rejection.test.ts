import { env, runInDurableObject } from "cloudflare:test";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
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
type Row = Record<string, string | number | null>;

const bindings = env as unknown as Env;
const CREDENTIAL = "4e".repeat(32);
const HOLD_LIFE_MS = 3000;
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
    name: "the host refuses the time that it is given",
    account: "6f1e2d3c-4b5a-4968-8776-5a4b3c2d1e0f",
    reason: "TypeError",
    call: (real, storage) => real.call(storage, Number.NaN),
  },
  {
    name: "the host's write resolves and the promise then rejects",
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
beforeEach(() => {
  lines = [];
  rejections = [];
  process.on("unhandledRejection", onRejection);
  vi.spyOn(console, "log").mockImplementation((line: unknown) => {
    lines.push(String(line));
  });
});
afterEach(() => {
  process.off("unhandledRejection", onRejection);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

for (const fault of faults) {
  test(`unit 4.7, ruling B-51, on workerd: when the promise of the host's alarm call rejects after the dispatch step has committed (${fault.name}), the gateway gets go, the hold keeps the mark, the upstream is called once and the hold settles at the reported usage; the object is not reset, nothing calls the alarm again, and the rejection reaches the isolate unhandled while the app logs no line`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(fault.account));
    const hash = await credentialHash(CREDENTIAL);
    let during = "";
    const alarmCalls: string[] = [];
    const faulted: { during: string; promise: Promise<void> }[] = [];
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
      const storage = state.storage;
      const real = storage.setAlarm as unknown as SetAlarm;
      (storage as unknown as { setAlarm: SetAlarm }).setAlarm = function (this: DurableObjectStorage, at: unknown, options?: unknown) {
        alarmCalls.push(during);
        if (during !== "dispatch") return real.call(this, at, options);
        const promise = fault.call(real, this, at, options);
        faulted.push({ during, promise });
        return promise;
      };
    });

    const calls: string[] = [];
    const dispatched: string[] = [];
    const call = async (name: string, run: () => Promise<unknown>): Promise<unknown> => {
      calls.push(name);
      during = name;
      try {
        return await run();
      } finally {
        during = "";
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
    const waits: Promise<unknown>[] = [];
    const ctx = { waitUntil: (promise: Promise<unknown>) => waits.push(promise) } as unknown as ExecutionContext;
    const requestId = `alarm-rejection-${fault.reason}`;
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
    await vi.waitFor(() => expect(rejections).toHaveLength(1));

    expect(response.status).toBe(200);
    expect(text).toContain("[DONE]");
    expect(calls).toEqual(["reserve", "dispatch", "settle"]);
    expect(dispatched).toEqual(["go"]);
    expect(upstreamCalls).toBe(1);
    expect(alarmCalls).toEqual(["reserve", "dispatch"]);
    expect(faulted.map((each) => each.during)).toEqual(["dispatch"]);
    expect(rejections.map((each) => each.name)).toEqual([fault.reason]);
    expect(rejections[0]?.promise).toBe(faulted[0]?.promise);
    expect(lines).toEqual([]);
    let after: AccountObject | null = null;
    let rows: Row[] = [];
    await runInDurableObject(stub, async (instance: AccountObject, state) => {
      after = instance;
      rows = state.storage.sql.exec("SELECT state, dispatched, cost FROM ledger_holds WHERE request_id = ?", requestId).toArray() as Row[];
    });
    expect(after).not.toBeNull();
    expect(after).toBe(before);
    expect(rows).toEqual([{ state: "settled", dispatched: 1, cost: costOf(LOW, USAGE) }]);
  });
}
