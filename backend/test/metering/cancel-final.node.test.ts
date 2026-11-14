import { describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderEvent, SubscriptionState } from "../../src/billing/port.ts";
import { reduceEvent } from "../../src/billing/stripe/reduce.ts";
import { openSql } from "../../test-kit/sql/node.ts";
import { ACCOUNT, HASH, HOUR, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

type Row = Record<string, string | number | null>;

const MINUTE = 60_000;
const PERIOD = { start: START, end: START + 720 * HOUR };
const SESSION = { sessionId: "cs_final", customerId: "cus_1", subscriptionId: "sub_1", payment: "paid" as const };
const CHECKOUT = { sessionId: SESSION.sessionId, url: "http://127.0.0.1:1/x" };
const COMPLETED: ProviderEvent = { kind: "checkout-completed", eventId: "evt_completed", providerTime: START + 2 * MINUTE, accountId: ACCOUNT, ...SESSION };
const REFUSED = { outcome: "refused", reason: "not-active" };

function at(eventId: string, minute: number, state: SubscriptionState, over: Partial<Extract<ProviderEvent, { kind: "subscription" }>> = {}): ProviderEvent {
  const providerTime = START + minute * MINUTE;
  return subscriptionEvent({ eventId, providerTime, state, period: PERIOD, endedAt: state === "cancelled" ? providerTime : null, ...over });
}

const ACTIVE = at("evt_active", 1, "active");
const CANCEL = at("evt_cancel", 5, "cancelled");
const ACTIVE_AGAIN = at("evt_active_again", 6, "active");
const FAILED_AFTER = at("evt_failed_after", 7, "payment-failed");

function orders<T>(items: T[]): T[][] {
  if (items.length <= 1) return [items];
  return items.flatMap((item, index) => orders([...items.slice(0, index), ...items.slice(index + 1)]).map((rest) => [item, ...rest]));
}

function sorted(rows: Row[]): Row[] {
  return rows.map((row) => JSON.stringify(row)).sort().map((row) => JSON.parse(row) as Row);
}

const MARK = ["m1_count", "m1_newest", "m2_count", "m2_newest"];
const NO_MARK = { m1_count: 0, m1_newest: null, m2_count: 0, m2_newest: null };

function markOf(row: Row | undefined): Record<string, string | number | null> {
  return Object.fromEntries(MARK.map((column) => [column, row === undefined || !(column in row) ? "absent" : (row[column] ?? null)]));
}

function unmarked(name: string, row: Row): Row {
  if (name !== "account" && name !== "kept_event") return row;
  return Object.fromEntries(Object.entries(row).filter(([column]) => !MARK.includes(column)));
}

async function snapshot(w: World): Promise<Record<string, Row[]>> {
  const tables = await w.tables();
  return Object.fromEntries(Object.keys(tables).sort().map((name) => [name, sorted((tables[name] ?? []).map((row) => unmarked(name, row)))]));
}

async function stored(w: World): Promise<Record<string, Row[]>> {
  const { processed_event: _ids, ...rest } = await snapshot(w);
  return rest;
}

function sent(w: World): unknown {
  return w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor, each.timestamp]);
}

function done(w: World): void {
  const identifiers = w.meter.recordedEvents(ACCOUNT).map((each) => each.identifier);
  expect({ distinct: new Set(identifiers).size, otherContent: w.meter.stats().duplicatesWithOtherContent }).toEqual({ distinct: identifiers.length, otherContent: 0 });
  w.close();
}

async function deliver(w: World, events: ProviderEvent[], lines: string[] = []): Promise<void> {
  for (const event of events) await quiet(lines, () => w.entries().webhookEvent(event));
}

async function account(w: World): Promise<Row | undefined> {
  return (await w.tables()).account?.[0];
}

async function metered(w: World): Promise<unknown> {
  return ((await w.tables()).metering_account ?? []).map((row) => ({ cut_kind: row.cut_kind, cut_at: row.cut_at, carry: row.carry, written_off: row.written_off }));
}

interface Held {
  state: SubscriptionState;
  time: number | null;
  periodEnd: number | null;
}

const RANKS: Record<SubscriptionState, number> = { "payment-failed": 0, active: 1, cancelled: 2 };

function newer(event: Extract<ProviderEvent, { kind: "subscription" }>, held: Held): boolean {
  const cancel = event.state === "cancelled";
  if (cancel !== (held.state === "cancelled")) return cancel;
  if (held.time === null) return true;
  if (event.providerTime !== held.time) return event.providerTime > held.time;
  if (event.state !== held.state) return RANKS[event.state] > RANKS[held.state];
  if (event.period === null) return false;
  return held.periodEnd === null || event.period.end > held.periodEnd;
}

function implied(order: ProviderEvent[], start: Held | null): { mark: Record<string, number | null>; lines: number } {
  let held = start;
  const mark = { m1_count: 0, m1_newest: null as number | null, m2_count: 0, m2_newest: null as number | null };
  let lines = 0;
  for (const event of order) {
    if (event.kind === "checkout-completed") {
      held ??= { state: "active", time: null, periodEnd: null };
      continue;
    }
    if (event.kind !== "subscription") continue;
    if (held === null) {
      held = { state: event.state, time: event.providerTime, periodEnd: event.period?.end ?? null };
      continue;
    }
    const cancel = event.state === "cancelled";
    if (held.state === "cancelled" && !cancel && (held.time === null || event.providerTime > held.time)) {
      mark.m1_count += 1;
      mark.m1_newest = Math.max(mark.m1_newest ?? event.providerTime, event.providerTime);
      lines += 1;
    }
    if (held.state !== "cancelled" && cancel && held.time !== null && event.providerTime < held.time) {
      mark.m2_count += 1;
      mark.m2_newest = Math.max(mark.m2_newest ?? event.providerTime, event.providerTime);
      lines += 1;
    }
    if (newer(event, held)) held = { state: event.state, time: event.providerTime, periodEnd: event.period?.end ?? null };
  }
  return { mark, lines };
}

async function pending(): Promise<World> {
  const w = await world({ seed: false, checkout: CHECKOUT });
  expect(await w.entries().start(ACCOUNT, HASH, { amount: 1000, period: "month" })).toEqual({ result: "session", url: CHECKOUT.url });
  return w;
}

describe("an event that is not a cancel changes nothing of a cancelled account (ruling P7-80, R1; unit 5.5)", () => {
  test("B, the shortest: a cancel, an active event stamped later, usage and a second cancel leave the account cancelled with the first end, and nothing is billed", async () => {
    const w = await world();
    await deliver(w, [at("evt_c1", 0, "cancelled"), at("evt_a", 0.5, "active")]);
    const status = await w.entries().status(HASH);
    const reserved = await w.reserve("r1");
    await w.settle("r1", 12_345);
    await deliver(w, [at("evt_c2", 1, "cancelled")]);
    await w.clock(START + 2 * HOUR);
    expect({ status, reserved, total: w.meter.exactTotal(ACCOUNT, 0, Number.MAX_SAFE_INTEGER), sent: sent(w) }).toEqual({
      status: { result: "cancelled" },
      reserved: REFUSED,
      total: 0,
      sent: [],
    });
    expect(await account(w)).toEqual(expect.objectContaining({ state: "cancelled", ended_at: START, cancel_first: START, provider_time: START + MINUTE }));
    expect(await metered(w)).toEqual([{ cut_kind: "cancel", cut_at: START, carry: 0, written_off: 0 }]);
    done(w);
  });

  test("B with usage first and the second cancel 600 s after the active event: only the usage before the cut is billed, stamped inside the cut interval, and one remainder is written off", async () => {
    const w = await world();
    await w.use("r0", 12_345);
    await w.clock(START + MINUTE);
    await deliver(w, [at("evt_c1", 1, "cancelled")]);
    await w.clock(START + 2 * HOUR);
    await deliver(w, [at("evt_a", 120, "active")]);
    const reserved = await w.reserve("r1");
    await w.settle("r1", 12_345);
    await w.clock(START + 2 * HOUR + 10 * MINUTE);
    await deliver(w, [at("evt_c2", 130, "cancelled")]);
    await w.clock(START + 4 * HOUR);
    expect({ reserved, sent: sent(w), cut: await metered(w) }).toEqual({
      reserved: REFUSED,
      sent: [[`${ACCOUNT}:${START / 1000}`, 1, (START + MINUTE) / 1000 - 1]],
      cut: [{ cut_kind: "cancel", cut_at: START + MINUTE, carry: 0, written_off: 2_345 }],
    });
    expect(await account(w)).toEqual(expect.objectContaining({ state: "cancelled", ended_at: START + MINUTE, cancel_first: START + MINUTE }));
    done(w);
  });

  test("C: a cancel while its interval is open, an active event, 2 h, usage, 2 h: the write-off is that of a cancelled account, the later reserve is refused, and the delete finds a carry of 0", async () => {
    const w = await world();
    await w.use("r0", 12_345);
    await w.clock(START + MINUTE);
    await deliver(w, [at("evt_c", 1, "cancelled"), at("evt_a", 1.5, "active")]);
    await w.clock(START + 2 * HOUR);
    const reserved = await w.reserve("r2");
    await w.settle("r2", 5_000);
    await w.clock(START + 4 * HOUR);
    const before = { status: await w.entries().status(HASH), reserved, cut: await metered(w), sent: sent(w) };
    const deleted = await w.entries().destroy(HASH);
    expect({ before, deleted }).toEqual({
      before: {
        status: { result: "cancelled" },
        reserved: REFUSED,
        cut: [{ cut_kind: "cancel", cut_at: START + MINUTE, carry: 0, written_off: 2_345 }],
        sent: [[`${ACCOUNT}:${START / 1000}`, 1, (START + MINUTE) / 1000 - 1]],
      },
      deleted: { result: "deleted" },
    });
    done(w);
  });

  test("C with the active event after the cut interval sealed: the account is cancelled after the active event, and the run ends with a carry of 0", async () => {
    const w = await world();
    await w.use("r0", 12_345);
    await w.clock(START + MINUTE);
    await deliver(w, [at("evt_c", 1, "cancelled")]);
    await w.clock(START + 2 * HOUR);
    await deliver(w, [at("evt_a", 120, "active")]);
    expect(await w.entries().status(HASH)).toEqual({ result: "cancelled" });
    const reserved = await w.reserve("r2");
    await w.settle("r2", 5_000);
    await w.clock(START + 4 * HOUR);
    expect({ reserved, cut: await metered(w), sent: sent(w) }).toEqual({
      reserved: REFUSED,
      cut: [{ cut_kind: "cancel", cut_at: START + MINUTE, carry: 0, written_off: 2_345 }],
      sent: [[`${ACCOUNT}:${START / 1000}`, 1, (START + MINUTE) / 1000 - 1]],
    });
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    done(w);
  });

  test("a second object of the older active state, stamped later than the cancel, changes no stored row but the mark, where it counts once with its provider time (the list of event ids apart), and no reserve is admitted after it", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [ACTIVE, CANCEL]);
    const before = await stored(w);
    await deliver(w, [at("evt_active_repeat", 6, "active")]);
    expect(await stored(w)).toEqual(before);
    expect(markOf(await account(w))).toEqual({ m1_count: 1, m1_newest: START + 6 * MINUTE, m2_count: 0, m2_newest: null });
    expect(await w.reserve("r1")).toEqual(REFUSED);
    expect(await w.entries().status(HASH)).toEqual({ result: "cancelled" });
    done(w);
  });

  test("a payment-failed event stamped later than the cancel leaves the row cancelled with its cancel's time, and status answers cancelled", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [ACTIVE, CANCEL]);
    const before = await stored(w);
    await deliver(w, [FAILED_AFTER]);
    expect(await stored(w)).toEqual(before);
    expect(await account(w)).toEqual(
      expect.objectContaining({ state: "cancelled", provider_time: START + 5 * MINUTE, ended_at: START + 5 * MINUTE, cancel_first: START + 5 * MINUTE }),
    );
    expect(await w.entries().status(HASH)).toEqual({ result: "cancelled" });
    done(w);
  });
});

describe("a cancel decides over a later-stamped state (ruling P7-80, R2; unit 5.5)", () => {
  async function pair(events: ProviderEvent[]): Promise<unknown> {
    const w = await world();
    await w.use("r0", 12_345);
    await w.clock(START + 10 * MINUTE);
    await deliver(w, events);
    const reserved = await w.reserve("r1");
    await w.clock(START + 2 * HOUR);
    const out = { reserved, status: await w.entries().status(HASH), rows: await stored(w), sent: sent(w) };
    done(w);
    return out;
  }

  test("a cancel stamped earlier than a stored active state applies, and both orders of the pair leave the same account row and the same metering account", async () => {
    const seen = [await pair([ACTIVE_AGAIN, CANCEL]), await pair([CANCEL, ACTIVE_AGAIN])];
    expect(seen[0]).toEqual(seen[1]);
    expect(seen[0]).toEqual(
      expect.objectContaining({ reserved: REFUSED, status: { result: "cancelled" }, sent: [[`${ACCOUNT}:${START / 1000}`, 1, (START + 5 * MINUTE) / 1000 - 1]] }),
    );
    const rows = (seen[0] as { rows: Record<string, Row[]> }).rows;
    expect(rows.account).toEqual([
      expect.objectContaining({
        state: "cancelled",
        subscription_id: "sub_1",
        period_start: PERIOD.start,
        period_end: PERIOD.end,
        provider_time: START + 5 * MINUTE,
        ended_at: START + 5 * MINUTE,
        cancel_first: START + 5 * MINUTE,
      }),
    ]);
    expect(rows.metering_account).toEqual([expect.objectContaining({ cut_kind: "cancel", cut_at: START + 5 * MINUTE, carry: 0, written_off: 2_345 })]);
  });
});

describe("the kept events of a pending checkout (ruling P7-80, R1 and R2; unit 5.5)", () => {
  async function made(events: ProviderEvent[]): Promise<unknown> {
    const w = await pending();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [...events, COMPLETED]);
    const rows = await stored(w);
    const out = { status: await w.entries().status(HASH), account: rows.account, metering: await metered(w), kept: rows.kept_event };
    done(w);
    return out;
  }

  const cancelled = {
    status: { result: "cancelled" },
    account: [
      expect.objectContaining({
        state: "cancelled",
        provider_time: START + 5 * MINUTE,
        ended_at: START + 5 * MINUTE,
        cancel_first: START + 5 * MINUTE,
      }),
    ],
    metering: [{ cut_kind: "cancel", cut_at: START + 5 * MINUTE, carry: 0, written_off: 0 }],
    kept: [],
  };

  test("a kept cancel stays cancelled under a second object of the older active state, and complete makes the account cancelled with its cut", async () => {
    expect(await made([ACTIVE, CANCEL, at("evt_active_repeat", 6, "active")])).toEqual(cancelled);
  });

  test("a kept cancel stays cancelled under a payment-failed event stamped later, and complete makes the account cancelled with its cut", async () => {
    expect(await made([CANCEL, FAILED_AFTER])).toEqual(cancelled);
  });

  test("a cancel stamped earlier than a kept active event is kept, and both orders make the same cancelled account with its cut", async () => {
    expect([await made([ACTIVE_AGAIN, CANCEL]), await made([CANCEL, ACTIVE_AGAIN])]).toEqual([cancelled, cancelled]);
  });
});

describe("every order of a small set of events, with every stored row read, the mark apart (unit 5.5's gate; ruling P7-80, part 6; ruling P7-100, J6)", () => {
  test("an account: an active event, a payment failure, a cancel, a second cancel and an active event stamped later than both end in one cancelled set of rows in all 120 orders, the mark apart, and each order leaves the mark and the count of lines that its order of delivery implies", { timeout: 120_000 }, async () => {
    const events = [ACTIVE, at("evt_failed", 3, "payment-failed"), CANCEL, at("evt_deleted", 6, "cancelled", { endedAt: null }), at("evt_active_latest", 8, "active")];
    const seen = new Set<string>();
    const marks = new Set<string>();
    let first: unknown;
    for (const order of orders(events)) {
      const w = await world();
      await w.use("r0", 12_345);
      await w.clock(START + 10 * MINUTE);
      const lines: string[] = [];
      await deliver(w, order, lines);
      const left = { mark: markOf(await account(w)), lines: lines.filter((line) => line === '{"event":"cancel-overrides-order"}').length };
      expect(left).toEqual(implied(order, { state: "active", time: null, periodEnd: null }));
      marks.add(JSON.stringify(left.mark));
      const reserved = await w.reserve("r1");
      await w.clock(START + 2 * HOUR);
      const end = { reserved, status: await w.entries().status(HASH), rows: await snapshot(w), sent: sent(w) };
      done(w);
      first ??= end;
      seen.add(JSON.stringify(end));
    }
    expect(seen.size).toBe(1);
    expect(first).toEqual(
      expect.objectContaining({ reserved: REFUSED, status: { result: "cancelled" }, sent: [[`${ACCOUNT}:${START / 1000}`, 1, (START + 5 * MINUTE) / 1000 - 1]] }),
    );
    const rows = (first as { rows: Record<string, Row[]> }).rows;
    expect(rows.account).toEqual([
      expect.objectContaining({ state: "cancelled", provider_time: START + 6 * MINUTE, ended_at: START + 5 * MINUTE, cancel_first: START + 5 * MINUTE }),
    ]);
    expect(rows.metering_account).toEqual([expect.objectContaining({ cut_kind: "cancel", cut_at: START + 5 * MINUTE, carry: 0, written_off: 2_345 })]);
    expect(marks).toEqual(
      new Set([
        JSON.stringify({ m1_count: 1, m1_newest: START + 8 * MINUTE, m2_count: 0, m2_newest: null }),
        JSON.stringify({ m1_count: 0, m1_newest: null, m2_count: 1, m2_newest: START + 5 * MINUTE }),
        JSON.stringify({ m1_count: 0, m1_newest: null, m2_count: 1, m2_newest: START + 6 * MINUTE }),
      ]),
    );
  });

  test("a pending checkout: its completed event, an active event, a cancel, an active event stamped later than it and a later payment failure end in one cancelled account with its cut in all 120 orders, the mark apart, and each order leaves the mark and the count of lines that its order of delivery implies", { timeout: 120_000 }, async () => {
    const events = [COMPLETED, ACTIVE, CANCEL, ACTIVE_AGAIN, FAILED_AFTER];
    const seen = new Set<string>();
    let first: unknown;
    for (const order of orders(events)) {
      const w = await pending();
      await w.clock(START + 10 * MINUTE);
      const lines: string[] = [];
      await deliver(w, order, lines);
      const left = { mark: markOf(await account(w)), lines: lines.filter((line) => line === '{"event":"cancel-overrides-order"}').length };
      expect(left).toEqual(implied(order, null));
      const end = { status: await w.entries().status(HASH), rows: await snapshot(w) };
      done(w);
      first ??= end;
      seen.add(JSON.stringify(end));
    }
    expect(seen.size).toBe(1);
    const rows = (first as { rows: Record<string, Row[]> }).rows;
    expect((first as { status: unknown }).status).toEqual({ result: "cancelled" });
    expect(rows.account).toEqual([
      expect.objectContaining({ state: "cancelled", provider_time: START + 5 * MINUTE, ended_at: START + 5 * MINUTE, cancel_first: START + 5 * MINUTE }),
    ]);
    expect(rows.metering_account).toEqual([expect.objectContaining({ cut_kind: "cancel", cut_at: START + 5 * MINUTE })]);
    expect(rows.kept_event).toEqual([]);
  });
});

describe("what stays as it was (pins of unit 5.5; ruling P7-80, part 5)", () => {
  test("a cancel delivered twice under one id, and again as a second object with the same stated end, gives one cut and one write-off", async () => {
    const w = await world();
    await w.use("r0", 12_345);
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [CANCEL]);
    const once = await snapshot(w);
    await deliver(w, [CANCEL]);
    expect(await snapshot(w)).toEqual(once);
    await deliver(w, [at("evt_cancel_again", 6, "cancelled", { endedAt: START + 5 * MINUTE })]);
    expect((await stored(w)).metering_account).toEqual(once.metering_account);
    expect(await account(w)).toEqual(expect.objectContaining({ state: "cancelled", provider_time: START + 6 * MINUTE, ended_at: START + 5 * MINUTE }));
    await w.clock(START + 2 * HOUR);
    expect({ cut: await metered(w), sent: sent(w) }).toEqual({
      cut: [{ cut_kind: "cancel", cut_at: START + 5 * MINUTE, carry: 0, written_off: 2_345 }],
      sent: [[`${ACCOUNT}:${START / 1000}`, 1, (START + 5 * MINUTE) / 1000 - 1]],
    });
    done(w);
  });

  test("an event that is not a cancel, stamped earlier than the stored cancel and arriving after it, changes nothing and writes no line", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [CANCEL], lines);
    const before = await stored(w);
    await deliver(w, [at("evt_active_older", 4, "active"), at("evt_failed_older", 3, "payment-failed")], lines);
    expect({ rows: await stored(w), lines }).toEqual({ rows: before, lines: [] });
    done(w);
  });

  test("after the first subscription ended, the events of a second subscription under a new id change nothing and the account stays cancelled", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [CANCEL]);
    const before = await stored(w);
    await deliver(w, [at("evt_new_active", 9, "active", { subscriptionId: "sub_2" }), at("evt_new_failed", 9.5, "payment-failed", { subscriptionId: "sub_2" })]);
    expect(await stored(w)).toEqual(before);
    expect(await account(w)).toEqual(expect.objectContaining({ state: "cancelled", subscription_id: "sub_1" }));
    expect(await w.reserve("r1")).toEqual(REFUSED);
    done(w);
  });

  test("a cancel at the end of the period and its taking back reach the account as active with no end and no cut, and the deleted event at the period's end gives one cut", async () => {
    const end = (START + 2 * HOUR) / 1000;
    const body = (id: string, type: string, created: number, object: Record<string, unknown>) =>
      reduceEvent({
        id,
        type,
        created,
        data: { object: { id: "sub_1", customer: "cus_1", metadata: { halvo_account: ACCOUNT }, items: { data: [{ current_period_start: START / 1000, current_period_end: end }] }, ...object } },
      }) as ProviderEvent;
    const scheduled = body("evt_scheduled", "customer.subscription.updated", START / 1000 + 60, { status: "active", cancel_at_period_end: true, ended_at: null });
    const takenBack = body("evt_taken_back", "customer.subscription.updated", START / 1000 + 120, { status: "active", cancel_at_period_end: false, ended_at: null });
    const ended = body("evt_ended", "customer.subscription.deleted", end, { status: "canceled", cancel_at_period_end: true, ended_at: end });
    expect([scheduled, takenBack].map((each) => [each.kind, each.kind === "subscription" ? [each.state, each.endedAt] : null])).toEqual([
      ["subscription", ["active", null]],
      ["subscription", ["active", null]],
    ]);
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [scheduled]);
    const afterScheduled = { status: await w.entries().status(HASH), cut: await metered(w) };
    await deliver(w, [takenBack]);
    const afterTakenBack = { status: await w.entries().status(HASH), cut: await metered(w), reserved: await w.reserve("r1") };
    await w.clock(START + 2 * HOUR);
    await deliver(w, [ended]);
    const afterEnd = { status: await w.entries().status(HASH), cut: ((await w.tables()).metering_account ?? []).map((row) => [row.cut_kind, row.cut_at]) };
    expect({ afterScheduled, afterTakenBack, afterEnd }).toEqual({
      afterScheduled: { status: { result: "active" }, cut: [{ cut_kind: null, cut_at: null, carry: 0, written_off: null }] },
      afterTakenBack: { status: { result: "active" }, cut: [{ cut_kind: null, cut_at: null, carry: 0, written_off: null }], reserved: expect.objectContaining({ outcome: "admitted" }) },
      afterEnd: { status: { result: "cancelled" }, cut: [["cancel", end * 1000]] },
    });
    done(w);
  });
});

describe("what the one-rule changes of unit 5.5 asked for", () => {
  test("an account seeded cancelled, whose row holds no provider time, stays cancelled under an event that is not a cancel", () => {
    const db = openSql();
    const accounts = makeAccounts();
    const seen = db.step((sql) => {
      accounts.init(sql);
      accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "cancelled", customerId: "cus_1" }, START);
      return { answer: accounts.applyEvent(sql, at("evt_active_after_seed", 1, "active"), START), state: accounts.subscription(sql)?.state };
    });
    db.close();
    expect(seen).toEqual({ answer: { kind: "none", overrode: true }, state: "cancelled" });
  });

  test("an older cancel that states an earlier end, kept beside a kept cancel, gives the account made from it the earlier end and the cut there", async () => {
    const w = await pending();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [CANCEL, at("evt_cancel_older", 4, "cancelled", { endedAt: START + 3 * MINUTE }), COMPLETED]);
    expect({ account: await account(w), cut: await metered(w) }).toEqual({
      account: expect.objectContaining({ state: "cancelled", provider_time: START + 5 * MINUTE, ended_at: START + 3 * MINUTE, cancel_first: START + 4 * MINUTE }),
      cut: [{ cut_kind: "cancel", cut_at: START + 3 * MINUTE, carry: 0, written_off: 0 }],
    });
    done(w);
  });
});

describe("the mark of a cancel that decided otherwise than the rule of newer (ruling P7-80, part 8; unit 5.5, the record)", () => {
  test("M1: each event that is not a cancel and is stamped later than the stored cancel counts in the mark, which keeps the newest provider time", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [CANCEL, ACTIVE_AGAIN, FAILED_AFTER, at("evt_active_between", 6.5, "active")]);
    expect(markOf(await account(w))).toEqual({ m1_count: 3, m1_newest: START + 7 * MINUTE, m2_count: 0, m2_newest: null });
    done(w);
  });

  test("M2: a cancel stamped earlier than the stored state counts in the mark with its own provider time", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [ACTIVE_AGAIN, CANCEL]);
    expect(markOf(await account(w))).toEqual({ m1_count: 0, m1_newest: null, m2_count: 1, m2_newest: START + 5 * MINUTE });
    done(w);
  });

  test("in each order of one pair exactly one of the two marks is set, and the rows agree apart from the mark", async () => {
    const end = async (events: ProviderEvent[]) => {
      const w = await world();
      await w.clock(START + 10 * MINUTE);
      await deliver(w, events);
      const out = { mark: markOf(await account(w)), rows: await stored(w) };
      done(w);
      return out;
    };
    const [laterFirst, cancelFirst] = [await end([ACTIVE_AGAIN, CANCEL]), await end([CANCEL, ACTIVE_AGAIN])];
    expect(laterFirst.rows).toEqual(cancelFirst.rows);
    expect([laterFirst.mark, cancelFirst.mark]).toEqual([
      { m1_count: 0, m1_newest: null, m2_count: 1, m2_newest: START + 5 * MINUTE },
      { m1_count: 1, m1_newest: START + 6 * MINUTE, m2_count: 0, m2_newest: null },
    ]);
  });

  test("neither case leaves a mark: an event of the same second, an older event after the cancel, another subscription, a repeated id and a newer cancel", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [
      CANCEL,
      at("evt_same_second", 5, "active"),
      at("evt_older", 4, "payment-failed"),
      at("evt_other_subscription", 9, "active", { subscriptionId: "sub_2" }),
      CANCEL,
      at("evt_newer_cancel", 8, "cancelled"),
    ]);
    expect(markOf(await account(w))).toEqual(NO_MARK);
    done(w);
  });

  test("a row seeded cancelled, which holds no provider time, counts an event that is not a cancel in M1", () => {
    const db = openSql();
    const accounts = makeAccounts();
    const row = db.step((sql) => {
      accounts.init(sql);
      accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "cancelled", customerId: "cus_1" }, START);
      accounts.applyEvent(sql, at("evt_active_after_seed", 1, "active"), START);
      return sql.exec("SELECT * FROM account")[0];
    });
    db.close();
    expect(markOf(row)).toEqual({ m1_count: 1, m1_newest: START + MINUTE, m2_count: 0, m2_newest: null });
  });

  test("a kept event keeps the mark of both cases, and complete carries it into the account", async () => {
    const w = await pending();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [ACTIVE_AGAIN, CANCEL, FAILED_AFTER]);
    const kept = ((await w.tables()).kept_event ?? []).map(markOf);
    await deliver(w, [COMPLETED]);
    const mark = { m1_count: 1, m1_newest: START + 7 * MINUTE, m2_count: 1, m2_newest: START + 5 * MINUTE };
    expect({ kept, account: markOf(await account(w)) }).toEqual({ kept: [mark], account: mark });
    done(w);
  });

  test("an account made with no kept event, and one seeded, start with no mark", async () => {
    const made = await pending();
    await made.clock(START + 10 * MINUTE);
    await deliver(made, [COMPLETED]);
    const seeded = await world();
    expect([markOf(await account(made)), markOf(await account(seeded))]).toEqual([NO_MARK, NO_MARK]);
    done(made);
    done(seeded);
  });
});

describe("overrode on the answer of applyEvent (ruling P7-80, part 8; contract 3.2, the type Applied)", () => {
  function answers(events: ProviderEvent[], checkout: boolean): unknown[] {
    const db = openSql();
    const accounts = makeAccounts();
    const seen = db.step((sql) => {
      accounts.init(sql);
      if (checkout) {
        accounts.keepPending(sql, { accountId: ACCOUNT, credentialHash: HASH, cap: { amount: 1000, period: "month" }, sessionId: SESSION.sessionId, url: CHECKOUT.url, expiresAt: START + HOUR }, START);
      } else {
        accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "active", customerId: "cus_1" }, START);
      }
      return events.map((event) => accounts.applyEvent(sql, event, START + 10 * MINUTE));
    });
    db.close();
    return seen;
  }

  test("on an account it is true on none for M1 and on state for M2, and false on every other answer", () => {
    const events = [ACTIVE_AGAIN, CANCEL, at("evt_active_later", 7, "active"), at("evt_older", 4, "active"), at("evt_cancel_newer", 8, "cancelled"), CANCEL];
    expect(answers(events, false)).toEqual([
      { kind: "state", state: "active", was: "active", period: PERIOD, overrode: false },
      { kind: "state", state: "cancelled", was: "active", period: PERIOD, overrode: true },
      { kind: "none", overrode: true },
      { kind: "none", overrode: false },
      { kind: "state", state: "cancelled", was: "cancelled", period: PERIOD, overrode: false },
      { kind: "none", overrode: false },
    ]);
  });

  test("on the kept events of a pending checkout it is true on kept for either case, and false on every other answer", () => {
    const events = [ACTIVE_AGAIN, CANCEL, FAILED_AFTER, at("evt_older", 4, "active"), at("evt_other", 3, "active", { subscriptionId: "sub_2" })];
    expect(answers(events, true)).toEqual([
      { kind: "kept", overrode: false },
      { kind: "kept", overrode: true },
      { kind: "kept", overrode: true },
      { kind: "none", overrode: false },
      { kind: "kept", overrode: false },
    ]);
  });

  test("an event of another subscription, an event for an id that holds nothing and a completed event with no checkout of its session answer none with overrode false", () => {
    const db = openSql();
    const accounts = makeAccounts();
    const seen = db.step((sql) => {
      accounts.init(sql);
      const nothing = accounts.applyEvent(sql, CANCEL, START);
      accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "active", customerId: "cus_1" }, START);
      accounts.applyEvent(sql, at("evt_cancel_bind", 5, "cancelled"), START);
      return [nothing, accounts.applyEvent(sql, at("evt_other", 9, "active", { subscriptionId: "sub_2" }), START), accounts.applyEvent(sql, COMPLETED, START)];
    });
    db.close();
    expect(seen).toEqual(Array(3).fill({ kind: "none", overrode: false }));
  });
});

describe("the line cancel-overrides-order (ruling P7-80, part 8; contracts 1.4 and 3.4, by the Revision of 2026-11-13)", () => {
  const LINE = '{"event":"cancel-overrides-order"}';

  test("on an account the skeleton writes one line after the step for each event of M1 and for no other event", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [ACTIVE, CANCEL, at("evt_older", 4, "active"), ACTIVE_AGAIN, FAILED_AFTER, CANCEL, at("evt_cancel_newer", 8, "cancelled")], lines);
    expect(lines).toEqual([LINE, LINE]);
    done(w);
  });

  test("on an account the skeleton writes one line after the step for a cancel of M2, which also makes the cut", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [ACTIVE_AGAIN, CANCEL], lines);
    expect({ lines, cut: await metered(w) }).toEqual({ lines: [LINE], cut: [{ cut_kind: "cancel", cut_at: START + 5 * MINUTE, carry: 0, written_off: 0 }] });
    done(w);
  });

  test("on the kept events of a pending checkout the skeleton writes one line for each case, and complete writes none", async () => {
    const w = await pending();
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [ACTIVE_AGAIN, CANCEL, FAILED_AFTER, at("evt_older", 4, "active")], lines);
    const kept = [...lines];
    await deliver(w, [COMPLETED], lines);
    expect({ kept, all: lines }).toEqual({ kept: [LINE, LINE], all: [LINE, LINE] });
    done(w);
  });

  test("an event of another subscription and a repeated event id write no line", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [CANCEL, ACTIVE_AGAIN], lines);
    await deliver(w, [ACTIVE_AGAIN, at("evt_other_subscription", 9, "active", { subscriptionId: "sub_2" })], lines);
    expect(lines).toEqual([LINE]);
    done(w);
  });
});

describe("what the one-rule changes of unit 5.5's record asked for", () => {
  const LINE = '{"event":"cancel-overrides-order"}';

  test("a cancel in the same second as a stored active state is neither case: no mark and no line", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [at("evt_active_same_second", 5, "active"), CANCEL], lines);
    expect({ lines, mark: markOf(await account(w)), state: (await account(w))?.state }).toEqual({ lines: [], mark: NO_MARK, state: "cancelled" });
    done(w);
  });

  test("an account that is not metered, with no customer id, still gets the line for a cancel of M2", async () => {
    const w = await world({ customer: false });
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [ACTIVE_AGAIN, CANCEL], lines);
    expect({ lines, mark: markOf(await account(w)) }).toEqual({ lines: [LINE], mark: { m1_count: 0, m1_newest: null, m2_count: 1, m2_newest: START + 5 * MINUTE } });
    done(w);
  });

  test("an older cancel that states an earlier end over a cancelled row moves the cut back and writes no line", async () => {
    const w = await world();
    await w.use("r0", 12_345);
    await w.clock(START + 10 * MINUTE);
    const lines: string[] = [];
    await deliver(w, [CANCEL, at("evt_cancel_earlier_end", 4, "cancelled", { endedAt: START + 3 * MINUTE })], lines);
    expect({ lines, mark: markOf(await account(w)), cut: await metered(w) }).toEqual({
      lines: [],
      mark: NO_MARK,
      cut: [{ cut_kind: "cancel", cut_at: START + 3 * MINUTE, carry: 0, written_off: null }],
    });
    done(w);
  });
});

describe("the line after the commit, and events that are no case (ruling P7-100, J4 and J5)", () => {
  const LINE = '{"event":"cancel-overrides-order"}';
  const NO_CASE = [ACTIVE, at("evt_failed_no_case", 3, "payment-failed"), ACTIVE_AGAIN, FAILED_AFTER];

  test("J4: the line is written after the step's commit: a stop between the commit and the line keeps the mark and writes no line, and a redelivery of the id writes none", async () => {
    const w = await world();
    await w.clock(START + 10 * MINUTE);
    await deliver(w, [CANCEL]);
    const commit = w.host.step;
    let armed = true;
    w.host.step = ((run: Parameters<typeof commit>[0]) => {
      const value = commit(run);
      if (armed) {
        armed = false;
        const error = new Error("the object stopped after the commit");
        error.name = "ObjectStoppedError";
        throw error;
      }
      return value;
    }) as typeof commit;
    w.restart();
    const lines: string[] = [];
    let thrown = "";
    await quiet(lines, () => w.entries().webhookEvent(ACTIVE_AGAIN)).catch((error: Error) => {
      thrown = error.name;
    });
    const mark = markOf(await account(w));
    await deliver(w, [ACTIVE_AGAIN], lines);
    const once = { m1_count: 1, m1_newest: START + 6 * MINUTE, m2_count: 0, m2_newest: null };
    expect({ thrown, lines, mark, again: markOf(await account(w)) }).toEqual({ thrown: "ObjectStoppedError", lines: [], mark: once, again: once });
    done(w);
  });

  test("J5: in all 24 orders four events that are not a cancel leave no mark on an account, on each kept event and on the account made from them, and write no line", { timeout: 120_000 }, async () => {
    const seen: unknown[] = [];
    for (const order of orders(NO_CASE)) {
      const lines: string[] = [];
      const seeded = await world();
      await seeded.clock(START + 10 * MINUTE);
      await deliver(seeded, order, lines);
      const onTheAccount = markOf(await account(seeded));
      done(seeded);
      const kept = await pending();
      await kept.clock(START + 10 * MINUTE);
      await deliver(kept, order, lines);
      const onTheKeptEvents = ((await kept.tables()).kept_event ?? []).map(markOf);
      await deliver(kept, [COMPLETED], lines);
      const onTheMadeAccount = markOf(await account(kept));
      done(kept);
      seen.push({ onTheAccount, onTheKeptEvents, onTheMadeAccount, lines: lines.filter((line) => line === LINE) });
    }
    expect(seen).toEqual(Array(24).fill({ onTheAccount: NO_MARK, onTheKeptEvents: [NO_MARK], onTheMadeAccount: NO_MARK, lines: [] }));
  });
});
