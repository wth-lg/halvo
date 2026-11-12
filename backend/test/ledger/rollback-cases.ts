import { type Ledger, makeLedger, type Price } from "../../src/ledger/index.ts";
import type { Sql, Step } from "../../src/types.ts";

const LOW: Price = { input: 3_100_000, output: 15_700_000 };
const TABLES = ["ledger_state", "ledger_cap", "ledger_periods", "ledger_holds"];
export const FAULT = "a fault in the middle of the step";

export interface RollbackCase {
  step: string;
  failAt: number;
  run(sql: Sql, ledger: Ledger): unknown;
}

const hold = (requestId: string, amount: number) => ({ requestId, tier: "low_cost_hosted" as const, amount, price: LOW });

export const cases: RollbackCase[] = [
  { step: "reserve", failAt: 3, run: (sql, ledger) => ledger.reserve(sql, hold("new", 100), 3500, { active: true, interval: 0 }) },
  { step: "dispatch", failAt: 3, run: (sql, ledger) => ledger.dispatch(sql, "live", 3500) },
  { step: "settle", failAt: 4, run: (sql, ledger) => ledger.settle(sql, "live", { inputTokens: 10, outputTokens: 10 }, 3500) },
  { step: "void", failAt: 4, run: (sql, ledger) => ledger.void(sql, "live", 3500) },
  { step: "expire", failAt: 3, run: (sql, ledger) => ledger.expire(sql, 6000) },
  { step: "setCap", failAt: 3, run: (sql, ledger) => ledger.setCap(sql, { amount: 3, period: "month" }, 3500) },
  { step: "cap", failAt: 2, run: (sql, ledger) => ledger.cap(sql, 3500) },
];

export function setUp(step: Step, ledger: Ledger): void {
  step((sql) => {
    ledger.init(sql);
    ledger.setCap(sql, { amount: 10, period: "week" }, 0);
    ledger.reserve(sql, hold("due", 100), 0, { active: true, interval: 0 });
    ledger.reserve(sql, hold("live", 200), 2000, { active: true, interval: 0 });
  });
}

export function dump(step: Step): string {
  return step((sql) => JSON.stringify(TABLES.map((table) => sql.exec(`SELECT * FROM ${table} ORDER BY rowid`))));
}

function faulty(sql: Sql, failAt: number): { sql: Sql; writes(): number } {
  let writes = 0;
  return {
    sql: {
      exec(query, ...bindings) {
        if (/^(INSERT|UPDATE|DELETE) /.test(query)) {
          writes += 1;
          if (writes === failAt) throw new Error(FAULT);
        }
        return sql.exec(query, ...bindings);
      },
    },
    writes: () => writes,
  };
}

export function runCase(step: Step, each: RollbackCase): { before: string; afterFault: string; fault: string | null; writesBeforeFault: number; afterClean: string } {
  const ledger = makeLedger({ holdLifeMs: 3000 });
  setUp(step, ledger);
  const before = dump(step);
  let fault: string | null = null;
  let writesBeforeFault = 0;
  try {
    step((sql) => {
      const wrapped = faulty(sql, each.failAt);
      try {
        return each.run(wrapped.sql, ledger);
      } finally {
        writesBeforeFault = wrapped.writes() - 1;
      }
    });
  } catch (error) {
    fault = (error as Error).message;
  }
  const afterFault = dump(step);
  step((sql) => each.run(sql, ledger));
  return { before, afterFault, fault, writesBeforeFault, afterClean: dump(step) };
}

const SUM_PAST_THE_RANGE = { inputTokens: 0, outputTokens: 300_000_000_000_000 };
const RANGE_COST = 4_710_000_000_000_000;
const WEEK_MS = 604_800_000;

export const rangeExpected = {
  first: { outcome: "settled", cost: RANGE_COST, overrun: RANGE_COST - 100, repeat: false },
  second: "RangeError",
  beforeExpiry: {
    state: "open",
    cap: { cap: { amount: 10, period: "week" }, spent: RANGE_COST, held: 100, periodEnd: WEEK_MS },
    reserve: { outcome: "refused", reason: "over-cap" },
  },
  afterExpiry: {
    state: "expired",
    cap: { cap: { amount: 10, period: "week" }, spent: RANGE_COST + 100, held: 0, periodEnd: WEEK_MS },
    reserve: { outcome: "refused", reason: "over-cap" },
    periods: [{ reported: RANGE_COST, unreported: 100, open: 0 }],
  },
};

export function rangeCase(step: Step): unknown {
  const ledger = makeLedger({ holdLifeMs: 3000 });
  const active = { active: true, interval: 0 };
  step((sql) => {
    ledger.init(sql);
    ledger.setCap(sql, { amount: 10, period: "week" }, 0);
    ledger.reserve(sql, hold("first", 100), 0, active);
    ledger.reserve(sql, hold("second", 100), 0, active);
  });
  const first = step((sql) => ledger.settle(sql, "first", SUM_PAST_THE_RANGE, 1000));
  let second = "accepted";
  try {
    step((sql) => ledger.settle(sql, "second", SUM_PAST_THE_RANGE, 1000));
  } catch (error) {
    second = (error as Error).name;
  }
  const stateOf = (sql: Sql) => sql.exec("SELECT state FROM ledger_holds WHERE request_id = ?", "second")[0]?.state;
  const beforeExpiry = step((sql) => ({ state: stateOf(sql), cap: ledger.cap(sql, 1000), reserve: ledger.reserve(sql, hold("third", 100), 1000, active) }));
  const afterExpiry = step((sql) => {
    ledger.expire(sql, 3000);
    return {
      state: stateOf(sql),
      cap: ledger.cap(sql, 3000),
      reserve: ledger.reserve(sql, hold("fourth", 100), 3000, active),
      periods: sql.exec("SELECT reported, unreported, open FROM ledger_periods").map((row) => ({ ...row })),
    };
  });
  return { first, second, beforeExpiry, afterExpiry };
}

export interface ReadCase {
  name: string;
  run(sql: Sql, ledger: Ledger): unknown;
  expected: unknown;
}

export const reads: ReadCase[] = [
  { name: "intervalTotals with two open holds", run: (sql, ledger) => ledger.intervalTotals(sql, 0), expected: { reported: 0, unreported: 0, open: 2 } },
  {
    name: "intervalTotals after a settle and an expiry, and for an interval with no hold",
    run: (sql, ledger) => {
      ledger.settle(sql, "live", { inputTokens: 10, outputTokens: 10 }, 3500);
      return [ledger.intervalTotals(sql, 0), ledger.intervalTotals(sql, 1)];
    },
    expected: [
      { reported: 188, unreported: 100, open: 0 },
      { reported: 0, unreported: 0, open: 0 },
    ],
  },
  {
    name: "openHolds before and after both holds expire",
    run: (sql, ledger) => {
      const before = ledger.openHolds(sql);
      ledger.expire(sql, 6000);
      return [before, ledger.openHolds(sql)];
    },
    expected: [2, 0],
  },
];

const UNIT: Price = { input: 1_000_000, output: 1_000_000 };
const MAX = Number.MAX_SAFE_INTEGER;
const tokens = (inputTokens: number) => ({ inputTokens, outputTokens: 0 });
const settledAt = (cost: number) => ({ outcome: "settled", cost, overrun: cost - 100, repeat: false });
const periodsOf = (sql: Sql) => sql.exec("SELECT reported, unreported, open FROM ledger_periods ORDER BY rowid").map((row) => ({ ...row }));
const stateOfHold = (sql: Sql, requestId: string) => sql.exec("SELECT state FROM ledger_holds WHERE request_id = ?", requestId)[0]?.state;

function attempt(run: () => unknown): unknown {
  try {
    return run();
  } catch (error) {
    return { threw: (error as Error).name };
  }
}

function startWith(step: Step, holds: [string, number][]): Ledger {
  const ledger = makeLedger({ holdLifeMs: 3000 });
  step((sql) => {
    ledger.init(sql);
    ledger.setCap(sql, { amount: 10, period: "week" }, 0);
    for (const [requestId, at] of holds) ledger.reserve(sql, { requestId, tier: "low_cost_hosted", amount: 100, price: UNIT }, at, { active: true, interval: 0 });
  });
  return ledger;
}

export interface RangeEdgeCase {
  name: string;
  run(step: Step): unknown;
  expected: unknown;
}

export const rangeEdges: RangeEdgeCase[] = [
  {
    name: "B1: a settle that takes the counted spend of its period to exactly 2^53 - 1 is accepted",
    run(step) {
      const ledger = startWith(step, [["a", 0], ["b", 0]]);
      const a = attempt(() => step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), 1000)));
      const b = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(MAX - RANGE_COST), 1000)));
      return { a, b, periods: step(periodsOf), cap: step((sql) => ledger.cap(sql, 1000)) };
    },
    expected: {
      a: settledAt(RANGE_COST),
      b: settledAt(MAX - RANGE_COST),
      periods: [{ reported: MAX, unreported: 0, open: 0 }],
      cap: { cap: { amount: 10, period: "week" }, spent: MAX, held: 0, periodEnd: WEEK_MS },
    },
  },
  {
    name: "B2: a settle that takes it one above 2^53 - 1 throws, and the hold stays open",
    run(step) {
      const ledger = startWith(step, [["a", 0], ["b", 0]]);
      const a = attempt(() => step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), 1000)));
      const b = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(MAX - RANGE_COST + 1), 1000)));
      return { a, b, state: step((sql) => stateOfHold(sql, "b")), periods: step(periodsOf) };
    },
    expected: { a: settledAt(RANGE_COST), b: { threw: "RangeError" }, state: "open", periods: [{ reported: RANGE_COST, unreported: 0, open: 100 }] },
  },
  {
    name: "B3: with a third hold of 100 open, the settle one above the edge throws and the settle to the edge is accepted",
    run(step) {
      const ledger = startWith(step, [["a", 0], ["b", 0], ["c", 0]]);
      const a = attempt(() => step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), 1000)));
      const oneOver = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(MAX - RANGE_COST - 99), 1000)));
      const atTheEdge = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(MAX - RANGE_COST - 100), 1000)));
      return { a, oneOver, atTheEdge, periods: step(periodsOf) };
    },
    expected: {
      a: settledAt(RANGE_COST),
      oneOver: { threw: "RangeError" },
      atTheEdge: settledAt(MAX - RANGE_COST - 100),
      periods: [{ reported: MAX - 100, unreported: 0, open: 100 }],
    },
  },
  {
    name: "B4: with the third hold expired (unreported 100), the settle one above the edge throws and the settle to the edge is accepted",
    run(step) {
      const ledger = startWith(step, [["old", 0], ["new", 2000]]);
      const expired = attempt(() => step((sql) => ledger.expire(sql, 3000)));
      const oneOver = attempt(() => step((sql) => ledger.settle(sql, "new", tokens(MAX - 99), 3500)));
      const atTheEdge = attempt(() => step((sql) => ledger.settle(sql, "new", tokens(MAX - 100), 3500)));
      return { expired, oneOver, atTheEdge, periods: step(periodsOf), cap: step((sql) => ledger.cap(sql, 3500)) };
    },
    expected: {
      expired: 1,
      oneOver: { threw: "RangeError" },
      atTheEdge: settledAt(MAX - 100),
      periods: [{ reported: MAX - 100, unreported: 100, open: 0 }],
      cap: { cap: { amount: 10, period: "week" }, spent: MAX, held: 0, periodEnd: WEEK_MS },
    },
  },
  {
    name: "B-17: a settle that would take the reported sum of its sync interval, across two cap periods each in range, above 2^53 - 1 throws a RangeError and rolls back; the hold stays open, expires at its amount, and the interval still answers",
    run(step) {
      const ledger = acrossTwoPeriods(step, 100);
      const a = attempt(() => step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), WEEK_MS - 500)));
      const b = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(RANGE_COST), WEEK_MS + 1500)));
      const before = step((sql) => ({ state: stateOfHold(sql, "b"), totals: ledger.intervalTotals(sql, ACROSS) }));
      const expired = step((sql) => ledger.expire(sql, WEEK_MS + 5000));
      const after = step((sql) => ({ state: stateOfHold(sql, "b"), totals: ledger.intervalTotals(sql, ACROSS), periods: periodsOf(sql) }));
      return { a, b, before, expired, after };
    },
    expected: {
      a: settledAt(RANGE_COST),
      b: { threw: "RangeError" },
      before: { state: "open", totals: { reported: RANGE_COST, unreported: 0, open: 1 } },
      expired: 1,
      after: {
        state: "expired",
        totals: { reported: RANGE_COST, unreported: 100, open: 0 },
        periods: [
          { reported: RANGE_COST, unreported: 0, open: 0 },
          { reported: 0, unreported: 100, open: 0 },
        ],
      },
    },
  },
  {
    name: "B-17 edge: a settle that takes the reported sum of its sync interval across two periods to exactly 2^53 - 1 is taken, and intervalTotals answers that sum",
    run(step) {
      const ledger = acrossTwoPeriods(step, 100);
      step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), WEEK_MS - 500));
      const b = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(MAX - RANGE_COST), WEEK_MS + 1500)));
      return { b, totals: attempt(() => step((sql) => ledger.intervalTotals(sql, ACROSS))) };
    },
    expected: { b: settledAt(MAX - RANGE_COST), totals: { reported: MAX, unreported: 0, open: 0 } },
  },
  {
    name: "B-17 edge: a settle one above 2^53 - 1 across two periods throws a RangeError, the hold stays open, and an interval of no hold answers zeros",
    run(step) {
      const ledger = acrossTwoPeriods(step, 100);
      step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), WEEK_MS - 500));
      const b = attempt(() => step((sql) => ledger.settle(sql, "b", tokens(MAX - RANGE_COST + 1), WEEK_MS + 1500)));
      return {
        b,
        state: step((sql) => stateOfHold(sql, "b")),
        across: step((sql) => ledger.intervalTotals(sql, ACROSS)),
        other: step((sql) => ledger.intervalTotals(sql, ACROSS + 1000)),
      };
    },
    expected: {
      b: { threw: "RangeError" },
      state: "open",
      across: { reported: RANGE_COST, unreported: 0, open: 1 },
      other: { reported: 0, unreported: 0, open: 0 },
    },
  },
  {
    name: "intervalTotals guard: a settled row past the bound that the ledger never writes makes intervalTotals throw a RangeError",
    run(step) {
      const ledger = acrossTwoPeriods(step, 100);
      step((sql) => ledger.settle(sql, "a", tokens(RANGE_COST), WEEK_MS - 500));
      step((sql) => ledger.settle(sql, "b", tokens(100), WEEK_MS + 1500));
      step((sql) => sql.exec("UPDATE ledger_holds SET cost = ? WHERE request_id = ?", RANGE_COST, "b"));
      return attempt(() => step((sql) => ledger.intervalTotals(sql, ACROSS)));
    },
    expected: { threw: "RangeError" },
  },
  {
    name: "intervalTotals guard: the expired amounts of one sync interval in two periods sum past 2^53 - 1, and intervalTotals throws a RangeError",
    run(step) {
      const ledger = acrossTwoPeriods(step, RANGE_COST);
      const expired = step((sql) => ledger.expire(sql, WEEK_MS + 5000));
      return { expired, periods: step(periodsOf), totals: attempt(() => step((sql) => ledger.intervalTotals(sql, ACROSS))) };
    },
    expected: {
      expired: 2,
      periods: [
        { reported: 0, unreported: RANGE_COST, open: 0 },
        { reported: 0, unreported: RANGE_COST, open: 0 },
      ],
      totals: { threw: "RangeError" },
    },
  },
];

const ACROSS = WEEK_MS - 1000;

function acrossTwoPeriods(step: Step, amount: number): Ledger {
  const ledger = makeLedger({ holdLifeMs: 3000 });
  const interval = { active: true, interval: ACROSS };
  step((sql) => {
    ledger.init(sql);
    ledger.setCap(sql, { amount: Math.ceil(amount / 10_000) * 2, period: "week" }, 0);
    ledger.reserve(sql, { requestId: "a", tier: "low_cost_hosted", amount, price: UNIT }, WEEK_MS - 1000, interval);
    ledger.reserve(sql, { requestId: "b", tier: "low_cost_hosted", amount, price: UNIT }, WEEK_MS + 1000, interval);
  });
  return ledger;
}

export function runRead(step: Step, each: ReadCase): unknown {
  const ledger = makeLedger({ holdLifeMs: 3000 });
  setUp(step, ledger);
  return step((sql) => each.run(sql, ledger));
}
