import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { makeTestHost } from "../../test-kit/sql/node.ts";
import type { Accounts, Applied, Look, PendingCheckout, Subscription } from "../../src/accounts/index.ts";
import { SESSION_LENGTH_MS } from "../../src/accounts/index.ts";
import type { ProviderEvent, ProviderPort } from "../../src/billing/port.ts";
import {
  ALARM_CALL_LIMIT,
  ALARM_RETRY_LIMIT,
  ALARM_RETRY_MS,
  CLOCK_RUN_LIMIT,
  type Entries,
  type Host,
  makeEntries,
  START_RESTARTS,
} from "../../src/do/entries.ts";
import type { Ledger } from "../../src/ledger/index.ts";
import type { Metering, Next, UsageCall } from "../../src/metering/index.ts";
import type { Cap, Ms } from "../../src/types.ts";

const ACCOUNT = "0f8e2d4c-6b1a-4c3e-8d2f-1a2b3c4d5e6f";
const HASH = "a".repeat(64);
const OTHER = "b".repeat(64);
const CAP: Cap = { amount: 5, period: "week" };
const NEW_CAP: Cap = { amount: 9, period: "month" };
const PENDING: PendingCheckout = { sessionId: "cs_one", url: "http://127.0.0.1:1/pay", expiresAt: 10_000, cap: CAP };
const METERED: Subscription = { accountId: ACCOUNT, state: "active", customerId: "cus_one", subscriptionId: "sub_one" };
const UNMETERED: Subscription = { ...METERED, customerId: null, subscriptionId: null };
const SEND: UsageCall = { kind: "send", interval: 0, identifier: `${ACCOUNT}:0`, amountMinor: 3, timestamp: 3599 };

type Call = [string, ...unknown[]];

interface Answers {
  look: Look;
  pending: PendingCheckout | null;
  subscription: Subscription | null;
  isActive: boolean;
  applyEvent: Applied;
  complete: Extract<Applied, { kind: "account-made" | "none" }>;
  accountsNextDue: Ms | null;
  wake: { sessionId: string } | null;
  record: "expired-unpaid" | "dropped" | "unanswered" | "kept" | "stale";
  cap: { cap: Cap; spent: number; held: number; periodEnd: Ms } | null;
  ledgerNextDue: () => Ms | null;
  openHolds: number;
  intervalOpen: number;
  oldestUnsealed: () => { interval: Ms; end: Ms } | null;
  send: Next;
  record5: Next;
  nextSend: Ms | null;
  settled: boolean;
  throwIn: string | null;
  onForget: () => void;
}

function defaults(): Answers {
  return {
    look: "account",
    pending: null,
    subscription: METERED,
    isActive: true,
    applyEvent: { kind: "none", overrode: false },
    complete: { kind: "none", overrode: false },
    accountsNextDue: null,
    wake: null,
    record: "kept",
    cap: { cap: CAP, spent: 1, held: 2, periodEnd: 77 },
    ledgerNextDue: () => null,
    openHolds: 0,
    intervalOpen: 0,
    oldestUnsealed: () => null,
    send: { call: null, notBillable: 0 },
    record5: { call: null, notBillable: 0 },
    nextSend: null,
    settled: true,
    throwIn: null,
    onForget: () => {},
  };
}

function scripted() {
  const calls: Call[] = [];
  const answers = defaults();
  const note = (name: string, args: unknown[]) => {
    calls.push([name, ...args.slice(1)]);
    if (answers.throwIn === name) {
      const error = new Error("scripted");
      error.name = "scripted-error";
      throw error;
    }
  };
  const answering =
    <T>(name: string, value: () => T) =>
    (...a: unknown[]): T => {
      note(name, a);
      return value();
    };
  const accounts: Accounts = {
    init: (...a) => note("accounts.init", a),
    look: answering("accounts.look", () => answers.look),
    pending: answering("accounts.pending", () => answers.pending),
    subscription: answering("accounts.subscription", () => answers.subscription),
    isActive: answering("accounts.isActive", () => answers.isActive),
    keepPending: (...a) => note("accounts.keepPending", a),
    replacePendingCap: (...a) => note("accounts.replacePendingCap", a),
    applyEvent: answering("accounts.applyEvent", () => answers.applyEvent),
    complete: answering("accounts.complete", () => answers.complete),
    seed: (...a) => note("accounts.seed", a),
    nextDue: answering("accounts.nextDue", () => answers.accountsNextDue),
    wake: answering("accounts.wake", () => answers.wake),
    record: answering("accounts.record", () => answers.record),
    forget: answering("accounts.forget", () => {
      answers.onForget();
      return 0;
    }),
  };
  const ledger: Ledger = {
    init: (...a) => note("ledger.init", a),
    setCap: (...a) => note("ledger.setCap", a),
    cap: answering("ledger.cap", () => answers.cap),
    reserve: answering("ledger.reserve", () => ({ outcome: "refused", reason: "no-cap" }) as const),
    dispatch: answering("ledger.dispatch", () => "go" as const),
    settle: answering("ledger.settle", () => ({ outcome: "voided" }) as const),
    void: answering("ledger.void", () => ({ outcome: "voided" }) as const),
    expire: answering("ledger.expire", () => 0),
    intervalTotals: answering("ledger.intervalTotals", () => ({ reported: 4, unreported: 0, open: answers.intervalOpen })),
    openHolds: answering("ledger.openHolds", () => answers.openHolds),
    nextDue: answering("ledger.nextDue", () => answers.ledgerNextDue()),
  };
  const metering: Metering = {
    init: (...a) => note("metering.init", a),
    begin: (...a) => note("metering.begin", a),
    billingPeriod: (...a) => note("metering.billingPeriod", a),
    intervalAt: answering("metering.intervalAt", () => 3_600_000),
    cut: (...a) => note("metering.cut", a),
    oldestUnsealed: answering("metering.oldestUnsealed", () => answers.oldestUnsealed()),
    seal: (...a) => note("metering.seal", a),
    send: answering("metering.send", () => answers.send),
    record: answering("metering.record", () => answers.record5),
    nextSend: answering("metering.nextSend", () => answers.nextSend),
    settled: answering("metering.settled", () => answers.settled),
  };
  return { calls, answers, modules: { accounts, ledger, metering } };
}

function makePort(calls: Call[], overrides: Partial<ProviderPort> = {}): ProviderPort {
  const port: ProviderPort = {
    async startCheckout(a) {
      calls.push(["startCheckout", a]);
      return { kind: "session", sessionId: "cs_new", url: "http://127.0.0.1:1/new", expiresAt: a.expiresAt };
    },
    verifyWebhook: async () => ({ kind: "rejected" }),
    async readSession(id) {
      calls.push(["readSession", id]);
      return { kind: "open" };
    },
    async openManagePage(a) {
      calls.push(["openManagePage", a]);
      return { kind: "page", url: "http://127.0.0.1:1/manage" };
    },
    async cancelSubscription(id) {
      calls.push(["cancelSubscription", id]);
      return { kind: "cancelled" };
    },
    async sendUsage(e) {
      calls.push(["sendUsage", e]);
      return { kind: "failed" };
    },
    async readUsageTotal(a) {
      calls.push(["readUsageTotal", a]);
      return { kind: "failed" };
    },
  };
  return { ...port, ...overrides };
}

let lines: Record<string, unknown>[] = [];
beforeEach(() => {
  lines = [];
  vi.spyOn(console, "log").mockImplementation((text: unknown) => {
    lines.push(JSON.parse(String(text)));
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

function world(o: { clock?: () => Ms; config?: Partial<Host["config"]>; port?: Partial<ProviderPort> } = {}) {
  const { calls, answers, modules } = scripted();
  const portCalls: Call[] = [];
  const test = makeTestHost({ port: makePort(portCalls, o.port), config: o.config, clock: o.clock ?? (() => 1000) });
  const alarms: (Ms | null)[] = [];
  const setAlarm = test.host.setAlarm.bind(test.host);
  test.host.setAlarm = (at) => {
    alarms.push(at);
    setAlarm(at);
  };
  const entries: Entries = makeEntries(test.host, modules);
  const names = () => calls.map((call) => call[0]);
  const reset = () => {
    calls.length = 0;
    portCalls.length = 0;
    alarms.length = 0;
    lines.length = 0;
  };
  const holds = async () => (await entries.testInspect()).result === "snapshot";
  const seeded = async (seed: { time?: Ms; customerId?: string } = {}) => {
    expect((await entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", ...seed })).result).toBe("seeded");
    reset();
  };
  return { calls, answers, entries, host: test.host, alarms, portCalls, names, reset, holds, seeded, alarmAt: test.alarmAt };
}

const events = () => lines.map((line) => line.event);

describe("the guard G1 and G2", () => {
  test("an object that holds nothing answers no-such-account, stores nothing and sets no alarm", async () => {
    const w = world();
    expect(await w.entries.status(HASH)).toEqual({ result: "no-such-account" });
    expect(await w.entries.capGet(HASH)).toEqual({ result: "no-such-account" });
    expect(await w.entries.capSet(HASH, CAP)).toEqual({ result: "no-such-account" });
    expect(await w.entries.manage(HASH)).toEqual({ result: "no-such-account" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "no-such-account" });
    expect(await w.entries.reserve(HASH, { requestId: "r", tier: "frontier", amount: 1, price: { input: 1, output: 1 } })).toEqual({
      outcome: "refused",
      reason: "no-such-account",
    });
    expect(w.names()).toEqual([]);
    expect(w.alarms).toEqual([]);
    expect(await w.holds()).toBe(false);
  });

  test("mismatch answers credential-mismatch and pending answers pending before any ledger call", async () => {
    const w = world();
    await w.seeded();
    w.answers.look = "mismatch";
    expect(await w.entries.status(OTHER)).toEqual({ result: "credential-mismatch" });
    expect(await w.entries.capGet(OTHER)).toEqual({ result: "credential-mismatch" });
    expect(await w.entries.destroy(OTHER)).toEqual({ result: "credential-mismatch" });
    w.answers.look = "pending";
    expect(await w.entries.status(HASH)).toEqual({ result: "pending" });
    expect(await w.entries.capGet(HASH)).toEqual({ result: "pending" });
    expect(await w.entries.capSet(HASH, CAP)).toEqual({ result: "pending" });
    expect(await w.entries.manage(HASH)).toEqual({ result: "pending" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "pending" });
    expect(await w.entries.reserve(HASH, { requestId: "r", tier: "frontier", amount: 1, price: { input: 1, output: 1 } })).toEqual({
      outcome: "refused",
      reason: "pending",
    });
    expect(w.names().filter((name) => name.startsWith("ledger.") && name !== "ledger.nextDue")).toEqual([]);
    expect(w.portCalls).toEqual([]);
  });
});

describe("start", () => {
  test("step 4 and 5 on an object that holds nothing: one session, the tables and the pending checkout", async () => {
    const w = world({ clock: () => 5000 });
    expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/new" });
    expect(w.portCalls).toEqual([
      ["startCheckout", { accountId: ACCOUNT, successUrl: "http://halvo.test/v1/done", expiresAt: 5000 + SESSION_LENGTH_MS }],
    ]);
    expect(w.names().slice(0, 4)).toEqual(["accounts.init", "ledger.init", "metering.init", "accounts.keepPending"]);
    expect(w.calls[3]).toEqual([
      "accounts.keepPending",
      { accountId: ACCOUNT, credentialHash: HASH, cap: CAP, sessionId: "cs_new", url: "http://127.0.0.1:1/new", expiresAt: 5000 + SESSION_LENGTH_MS },
      5000,
    ]);
    expect(w.alarms).toHaveLength(1);
    expect(await w.holds()).toBe(true);
  });

  test("step 4: an empty PUBLIC_ORIGIN and a failed session answer unavailable and store nothing", async () => {
    const empty = world({ config: { publicOrigin: "" } });
    expect(await empty.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "unavailable" });
    expect(empty.portCalls).toEqual([]);
    const failed = world({ port: { startCheckout: async () => ({ kind: "failed" }) } });
    expect(await failed.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "unavailable" });
    expect(await failed.holds()).toBe(false);
  });

  test("step 1: mismatch and account", async () => {
    const w = world();
    await w.seeded();
    w.answers.look = "mismatch";
    expect(await w.entries.start(ACCOUNT, OTHER, CAP)).toEqual({ result: "credential-mismatch" });
    w.answers.look = "account";
    expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "account-exists" });
    expect(w.portCalls).toEqual([]);
  });

  test("step 2: a pending checkout before its expiry takes the new cap and answers its url", async () => {
    const w = world({ clock: () => 9_999 });
    await w.seeded();
    w.answers.look = "pending";
    w.answers.pending = PENDING;
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "session", url: PENDING.url });
    expect(w.calls).toContainEqual(["accounts.replacePendingCap", NEW_CAP]);
    expect(w.portCalls).toEqual([]);
  });

  test("step 3: a read that shows the session complete makes the account in the step after the read", async () => {
    const period = { start: 1, end: 2 };
    const session = { sessionId: "cs_one", customerId: "cus_one", subscriptionId: "sub_one", payment: "paid" as const };
    const w = world({ clock: () => 10_000, port: { readSession: async () => ({ kind: "complete", session }) } });
    await w.seeded();
    w.answers.look = "pending";
    w.answers.pending = PENDING;
    w.answers.complete = { kind: "account-made", cap: CAP, period };
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "account-exists" });
    const made = w.calls.slice(w.names().indexOf("accounts.complete"));
    expect(made.slice(0, 4)).toEqual([
      ["accounts.complete", session],
      ["ledger.setCap", CAP, 10_000],
      ["metering.begin", ACCOUNT, 10_000],
      ["metering.billingPeriod", period, 10_000],
    ]);
  });

  test("step 3: each read is handed to record as the port answered it; kept answers the stored url", async () => {
    for (const kind of ["open", "expired", "unknown", "failed"] as const) {
      const w = world({ clock: () => 10_000, port: { readSession: async () => ({ kind }) } });
      await w.seeded();
      w.answers.look = "pending";
      w.answers.pending = PENDING;
      w.answers.record = "kept";
      expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "session", url: PENDING.url });
      expect(w.calls).toContainEqual(["accounts.record", "cs_one", kind, 10_000]);
      expect(w.calls).toContainEqual(["accounts.replacePendingCap", NEW_CAP]);
    }
  });

  test("step 3: expired-unpaid, dropped and unanswered delete all storage, log the bound, and start again at 4", async () => {
    for (const [record, line] of [
      ["expired-unpaid", undefined],
      ["dropped", "pending-dropped"],
      ["unanswered", "pending-unanswered"],
    ] as const) {
      const w = world({ clock: () => 10_000 });
      await w.seeded();
      w.answers.look = "pending";
      w.answers.pending = PENDING;
      w.answers.record = record;
      expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/new" });
      expect(events()).toEqual(line === undefined ? [] : [line]);
      expect(w.portCalls.map((call) => call[0])).toEqual(["readSession", "startCheckout"]);
      expect(w.calls.filter((call) => call[0] === "accounts.keepPending")).toHaveLength(1);
      expect(w.names()).toContain("accounts.init");
    }
  });

  test("the restarts on stale end at START_RESTARTS with unavailable, and the call stores nothing", async () => {
    const w = world({ clock: () => 10_000 });
    await w.seeded();
    w.answers.look = "pending";
    w.answers.pending = PENDING;
    w.answers.record = "stale";
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "unavailable" });
    expect(w.portCalls.filter((call) => call[0] === "readSession")).toHaveLength(START_RESTARTS + 1);
    expect(w.names()).not.toContain("accounts.replacePendingCap");
    expect(w.names()).not.toContain("accounts.keepPending");
  });

  test("step 3: a complete read for which complete answers none goes on as on stale, up to unavailable", async () => {
    const session = { sessionId: "cs_one", customerId: "cus_one", subscriptionId: null, payment: "paid" as const };
    const w = world({ clock: () => 10_000, port: { readSession: async () => ({ kind: "complete", session }) } });
    await w.seeded();
    w.answers.look = "pending";
    w.answers.pending = PENDING;
    w.answers.complete = { kind: "none", overrode: false };
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "unavailable" });
    expect(w.calls.filter((call) => call[0] === "accounts.complete")).toHaveLength(START_RESTARTS + 1);
    expect(w.names()).not.toContain("accounts.record");
  });

  test("step 4 takes the expiry from the now of the entry's last step, the test time when one is stored", async () => {
    const w = world({ clock: () => 1000 });
    await w.seeded({ time: 50_000 });
    w.answers.look = "pending";
    w.answers.pending = PENDING;
    w.answers.record = "dropped";
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/new" });
    expect(w.portCalls.at(-1)).toEqual([
      "startCheckout",
      { accountId: ACCOUNT, successUrl: "http://halvo.test/v1/done", expiresAt: 50_000 + SESSION_LENGTH_MS },
    ]);
  });

  test("step 3: a read after which the object holds nothing goes on as on stale", async () => {
    let w: ReturnType<typeof world> | null = null;
    const reads: string[] = [];
    w = world({
      clock: () => 10_000,
      port: {
        readSession: async (id) => {
          reads.push(id);
          await w?.host.deleteAll();
          return { kind: "open" };
        },
      },
    });
    await w.seeded();
    w.answers.look = "pending";
    w.answers.pending = PENDING;
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/new" });
    expect(reads).toEqual(["cs_one"]);
    expect(w.names()).not.toContain("accounts.record");
  });

  test("step 5: another start stored a pending checkout meanwhile: the cap is replaced and the stored url answered", async () => {
    const w = world({ clock: () => 10_000 });
    await w.seeded();
    w.answers.look = "nothing";
    w.answers.pending = PENDING;
    w.host.port.startCheckout = async () => {
      w.answers.look = "pending";
      return { kind: "session", sessionId: "cs_second", url: "http://127.0.0.1:1/second", expiresAt: 1 };
    };
    expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result: "session", url: PENDING.url });
    expect(w.calls).toContainEqual(["accounts.replacePendingCap", NEW_CAP]);
    expect(w.names()).not.toContain("accounts.keepPending");
    for (const [look, result] of [
      ["mismatch", "credential-mismatch"],
      ["account", "account-exists"],
    ] as const) {
      w.answers.look = "nothing";
      w.host.port.startCheckout = async () => {
        w.answers.look = look;
        return { kind: "session", sessionId: "cs_third", url: "http://127.0.0.1:1/third", expiresAt: 1 };
      };
      expect(await w.entries.start(ACCOUNT, HASH, NEW_CAP)).toEqual({ result });
    }
  });
});

describe("webhookEvent", () => {
  const completed: ProviderEvent = {
    kind: "checkout-completed",
    eventId: "evt_1",
    providerTime: 4000,
    accountId: ACCOUNT,
    sessionId: "cs_one",
    customerId: "cus_one",
    subscriptionId: "sub_one",
    payment: "paid",
  };
  const cancelled: ProviderEvent = {
    kind: "subscription",
    eventId: "evt_2",
    providerTime: 6000,
    accountId: ACCOUNT,
    subscriptionId: "sub_one",
    customerId: "cus_one",
    state: "cancelled",
    period: { start: 1, end: 2 },
    endedAt: null,
  };

  test("ignored makes no step, and a completed event for an object that holds nothing logs completed-no-record", async () => {
    const w = world();
    await w.entries.webhookEvent({ kind: "ignored" });
    await w.entries.webhookEvent(cancelled);
    expect(events()).toEqual([]);
    await w.entries.webhookEvent(completed);
    expect(events()).toEqual(["completed-no-record"]);
    expect(lines[0]).toEqual({ event: "completed-no-record" });
    expect(w.names()).toEqual([]);
  });

  test("account-made: setCap, begin and billingPeriod in the one step", async () => {
    const w = world({ clock: () => 2000 });
    await w.seeded();
    const period = { start: 10, end: 20 };
    w.answers.applyEvent = { kind: "account-made", cap: CAP, period };
    await w.entries.webhookEvent(completed);
    const at = w.names().indexOf("accounts.applyEvent");
    expect(w.calls.slice(at, at + 4)).toEqual([
      ["accounts.applyEvent", completed, 2000],
      ["ledger.setCap", CAP, 2000],
      ["metering.begin", ACCOUNT, 2000],
      ["metering.billingPeriod", period, 2000],
    ]);
  });

  test("state: the period and the cut at a cancel for a metered account, and nothing more for one that is not", async () => {
    const w = world({ clock: () => 7000 });
    await w.seeded();
    w.answers.applyEvent = { kind: "state", state: "cancelled", was: "active", period: { start: 1, end: 2 }, overrode: false };
    await w.entries.webhookEvent(cancelled);
    expect(w.calls).toContainEqual(["metering.billingPeriod", { start: 1, end: 2 }, 7000]);
    expect(w.calls).toContainEqual(["metering.cut", "cancel", 6000, 7000]);
    w.reset();
    w.answers.applyEvent = { kind: "state", state: "cancelled", was: "cancelled", period: null, overrode: false };
    await w.entries.webhookEvent(cancelled);
    expect(w.names()).not.toContain("metering.cut");
    w.reset();
    w.answers.subscription = UNMETERED;
    w.answers.applyEvent = { kind: "state", state: "cancelled", was: "active", period: { start: 1, end: 2 }, overrode: false };
    await w.entries.webhookEvent(cancelled);
    expect(w.names().filter((name) => name.startsWith("metering.") && name !== "metering.nextSend" && name !== "metering.oldestUnsealed")).toEqual(
      [],
    );
  });

  test("none for a completed event logs completed-no-record when a pending checkout of another session is stored", async () => {
    const w = world();
    await w.seeded();
    w.answers.applyEvent = { kind: "none", overrode: false };
    w.answers.pending = { ...PENDING, sessionId: "cs_other" };
    await w.entries.webhookEvent(completed);
    expect(events()).toEqual(["completed-no-record"]);
    w.reset();
    w.answers.pending = PENDING;
    await w.entries.webhookEvent(completed);
    expect(events()).toEqual([]);
  });
});

describe("status, capGet, capSet and manage", () => {
  test("their answers on an account", async () => {
    const w = world({ clock: () => 3000 });
    await w.seeded();
    expect(await w.entries.status(HASH)).toEqual({ result: "active" });
    expect(await w.entries.capGet(HASH)).toEqual({ result: "cap", cap: CAP, spend: { spent: 1, held: 2, period_end: 77 } });
    expect(await w.entries.capSet(HASH, NEW_CAP)).toEqual({ result: "cap", cap: CAP, spend: { spent: 1, held: 2, period_end: 77 } });
    expect(w.calls).toContainEqual(["ledger.setCap", NEW_CAP, 3000]);
    w.answers.cap = null;
    expect(await w.entries.capGet(HASH)).toEqual({ result: "no-cap" });
    expect(await w.entries.manage(HASH)).toEqual({ result: "page", url: "http://127.0.0.1:1/manage" });
    expect(w.portCalls).toEqual([["openManagePage", { customerId: "cus_one", returnUrl: "http://halvo.test/v1/done" }]]);
  });

  test("manage answers unavailable with no customer id, with an empty PUBLIC_ORIGIN and on a failed page", async () => {
    const w = world();
    await w.seeded();
    w.answers.subscription = UNMETERED;
    expect(await w.entries.manage(HASH)).toEqual({ result: "unavailable" });
    expect(w.portCalls).toEqual([]);
    const empty = world({ config: { publicOrigin: "" } });
    await empty.seeded();
    expect(await empty.entries.manage(HASH)).toEqual({ result: "unavailable" });
    expect(empty.portCalls).toEqual([]);
    const failed = world({ port: { openManagePage: async () => ({ kind: "failed" }) } });
    await failed.seeded();
    expect(await failed.entries.manage(HASH)).toEqual({ result: "unavailable" });
  });
});

describe("destroy", () => {
  test("a metered account: the mark and the cut, the seal and send part, the cancel and the delete", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(w.calls).toContainEqual(["metering.cut", "delete", 8000, 8000]);
    expect(w.names()).toContain("metering.send");
    expect(w.names()).toContain("metering.settled");
    expect(w.portCalls).toEqual([["cancelSubscription", "sub_one"]]);
    expect(await w.holds()).toBe(false);
  });

  test("open holds, an unsettled sync and a failed cancel answer not-finished, and a repeat cuts once", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.answers.openHolds = 0;
    w.answers.settled = false;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(w.portCalls).toEqual([]);
    w.answers.settled = true;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(w.calls.filter((call) => call[0] === "metering.cut")).toHaveLength(1);
    expect(await w.entries.reserve(HASH, { requestId: "r", tier: "frontier", amount: 1, price: { input: 1, output: 1 } })).toEqual({
      outcome: "refused",
      reason: "closing",
    });
    expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "account-exists" });
    w.host.port.cancelSubscription = async () => ({ kind: "nothing-to-cancel" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
  });

  test("an account that is not metered: no cut, step 3 is one step of expire, and settled is not asked", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded();
    w.answers.subscription = UNMETERED;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(w.names().filter((name) => ["metering.cut", "metering.settled", "metering.send", "metering.oldestUnsealed"].includes(name))).toEqual([]);
    expect(w.names()).toContain("ledger.expire");
    expect(w.portCalls).toEqual([["cancelSubscription", null]]);
  });

  test("step 6: an account deleted and made again during the cancel has no mark, so the call answers not-finished", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.host.port.cancelSubscription = async () => {
      w.host.step((sql) => sql.exec("UPDATE skeleton SET destroying_since = ?", null));
      return { kind: "cancelled" };
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(await w.holds()).toBe(true);
  });

  test("step 6: a cancel call that spans a delete and a remake of the id answers not-finished, and the open hold and the new mark stay stored", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    let release: (answer: { kind: "cancelled" }) => void = () => {};
    const waiting = new Promise<{ kind: "cancelled" }>((resolve) => {
      release = resolve;
    });
    let cancels = 0;
    w.host.port.cancelSubscription = async () => {
      cancels += 1;
      return cancels === 1 ? waiting : { kind: "cancelled" };
    };
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(cancels).toBe(1);
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(await w.holds()).toBe(false);
    expect((await w.entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", customerId: "cus_one", time: 9000 })).result).toBe("seeded");
    w.answers.openHolds = 1;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    release({ kind: "cancelled" });
    expect(await first).toEqual({ result: "not-finished" });
    const snapshot = await w.entries.testInspect();
    expect(snapshot.result === "snapshot" && snapshot.tables.skeleton?.[0]?.destroying_since).toBe(9000);
    expect(cancels).toBe(2);
  });

  test("step 6: the guard after the cancel answers not-finished when the id holds no account", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.host.port.cancelSubscription = async () => {
      w.answers.look = "pending";
      return { kind: "cancelled" };
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(await w.holds()).toBe(true);
  });
});

describe("reserve, dispatch, settle, void and rearm", () => {
  const req = { requestId: "r1", tier: "low_cost_hosted" as const, amount: 10, price: { input: 3, output: 4 } };

  test("reserve passes the interval and the active fact; an account that is not metered passes interval 0", async () => {
    const w = world({ clock: () => 4000 });
    await w.seeded();
    await w.entries.reserve(HASH, req);
    expect(w.calls).toContainEqual(["ledger.reserve", req, 4000, { active: true, interval: 3_600_000 }]);
    w.answers.subscription = UNMETERED;
    w.answers.isActive = false;
    w.reset();
    await w.entries.reserve(HASH, req);
    expect(w.calls).toContainEqual(["ledger.reserve", req, 4000, { active: false, interval: 0 }]);
    expect(w.names()).not.toContain("metering.intervalAt");
  });

  test("dispatch, settle and void need no credential and answer for an object that holds nothing", async () => {
    const empty = world();
    expect(await empty.entries.dispatch("r1")).toBe("stop");
    expect(await empty.entries.settle("r1", { inputTokens: 1, outputTokens: 1 })).toEqual({ outcome: "refused", reason: "unknown-request" });
    expect(await empty.entries.void("r1")).toEqual({ outcome: "refused", reason: "unknown-request" });
    const w = world({ clock: () => 4000 });
    await w.seeded();
    w.answers.look = "mismatch";
    expect(await w.entries.dispatch("r1")).toBe("go");
    expect(await w.entries.settle("r1", { inputTokens: 1, outputTokens: 2 })).toEqual({ outcome: "voided" });
    expect(await w.entries.void("r1")).toEqual({ outcome: "voided" });
    expect(w.calls).toContainEqual(["ledger.settle", "r1", { inputTokens: 1, outputTokens: 2 }, 4000]);
  });

  test("rearm sets the alarm from storage, and on an object that holds nothing sets none", async () => {
    const empty = world();
    await empty.entries.rearm();
    expect(empty.alarms).toEqual([]);
    const w = world();
    await w.seeded();
    w.answers.ledgerNextDue = () => 5000;
    w.answers.accountsNextDue = 4500;
    await w.entries.rearm();
    expect(w.alarms).toEqual([4500]);
  });
});

describe("alarm", () => {
  test("an object that holds nothing: no step and no alarm", async () => {
    const w = world();
    await w.entries.alarm();
    expect(w.names()).toEqual([]);
    expect(w.alarms).toEqual([]);
  });

  test("step 1 ends due holds first, then the seal loop, forget and wake", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded({ customerId: "cus_one" });
    await w.entries.alarm();
    const first = w.names().slice(0, w.names().indexOf("accounts.wake") + 1);
    expect(first.filter((name) => !name.startsWith("accounts.subscription"))).toEqual([
      "ledger.expire",
      "metering.oldestUnsealed",
      "accounts.forget",
      "accounts.wake",
    ]);
  });

  test("the seal loop stops at an interval that seal did not seal: one call of seal in the run", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.oldestUnsealed = () => ({ interval: 0, end: 3600 });
    await w.entries.alarm();
    expect(w.calls.filter((call) => call[0] === "metering.seal")).toEqual([["metering.seal", 0, 4, 9000]]);
  });

  test("the chain of provider calls stops at ALARM_CALL_LIMIT", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.send = { call: SEND, notBillable: 1 };
    w.answers.record5 = { call: SEND, notBillable: 0 };
    await w.entries.alarm();
    expect(w.portCalls.filter((call) => call[0] === "sendUsage")).toHaveLength(ALARM_CALL_LIMIT);
    expect(w.portCalls[0]).toEqual([
      "sendUsage",
      { accountId: ACCOUNT, customerId: "cus_one", identifier: SEND.identifier, amountMinor: 3, timestamp: 3599 },
    ]);
    expect(events()).toEqual(["usage-not-billable"]);
  });

  test("after a provider call, an object that is no longer metered gets no call of metering.record", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.send = { call: SEND, notBillable: 0 };
    w.host.port.sendUsage = async (event) => {
      w.portCalls.push(["sendUsage", event]);
      w.answers.subscription = null;
      return { kind: "failed" };
    };
    await w.entries.alarm();
    expect(w.portCalls.map((call) => call[0])).toEqual(["sendUsage"]);
    expect(w.names()).toContain("metering.send");
    expect(w.names()).not.toContain("metering.record");
    expect(events()).toEqual([]);
  });

  test("steps 3 and 4: the read goes to record, and a removal deletes all storage with its line", async () => {
    for (const [record, line] of [
      ["expired-unpaid", []],
      ["dropped", ["pending-dropped"]],
      ["unanswered", ["pending-unanswered"]],
    ] as const) {
      const w = world({ clock: () => 9000, port: { readSession: async () => ({ kind: "failed" }) } });
      await w.seeded();
      w.answers.wake = { sessionId: "cs_one" };
      w.answers.record = record;
      await w.entries.alarm();
      expect(w.calls).toContainEqual(["accounts.record", "cs_one", "failed", 9000]);
      expect(events()).toEqual(line);
      expect(await w.holds()).toBe(false);
    }
  });

  test("step 4: a read that shows the session complete makes the account, and step 5 then sends", async () => {
    const session = { sessionId: "cs_one", customerId: "cus_one", subscriptionId: null, payment: "paid" as const };
    const w = world({ clock: () => 9000, port: { readSession: async () => ({ kind: "complete", session }) } });
    await w.seeded();
    w.answers.subscription = null;
    w.answers.wake = { sessionId: "cs_one" };
    w.host.port.readSession = async () => {
      w.answers.subscription = METERED;
      return { kind: "complete", session };
    };
    w.answers.complete = { kind: "account-made", cap: CAP, period: null };
    await w.entries.alarm();
    expect(w.calls).toContainEqual(["ledger.setCap", CAP, 9000]);
    expect(w.calls).toContainEqual(["metering.begin", ACCOUNT, 9000]);
    expect(w.names()).toContain("metering.send");
  });

  test("the retry after a caught error: 4 alarms with a time, then none and alarm-stopped; the next run writes the limit again", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded();
    w.answers.throwIn = "ledger.expire";
    for (let run = 0; run < ALARM_RETRY_LIMIT; run += 1) await w.entries.alarm();
    expect(w.alarms).toEqual([9000 + ALARM_RETRY_MS, 9000 + ALARM_RETRY_MS, 9000 + ALARM_RETRY_MS, 9000 + ALARM_RETRY_MS, null]);
    expect(lines.filter((line) => line.event === "alarm-error")).toHaveLength(ALARM_RETRY_LIMIT);
    expect(lines.filter((line) => line.event === "alarm-stopped")).toEqual([{ event: "alarm-stopped", error: "Error" }]);
    w.reset();
    await w.entries.alarm();
    expect(w.alarms).toEqual([null]);
    expect(events()).toEqual(["alarm-error", "alarm-stopped"]);
    const snapshot = await w.entries.testInspect();
    expect(snapshot.result === "snapshot" && snapshot.tables.skeleton?.[0]?.alarm_failures).toBe(ALARM_RETRY_LIMIT);
    w.answers.throwIn = null;
    await w.entries.alarm();
    const after = await w.entries.testInspect();
    expect(after.result === "snapshot" && after.tables.skeleton?.[0]?.alarm_failures).toBe(0);
  });

  test("ruling 8: when testSetClock stores a test time during the alarm's port call and the next step throws, no call of setAlarm with a time follows", async () => {
    let release: (answer: { kind: "open" }) => void = () => {};
    const w = world({ clock: () => 9000 });
    await w.seeded();
    w.answers.wake = { sessionId: "cs_one" };
    w.host.port.readSession = () =>
      new Promise((resolve) => {
        release = resolve;
      });
    const run = w.entries.alarm();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(await w.entries.testSetClock(12_000)).toEqual({ result: "clock-set" });
    w.alarms.length = 0;
    w.answers.throwIn = "ledger.expire";
    release({ kind: "open" });
    await run;
    expect(events()).toEqual(["alarm-error"]);
    expect(w.alarms.filter((at) => at !== null)).toEqual([]);
    const stored = world({ clock: () => 9000 });
    await stored.seeded({ time: 5000 });
    stored.answers.throwIn = "ledger.expire";
    await stored.entries.alarm();
    expect(stored.alarms.filter((at) => at !== null)).toEqual([]);
    expect(stored.alarmAt()).toBeNull();
  });

  test("under a test time: 5 calls of testSetClock count the failures and the fifth logs alarm-stopped, with no alarm", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded({ time: 1000 });
    w.answers.ledgerNextDue = () => 1500;
    w.answers.throwIn = "ledger.expire";
    for (let call = 1; call <= ALARM_RETRY_LIMIT; call += 1) {
      expect(await w.entries.testSetClock(2000 + call)).toEqual({ result: "clock-set" });
      expect(events().includes("alarm-stopped")).toBe(call === ALARM_RETRY_LIMIT);
    }
    expect(w.alarms.every((at) => at === null)).toBe(true);
  });
});

describe("the test methods", () => {
  test("refused when TEST_CLOCK is off, before anything else", async () => {
    const w = world({ config: { testClock: false } });
    expect(await w.entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active" })).toEqual({ result: "refused" });
    expect(await w.entries.testSetClock(5)).toEqual({ result: "refused" });
    expect(await w.entries.testInspect()).toEqual({ result: "refused" });
    expect(w.names()).toEqual([]);
  });

  test("the seed: not-empty on an object that holds anything, the test time, the metering row and the cap", async () => {
    const w = world({ clock: () => 9000 });
    expect(
      await w.entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "payment-failed", customerId: "cus_x", cap: CAP, anchor: 77, time: 500 }),
    ).toEqual({ result: "seeded" });
    expect(w.calls).toContainEqual(["accounts.seed", { accountId: ACCOUNT, credentialHash: HASH, state: "payment-failed", customerId: "cus_x" }, 500]);
    expect(w.calls).toContainEqual(["metering.begin", ACCOUNT, 500]);
    expect(w.calls).toContainEqual(["ledger.setCap", CAP, 500, 77]);
    expect(await w.entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active" })).toEqual({ result: "not-empty" });
    const snapshot = await w.entries.testInspect();
    expect(snapshot).toMatchObject({ result: "snapshot", now: 9000, test_time: 500, alarm: null });
    expect(snapshot.result === "snapshot" && snapshot.tables).toEqual({
      skeleton: [{ destroying_since: null, last_now: 500, alarm_failures: 0 }],
      test_clock: [{ time: 500 }],
    });
  });

  test("the clock: no-such-account, behind, and clock-set with the runs of the alarm's body", async () => {
    const w = world({ clock: () => 9000 });
    expect(await w.entries.testSetClock(100)).toEqual({ result: "no-such-account" });
    await w.seeded({ time: 1000 });
    expect(await w.entries.testSetClock(999)).toEqual({ result: "behind" });
    expect(w.alarms).toEqual([]);
    w.answers.accountsNextDue = 1500;
    w.answers.wake = null;
    expect(await w.entries.testSetClock(2000)).toEqual({ result: "clock-set" });
    expect(w.calls.filter((call) => call[0] === "accounts.forget")).toEqual([["accounts.forget", 2000]]);
    expect(w.alarms.every((at) => at === null)).toBe(true);
  });

  test("one clock call ends after CLOCK_RUN_LIMIT runs, and a second call with the same time goes on", async () => {
    const w = world({ clock: () => 9000 });
    await w.seeded({ time: 1000 });
    let due = 1000;
    w.answers.ledgerNextDue = () => due;
    w.answers.onForget = () => {
      due += 1;
    };
    const runs = () => w.calls.filter((call) => call[0] === "accounts.forget").length;
    expect(await w.entries.testSetClock(1_000_000)).toEqual({ result: "clock-set" });
    expect(runs()).toBe(CLOCK_RUN_LIMIT);
    expect(await w.entries.testSetClock(1_000_000)).toEqual({ result: "clock-set" });
    expect(runs()).toBe(2 * CLOCK_RUN_LIMIT);
  });

  test("every step takes the later of time() and last_now, and the time never goes back", async () => {
    let clock = 5000;
    const w = world({ clock: () => clock });
    await w.seeded();
    clock = 4000;
    await w.entries.capSet(HASH, CAP);
    expect(w.calls).toContainEqual(["ledger.setCap", CAP, 5000]);
    clock = 6000;
    await w.entries.capSet(HASH, NEW_CAP);
    expect(w.calls).toContainEqual(["ledger.setCap", NEW_CAP, 6000]);
  });
});
