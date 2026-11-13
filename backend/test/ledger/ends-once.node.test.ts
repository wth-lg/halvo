import { expect, test } from "vitest";
import { makeLedger, type Price } from "../../src/ledger/index.ts";
import { openSql } from "../../test-kit/sql/node.ts";

type End = "settle" | "void" | "expire";
const LOW: Price = { input: 3_100_000, output: 15_700_000 };
const AMOUNT = 1000;
const USAGE = { inputTokens: 100, outputTokens: 100 };
const COST = 1880;
const EXPIRY = 3000;

function orderings(left: Record<End, number>): End[][] {
  const ends = (Object.keys(left) as End[]).filter((end) => left[end] > 0);
  if (ends.length === 0) return [[]];
  return ends.flatMap((end) => orderings({ ...left, [end]: left[end] - 1 }).map((rest) => [end, ...rest]));
}

function expected(order: End[], dispatched: boolean) {
  let state: "open" | "settled" | "voided" | "expired" = "open";
  let late = false;
  let voidedAtExpiry = false;
  const totals = { reported: 0, unreported: 0, open: AMOUNT };
  const steps = order.map((end) => {
    let answer: unknown;
    if (end === "expire") {
      answer = state === "open" ? 1 : 0;
      if (state === "open") {
        state = dispatched ? "expired" : "voided";
        voidedAtExpiry = !dispatched;
        totals.open = 0;
        totals.unreported = dispatched ? AMOUNT : 0;
      }
    } else if (end === "settle") {
      if (state === "open") {
        state = "settled";
        totals.open = 0;
        totals.reported = COST;
        answer = { outcome: "settled", cost: COST, overrun: COST - AMOUNT, repeat: false };
      } else if (state === "settled") answer = { outcome: "settled", cost: COST, overrun: COST - AMOUNT, repeat: true };
      else if (state === "expired") {
        late = true;
        answer = { outcome: "expired", cost: COST, overrun: COST - AMOUNT };
      } else answer = { outcome: "refused", reason: "already-ended" };
    } else if (state === "open" && dispatched) answer = { outcome: "refused", reason: "dispatched" };
    else if (state === "open") {
      state = "voided";
      totals.open = 0;
      answer = { outcome: "voided" };
    } else answer = state === "voided" && !voidedAtExpiry ? { outcome: "voided" } : { outcome: "refused", reason: "already-ended" };
    return { end, answer, totals: { ...totals }, state };
  });
  return { steps, late };
}

const all = orderings({ settle: 2, void: 2, expire: 2 });

test("there are 90 orders of settle, void and expire, each sent twice", () => {
  expect(all).toHaveLength(90);
  expect(new Set(all.map((order) => order.join(" "))).size).toBe(90);
});

for (const dispatched of [false, true]) {
  test(`every hold ends once in each of the 90 orders, ${dispatched ? "with" : "without"} the dispatch mark`, () => {
    const ledger = makeLedger({ holdLifeMs: EXPIRY });
    for (const order of all) {
      const db = openSql();
      const seen = db.step((sql) => {
        ledger.init(sql);
        ledger.setCap(sql, { amount: 1, period: "week" }, 0);
        ledger.reserve(sql, { requestId: "h", tier: "low_cost_hosted", amount: AMOUNT, price: LOW }, 0, { active: true, interval: 0 });
        if (dispatched) ledger.dispatch(sql, "h", 0);
        let now = 0;
        let endedSeq: unknown = null;
        return order.map((end) => {
          if (end === "expire") now = Math.max(now, EXPIRY);
          else now += 1;
          const answer =
            end === "settle" ? ledger.settle(sql, "h", USAGE, now) : end === "void" ? ledger.void(sql, "h", now) : ledger.expire(sql, now);
          const row = sql.exec("SELECT state, ended_seq, late_at FROM ledger_holds WHERE request_id = ?", "h")[0] ?? {};
          const period = sql.exec("SELECT reported, unreported, open FROM ledger_periods")[0] ?? {};
          if (endedSeq === null) endedSeq = row.ended_seq;
          return { end, answer, totals: { ...period }, state: row.state, endedSeqKept: row.ended_seq === endedSeq };
        });
      });
      db.close();
      const want = expected(order, dispatched);
      const label = order.join(" ");
      expect({ label, steps: seen.map(({ endedSeqKept: _kept, ...step }) => step) }).toEqual({ label, steps: want.steps });
      expect({ label, kept: seen.every((step) => step.endedSeqKept) }).toEqual({ label, kept: true });
    }
  });
}
