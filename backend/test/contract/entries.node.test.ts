import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { makeTestHost } from "../../test-kit/sql/node.ts";
import type { Accounts, Applied, Look, PendingCheckout, Subscription } from "../../src/accounts/index.ts";
import { SESSION_LENGTH_MS } from "../../src/accounts/index.ts";
import type { ProviderEvent, ProviderPort } from "../../src/billing/port.ts";
import {
  ALARM_CALL_LIMIT,
  ALARM_RETRY_LIMIT,
  ALARM_PAUSE_CAP_MS,
  ALARM_RETRY_MS,
  CALL_IN_FLIGHT_MS,
  CANCEL_CALL_BOUND_MS,
  CLOCK_RUN_LIMIT,
  DELETE_REMOVAL_BOUND_MS,
  type Entries,
  type Host,
  makeEntries,
  START_RESTARTS,
} from "../../src/do/entries.ts";
import type { Ledger } from "../../src/ledger/index.ts";
import type { Metering, Next, UsageCall } from "../../src/metering/index.ts";
import type { Cap, Ms, Sql } from "../../src/types.ts";
import { PROVIDER_CALL_TIMEOUT_MS } from "../../src/billing/stripe/timeout.ts";

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
  closeOpen: number;
  unacknowledged: number;
  sendInFlight: (now: Ms) => Ms | null;
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
    closeOpen: 0,
    unacknowledged: 0,
    sendInFlight: () => null,
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
    openHoldsAt: answering("ledger.openHoldsAt", () => answers.openHolds),
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
    closeOpen: answering("metering.closeOpen", () => answers.closeOpen),
    unacknowledged: answering("metering.unacknowledged", () => answers.unacknowledged),
    closedAtBound: answering("metering.closedAtBound", () => answers.closeOpen),
    sendsInFlight: (...a: unknown[]) => {
      note("metering.sendsInFlight", a);
      const ends = answers.sendInFlight(a[1] as Ms);
      return ends === null ? [] : [{ interval: 0, sentAt: ends - CALL_IN_FLIGHT_MS }];
    },
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
  return { calls, answers, entries, host: test.host, alarms, portCalls, names, reset, holds, seeded, alarmAt: test.alarmAt, modules, pauses: test.pauses, endPauses: test.endPauses };
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

  test("open holds and a failed cancel answer not-finished; an unsettled sync no longer holds the cancel back, its answer is stored, a repeat makes no second cancel call and cuts once, and the storage goes once the sync is settled (unit 5.6)", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(w.portCalls).toEqual([]);
    w.answers.openHolds = 0;
    const cancelled = w.host.port.cancelSubscription;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.host.port.cancelSubscription = cancelled;
    w.answers.settled = false;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(w.portCalls).toEqual([["cancelSubscription", "sub_one"]]);
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(w.portCalls).toHaveLength(1);
    expect(w.calls.filter((call) => call[0] === "metering.cut")).toHaveLength(1);
    expect(await w.entries.reserve(HASH, { requestId: "r", tier: "frontier", amount: 1, price: { input: 1, output: 1 } })).toEqual({
      outcome: "refused",
      reason: "closing",
    });
    expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "account-exists" });
    w.answers.settled = true;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(w.portCalls).toHaveLength(1);
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

  test("step 6: a cancel call that spans a delete and a remake of the id answers not-finished, and the open hold and the new mark stay stored (the call made for a null id, which no call in flight holds back: P7-110 L4)", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.subscription = { ...METERED, subscriptionId: null };
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
    expect(snapshot.result === "snapshot" && snapshot.tables.skeleton?.[0]?.cancel_answered_at).toBe(null);
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
      skeleton: [
        {
          destroying_since: null,
          last_now: 500,
          alarm_failures: 0,
          cancel_answered_at: null,
          cancel_answered_for: null,
          cancel_next_at: null,
          cancel_delay: null,
          removal_decided_at: null,
        },
      ],
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

describe("destroy and the alarm under the mark (unit 5.6, ruling P7-82)", () => {
  const skeletonOf = async (w: ReturnType<typeof world>) => {
    const snapshot = await w.entries.testInspect();
    return snapshot.result === "snapshot" ? { row: snapshot.tables.skeleton?.[0], due: snapshot.due } : null;
  };
  const cancels = (w: ReturnType<typeof world>) => w.portCalls.filter((call) => call[0] === "cancelSubscription").length;

  test("status answers closing under the mark while no answer is stored for the row's id, and cancelled once one is; without the mark it answers the account's state", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    expect(await w.entries.status(HASH)).toEqual({ result: "active" });
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    w.answers.settled = false;
    await w.entries.destroy(HASH);
    expect(await w.entries.status(HASH)).toEqual({ result: "closing" });
    w.host.port.cancelSubscription = async () => ({ kind: "cancelled" });
    await w.entries.destroy(HASH);
    expect(await w.entries.status(HASH)).toEqual({ result: "cancelled" });
    w.answers.subscription = { ...METERED, subscriptionId: "sub_two" };
    expect(await w.entries.status(HASH)).toEqual({ result: "closing" });
  });

  test("a failed cancel stores the next try at now plus the delay, which doubles from 60 s to 3,600 s; the alarm tries at that time and not before, and the due time is the next try while no answer is stored", async () => {
    let now = 8000;
    const w = world({ clock: () => now, port: { cancelSubscription: async () => ({ kind: "failed" }) } });
    await w.seeded({ customerId: "cus_one" });
    const tries: Ms[] = [];
    w.host.port.cancelSubscription = async () => {
      tries.push(now);
      return { kind: "failed" };
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(await skeletonOf(w)).toEqual({ row: expect.objectContaining({ cancel_next_at: 68_000, cancel_delay: 120_000, cancel_answered_at: null }), due: 68_000 });
    now = 67_999;
    await w.entries.alarm();
    expect(tries).toEqual([8000]);
    for (let each = 0; each < 8; each += 1) {
      const read = await skeletonOf(w);
      now = Number(read?.due);
      await w.entries.alarm();
    }
    const gaps = tries.slice(1).map((at, index) => (at - (tries[index] ?? 0)) / 1000);
    expect(gaps).toEqual([60, 120, 240, 480, 960, 1920, 3600, 3600]);
    expect(await skeletonOf(w)).toEqual({ row: expect.objectContaining({ cancel_delay: 3_600_000 }), due: now + 3_600_000 });
    expect(w.alarms.at(-1)).toBe(now + 3_600_000);
  });

  test("a call of destroy from outside makes the cancel call at once, before the stored next try", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    await w.entries.destroy(HASH);
    let made = 0;
    w.host.port.cancelSubscription = async () => {
      made += 1;
      return { kind: "cancelled" };
    };
    now = 9000;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(made).toBe(1);
  });

  test("with an answer stored the due time is the answered time plus DELETE_REMOVAL_BOUND_MS; at that time the alarm calls closeOpen, logs one usage-not-billable for each interval it closed, and deletes", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(await skeletonOf(w)).toEqual({
      row: expect.objectContaining({ cancel_answered_at: 8000, cancel_answered_for: "sub_one" }),
      due: 8000 + DELETE_REMOVAL_BOUND_MS,
    });
    w.reset();
    now = 8000 + DELETE_REMOVAL_BOUND_MS - 1;
    await w.entries.alarm();
    expect([w.names().includes("metering.closeOpen"), w.portCalls, await w.holds()]).toEqual([false, [], true]);
    w.answers.closeOpen = 2;
    now = 8000 + DELETE_REMOVAL_BOUND_MS;
    await w.entries.alarm();
    expect(w.calls).toContainEqual(["metering.closeOpen", 8000 + DELETE_REMOVAL_BOUND_MS]);
    expect(events()).toEqual(["usage-not-billable", "usage-not-billable"]);
    expect([w.portCalls, await w.holds()]).toEqual([[], false]);
  });

  test("the alarm deletes before the bound once the sync is settled and no hold is open", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    await w.entries.destroy(HASH);
    now = 9000;
    w.answers.openHolds = 1;
    w.answers.settled = true;
    await w.entries.alarm();
    expect(await w.holds()).toBe(true);
    w.answers.openHolds = 0;
    await w.entries.alarm();
    expect([await w.holds(), cancels(w)]).toEqual([false, 1]);
  });

  test("an account that is not metered: the alarm under the mark makes the cancel call once the hold has ended, and deletes", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded();
    w.answers.subscription = UNMETERED;
    w.answers.openHolds = 1;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(w.portCalls).toEqual([]);
    w.answers.openHolds = 0;
    await w.entries.alarm();
    expect([w.portCalls, await w.holds()]).toEqual([[["cancelSubscription", null]], false]);
  });

  test("a cancel call that throws stores nothing, and destroy answers not-finished with a destroy-error line (P7-110 L1 (f)); the due time is then the mark, and the next run of the alarm calls again", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => {
      throw new Error("stopped");
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(lines).toEqual([{ event: "destroy-error", error: "Error" }]);
    expect(await skeletonOf(w)).toEqual({ row: expect.objectContaining({ destroying_since: 8000, cancel_answered_at: null, cancel_next_at: null }), due: 8000 });
    let made = 0;
    w.host.port.cancelSubscription = async () => {
      made += 1;
      return { kind: "nothing-to-cancel" };
    };
    now = 9000;
    await w.entries.alarm();
    expect(made).toBe(1);
    expect((await skeletonOf(w))?.row).toEqual(expect.objectContaining({ cancel_answered_at: 9000, cancel_answered_for: "sub_one" }));
  });

  test("an error in step 3 of destroy: the cancel call is made and its answer stored, and destroy answers what its steps reached with a destroy-error line (P7-110 L1 (f))", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.throwIn = "metering.send";
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(lines).toEqual([{ event: "destroy-error", error: "Error" }]);
    w.answers.throwIn = null;
    expect(w.portCalls).toEqual([["cancelSubscription", "sub_one"]]);
    expect(await w.holds()).toBe(false);
  });

  test("an error in the alarm's seal and send part under the mark: the owed cancel call is made in that run, and the error is counted", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    await w.entries.destroy(HASH);
    let made = 0;
    w.host.port.cancelSubscription = async () => {
      made += 1;
      return { kind: "cancelled" };
    };
    w.answers.throwIn = "metering.send";
    now = 68_000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(made).toBe(1);
    expect(events()).toContain("alarm-error");
    expect((await skeletonOf(w))?.row).toEqual(expect.objectContaining({ cancel_answered_at: 68_000, alarm_failures: 1 }));
  });

  test("an account deleted and made again during the alarm's cancel call: the step after it stores nothing and deletes nothing", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    await w.entries.destroy(HASH);
    w.host.port.cancelSubscription = async () => {
      w.host.step((sql) => sql.exec("UPDATE skeleton SET destroying_since = ?", 60_000));
      return { kind: "cancelled" };
    };
    w.answers.settled = true;
    now = 68_000;
    await w.entries.alarm();
    expect([await w.holds(), (await skeletonOf(w))?.row?.cancel_answered_at]).toEqual([true, null]);
  });

  test("a failed cancel during which the account is deleted and made again stores no next try on the new account's row", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => {
      w.host.step((sql) => sql.exec("UPDATE skeleton SET destroying_since = ?", 60_000));
      return { kind: "failed" };
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect((await skeletonOf(w))?.row).toEqual(expect.objectContaining({ destroying_since: 60_000, cancel_next_at: null, cancel_delay: null }));
  });

  test("a hold that is open at step 6 keeps the storage although the sync is settled", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.host.port.cancelSubscription = async () => {
      w.answers.openHolds = 1;
      return { kind: "cancelled" };
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(await w.holds()).toBe(true);
  });

  test("an error in the alarm's seal and send part of a run that then deletes the storage logs alarm-error and no alarm-stopped", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    await w.entries.destroy(HASH);
    w.host.port.cancelSubscription = async () => ({ kind: "cancelled" });
    w.answers.settled = true;
    w.answers.throwIn = "metering.send";
    w.reset();
    now = 68_000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect([await w.holds(), events()]).toEqual([false, ["alarm-error"]]);
  });
});

describe("the closed list of P7-110 (unit 5.6 part 1)", () => {
  const skeletonOf = async (w: ReturnType<typeof world>) => {
    const snapshot = await w.entries.testInspect();
    return snapshot.result === "snapshot" ? { row: snapshot.tables.skeleton?.[0], due: snapshot.due } : null;
  };
  const underTheMark = async (o: { clock?: () => Ms } = {}) => {
    const w = world({ clock: o.clock ?? (() => 8000) });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.reset();
    return w;
  };
  const stepsOf = (w: ReturnType<typeof world>) => {
    const starts: { calls: number; alarms: number }[] = [];
    const inner = w.host.step;
    w.host.step = ((run: Parameters<typeof inner>[0]) => {
      starts.push({ calls: w.calls.length, alarms: w.alarms.length });
      return inner(run);
    }) as typeof inner;
    return () =>
      starts.map((start, index) => {
        const next = starts[index + 1] ?? { calls: w.calls.length, alarms: w.alarms.length };
        return { calls: w.calls.slice(start.calls, next.calls).map((call) => call[0]), alarms: next.alarms - start.alarms };
      });
  };

  test("L1 (a): under the mark the alarm's first step reads the mark, calls no module and arms nothing, before the step that ends due holds", async () => {
    const w = await underTheMark();
    const segments = stepsOf(w);
    await w.entries.alarm();
    const [first, ...later] = segments();
    expect(first).toEqual({ calls: [], alarms: 0 });
    expect(later.some((segment) => segment.calls.includes("ledger.expire"))).toBe(true);
  });

  for (const failing of ["ledger.expire", "metering.seal", "accounts.forget", "the wake branch"] as const) {
    test(`L1 (b): under the mark an error of ${failing} is caught and logged by name, and the same run makes the owed cancel call`, async () => {
      let now = 8000;
      const w = world({ clock: () => now });
      await w.seeded({ customerId: "cus_one" });
      w.answers.settled = false;
      w.host.port.cancelSubscription = async () => ({ kind: "failed" });
      expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
      let made = 0;
      w.host.port.cancelSubscription = async () => {
        made += 1;
        return { kind: "cancelled" };
      };
      if (failing === "the wake branch") {
        w.answers.wake = { sessionId: "cs_one" };
        w.host.port.readSession = async () => {
          throw new RangeError("the read throws");
        };
      } else {
        w.answers.throwIn = failing;
        w.answers.oldestUnsealed = () => ({ interval: 0, end: 1000 });
      }
      w.reset();
      now = 68_000;
      await w.entries.alarm();
      w.answers.throwIn = null;
      expect(made).toBe(1);
      expect(lines.filter((line) => line.event === "alarm-error").length).toBeGreaterThan(0);
    });
  }

  for (const failing of ["ledger.nextDue", "accounts.nextDue", "metering.nextSend", "metering.oldestUnsealed", "ledger.intervalTotals"] as const) {
    test(`L1 (c): a due-time read that throws (${failing}) rolls back no step: the owed cancel is made and its answer stored, alarm-error names it, and the alarm is armed`, async () => {
      let now = 8000;
      const w = world({ clock: () => now });
      await w.seeded({ customerId: "cus_one" });
      w.answers.settled = false;
      w.host.port.cancelSubscription = async () => ({ kind: "failed" });
      expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
      w.host.port.cancelSubscription = async () => ({ kind: "cancelled" });
      if (failing === "ledger.intervalTotals") w.answers.oldestUnsealed = () => ({ interval: 0, end: 1000 });
      w.answers.throwIn = failing;
      w.reset();
      now = 68_000;
      await w.entries.alarm();
      w.answers.throwIn = null;
      const snapshot = await w.entries.testInspect();
      expect(snapshot.result === "snapshot" && snapshot.tables.skeleton?.[0]?.cancel_answered_at).toBe(68_000);
      expect(lines.filter((line) => line.event === "alarm-error").length).toBeGreaterThan(0);
      expect(w.alarms.length).toBeGreaterThan(0);
      expect(w.alarms.at(-1)).not.toBeNull();
    });
  }

  test("L1 (d): metering.settled throwing on every call after a cancelled answer: the answer is stored once in a step of its own, and the next run makes no second cancel call", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    let made = 0;
    w.host.port.cancelSubscription = async () => {
      made += 1;
      return { kind: "cancelled" };
    };
    w.answers.throwIn = "metering.settled";
    await w.entries.destroy(HASH).catch(() => null);
    const stored = await w.entries.testInspect();
    now = 68_000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(stored.result === "snapshot" && stored.tables.skeleton?.[0]?.cancel_answered_at).toBe(8000);
    expect(made).toBe(1);
  });

  test("L1 (e): closeOpen throwing at the bound is caught and logged by name, the storage goes all the same, and the count of closed intervals is logged as unknown", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.answers.throwIn = "metering.closeOpen";
    w.reset();
    now = 8000 + DELETE_REMOVAL_BOUND_MS;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(await w.holds()).toBe(false);
    expect(lines).toEqual([
      { event: "alarm-error", error: "Error" },
      { event: "usage-not-billable", error: "Error" },
    ]);
  });

  test("L1 (f): metering.cut throwing at the delete: the mark is stored in a step of its own, the error is logged as destroy-error, the cancel is made and its answer stored, and destroy answers", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.answers.throwIn = "metering.cut";
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.answers.throwIn = null;
    const snapshot = await w.entries.testInspect();
    expect(snapshot.result === "snapshot" && snapshot.tables.skeleton?.[0]).toEqual(expect.objectContaining({ destroying_since: 8000, cancel_answered_at: 8000 }));
    expect(w.portCalls).toEqual([["cancelSubscription", "sub_one"]]);
    expect(lines).toEqual([{ event: "destroy-error", error: "Error" }]);
  });

  test("L1 (f): after a cut that threw at the delete, the alarm under the mark makes the cut at the mark's time", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.answers.throwIn = "metering.cut";
    await w.entries.destroy(HASH);
    w.answers.throwIn = null;
    w.reset();
    now = 9000;
    await w.entries.alarm();
    expect(w.calls).toContainEqual(["metering.cut", "delete", 8000, 9000]);
  });

  test("L1 (f): a platform stop (ObjectStoppedError) and the removal's own rejection still leave destroy", async () => {
    const stopped = world({ clock: () => 8000 });
    await stopped.seeded({ customerId: "cus_one" });
    stopped.host.port.cancelSubscription = async () => {
      const error = new Error("the object stopped");
      error.name = "ObjectStoppedError";
      throw error;
    };
    await expect(stopped.entries.destroy(HASH)).rejects.toThrow("the object stopped");
    const rejected = world({ clock: () => 8000 });
    await rejected.seeded({ customerId: "cus_one" });
    rejected.host.deleteAll = async () => {
      throw new RangeError("the alarm call was refused");
    };
    await expect(rejected.entries.destroy(HASH)).rejects.toThrow("the alarm call was refused");
  });

  test("L2: ledger.openHolds throwing on every call blocks no cancel: step 4 counts the holds open at its now through ledger.openHoldsAt", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.host.port.cancelSubscription = async () => ({ kind: "cancelled" });
    w.answers.throwIn = "ledger.openHolds";
    w.reset();
    now = 68_000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(w.calls).toContainEqual(["ledger.openHoldsAt", 68_000]);
    const snapshot = await w.entries.testInspect();
    expect(snapshot.result === "snapshot" && snapshot.tables.skeleton?.[0]?.cancel_answered_at).toBe(68_000);
  });

  test("L3 (a): under the mark an error that recurs in every run never stops the alarm: ten runs, no alarm-stopped, and no run arms null", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.answers.throwIn = "ledger.expire";
    w.reset();
    for (let run = 0; run < 10; run += 1) {
      now = w.alarmAt() ?? now + ALARM_RETRY_MS;
      await w.entries.alarm();
    }
    w.answers.throwIn = null;
    expect(events().filter((event) => event === "alarm-stopped")).toEqual([]);
    expect(w.alarms.filter((at) => at === null)).toEqual([]);
    expect(w.alarmAt()).not.toBeNull();
  });

  test("L3 (b): a recurring error under the mark for one day: no two runs closer than the pause, which doubles from ALARM_RETRY_MS to ALARM_PAUSE_CAP_MS, one run and its alarm-error lines in each hour once the pause is capped", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.answers.throwIn = "ledger.expire";
    w.reset();
    const runs: { at: Ms; errors: number }[] = [];
    while (now < 8000 + 24 * 3_600_000) {
      const at = w.alarmAt();
      if (at === null || at <= now) break;
      now = at;
      const before = lines.length;
      await w.entries.alarm();
      runs.push({ at, errors: lines.slice(before).filter((line) => line.event === "alarm-error").length });
    }
    w.answers.throwIn = null;
    const pause = (failures: number) => Math.min(ALARM_RETRY_MS * 2 ** (failures - 1), ALARM_PAUSE_CAP_MS);
    const gaps = runs.slice(1).map((run, index) => run.at - (runs[index]?.at ?? 0));
    expect(gaps.every((gap, index) => gap >= pause(index + 1))).toBe(true);
    const hours = new Map<number, { runs: number; errors: number }>();
    for (const run of runs) {
      const hour = Math.floor((run.at - 8000) / 3_600_000);
      const seen = hours.get(hour) ?? { runs: 0, errors: 0 };
      hours.set(hour, { runs: seen.runs + 1, errors: seen.errors + run.errors });
    }
    const perRun = runs[0]?.errors ?? 0;
    expect(perRun).toBeGreaterThan(0);
    for (let hour = 3; hour < 24; hour += 1) expect(hours.get(hour)).toEqual({ runs: 1, errors: perRun });
    expect(events().filter((event) => event === "alarm-stopped")).toEqual([]);
  });

  test("L3 (d): a cancel that goes unanswered writes cancel-unanswered, naming no account, at each try once the delay has reached its cap", async () => {
    let now = 8000;
    const w = world({ clock: () => now, port: { cancelSubscription: async () => ({ kind: "failed" }) } });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    const perTry: number[] = [];
    const unansweredLines = () => lines.filter((line) => line.event === "cancel-unanswered").length;
    let seen = unansweredLines();
    await w.entries.destroy(HASH);
    perTry.push(unansweredLines() - seen);
    for (let each = 0; each < 9; each += 1) {
      seen = unansweredLines();
      const snapshot = await w.entries.testInspect();
      now = Number(snapshot.result === "snapshot" ? snapshot.due : now);
      await w.entries.alarm();
      perTry.push(unansweredLines() - seen);
    }
    expect(perTry).toEqual([0, 0, 0, 0, 0, 0, 1, 1, 1, 1]);
    expect(lines.filter((line) => line.event === "cancel-unanswered").every((line) => Object.keys(line).join() === "event")).toBe(true);
  });

  test("L1 (c), L3 (b): a due time that throws in every step under the mark makes each run a failing one: for one day no two runs closer than the doubling pause, and one run an hour once it is capped", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => ({ kind: "failed" });
    w.answers.throwIn = "ledger.nextDue";
    await w.entries.destroy(HASH);
    const runs: Ms[] = [];
    while (now < 8000 + 24 * 3_600_000) {
      const at = w.alarmAt();
      if (at === null || at <= now) break;
      now = at;
      await w.entries.alarm();
      runs.push(at);
    }
    w.answers.throwIn = null;
    const pause = (failures: number) => Math.min(ALARM_RETRY_MS * 2 ** (failures - 1), ALARM_PAUSE_CAP_MS);
    const gaps = runs.slice(1).map((at, index) => at - (runs[index] ?? 0));
    expect(gaps.every((gap, index) => gap >= pause(index + 1))).toBe(true);
    expect(runs.filter((at) => at >= 8000 + 3 * 3_600_000).length).toBeLessThanOrEqual(22);
  });

  test("L4 (b): a destroy from outside that meets a cancel call in flight for the row's id answers not-finished, makes no call, writes no answer and deletes nothing; status answers closing; the due time is the call's end bound", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    let release: (answer: { kind: "cancelled" }) => void = () => {};
    const waiting = new Promise<{ kind: "cancelled" }>((resolve) => {
      release = resolve;
    });
    let calls = 0;
    w.host.port.cancelSubscription = async () => {
      calls += 1;
      return waiting;
    };
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    const second = await w.entries.destroy(HASH);
    const during = { status: await w.entries.status(HASH), snapshot: await skeletonOf(w) };
    release({ kind: "cancelled" });
    expect(await first).toEqual({ result: "deleted" });
    expect([second, calls]).toEqual([{ result: "not-finished" }, 1]);
    expect(during.status).toEqual({ result: "closing" });
    expect(during.snapshot).toEqual({ row: expect.objectContaining({ cancel_answered_at: null, cancel_next_at: null }), due: 8000 + CALL_IN_FLIGHT_MS });
  });

  test("L4 (b): a cancel call in flight for one id does not hold back the call for another id that the row holds now", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    let release: (answer: { kind: "cancelled" }) => void = () => {};
    const waiting = new Promise<{ kind: "cancelled" }>((resolve) => {
      release = resolve;
    });
    const ids: (string | null)[] = [];
    w.host.port.cancelSubscription = async (id) => {
      ids.push(id);
      return ids.length === 1 ? waiting : { kind: "cancelled" };
    };
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    w.answers.subscription = { ...METERED, subscriptionId: "sub_two" };
    await w.entries.destroy(HASH);
    release({ kind: "cancelled" });
    await first;
    expect(ids).toEqual(["sub_one", "sub_two"]);
  });

  test("L4 (c): a send in flight delays the cancel: a destroy from outside during it makes no call, and the due time is the send's end bound in place of the mark", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.answers.send = { call: SEND, notBillable: 0 };
    let release: (answer: { kind: "recorded" }) => void = () => {};
    const waiting = new Promise<{ kind: "recorded" }>((resolve) => {
      release = resolve;
    });
    w.host.port.sendUsage = async () => waiting;
    w.answers.sendInFlight = (at) => (at < 8000 + CALL_IN_FLIGHT_MS ? 8000 + CALL_IN_FLIGHT_MS : null);
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    w.answers.send = { call: null, notBillable: 0 };
    const second = await w.entries.destroy(HASH);
    const during = { calls: w.portCalls.filter((call) => call[0] === "cancelSubscription").length, due: (await skeletonOf(w))?.due };
    w.answers.sendInFlight = () => null;
    release({ kind: "recorded" });
    await first;
    expect([second, during]).toEqual([{ result: "not-finished" }, { calls: 0, due: 8000 + CALL_IN_FLIGHT_MS }]);
    expect(w.portCalls.filter((call) => call[0] === "cancelSubscription")).toEqual([["cancelSubscription", "sub_one"]]);
  });

  test("L3 (b): with nothing owed (no answer, no next try), the pause after failing runs under the mark doubles: 30 s, 60 s, 120 s, 240 s", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => {
      throw new RangeError("the call throws");
    };
    await w.entries.destroy(HASH);
    const arms: (Ms | null)[] = [];
    for (let run = 0; run < 4; run += 1) {
      now = w.alarmAt() ?? now;
      await w.entries.alarm();
      arms.push(w.alarmAt());
    }
    expect(arms).toEqual([8000 + 30_000, 38_000 + 60_000, 98_000 + 120_000, 218_000 + 240_000]);
  });

  test("L3 (b): a failing run under the mark with the answer stored arms at the removal's bound, not at the pause", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    await w.entries.destroy(HASH);
    w.answers.throwIn = "ledger.expire";
    now = 9000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(w.alarmAt()).toBe(8000 + DELETE_REMOVAL_BOUND_MS);
  });

  test("L3 (a): when the count step itself throws under the mark, the alarm arms one ALARM_RETRY_MS later and writes no alarm-stopped", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.throwIn = "ledger.expire";
    const inner = w.host.step;
    w.host.step = (<T>(run: (sql: Sql) => T): T =>
      inner((sql) =>
        run({
          exec: (query: string, ...bindings: (string | number | null)[]) => {
            if (query.startsWith("UPDATE skeleton SET last_now = ?, alarm_failures = ?") || query.startsWith("UPDATE skeleton SET alarm_failures = ?")) throw new RangeError("the count step throws");
            return sql.exec(query, ...bindings);
          },
        } as Sql),
      )) as typeof inner;
    w.reset();
    now = 9000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(w.alarmAt()).toBe(9000 + ALARM_RETRY_MS);
    expect(events()).not.toContain("alarm-stopped");
  });
});

describe("the second closed list of P7-111 (unit 5.6 part 1)", () => {
  const skeleton = (w: ReturnType<typeof world>) => w.host.step((sql) => sql.exec("SELECT * FROM skeleton")[0]);

  test("M1: accounts.subscription throwing in every run under the mark, the count step among them: the count is stored and the gaps double", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.answers.openHolds = 0;
    w.answers.throwIn = "accounts.subscription";
    const gaps: Ms[] = [];
    for (let run = 0; run < 5; run += 1) {
      now = Math.max(now, w.alarmAt() ?? now);
      const at = now;
      await w.entries.alarm();
      gaps.push((w.alarmAt() ?? 0) - at);
    }
    w.answers.throwIn = null;
    expect(gaps).toEqual([30_000, 60_000, 120_000, 240_000, 480_000]);
    expect(skeleton(w)?.alarm_failures).toBe(5);
  });

  for (const failing of ["ledger.nextDue", "metering.nextSend"] as const) {
    test(`M2: ${failing} throwing from the due time alone for an account with no mark: 5 runs, alarm-stopped, alarm null; status still answers, and destroy begins the delete and makes the cancel`, async () => {
      let now = 8000;
      const w = world({ clock: () => now });
      await w.seeded({ customerId: "cus_one" });
      w.answers.throwIn = failing;
      await w.entries.rearm();
      const runs: Ms[] = [];
      for (let run = 0; run < 20; run += 1) {
        const due = w.alarmAt();
        if (due === null) break;
        now = Math.max(now, due);
        runs.push(now);
        await w.entries.alarm();
      }
      expect(runs).toHaveLength(5);
      expect(events().filter((event) => event === "alarm-stopped")).toHaveLength(1);
      expect(w.alarmAt()).toBeNull();
      expect(skeleton(w)?.alarm_failures).toBe(5);
      expect(await w.entries.status(HASH)).toEqual({ result: "active" });
      expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
      expect(w.portCalls.filter((call) => call[0] === "cancelSubscription")).toEqual([["cancelSubscription", "sub_one"]]);
    });
  }

  const record = (w: ReturnType<typeof world>) =>
    w.host.step((sql) => (sql.exec("SELECT name FROM sqlite_master WHERE type = ? AND name = ?", "table", "cancel_call").length === 0 ? [] : sql.exec("SELECT subscription_id, began_at FROM cancel_call")));

  test("M3 (a): the step that decides the cancel call stores the id and the call's begin before the call; the step after the call's answer clears it, and a call that throws has it cleared in a step of its own", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    const during: unknown[] = [];
    let answer: "cancelled" | "throw" = "throw";
    w.host.port.cancelSubscription = async () => {
      during.push(record(w));
      now += 2000;
      if (answer === "throw") throw new RangeError("the call throws");
      return { kind: "cancelled" };
    };
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(during).toEqual([[{ subscription_id: "sub_one", began_at: 8000 }]]);
    expect(record(w)).toEqual([]);
    expect(events()).toContain("destroy-error");
    answer = "cancelled";
    now = Math.max(now, w.alarmAt() ?? now);
    const began = now;
    await w.entries.alarm();
    expect(during[1]).toEqual([{ subscription_id: "sub_one", began_at: began }]);
    expect(record(w)).toEqual([]);
    expect(skeleton(w)?.cancel_answered_for).toBe("sub_one");
  });

  test("M3 (a): a call for no subscription id stores no record", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one" });
    w.answers.subscription = { ...METERED, subscriptionId: null };
    w.answers.settled = false;
    const during: unknown[] = [];
    w.host.port.cancelSubscription = async () => {
      during.push(record(w));
      return { kind: "nothing-to-cancel" };
    };
    await w.entries.destroy(HASH);
    expect(during).toEqual([[]]);
  });

  test("M3 (b): two instances over one storage: while the first instance's cancel call is in flight, the second makes no cancel call before the record's expiry, its destroy answers not-finished, status answers closing, and its due time is the expiry; at the expiry it calls", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    let release: (answer: { kind: "cancelled" }) => void = () => {};
    let cancels = 0;
    w.host.port.cancelSubscription = async () => {
      cancels += 1;
      if (cancels > 1) return { kind: "cancelled" };
      return new Promise((resolve) => {
        release = resolve;
      });
    };
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(cancels).toBe(1);
    const second = makeEntries(w.host, w.modules);
    now = 9000;
    expect(await second.destroy(HASH)).toEqual({ result: "not-finished" });
    expect(await second.status(HASH)).toEqual({ result: "closing" });
    expect(cancels).toBe(1);
    expect(w.alarmAt()).toBe(8000 + CALL_IN_FLIGHT_MS);
    now = 8000 + CALL_IN_FLIGHT_MS - 1;
    await second.alarm();
    expect(cancels).toBe(1);
    now = 8000 + CALL_IN_FLIGHT_MS;
    await second.alarm();
    expect(cancels).toBe(2);
    release({ kind: "cancelled" });
    await first;
    expect(skeleton(w)?.cancel_answered_for).toBe("sub_one");
  });

  test("M3 (b): a restart during the cancel call: the new instance makes no cancel call before the record's expiry and makes it at the expiry", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    let cancels = 0;
    w.host.port.cancelSubscription = async () => {
      cancels += 1;
      if (cancels > 1) return { kind: "cancelled" };
      return new Promise(() => {});
    };
    void w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    const restarted = makeEntries(w.host, w.modules);
    await restarted.rearm();
    expect(w.alarmAt()).toBe(8000 + CALL_IN_FLIGHT_MS);
    now = 9000;
    await restarted.alarm();
    expect(cancels).toBe(1);
    now = w.alarmAt() ?? now;
    await restarted.alarm();
    expect(now).toBe(8000 + CALL_IN_FLIGHT_MS);
    expect(cancels).toBe(2);
    expect(skeleton(w)?.cancel_answered_for).toBe("sub_one");
  });

  test("M3 (b): a failing run under the mark while a cancel call is in flight arms no earlier than the record's expiry", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    let release: (answer: { kind: "cancelled" }) => void = () => {};
    w.host.port.cancelSubscription = async () =>
      new Promise((resolve) => {
        release = resolve;
      });
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    w.answers.throwIn = "accounts.forget";
    now = 9000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(w.alarmAt()).toBe(8000 + CALL_IN_FLIGHT_MS);
    release({ kind: "cancelled" });
    await first;
  });

  test("M3 (c): the await of the cancel call is bounded on the host's timer, at least the provider call's timeout and strictly shorter than the record's expiry, which is at least the timeout plus ALARM_RETRY_MS", () => {
    expect(CALL_IN_FLIGHT_MS).toBeGreaterThanOrEqual(PROVIDER_CALL_TIMEOUT_MS + ALARM_RETRY_MS);
    expect(CANCEL_CALL_BOUND_MS).toBeGreaterThanOrEqual(PROVIDER_CALL_TIMEOUT_MS);
    expect(CANCEL_CALL_BOUND_MS).toBeLessThan(CALL_IN_FLIGHT_MS);
  });

  test("M3 (c): a call that settles 1 ms after the provider call's timeout still holds a second call back, and its answer is stored", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    let release: (answer: { kind: "cancelled" }) => void = () => {};
    let cancels = 0;
    w.host.port.cancelSubscription = async () => {
      cancels += 1;
      return new Promise((resolve) => {
        release = resolve;
      });
    };
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(w.pauses()).toEqual([CANCEL_CALL_BOUND_MS]);
    now = 8000 + PROVIDER_CALL_TIMEOUT_MS + 1;
    expect(await makeEntries(w.host, w.modules).destroy(HASH)).toEqual({ result: "not-finished" });
    await w.entries.alarm();
    expect(cancels).toBe(1);
    release({ kind: "cancelled" });
    expect(await first).toEqual({ result: "not-finished" });
    expect(skeleton(w)?.cancel_answered_for).toBe("sub_one");
    expect(record(w)).toEqual([]);
  });

  test("M3 (c): a call that never settles: at the bound one line and the run goes on as for a failed call; one run at the record's expiry, the cancel made after it, and the next run no earlier than one pause later", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    let cancels = 0;
    w.host.port.cancelSubscription = async () => {
      cancels += 1;
      if (cancels === 1) return new Promise(() => {});
      return { kind: "cancelled" };
    };
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    now = 8000 + CANCEL_CALL_BOUND_MS;
    w.endPauses();
    expect(await first).toEqual({ result: "not-finished" });
    expect(events()).toContain("cancel-call-unsettled");
    expect(skeleton(w)?.cancel_next_at).toBe(8000 + CANCEL_CALL_BOUND_MS + 60_000);
    expect(record(w)).toEqual([{ subscription_id: "sub_one", began_at: 8000 }]);
    expect(w.alarmAt()).toBe(8000 + CALL_IN_FLIGHT_MS);
    const runs: Ms[] = [];
    for (let run = 0; run < 3 && skeleton(w)?.cancel_answered_for !== "sub_one"; run += 1) {
      now = Math.max(now, w.alarmAt() ?? now);
      runs.push(now);
      await w.entries.alarm();
    }
    expect(runs).toEqual([8000 + CALL_IN_FLIGHT_MS, 8000 + CANCEL_CALL_BOUND_MS + 60_000]);
    expect((runs[1] as Ms) - (runs[0] as Ms)).toBeGreaterThanOrEqual(ALARM_RETRY_MS);
    expect(cancels).toBe(2);
    expect(skeleton(w)?.cancel_answered_for).toBe("sub_one");
  });

  test("M3 (c): a call that settles after its bound clears only its own record: the newer call's record stands, and the late answer is not stored", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    const releases: ((answer: { kind: "cancelled" }) => void)[] = [];
    w.host.port.cancelSubscription = async () =>
      new Promise((resolve) => {
        releases.push(resolve);
      });
    const first = w.entries.destroy(HASH);
    await new Promise((resolve) => setTimeout(resolve, 0));
    now = 8000 + CANCEL_CALL_BOUND_MS;
    w.endPauses();
    await first;
    now = Math.max(now, skeleton(w)?.cancel_next_at as Ms);
    const second = w.entries.alarm();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(releases).toHaveLength(2);
    expect(record(w)).toEqual([{ subscription_id: "sub_one", began_at: now }]);
    releases[0]?.({ kind: "cancelled" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(record(w)).toEqual([{ subscription_id: "sub_one", began_at: now }]);
    expect(skeleton(w)?.cancel_answered_at).toBeNull();
    releases[1]?.({ kind: "cancelled" });
    await second;
    expect(skeleton(w)?.cancel_answered_for).toBe("sub_one");
  });

  test("M3 (d): a failing run under the mark while a send is in flight arms no earlier than the send's end bound", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.openHolds = 0;
    w.answers.sendInFlight = (at) => (at < 8000 + CALL_IN_FLIGHT_MS ? 8000 + CALL_IN_FLIGHT_MS : null);
    w.answers.throwIn = "accounts.forget";
    now = 9000;
    await w.entries.alarm();
    w.answers.throwIn = null;
    expect(w.portCalls.filter((call) => call[0] === "cancelSubscription")).toEqual([]);
    expect(w.alarmAt()).toBe(8000 + CALL_IN_FLIGHT_MS);
  });

  test("M3 (e): under the mark a due time at or before the step's now that the run does not consume arms at now plus ALARM_RETRY_MS: no run without a pause", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.accountsNextDue = 5000;
    const runs: Ms[] = [];
    for (let run = 0; run < 4; run += 1) {
      now = Math.max(now, w.alarmAt() ?? now);
      runs.push(now);
      await w.entries.alarm();
    }
    expect(runs).toEqual([8000, 8000 + ALARM_RETRY_MS, 8000 + 2 * ALARM_RETRY_MS, 8000 + 3 * ALARM_RETRY_MS]);
    expect(events()).not.toContain("alarm-error");
  });

  test("M3 (e): under a frozen test clock the same due time makes one run and no second at that instant", async () => {
    const w = world({ clock: () => 8000 });
    await w.seeded({ customerId: "cus_one", time: 8000 });
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.accountsNextDue = 5000;
    w.reset();
    expect(await w.entries.testSetClock(9000)).toEqual({ result: "clock-set" });
    expect(w.names().filter((name) => name === "accounts.forget")).toHaveLength(1);
  });

  const failingWrites = (w: ReturnType<typeof world>, fails: (query: string) => boolean) => {
    const inner = w.host.step;
    w.host.step = (<T>(run: (sql: Sql) => T): T =>
      inner((sql) =>
        run({
          exec: (query: string, ...bindings: (string | number | null)[]) => {
            if (fails(query)) throw new RangeError("the write fails");
            return sql.exec(query, ...bindings);
          },
        } as Sql),
      )) as typeof inner;
  };

  const gapsOver = async (w: ReturnType<typeof world>, clock: { now: Ms }, runs: number, fresh: boolean) => {
    const gaps: Ms[] = [];
    for (let run = 0; run < runs; run += 1) {
      clock.now = Math.max(clock.now, w.alarmAt() ?? clock.now);
      const at = clock.now;
      await (fresh ? makeEntries(w.host, w.modules) : w.entries).alarm();
      gaps.push(((w.alarmAt() ?? at) - at) / 1000);
    }
    return gaps;
  };

  test("M4: every write of last_now fails and a fresh makeEntries runs each alarm over the same host: the count is written apart from last_now, and the gaps double 30 s, 60 s, 120 s to the cap", async () => {
    const clock = { now: 8000 };
    const w = world({ clock: () => clock.now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.openHolds = 0;
    w.answers.settled = false;
    w.host.port.cancelSubscription = async () => {
      throw new RangeError("the call throws");
    };
    failingWrites(w, (query) => query.startsWith("UPDATE skeleton SET last_now = ?"));
    expect(await gapsOver(w, clock, 9, true)).toEqual([30, 60, 120, 240, 480, 960, 1920, 3600, 3600]);
  });

  test("M4: the count's own write fails in every run of one instance: the count kept in memory is the floor, and the gaps double", async () => {
    const clock = { now: 8000 };
    const w = world({ clock: () => clock.now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.throwIn = "accounts.forget";
    failingWrites(w, (query) => query.includes("alarm_failures = ?"));
    expect(await gapsOver(w, clock, 5, false)).toEqual([30, 60, 120, 240, 480]);
    expect(events()).not.toContain("alarm-stopped");
  });

  test("M4: when every write of the object fails and the instance is new at each run, the pause stays at 30 s, as ALARM_PAUSE_CAP_MS's doc comment says", async () => {
    const clock = { now: 8000 };
    const w = world({ clock: () => clock.now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.openHolds = 1;
    await w.entries.destroy(HASH);
    w.answers.throwIn = "accounts.forget";
    failingWrites(w, (query) => query.startsWith("UPDATE"));
    expect(await gapsOver(w, clock, 4, true)).toEqual([30, 30, 30, 30]);
  });

  test("M5: past the removal's bound a removal that keeps failing is tried again within SYNC_RETRY_MS of each failing run, never after the pause, and each such run writes removal-overdue, which names no account", async () => {
    let now = 8000;
    const w = world({ clock: () => now });
    await w.seeded({ customerId: "cus_one" });
    w.answers.settled = false;
    expect(await w.entries.destroy(HASH)).toEqual({ result: "not-finished" });
    w.host.deleteAll = async () => {
      throw new RangeError("the removal is rejected");
    };
    now = 8000 + DELETE_REMOVAL_BOUND_MS;
    const runs: Ms[] = [];
    for (let run = 0; run < 5; run += 1) {
      now = Math.max(now, w.alarmAt() ?? now);
      runs.push(now - 8000 - DELETE_REMOVAL_BOUND_MS);
      await w.entries.alarm();
    }
    expect(runs).toEqual([0, 60_000, 120_000, 180_000, 240_000]);
    expect(lines.filter((line) => line.event === "removal-overdue")).toEqual(Array(5).fill({ event: "removal-overdue" }));
  });
});
