import { env, runInDurableObject } from "cloudflare:test";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { AccountObject, stepOf } from "../../src/do/account-object.ts";
import { type Host, makeEntries } from "../../src/do/entries.ts";
import { makeLedger, type Price } from "../../src/ledger/index.ts";
import { errorName } from "../../src/log.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Env } from "../../src/types.ts";
import prices from "../../test-kit/prices/v1.json";

type HostCall = (this: DurableObjectStorage, ...args: unknown[]) => Promise<void>;
type Row = Record<string, string | number | null>;

const bindings = env as unknown as Env;
const ACCOUNT = "d18293a4-b5c6-4d7e-8f90-a1b2c3d4e5f6";
const HASH = "6c".repeat(32);
const HOLD_LIFE_MS = 3000;
const RESERVE_AT = Date.UTC(2030, 0, 1) + 1_237;
const CAUGHT_AT = RESERVE_AT + 211;
const DISPATCH_AT = RESERVE_AT + 389;
const SECOND_RESERVE_AT = Date.UTC(2030, 0, 1) + 2_000;
const SECOND_CAUGHT_AT = Date.UTC(2030, 0, 1) + 3_000;
const SECOND_DISPATCH_AT = Date.UTC(2030, 0, 1) + 4_000;
const TIMER_TURN_MS = 50;
const ALARM_ERROR = 'log {"event":"alarm-error","error":"TypeError"}';
const LOW: Price = { input: prices.low_cost_hosted.input, output: prices.low_cost_hosted.output };
const SEED_PORT: ProviderPort = {
  startCheckout: async () => ({ kind: "failed" }),
  verifyWebhook: async () => ({ kind: "rejected" }),
  readSession: async () => ({ kind: "failed" }),
  openManagePage: async () => ({ kind: "failed" }),
  cancelSubscription: async () => ({ kind: "nothing-to-cancel" }),
  sendUsage: async () => ({ kind: "failed" }),
  readUsageTotal: async () => ({ kind: "failed" }),
};
const realNow = Date.now;
const timerTurn = () => new Promise((resolve) => setTimeout(resolve, TIMER_TURN_MS));

let lines: string[] = [];
let rejections: string[] = [];
const onRejection = (reason: unknown) => {
  rejections.push(errorName(reason));
};

beforeEach(() => {
  lines = [];
  rejections = [];
  process.on("unhandledRejection", onRejection);
  for (const method of ["log", "error", "warn", "info", "debug", "trace", "dir", "table"] as const) {
    vi.spyOn(console, method).mockImplementation((...args: unknown[]) => {
      lines.push(`${method} ${args.map(String).join(" ")}`);
    });
  }
});
afterEach(() => {
  process.off("unhandledRejection", onRejection);
  Date.now = realNow;
  vi.restoreAllMocks();
});

function tables(state: DurableObjectState): Record<string, Row[]> {
  const dump: Record<string, Row[]> = {};
  const entries = (state.storage.sql.exec("SELECT type, name, sql FROM sqlite_master ORDER BY type, name").toArray() as Row[]).filter((entry) => !String(entry.name).startsWith("_cf_"));
  dump.schema = entries;
  for (const entry of entries) {
    const name = String(entry.name);
    if (entry.type !== "table" || name.startsWith("_cf_")) continue;
    dump[name] = (state.storage.sql.exec(`SELECT * FROM "${name}"`).toArray() as Row[]).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  }
  return dump;
}

type Descriptors = Map<PropertyKey, PropertyDescriptor>;

function descriptorsOf(target: object): Descriptors {
  return new Map(Reflect.ownKeys(target).map((key) => [key, Object.getOwnPropertyDescriptor(target, key) as PropertyDescriptor]));
}

function changedKeys(before: Descriptors, after: Descriptors): string[] {
  const keys = new Set<PropertyKey>([...before.keys(), ...after.keys()]);
  const changed: string[] = [];
  for (const key of keys) {
    const a = before.get(key);
    const b = after.get(key);
    const fields = ["value", "get", "set", "writable", "enumerable", "configurable"] as const;
    if (a === undefined || b === undefined || fields.some((field) => !Object.is(a[field], b[field]))) changed.push(String(key));
  }
  return changed.sort();
}

function memoryOf(instance: AccountObject) {
  const fields = { ...(instance as unknown as Record<string, unknown>) };
  const host = fields.host as Host;
  return {
    fields,
    host,
    hostFields: { ...host },
    port: host.port,
    methods: { ...host.port },
    config: host.config,
    settings: { ...host.config },
    ownKeys: Reflect.ownKeys(instance).map(String).sort(),
    instance: descriptorsOf(instance),
    entries: descriptorsOf(fields.entries as object),
    hostOwn: descriptorsOf(host),
    prototype: Object.getPrototypeOf(instance) as object,
    prototypeOwn: descriptorsOf(AccountObject.prototype),
    env: descriptorsOf(fields.env as object),
  };
}

function memoryChanges(before: ReturnType<typeof memoryOf>, after: ReturnType<typeof memoryOf>) {
  return {
    ownKeys: after.ownKeys,
    instance: changedKeys(before.instance, after.instance),
    entries: changedKeys(before.entries, after.entries),
    host: changedKeys(before.hostOwn, after.hostOwn),
    prototype: after.prototype === before.prototype && after.prototype === AccountObject.prototype,
    prototypeOwn: changedKeys(before.prototypeOwn, after.prototypeOwn),
    env: changedKeys(before.env, after.env),
  };
}

function same(before: Record<string, unknown>, after: Record<string, unknown>): Record<string, boolean> {
  const names = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort();
  return Object.fromEntries(names.map((name) => [name, name in before && name in after && before[name] === after[name]]));
}

test("unit 4.9 (iii), ruling P7-96 point 7, on workerd: after a reserve whose setAlarm the host refuses, which leaves the open hold with no alarm, the dispatch step arms the alarm again at the open hold's expiry, the reserve's clock plus the hold's life, and not at a stale time, a whole second or its own clock plus the hold's life; the rejection, caught at a later clock that is no whole second, writes one line alarm-error TypeError and nothing else: no stored value, no alarm and no own property of the instance, of its entries, of its host, of its env or of AccountObject.prototype changes, and its prototype stays AccountObject.prototype, so its port and every field of its settings are what the constructor made; a second reserve whose setAlarm the host refuses, at clocks of whole seconds, leaves the alarm that stands, the dispatch keeps it at the earliest open expiry, and every stored value read at the end is the steps' own", async () => {
  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.newUniqueId());
  const run = await runInDurableObject(stub, async (instance: AccountObject, state) => {
    const seeded = makeEntries(
      {
        step: stepOf(state.storage),
        clock: () => RESERVE_AT - 1_000_000,
        setAlarm() {},
        getAlarm: () => state.storage.getAlarm(),
        deleteAll: async () => {},
        port: SEED_PORT,
        config: { holdLifeMs: HOLD_LIFE_MS, intervalSeconds: 3600, marginSeconds: 300, publicOrigin: "", testClock: true },
      },
      { accounts: makeAccounts(), ledger: makeLedger({ holdLifeMs: HOLD_LIFE_MS }), metering: makeMetering({ intervalSeconds: 3600, marginSeconds: 300 }) },
    );
    expect(await seeded.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", cap: { amount: 10, period: "week" } })).toEqual({ result: "seeded" });
    const made = memoryOf(instance);
    expect(made.settings).toEqual({ holdLifeMs: HOLD_LIFE_MS, intervalSeconds: 3600, marginSeconds: 300, publicOrigin: "http://halvo.test", testClock: false });
    const alarmBefore = await state.storage.getAlarm();

    const storage = Object.getPrototypeOf(state.storage) as Record<"setAlarm" | "deleteAlarm", HostCall>;
    const host = { setAlarm: storage.setAlarm, deleteAlarm: storage.deleteAlarm };
    const calls: string[] = [];
    let release: () => void = () => {};
    let during = "";
    storage.setAlarm = function (this: DurableObjectStorage, ...args: unknown[]) {
      calls.push(`${during} setAlarm ${String(args[0])}`);
      if (during !== "reserve") return host.setAlarm.apply(this, args);
      let reason: unknown = null;
      const refused = host.setAlarm.call(this, Number.NaN).catch((caught: unknown) => {
        reason = caught;
      });
      return new Promise<void>((_resolve, reject) => {
        release = () => {
          void refused.then(() => reject(reason));
        };
      });
    };
    storage.deleteAlarm = function (this: DurableObjectStorage, ...args: unknown[]) {
      calls.push(`${during} deleteAlarm`);
      return host.deleteAlarm.apply(this, args);
    };
    const step = async <T>(name: string, at: number, run: () => Promise<T>): Promise<T> => {
      Date.now = () => at;
      during = name;
      try {
        return await run();
      } finally {
        during = "";
      }
    };
    const rejected = async (at: number) => {
      Date.now = () => at;
      release();
      await timerTurn();
    };
    const snapshot = async () => ({ tables: tables(state), alarm: await state.storage.getAlarm(), lines: [...lines], rejections: [...rejections] });
    try {
      const reserved = await step("reserve", RESERVE_AT, () => instance.reserve(HASH, { requestId: "stale", tier: "low_cost_hosted", amount: 10_000, price: LOW }));
      const afterReserve = await snapshot();
      const memoryBeforeRejection = memoryOf(instance);
      await rejected(CAUGHT_AT);
      const afterRejection = await snapshot();
      const memoryAfterRejection = memoryOf(instance);
      const dispatched = await step("dispatch", DISPATCH_AT, () => instance.dispatch("stale"));
      await timerTurn();
      const afterDispatch = await snapshot();
      const secondReserved = await step("reserve", SECOND_RESERVE_AT, () => instance.reserve(HASH, { requestId: "stale-2", tier: "low_cost_hosted", amount: 10_000, price: LOW }));
      const afterSecondReserve = await snapshot();
      const memoryBeforeSecondRejection = memoryOf(instance);
      await rejected(SECOND_CAUGHT_AT);
      const afterSecondRejection = await snapshot();
      const memoryAfterSecondRejection = memoryOf(instance);
      const secondDispatched = await step("dispatch", SECOND_DISPATCH_AT, () => instance.dispatch("stale-2"));
      await timerTurn();
      const end = await snapshot();
      return {
        reserved,
        afterReserve,
        afterRejection,
        memoryBeforeRejection,
        memoryAfterRejection,
        dispatched,
        afterDispatch,
        secondReserved,
        afterSecondReserve,
        afterSecondRejection,
        memoryBeforeSecondRejection,
        memoryAfterSecondRejection,
        secondDispatched,
        end,
        made,
        alarmBefore,
        calls,
      };
    } finally {
      storage.setAlarm = host.setAlarm;
      storage.deleteAlarm = host.deleteAlarm;
    }
  });

  const due = RESERVE_AT + HOLD_LIFE_MS;
  const secondDue = SECOND_RESERVE_AT + HOLD_LIFE_MS;
  expect(run.alarmBefore).toBeNull();
  expect(run.reserved).toMatchObject({ outcome: "admitted" });
  expect(run.secondReserved).toMatchObject({ outcome: "admitted" });
  expect(run.calls).toEqual([`reserve setAlarm ${due}`, `dispatch setAlarm ${due}`, `reserve setAlarm ${due}`, `dispatch setAlarm ${due}`]);

  expect(run.afterReserve.alarm).toBeNull();
  expect(run.afterReserve.tables.ledger_holds).toEqual([expect.objectContaining({ request_id: "stale", state: "open", dispatched: 0, created_at: RESERVE_AT, expires_at: due })]);
  expect(run.afterReserve.tables.skeleton).toEqual([{ destroying_since: null, last_now: RESERVE_AT, alarm_failures: 0 }]);
  expect(run.afterRejection.lines).toEqual([ALARM_ERROR]);
  expect(run.afterRejection.rejections).toEqual([]);
  expect(run.afterRejection.alarm).toBeNull();
  expect(run.afterRejection.tables).toEqual(run.afterReserve.tables);

  const unchanged = { ownKeys: ["ctx", "entries", "env", "host"], instance: [], entries: [], host: [], prototype: true, prototypeOwn: [], env: [] };
  for (const [before, after] of [
    [run.made, run.memoryBeforeRejection],
    [run.memoryBeforeRejection, run.memoryAfterRejection],
    [run.memoryBeforeSecondRejection, run.memoryAfterSecondRejection],
    [run.made, run.memoryAfterSecondRejection],
  ] as const) {
    expect(memoryChanges(before, after)).toEqual(unchanged);
  }
  expect([...run.made.prototypeOwn.keys()].map(String)).toEqual(expect.arrayContaining(["constructor", "alarm", "reserve", "dispatch", "settle"]));
  expect([...run.made.entries.keys()].map(String)).toEqual(expect.arrayContaining(["alarm", "rearm", "reserve", "dispatch"]));
  for (const memory of [run.memoryAfterRejection, run.memoryAfterSecondRejection]) {
    expect(same(run.made.fields, memory.fields)).toEqual({ ctx: true, entries: true, env: true, host: true });
    expect(Object.keys(run.made.fields).sort()).toEqual(["ctx", "entries", "env", "host"]);
    expect(same(run.made.hostFields, memory.hostFields)).toEqual({ clock: true, config: true, deleteAll: true, getAlarm: true, port: true, setAlarm: true, step: true });
    expect(memory.port).toBe(run.made.port);
    expect(same(run.made.methods, memory.methods)).toEqual(Object.fromEntries(Object.keys(SEED_PORT).sort().map((name) => [name, true])));
    expect(memory.config).toBe(run.made.config);
    expect(memory.settings).toEqual(run.made.settings);
    expect({ ...memory.config }).toEqual(run.made.settings);
  }

  expect(run.dispatched).toBe("go");
  expect(run.afterDispatch.alarm).toBe(due);
  expect(run.afterDispatch.lines).toEqual([ALARM_ERROR]);
  expect(run.afterDispatch.rejections).toEqual([]);
  const holdAfterDispatch = { ...(run.afterReserve.tables.ledger_holds?.[0] as Row), dispatched: 1 };
  expect(run.afterDispatch.tables).toEqual({ ...run.afterReserve.tables, ledger_holds: [holdAfterDispatch], skeleton: [{ destroying_since: null, last_now: DISPATCH_AT, alarm_failures: 0 }] });

  expect(run.afterSecondReserve.alarm).toBe(due);
  expect(run.afterSecondRejection.lines).toEqual([ALARM_ERROR, ALARM_ERROR]);
  expect(run.afterSecondRejection.rejections).toEqual([]);
  expect(run.afterSecondRejection.alarm).toBe(due);
  expect(run.afterSecondRejection.tables).toEqual(run.afterSecondReserve.tables);
  const secondHold = run.afterSecondReserve.tables.ledger_holds?.find((row) => row.request_id === "stale-2");
  expect(secondHold).toMatchObject({ state: "open", dispatched: 0, created_at: SECOND_RESERVE_AT, expires_at: secondDue });

  expect(run.secondDispatched).toBe("go");
  expect(run.end.alarm).toBe(due);
  expect(run.end.lines).toEqual([ALARM_ERROR, ALARM_ERROR]);
  expect(run.end.rejections).toEqual([]);
  const holdsAtEnd = [holdAfterDispatch, { ...(secondHold as Row), dispatched: 1 }].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  expect(run.end.tables).toEqual({ ...run.afterSecondReserve.tables, ledger_holds: holdsAtEnd, skeleton: [{ destroying_since: null, last_now: SECOND_DISPATCH_AT, alarm_failures: 0 }] });
});
