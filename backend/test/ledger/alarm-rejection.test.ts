import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { credentialHash } from "../../src/app.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { type AccountObject, stepOf } from "../../src/do/account-object.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { chat } from "../../src/gateway/chat.ts";
import { costOf } from "../../src/ledger/cost.ts";
import { makeLedger, type Price, type Usage } from "../../src/ledger/index.ts";
import { errorName } from "../../src/log.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Env, Tier } from "../../src/types.ts";
import prices from "../../test-kit/prices/v1.json";

type Method = "setAlarm" | "deleteAlarm";
type StepName = "reserve" | "dispatch" | "settle";
type HostCall = (this: DurableObjectStorage, ...args: unknown[]) => Promise<void>;
type AlarmMethods = Record<Method, HostCall>;
type Row = Record<string, string | number | null>;
type Fault = { name: string; account: string; step: StepName; method: Method; logged: string; call(real: HostCall, storage: DurableObjectStorage, args: unknown[]): Promise<void> };

const bindings = env as unknown as Env;
const CREDENTIAL = "4e".repeat(32);
const HOLD_LIFE_MS = 3000;
const TIMER_TURN_MS = 50;
const FIXED_NOW = Date.UTC(2030, 0, 1);
const realNow = Date.now;
const CONTROL_ACCOUNT = "cf708192-a3b4-4ecf-9fe1-5c6d7e8f90a1";
const EXPECTED_CALLS = ["setAlarm reserve", "setAlarm dispatch", "deleteAlarm settle", "setAlarm reserve", "setAlarm dispatch", "deleteAlarm settle"];
const ARMED = [1, 2].flatMap((request) => [
  `${request} setAlarm reserve at the open hold's expiry`,
  `${request} setAlarm dispatch at the open hold's expiry`,
  `${request} deleteAlarm settle with no open hold`,
]);
const timerTurn =() => new Promise((resolve) => setTimeout(resolve, TIMER_TURN_MS));
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

function accountNamedRejection(account: string): Error {
  const error = new Error(`the alarm write of account ${account} was not kept`);
  error.name = `alarm store of account ${account} failed: QuotaError`;
  return error;
}

function unreadableNameRejection(): object {
  return Object.defineProperty({}, "name", {
    get() {
      throw new Error("the name cannot be read");
    },
  });
}

function nameThatChanges(account: string): object {
  let reads = 0;
  return Object.defineProperty({}, "name", {
    get() {
      reads += 1;
      return reads === 1 ? "TypeError" : `alarm store of account ${account}`;
    },
  });
}

const refused = (real: HostCall, storage: DurableObjectStorage) => real.call(storage, Number.NaN);
const rejectsAfterTheWrite =
  (reason: () => unknown) =>
  (real: HostCall, storage: DurableObjectStorage, args: unknown[]): Promise<void> =>
    real.apply(storage, args).then(() => {
      throw reason();
    });

const faults: Fault[] = [
  {
    name: "the host refuses NaN, which the test passes in place of the step's time",
    account: "6f1e2d3c-4b5a-4968-8776-5a4b3c2d1e0f",
    step: "dispatch",
    method: "setAlarm",
    logged: "TypeError",
    call: refused,
  },
  {
    name: "the host's write of the step's time resolves and the promise that the step gets then rejects",
    account: "7a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
    step: "dispatch",
    method: "setAlarm",
    logged: "AlarmRejectionError",
    call: rejectsAfterTheWrite(lateRejection),
  },
  {
    name: "the host refuses NaN, which the test passes in place of the step's time",
    account: "8b3c4d5e-6f70-4a8b-9cad-1e2f3a4b5c6d",
    step: "reserve",
    method: "setAlarm",
    logged: "TypeError",
    call: refused,
  },
  {
    name: "the host's write resolves and the promise then rejects with an error whose name holds the account id and is not of the pattern of errorName",
    account: "9c4d5e6f-7081-4b9c-8dbe-2f3a4b5c6d7e",
    step: "reserve",
    method: "setAlarm",
    logged: "Error",
    call: rejectsAfterTheWrite(() => accountNamedRejection("9c4d5e6f-7081-4b9c-8dbe-2f3a4b5c6d7e")),
  },
  {
    name: "the host's delete resolves and the promise that the step gets then rejects",
    account: "ad5e6f70-8192-4cad-9ecf-3a4b5c6d7e8f",
    step: "settle",
    method: "deleteAlarm",
    logged: "AlarmRejectionError",
    call: rejectsAfterTheWrite(lateRejection),
  },
  {
    name: "the host's delete resolves and the promise then rejects with a value whose name throws when it is read",
    account: "be6f7081-92a3-4dbe-8fd0-4b5c6d7e8f90",
    step: "settle",
    method: "deleteAlarm",
    logged: "Error",
    call: rejectsAfterTheWrite(unreadableNameRejection),
  },
  {
    name: "the host's delete resolves and the promise then rejects with a value whose name reads TypeError at its first read and a text with the account id at every later read",
    account: "c07a8192-b3c4-4d5e-8f60-718293a4b5c6",
    step: "settle",
    method: "deleteAlarm",
    logged: "TypeError",
    call: rejectsAfterTheWrite(() => nameThatChanges("c07a8192-b3c4-4d5e-8f60-718293a4b5c6")),
  },
];

let lines: string[] = [];
let rejections: { name: string; promise: unknown }[] = [];
const onRejection = (reason: unknown, promise: unknown) => {
  rejections.push({ name: errorName(reason), promise });
};
const alarm: {
  request: number;
  during: string;
  calls: string[];
  armed: { request: number; during: string; method: Method; at: unknown; due: unknown }[];
  fault: Fault | null;
  faulted: { request: number; during: string; promise: Promise<void> }[];
} = {
  request: 0,
  during: "",
  calls: [],
  armed: [],
  fault: null,
  faulted: [],
};
const secondRejection = rejectsAfterTheWrite(() => "AlarmRejectionError");
let storagePrototype: AlarmMethods | null = null;
let hostMethods: AlarmMethods | null = null;
let controlStates: Record<string, string[]>[] | null = null;
function methodsOf(target: object): string[] {
  const names = new Set<string>();
  for (let at: object | null = target; at !== null && at !== Object.prototype; at = Object.getPrototypeOf(at)) {
    for (const name of Object.getOwnPropertyNames(at)) names.add(name);
  }
  return [...names].filter((name) => name !== "constructor" && typeof (target as Record<string, unknown>)[name] === "function").sort();
}
const consoleMethods = methodsOf(console);

function arming(each: (typeof alarm.armed)[number]): string {
  const head = `${each.request} ${each.method} ${each.during}`;
  if (each.method === "deleteAlarm") return each.due === null ? `${head} with no open hold` : `${head} with an open hold to ${String(each.due)}`;
  return each.due !== null && each.at === each.due ? `${head} at the open hold's expiry` : `${head} at ${String(each.at)} with the open hold's expiry at ${String(each.due)}`;
}

function counted(method: Method): HostCall {
  return function (this: DurableObjectStorage, ...args: unknown[]) {
    const real = (hostMethods as AlarmMethods)[method];
    alarm.calls.push(`${method} ${alarm.during}`);
    const [open] = this.sql.exec("SELECT min(expires_at) AS due FROM ledger_holds WHERE state = 'open'").toArray() as { due: unknown }[];
    alarm.armed.push({ request: alarm.request, during: alarm.during, method, at: method === "setAlarm" ? args[0] : null, due: open?.due ?? null });
    const fault = alarm.fault;
    if (fault === null || fault.method !== method || fault.step !== alarm.during || alarm.faulted.some((each) => each.request === alarm.request)) return real.apply(this, args);
    const promise = alarm.request === 1 ? fault.call(real, this, args) : secondRejection(real, this, args);
    alarm.faulted.push({ request: alarm.request, during: alarm.during, promise });
    return promise;
  };
}

function masked(value: unknown, account: string): unknown {
  return typeof value === "string" ? value.split(account).join("<account>") : value;
}

async function contents(state: DurableObjectState, account: string): Promise<Record<string, string[]>> {
  const dump: Record<string, string[]> = {};
  const entries = state.storage.sql.exec("SELECT type, name, sql FROM sqlite_master ORDER BY type, name").toArray() as Row[];
  dump.schema = entries.map((entry) => JSON.stringify(entry));
  for (const entry of entries) {
    const name = String(entry.name);
    if (entry.type !== "table" || name.startsWith("_cf_")) continue;
    const rows = state.storage.sql.exec(`SELECT * FROM "${name}"`).toArray() as Row[];
    dump[name] = rows.map((row) => JSON.stringify(Object.entries(row).map(([key, value]) => [key, masked(value, account)]))).sort();
  }
  dump["key-value entries"] = [...(await state.storage.list()).entries()].map(([key, value]) => JSON.stringify([masked(key, account), masked(value, account)])).sort();
  const alarmAt = await state.storage.getAlarm();
  dump.alarm = [JSON.stringify(masked(alarmAt, account))];
  return dump;
}

async function twoRequests(account: string, afterRequest: (request: number) => Promise<unknown>) {
  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(account));
  const hash = await credentialHash(CREDENTIAL);
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
    expect(await entries.testSeed({ accountId: account, hash, subscription: "active", cap: { amount: 10_000, period: "week" } })).toEqual({ result: "seeded" });
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
        headers: { "halvo-account": account, authorization: `Bearer ${CREDENTIAL}`, "content-type": "application/json", "x-request-id": requestId },
        body: BODY,
      }),
      gatewayEnv,
      ctx,
    );
    const text = await response.text();
    await Promise.all(waits);
    return { status: response.status, text };
  };
  const requestId = `alarm-rejection-${account}`;
  const nextId = `after-rejection-${account}`;
  const states: Record<string, string[]>[] = [];
  const stateNow = () =>
    runInDurableObject(stub, async (_instance: AccountObject, state) => {
      states.push(await contents(state, account));
    });
  alarm.request = 1;
  const sent = await send(requestId);
  await afterRequest(1);
  await timerTurn();
  await stateNow();
  alarm.request = 2;
  const next = await send(nextId);
  await afterRequest(2);
  await timerTurn();
  alarm.request = 0;
  vi.unstubAllGlobals();

  let after: AccountObject | null = null;
  let rows: Row[] = [];
  let periods: Row[] = [];
  await runInDurableObject(stub, async (instance: AccountObject, state) => {
    after = instance;
    rows = state.storage.sql.exec("SELECT request_id, state, dispatched, cost FROM ledger_holds ORDER BY created_seq").toArray() as Row[];
    periods = state.storage.sql.exec("SELECT reported, unreported, open FROM ledger_periods").toArray() as Row[];
    states.push(await contents(state, account));
  });
  return { requestId, nextId, sent, next, calls, dispatched, upstreamCalls, before, after, rows, periods, states };
}

beforeAll(async () => {
  await runInDurableObject(bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName("alarm-rejection-prototype")), async (_instance: AccountObject, state) => {
    storagePrototype = Object.getPrototypeOf(state.storage) as AlarmMethods;
    hostMethods = { setAlarm: storagePrototype.setAlarm, deleteAlarm: storagePrototype.deleteAlarm };
  });
  Date.now = () => FIXED_NOW;
  const control = await twoRequests(CONTROL_ACCOUNT, async () => {});
  expect(control.dispatched).toEqual(["go", "go"]);
  expect(control.rows.map((row) => row.state)).toEqual(["settled", "settled"]);
  controlStates = control.states;
});
beforeEach(() => {
  lines = [];
  rejections = [];
  alarm.during = "";
  alarm.calls = [];
  alarm.armed = [];
  alarm.fault = null;
  alarm.faulted = [];
  const proto = storagePrototype as AlarmMethods;
  proto.setAlarm = counted("setAlarm");
  proto.deleteAlarm = counted("deleteAlarm");
  process.on("unhandledRejection", onRejection);
  for (const method of consoleMethods) {
    vi.spyOn(console as unknown as Record<string, (...args: unknown[]) => void>, method).mockImplementation((...args: unknown[]) => {
      lines.push(`${method} ${args.map(String).join(" ")}`);
    });
  }
});
afterAll(() => {
  Date.now = realNow;
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
  test(`unit 4.8, contract 1.4, on workerd: when the promise of the host's ${fault.method} in the ${fault.step} step rejects after the step has committed (${fault.name}), and in the next request on the object the same call's promise rejects with the string AlarmRejectionError after the host's call, the app writes, by any console method, exactly the two lines console.log of {"event":"alarm-error","error":"${fault.logged}"} and of {"event":"alarm-error","error":"Error"}, errorName of each rejection and no other field of it, and both rejections are handled; the gateway gets go, the upstream is called once and the hold settles at the reported usage with the period totals to match, and the next request on the object gets go and settles; up to ${TIMER_TURN_MS} ms after the first request and again after the second, the hold keeps the mark, the object is not reset, the host's alarm calls are, for each of the two requests in order, setAlarm for the reserve, setAlarm for the dispatch and deleteAlarm for the settle, and no other call of setAlarm or deleteAlarm, each setAlarm of the object, also after a rejection, passes the expiry of the open hold as the rows give it when the call is made and each deleteAlarm comes when no hold is open; and, with Date.now giving one fixed time in this run and in the control, after each request the object's schema, the rows of each table and of its key-value store, and its alarm, every value held with the account id alone masked because the control runs on another account, equal those after the same request with no rejection`, async () => {
    alarm.fault = fault;
    const run = await twoRequests(fault.account, (request) => vi.waitFor(() => expect(lines).toHaveLength(request)));

    expect(run.sent.status).toBe(200);
    expect(run.sent.text).toContain("[DONE]");
    expect(run.next.status).toBe(200);
    expect(run.next.text).toContain("[DONE]");
    expect(run.calls).toEqual(["reserve", "dispatch", "settle", "reserve", "dispatch", "settle"]);
    expect(run.dispatched).toEqual(["go", "go"]);
    expect(run.upstreamCalls).toBe(2);
    expect(alarm.calls).toEqual(EXPECTED_CALLS);
    expect(alarm.armed.map(arming)).toEqual(ARMED);
    expect(consoleMethods).toEqual(expect.arrayContaining(["log", "error", "warn", "info", "debug", "table", "dir", "trace"]));
    expect(alarm.faulted.map((each) => `${each.request} ${each.during}`)).toEqual([`1 ${fault.step}`, `2 ${fault.step}`]);
    expect(lines).toEqual([`log {"event":"alarm-error","error":"${fault.logged}"}`, `log {"event":"alarm-error","error":"Error"}`]);
    expect(rejections).toEqual([]);
    const cost = costOf(LOW, USAGE);
    expect(run.after).not.toBeNull();
    expect(run.after).toBe(run.before);
    expect(run.rows).toEqual([
      { request_id: run.requestId, state: "settled", dispatched: 1, cost },
      { request_id: run.nextId, state: "settled", dispatched: 1, cost },
    ]);
    expect(run.periods).toEqual([{ reported: 2 * cost, unreported: 0, open: 0 }]);
    expect(run.states).toEqual(controlStates);
  });
}
