import { describe, expect, test } from "vitest";
import { costOf } from "../../src/ledger/cost.ts";
import { type Ledger, makeLedger, type Price } from "../../src/ledger/index.ts";
import type { Sql, Tier } from "../../src/types.ts";
import { openSql } from "../../test-kit/sql/node.ts";

const WEEK = 604_800_000;
const LOW: Price = { input: 3_100_000, output: 15_700_000 };
const HIGH: Price = { input: 15_500_000, output: 78_500_000 };
const ACTIVE = { active: true, interval: 0 };

function withLedger(run: (sql: Sql, ledger: Ledger) => void, holdLifeMs = 3000): void {
  const db = openSql();
  const ledger = makeLedger({ holdLifeMs });
  db.step((sql) => {
    ledger.init(sql);
    run(sql, ledger);
  });
  db.close();
}

const req = (requestId: string, amount: number, tier: Tier = "low_cost_hosted", price: Price = LOW) => ({ requestId, tier, amount, price });
const totals = (sql: Sql) => sql.exec("SELECT period_start, period_end, reported, unreported, open FROM ledger_periods ORDER BY period_start");
const hold = (sql: Sql, id: string) => sql.exec("SELECT * FROM ledger_holds WHERE request_id = ?", id)[0];

describe("the cap and its periods (contract 4)", () => {
  test("the first setCap sets the anchor to its now, and a later one never moves it at once", () => {
    withLedger((sql, ledger) => {
      expect(ledger.cap(sql, 5)).toBeNull();
      ledger.setCap(sql, { amount: 5, period: "week" }, 1000);
      expect(ledger.cap(sql, 1000)).toEqual({ cap: { amount: 5, period: "week" }, spent: 0, held: 0, periodEnd: 1000 + WEEK });
      ledger.setCap(sql, { amount: 3, period: "week" }, 2000, 99);
      expect(sql.exec("SELECT amount, period, anchor, next_period, switch_at FROM ledger_cap")).toEqual([
        { amount: 3, period: "week", anchor: 1000, next_period: null, switch_at: null },
      ]);
      expect(ledger.cap(sql, 1000 + WEEK)).toEqual({ cap: { amount: 3, period: "week" }, spent: 0, held: 0, periodEnd: 1000 + 2 * WEEK });
    });
  });

  test("the seed's anchor is taken when the object has no cap yet", () => {
    withLedger((sql, ledger) => {
      const seededAt = 1_893_456_000_000;
      ledger.setCap(sql, { amount: 5, period: "week" }, seededAt, seededAt + 400 - WEEK);
      expect(ledger.cap(sql, seededAt)).toMatchObject({ periodEnd: seededAt + 400 });
      expect(ledger.cap(sql, seededAt + 400)).toMatchObject({ periodEnd: seededAt + 400 + WEEK });
      ledger.setCap(sql, { amount: 6, period: "week" }, seededAt + 1, 7);
      expect(sql.exec("SELECT amount, anchor FROM ledger_cap")).toEqual([{ amount: 6, anchor: seededAt + 400 - WEEK }]);
    });
  });

  test("a month boundary is the anchor's day and time n months later in UTC, at the last day of a shorter month", () => {
    withLedger((sql, ledger) => {
      const anchor = Date.UTC(2027, 0, 31, 10, 30, 0, 5);
      ledger.setCap(sql, { amount: 5, period: "month" }, anchor);
      expect(ledger.cap(sql, anchor)?.periodEnd).toBe(Date.UTC(2027, 1, 28, 10, 30, 0, 5));
      expect(ledger.cap(sql, Date.UTC(2027, 1, 28, 10, 30, 0, 5))?.periodEnd).toBe(Date.UTC(2027, 2, 31, 10, 30, 0, 5));
      expect(ledger.cap(sql, Date.UTC(2027, 2, 31, 10, 30, 0, 4))?.periodEnd).toBe(Date.UTC(2027, 2, 31, 10, 30, 0, 5));
      expect(ledger.cap(sql, Date.UTC(2028, 1, 29, 11, 0))?.periodEnd).toBe(Date.UTC(2028, 2, 31, 10, 30, 0, 5));
      expect(ledger.cap(sql, Date.UTC(2028, 1, 29, 10, 0))?.periodEnd).toBe(Date.UTC(2028, 1, 29, 10, 30, 0, 5));
      expect(sql.exec("SELECT anchor FROM ledger_cap")).toEqual([{ anchor }]);
    });
  });

  test("a set with another period type keeps the period in force until its end, which becomes the anchor", () => {
    withLedger((sql, ledger) => {
      const anchor = Date.UTC(2027, 4, 3);
      ledger.setCap(sql, { amount: 5, period: "week" }, anchor);
      ledger.setCap(sql, { amount: 6, period: "month" }, anchor + 2 * 86_400_000);
      expect(ledger.cap(sql, anchor + 3 * 86_400_000)).toEqual({ cap: { amount: 6, period: "month" }, spent: 0, held: 0, periodEnd: anchor + WEEK });
      expect(sql.exec("SELECT period, anchor, next_period, switch_at FROM ledger_cap")).toEqual([
        { period: "week", anchor, next_period: "month", switch_at: anchor + WEEK },
      ]);
      expect(ledger.cap(sql, anchor + WEEK - 1)).toMatchObject({ cap: { amount: 6, period: "month" }, periodEnd: anchor + WEEK });
      expect(ledger.cap(sql, anchor + WEEK)).toMatchObject({ cap: { amount: 6, period: "month" }, periodEnd: Date.UTC(2027, 5, 10) });
      expect(ledger.cap(sql, anchor + 40 * 86_400_000)?.periodEnd).toBe(Date.UTC(2027, 6, 10));
      expect(sql.exec("SELECT period, anchor, next_period, switch_at FROM ledger_cap")).toEqual([
        { period: "month", anchor: anchor + WEEK, next_period: null, switch_at: null },
      ]);
    });
  });

  test("a change of type waits for its boundary, which becomes the anchor also when the first call after it comes later", () => {
    withLedger((sql, ledger) => {
      const anchor = Date.UTC(2027, 4, 3);
      ledger.setCap(sql, { amount: 5, period: "week" }, anchor);
      ledger.setCap(sql, { amount: 6, period: "month" }, anchor + 86_400_000);
      ledger.cap(sql, anchor + WEEK - 1);
      expect(sql.exec("SELECT period, anchor, next_period, switch_at FROM ledger_cap")).toEqual([
        { period: "week", anchor, next_period: "month", switch_at: anchor + WEEK },
      ]);
      ledger.cap(sql, anchor + WEEK + 5 * 86_400_000);
      expect(sql.exec("SELECT period, anchor, next_period, switch_at FROM ledger_cap")).toEqual([
        { period: "month", anchor: anchor + WEEK, next_period: null, switch_at: null },
      ]);
    });
  });

  test("setting the type in force again before the boundary cancels the pending change", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 5, period: "week" }, 0);
      ledger.setCap(sql, { amount: 5, period: "month" }, 10);
      ledger.setCap(sql, { amount: 7, period: "week" }, 20);
      expect(sql.exec("SELECT amount, period, anchor, next_period, switch_at FROM ledger_cap")).toEqual([
        { amount: 7, period: "week", anchor: 0, next_period: null, switch_at: null },
      ]);
    });
  });
});

describe("reserve (contract 3.2)", () => {
  test("it tests the request id, then not-active, then no-cap, then over-cap, and a refusal stores no hold", () => {
    withLedger((sql, ledger) => {
      expect(ledger.reserve(sql, req("a", 10), 0, { active: false, interval: 0 })).toEqual({ outcome: "refused", reason: "not-active" });
      expect(ledger.reserve(sql, req("a", 10), 0, ACTIVE)).toEqual({ outcome: "refused", reason: "no-cap" });
      ledger.setCap(sql, { amount: 1, period: "week" }, 0);
      expect(ledger.reserve(sql, req("a", 10_001), 0, ACTIVE)).toEqual({ outcome: "refused", reason: "over-cap" });
      expect(sql.exec("SELECT request_id FROM ledger_holds")).toEqual([]);
      expect(ledger.reserve(sql, req("a", 10_000), 5, ACTIVE)).toEqual({
        outcome: "admitted",
        receipt: { requestId: "a", amount: 10_000, now: 5, expiresAt: 3005 },
      });
      expect(ledger.reserve(sql, req("a", 10_000), 7, { active: false, interval: 0 })).toEqual({
        outcome: "repeat",
        state: "open",
        dispatched: false,
        receipt: { requestId: "a", amount: 10_000, now: 7, expiresAt: 3005 },
      });
      expect(ledger.reserve(sql, req("a", 9_999), 7, ACTIVE)).toEqual({ outcome: "refused", reason: "other-arguments" });
      expect(ledger.reserve(sql, req("a", 10_000, "frontier"), 7, ACTIVE)).toEqual({ outcome: "refused", reason: "other-arguments" });
      expect(ledger.reserve(sql, req("a", 10_000, "low_cost_hosted", HIGH), 7, ACTIVE)).toEqual({ outcome: "refused", reason: "other-arguments" });
      expect(ledger.reserve(sql, req("b", 1), 8, ACTIVE)).toEqual({ outcome: "refused", reason: "over-cap" });
    });
  });

  test("admission counts the open holds, so a cap of K holds admits K and refuses the next until one ends", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 3, period: "week" }, 0);
      for (const id of ["a", "b", "c"]) expect(ledger.reserve(sql, req(id, 10_000), 1, ACTIVE).outcome).toBe("admitted");
      expect(ledger.reserve(sql, req("d", 10_000), 1, ACTIVE)).toEqual({ outcome: "refused", reason: "over-cap" });
      expect(ledger.settle(sql, "a", { inputTokens: 0, outputTokens: 0 }, 2)).toEqual({ outcome: "settled", cost: 0, overrun: 0, repeat: false });
      expect(ledger.reserve(sql, req("d", 10_000), 2, ACTIVE).outcome).toBe("admitted");
      expect(totals(sql)).toEqual([{ period_start: 0, period_end: WEEK, reported: 0, unreported: 0, open: 30_000 }]);
    });
  });

  test("a lower amount applies to the next admission, and each cap period admits against its own total", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 2, period: "week" }, 0);
      expect(ledger.reserve(sql, req("a", 15_000), 1, ACTIVE).outcome).toBe("admitted");
      ledger.setCap(sql, { amount: 1, period: "week" }, 2);
      expect(ledger.cap(sql, 2)).toEqual({ cap: { amount: 1, period: "week" }, spent: 0, held: 15_000, periodEnd: WEEK });
      expect(ledger.reserve(sql, req("b", 1), 3, ACTIVE)).toEqual({ outcome: "refused", reason: "over-cap" });
      expect(ledger.reserve(sql, req("b", 10_000), WEEK, ACTIVE).outcome).toBe("admitted");
      expect(totals(sql)).toEqual([
        { period_start: 0, period_end: WEEK, reported: 0, unreported: 0, open: 15_000 },
        { period_start: WEEK, period_end: 2 * WEEK, reported: 0, unreported: 0, open: 10_000 },
      ]);
    }, 10 * WEEK);
  });
});

describe("dispatch, settle, void and expire (contract 3.2)", () => {
  test("dispatch answers go once for an open hold and stop otherwise; void refuses a marked hold", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 10, period: "week" }, 0);
      ledger.reserve(sql, req("a", 10), 0, ACTIVE);
      expect(ledger.dispatch(sql, "nobody", 1)).toBe("stop");
      expect(ledger.dispatch(sql, "a", 1)).toBe("go");
      expect(ledger.dispatch(sql, "a", 1)).toBe("stop");
      expect(ledger.reserve(sql, req("a", 10), 2, ACTIVE)).toMatchObject({ outcome: "repeat", state: "open", dispatched: true });
      expect(ledger.void(sql, "a", 2)).toEqual({ outcome: "refused", reason: "dispatched" });
      expect(ledger.settle(sql, "a", { inputTokens: 1, outputTokens: 1 }, 3)).toMatchObject({ outcome: "settled", repeat: false });
      expect(ledger.dispatch(sql, "a", 4)).toBe("stop");
    });
  });

  test("settle costs the usage at the price the hold recorded, rounded up, and records an overrun above the hold", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 1000, period: "week" }, 0);
      ledger.reserve(sql, req("high", 100, "frontier", HIGH), 0, ACTIVE);
      expect(ledger.settle(sql, "high", { inputTokens: 1, outputTokens: 1 }, 1)).toEqual({ outcome: "settled", cost: 94, overrun: 0, repeat: false });
      ledger.reserve(sql, req("over", 50), 0, ACTIVE);
      expect(ledger.settle(sql, "over", { inputTokens: 10, outputTokens: 10 }, 1)).toEqual({ outcome: "settled", cost: 188, overrun: 138, repeat: false });
      expect(ledger.settle(sql, "over", { inputTokens: 0, outputTokens: 0 }, 2)).toEqual({ outcome: "settled", cost: 188, overrun: 138, repeat: true });
      expect(totals(sql)).toEqual([{ period_start: 0, period_end: WEEK, reported: 282, unreported: 0, open: 0 }]);
      expect(ledger.settle(sql, "nobody", { inputTokens: 1, outputTokens: 1 }, 2)).toEqual({ outcome: "refused", reason: "unknown-request" });
    });
  });

  test("void releases an open hold in full, repeats its outcome, and refuses an ended hold", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 1, period: "week" }, 0);
      ledger.reserve(sql, req("a", 10_000), 0, ACTIVE);
      expect(ledger.void(sql, "a", 1)).toEqual({ outcome: "voided" });
      expect(ledger.void(sql, "a", 2)).toEqual({ outcome: "voided" });
      expect(ledger.settle(sql, "a", { inputTokens: 1, outputTokens: 1 }, 2)).toEqual({ outcome: "refused", reason: "already-ended" });
      expect(totals(sql)).toEqual([{ period_start: 0, period_end: WEEK, reported: 0, unreported: 0, open: 0 }]);
      expect(ledger.reserve(sql, req("b", 10_000), 3, ACTIVE).outcome).toBe("admitted");
      ledger.settle(sql, "b", { inputTokens: 1, outputTokens: 1 }, 4);
      expect(ledger.void(sql, "b", 5)).toEqual({ outcome: "refused", reason: "already-ended" });
      expect(ledger.void(sql, "nobody", 5)).toEqual({ outcome: "refused", reason: "unknown-request" });
    });
  });

  test("expire ends every open hold at or before now with its full amount unreported, and every other call that takes now runs it first", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 10, period: "week" }, 0);
      ledger.reserve(sql, req("a", 100), 0, ACTIVE);
      ledger.reserve(sql, req("b", 200), 1, ACTIVE);
      expect(ledger.nextDue(sql)).toBe(3000);
      expect(ledger.expire(sql, 2999)).toBe(0);
      expect(ledger.expire(sql, 3000)).toBe(1);
      expect(ledger.nextDue(sql)).toBe(3001);
      expect(ledger.openHolds(sql)).toBe(1);
      expect(ledger.dispatch(sql, "b", 3001)).toBe("stop");
      expect(hold(sql, "b")).toMatchObject({ state: "expired", ended_at: 3001, ended_by: "dispatch" });
      expect(ledger.nextDue(sql)).toBeNull();
      expect(totals(sql)).toEqual([{ period_start: 0, period_end: WEEK, reported: 0, unreported: 300, open: 0 }]);
      expect(ledger.settle(sql, "a", { inputTokens: 100, outputTokens: 100 }, 4000)).toEqual({ outcome: "expired", cost: 1880, overrun: 1780 });
      expect(ledger.settle(sql, "a", { inputTokens: 1, outputTokens: 1 }, 4001)).toEqual({ outcome: "expired", cost: 1880, overrun: 1780 });
      expect(totals(sql)).toEqual([{ period_start: 0, period_end: WEEK, reported: 0, unreported: 300, open: 0 }]);
      expect(hold(sql, "a")).toMatchObject({ state: "expired", cost: 1880, overrun: 1780, late_at: 4000 });
      expect(ledger.void(sql, "a", 4002)).toEqual({ outcome: "refused", reason: "already-ended" });
      ledger.reserve(sql, req("c", 1000), 5000, ACTIVE);
      ledger.expire(sql, 8000);
      expect(ledger.settle(sql, "c", { inputTokens: 1, outputTokens: 1 }, 8001)).toEqual({ outcome: "expired", cost: 19, overrun: 0 });
      expect(hold(sql, "c")).toMatchObject({ state: "expired", cost: 19, overrun: 0 });
    });
  });

  test("each event takes the next number of the ledger's sequence, also a call that only ends due holds, and holds due together end in the order of their expiry", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 10, period: "week" }, 0);
      ledger.reserve(sql, req("a-due-later", 100), 2, ACTIVE);
      ledger.reserve(sql, req("b-due-first", 100), 1, ACTIVE);
      expect(ledger.expire(sql, 5000)).toBe(2);
      expect(sql.exec("SELECT seq FROM ledger_state")).toEqual([{ seq: 4 }]);
      ledger.reserve(sql, req("next", 100), 5001, ACTIVE);
      expect(sql.exec("SELECT request_id, created_seq, ended_seq FROM ledger_holds ORDER BY created_seq")).toEqual([
        { request_id: "a-due-later", created_seq: 1, ended_seq: 4 },
        { request_id: "b-due-first", created_seq: 2, ended_seq: 3 },
        { request_id: "next", created_seq: 5, ended_seq: null },
      ]);
    });
  });

  test("intervalTotals sums settled costs and expired amounts of one interval and counts its open holds; a late settle and a void add nothing", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 10, period: "week" }, 0);
      for (const [id, interval] of [["a", 0], ["b", 0], ["c", 0], ["d", 0], ["e", 3_600_000]] as const) {
        ledger.reserve(sql, req(id, 1000), 0, { active: true, interval });
      }
      ledger.settle(sql, "a", { inputTokens: 10, outputTokens: 10 }, 1);
      ledger.void(sql, "b", 1);
      expect(ledger.intervalTotals(sql, 0)).toEqual({ reported: 188, unreported: 0, open: 2 });
      ledger.expire(sql, 3000);
      ledger.settle(sql, "c", { inputTokens: 10, outputTokens: 10 }, 3001);
      expect(ledger.intervalTotals(sql, 0)).toEqual({ reported: 188, unreported: 2000, open: 0 });
      expect(ledger.intervalTotals(sql, 3_600_000)).toEqual({ reported: 0, unreported: 1000, open: 0 });
      expect(ledger.intervalTotals(sql, 7_200_000)).toEqual({ reported: 0, unreported: 0, open: 0 });
    });
  });

  test("every function is synchronous: none returns a promise", () => {
    withLedger((sql, ledger) => {
      ledger.setCap(sql, { amount: 1, period: "week" }, 0);
      const answers = [
        ledger.cap(sql, 0),
        ledger.reserve(sql, req("a", 1), 0, ACTIVE),
        ledger.dispatch(sql, "a", 0),
        ledger.settle(sql, "a", { inputTokens: 0, outputTokens: 0 }, 0),
        ledger.void(sql, "a", 0),
        ledger.expire(sql, 0),
        ledger.intervalTotals(sql, 0),
        ledger.openHolds(sql),
        ledger.nextDue(sql),
      ];
      expect(answers.filter((answer) => answer instanceof Promise)).toEqual([]);
    });
  });

  test("a new hold refuses an amount or a price that is not whole micro-units and the step rolls back; a repeat with one is other-arguments", () => {
    const db = openSql();
    const ledger = makeLedger({ holdLifeMs: 3000 });
    db.step((sql) => {
      ledger.init(sql);
      ledger.setCap(sql, { amount: 1, period: "week" }, 0);
    });
    for (const bad of [req("a", 1.5), req("a", -1), req("a", 1, "low_cost_hosted", { input: 0.5, output: 1 })]) {
      expect(() => db.step((sql) => ledger.reserve(sql, bad, 0, ACTIVE))).toThrow(RangeError);
    }
    expect(db.step((sql) => sql.exec("SELECT request_id FROM ledger_holds"))).toEqual([]);
    db.step((sql) => ledger.reserve(sql, req("b", 1), 0, ACTIVE));
    for (const bad of [req("b", 1.5), req("b", -1), req("b", 1, "low_cost_hosted", { input: 0.5, output: 1 })]) {
      expect(db.step((sql) => ledger.reserve(sql, bad, 1, ACTIVE))).toEqual({ outcome: "refused", reason: "other-arguments" });
    }
    db.close();
  });
});

describe("the one rounding function", () => {
  test("costOf rounds up in whole micro-units and refuses a count that is not whole", () => {
    expect(costOf(LOW, { inputTokens: 0, outputTokens: 0 })).toBe(0);
    expect(costOf(LOW, { inputTokens: 1, outputTokens: 0 })).toBe(4);
    expect(costOf({ input: 1_000_000, output: 0 }, { inputTokens: 3, outputTokens: 0 })).toBe(3);
    expect(costOf({ input: 1_000_001, output: 0 }, { inputTokens: 1, outputTokens: 0 })).toBe(2);
    expect(costOf(LOW, { inputTokens: 1_000_000_000, outputTokens: 1_000_000_000 })).toBe(18_800_000_000);
    expect(() => costOf(LOW, { inputTokens: 0.5, outputTokens: 0 })).toThrow(RangeError);
    expect(() => costOf(LOW, { inputTokens: -1, outputTokens: 0 })).toThrow(RangeError);
    expect(() => costOf({ input: Number.MAX_SAFE_INTEGER, output: 0 }, { inputTokens: Number.MAX_SAFE_INTEGER, outputTokens: 0 })).toThrow(RangeError);
  });
});

describe("a refusal stores no hold (contract 3.2)", () => {
  const everything = (sql: Sql) => JSON.stringify(["ledger_state", "ledger_cap", "ledger_periods", "ledger_holds"].map((table) => sql.exec(`SELECT * FROM ${table} ORDER BY rowid`)));
  const refusals: { reason: string; setUp(sql: Sql, ledger: Ledger): void; refused: { requestId: string; amount: number; active: boolean } }[] = [
    { reason: "not-active", setUp: (sql, ledger) => ledger.setCap(sql, { amount: 1, period: "week" }, 0), refused: { requestId: "x", amount: 10, active: false } },
    { reason: "no-cap", setUp: () => undefined, refused: { requestId: "x", amount: 10, active: true } },
    {
      reason: "over-cap",
      setUp: (sql, ledger) => {
        ledger.setCap(sql, { amount: 1, period: "week" }, 0);
        ledger.reserve(sql, req("a", 10_000), 0, ACTIVE);
      },
      refused: { requestId: "x", amount: 1, active: true },
    },
    {
      reason: "other-arguments",
      setUp: (sql, ledger) => {
        ledger.setCap(sql, { amount: 1, period: "week" }, 0);
        ledger.reserve(sql, req("x", 10_000), 0, ACTIVE);
      },
      refused: { requestId: "x", amount: 9_999, active: true },
    },
  ];
  for (const each of refusals) {
    test(`after a refusal for ${each.reason}, no hold row was added for the id and no total moved`, () => {
      withLedger((sql, ledger) => {
        each.setUp(sql, ledger);
        const rowsBefore = sql.exec("SELECT * FROM ledger_holds WHERE request_id = ?", each.refused.requestId).length;
        const before = everything(sql);
        const answer = ledger.reserve(sql, req(each.refused.requestId, each.refused.amount), 1, { active: each.refused.active, interval: 0 });
        expect(answer).toEqual({ outcome: "refused", reason: each.reason });
        expect(sql.exec("SELECT * FROM ledger_holds WHERE request_id = ?", each.refused.requestId).length).toBe(rowsBefore);
        expect(everything(sql)).toBe(before);
      });
    });
  }
});
