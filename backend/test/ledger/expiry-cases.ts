import { costOf } from "../../src/ledger/cost.ts";
import { type Ledger, makeLedger, type Price } from "../../src/ledger/index.ts";
import type { Sql, Step } from "../../src/types.ts";

const LOW: Price = { input: 3_100_000, output: 15_700_000 };
const LIFE_MS = 3000;
const WEEK_MS = 604_800_000;
const ACTIVE = { active: true, interval: 0 };
const USAGE = { inputTokens: 100, outputTokens: 100 };
const UNMARKED = 40_000;
const MARKED = 50_000;
const LATE_COST = costOf(LOW, USAGE);

export interface ExpiryCase {
  name: string;
  run(step: Step): unknown;
  expected: unknown;
}

const holdOf = (requestId: string, amount: number) => ({ requestId, tier: "low_cost_hosted" as const, amount, price: LOW });

function begin(step: Step): Ledger {
  const ledger = makeLedger({ holdLifeMs: LIFE_MS });
  step((sql) => {
    ledger.init(sql);
    ledger.setCap(sql, { amount: 10, period: "week" }, 0);
    ledger.reserve(sql, holdOf("unmarked", UNMARKED), 0, ACTIVE);
    ledger.reserve(sql, holdOf("marked", MARKED), 0, ACTIVE);
    ledger.dispatch(sql, "marked", 0);
  });
  return ledger;
}

function rowOf(sql: Sql, requestId: string) {
  const row = sql.exec("SELECT state, dispatched, ended_at, ended_by, cost, overrun, late_at FROM ledger_holds WHERE request_id = ?", requestId)[0];
  return row === undefined ? null : { ...row };
}

function storedOf(step: Step, ledger: Ledger) {
  return step((sql) => ({
    unmarked: rowOf(sql, "unmarked"),
    marked: rowOf(sql, "marked"),
    periods: sql.exec("SELECT reported, unreported, open FROM ledger_periods").map((row) => ({ ...row })),
    interval: ledger.intervalTotals(sql, 0),
    open: ledger.openHolds(sql),
    due: ledger.nextDue(sql),
  }));
}

const endedAt = (at: number, by: string) => ({
  unmarked: { state: "voided", dispatched: 0, ended_at: at, ended_by: by, cost: null, overrun: 0, late_at: null },
  marked: { state: "expired", dispatched: 1, ended_at: at, ended_by: by, cost: null, overrun: 0, late_at: null },
  periods: [{ reported: 0, unreported: MARKED, open: 0 }],
  interval: { reported: 0, unreported: MARKED, open: 0 },
  open: 0,
  due: null,
});

export const expiryCases: ExpiryCase[] = [
  {
    name: "the alarm's expire voids a due hold without the dispatch mark, which counts nothing, and expires a marked one at its full amount",
    run(step) {
      const ledger = begin(step);
      const early = step((sql) => ledger.expire(sql, LIFE_MS - 1));
      const atExpiry = step((sql) => ledger.expire(sql, LIFE_MS));
      return { early, atExpiry, stored: storedOf(step, ledger), cap: step((sql) => ledger.cap(sql, LIFE_MS)) };
    },
    expected: {
      early: 0,
      atExpiry: 2,
      stored: endedAt(LIFE_MS, "expire"),
      cap: { cap: { amount: 10, period: "week" }, spent: MARKED, held: 0, periodEnd: WEEK_MS },
    },
  },
  {
    name: "a reserve step at the expiry ends both due holds first, so the amount of the hold without the mark is room again under the cap",
    run(step) {
      const ledger = begin(step);
      const before = step((sql) => ledger.reserve(sql, holdOf("next", MARKED), LIFE_MS - 1, ACTIVE));
      const after = step((sql) => ledger.reserve(sql, holdOf("next", MARKED), LIFE_MS, ACTIVE));
      return { before, after, stored: storedOf(step, ledger) };
    },
    expected: {
      before: { outcome: "refused", reason: "over-cap" },
      after: { outcome: "admitted", receipt: { requestId: "next", amount: MARKED, now: LIFE_MS, expiresAt: 2 * LIFE_MS } },
      stored: {
        ...endedAt(LIFE_MS, "reserve"),
        periods: [{ reported: 0, unreported: MARKED, open: MARKED }],
        interval: { reported: 0, unreported: MARKED, open: 1 },
        open: 1,
        due: 2 * LIFE_MS,
      },
    },
  },
  {
    name: "a settle and a void of a hold that was voided at its expiry answer already-ended and change nothing; a repeated reserve of it answers its state",
    run(step) {
      const ledger = begin(step);
      step((sql) => ledger.expire(sql, LIFE_MS));
      const before = storedOf(step, ledger);
      const settle = step((sql) => ledger.settle(sql, "unmarked", USAGE, LIFE_MS + 1));
      const voided = step((sql) => ledger.void(sql, "unmarked", LIFE_MS + 2));
      const dispatch = step((sql) => ledger.dispatch(sql, "unmarked", LIFE_MS + 3));
      const repeat = step((sql) => ledger.reserve(sql, holdOf("unmarked", UNMARKED), LIFE_MS + 4, ACTIVE));
      const after = storedOf(step, ledger);
      return { settle, voided, dispatch, repeat, unchanged: JSON.stringify(after) === JSON.stringify(before) };
    },
    expected: {
      settle: { outcome: "refused", reason: "already-ended" },
      voided: { outcome: "refused", reason: "already-ended" },
      dispatch: "stop",
      repeat: { outcome: "repeat", state: "voided", dispatched: false, receipt: { requestId: "unmarked", amount: UNMARKED, now: LIFE_MS + 4, expiresAt: LIFE_MS } },
      unchanged: true,
    },
  },
  {
    name: "a hold with the dispatch mark expires at its full amount: a void of it answers already-ended, and a settle after the expiry stores its cost in no total",
    run(step) {
      const ledger = begin(step);
      step((sql) => ledger.expire(sql, LIFE_MS));
      const voided = step((sql) => ledger.void(sql, "marked", LIFE_MS + 1));
      const settle = step((sql) => ledger.settle(sql, "marked", USAGE, LIFE_MS + 2));
      return { voided, settle, stored: storedOf(step, ledger) };
    },
    expected: {
      voided: { outcome: "refused", reason: "already-ended" },
      settle: { outcome: "expired", cost: LATE_COST, overrun: 0 },
      stored: {
        ...endedAt(LIFE_MS, "expire"),
        marked: { state: "expired", dispatched: 1, ended_at: LIFE_MS, ended_by: "expire", cost: LATE_COST, overrun: 0, late_at: LIFE_MS + 2 },
      },
    },
  },
  {
    name: "a hold that the gateway voided before its expiry answers voided to a repeated void, also after its expiry time has passed",
    run(step) {
      const ledger = begin(step);
      const first = step((sql) => ledger.void(sql, "unmarked", 1000));
      const again = step((sql) => ledger.void(sql, "unmarked", 1001));
      step((sql) => ledger.expire(sql, LIFE_MS));
      const late = step((sql) => ledger.void(sql, "unmarked", LIFE_MS + 1));
      const settle = step((sql) => ledger.settle(sql, "unmarked", USAGE, LIFE_MS + 2));
      return { first, again, late, settle, stored: storedOf(step, ledger) };
    },
    expected: {
      first: { outcome: "voided" },
      again: { outcome: "voided" },
      late: { outcome: "voided" },
      settle: { outcome: "refused", reason: "already-ended" },
      stored: {
        ...endedAt(LIFE_MS, "expire"),
        unmarked: { state: "voided", dispatched: 0, ended_at: 1000, ended_by: "void", cost: null, overrun: 0, late_at: null },
      },
    },
  },
  {
    name: "a void one millisecond before the expiry is a void by the call, whose repeat answers voided; an expiry by a later step stores that step's time as the end",
    run(step) {
      const ledger = begin(step);
      const first = step((sql) => ledger.void(sql, "unmarked", LIFE_MS - 1));
      const late = step((sql) => ledger.expire(sql, LIFE_MS + 500));
      const again = step((sql) => ledger.void(sql, "unmarked", LIFE_MS + 501));
      return { first, late, again, stored: storedOf(step, ledger) };
    },
    expected: {
      first: { outcome: "voided" },
      late: 1,
      again: { outcome: "voided" },
      stored: {
        ...endedAt(LIFE_MS + 500, "expire"),
        unmarked: { state: "voided", dispatched: 0, ended_at: LIFE_MS - 1, ended_by: "void", cost: null, overrun: 0, late_at: null },
      },
    },
  },
  {
    name: "a hold without the mark that a step ends after its expiry is voided at that step's time, and a void of it answers already-ended",
    run(step) {
      const ledger = begin(step);
      const late = step((sql) => ledger.expire(sql, LIFE_MS + 500));
      const voided = step((sql) => ledger.void(sql, "unmarked", LIFE_MS + 501));
      return { late, voided, stored: storedOf(step, ledger) };
    },
    expected: { late: 2, voided: { outcome: "refused", reason: "already-ended" }, stored: endedAt(LIFE_MS + 500, "expire") },
  },
  {
    name: "a dispatch at the expiry finds the hold ended: it answers stop, and the hold without the mark is voided by that step",
    run(step) {
      const ledger = begin(step);
      const dispatch = step((sql) => ledger.dispatch(sql, "unmarked", LIFE_MS));
      return { dispatch, stored: storedOf(step, ledger) };
    },
    expected: { dispatch: "stop", stored: endedAt(LIFE_MS, "dispatch") },
  },
];
