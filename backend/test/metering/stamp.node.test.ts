import { describe, expect, test } from "vitest";
import { makeMetering } from "../../src/metering/index.ts";
import type { BillingPeriod, Ms, Sql } from "../../src/types.ts";
import { openSql } from "../../test-kit/sql/node.ts";

const ACCOUNT = "00000000-0000-4000-8000-0000000000b7";
const S: Ms = 1_800_000_017_000;
const HOUR: Ms = 3_600_000;
const DAY: Ms = 86_400_000;
const PERIOD: BillingPeriod = { start: S - 2 * DAY, end: S + 28 * DAY };

interface Cut {
  name: string;
  cut(sql: Sql, at: Ms): void;
  end(at: Ms): Ms;
  period(at: Ms): BillingPeriod;
}

const metering = makeMetering({ intervalSeconds: 3600, marginSeconds: 300 });
const wholeSecond = (at: Ms) => Math.floor(at / 1000) * 1000;

const CUTS: Cut[] = [
  {
    name: "the delete of the destroy step",
    cut: (sql, at) => metering.cut(sql, "delete", at, at),
    end: wholeSecond,
    period: () => PERIOD,
  },
  {
    name: "a cancel",
    cut: (sql, at) => metering.cut(sql, "cancel", at, at),
    end: wholeSecond,
    period: () => PERIOD,
  },
  {
    name: "a cancel event that moves the cut back to an earlier time",
    cut: (sql, at) => {
      metering.cut(sql, "cancel", S + HOUR - 1000, at);
      metering.cut(sql, "cancel", at, at);
    },
    end: wholeSecond,
    period: () => PERIOD,
  },
  {
    name: "a delete after a cancel",
    cut: (sql, at) => {
      metering.cut(sql, "cancel", S + HOUR - 1000, at);
      metering.cut(sql, "delete", at, at);
    },
    end: wholeSecond,
    period: () => PERIOD,
  },
  {
    name: "a billing period whose end falls in the interval",
    cut: (sql, at) => metering.billingPeriod(sql, { start: PERIOD.start, end: at }, at),
    end: (at) => at,
    period: (at) => ({ start: PERIOD.start, end: at }),
  },
];

function sealed(period: BillingPeriod, cut: Cut["cut"], cutAt: Ms) {
  const db = openSql();
  try {
    return db.step((sql) => {
      metering.init(sql);
      metering.begin(sql, ACCOUNT, period.start);
      metering.billingPeriod(sql, period, S);
      const start = metering.intervalAt(sql, S + 300);
      cut(sql, cutAt);
      metering.seal(sql, start, 12_345, cutAt);
      const row = sql.exec("SELECT start, end_at, timestamp, amount, carry_out, identifier FROM metering_interval WHERE start = ?", start)[0];
      if (row === undefined) throw new Error("no interval row");
      return row;
    });
  } finally {
    db.close();
  }
}

function inPeriod(second: number, period: BillingPeriod): boolean {
  return second * 1000 >= period.start && second * 1000 < period.end;
}

describe("the stamp of an interval that a cut ends in its first second (ruling P7-59)", () => {
  for (const each of CUTS) {
    test(`${each.name} in the second in which the interval starts: the event is stamped with the start second, inside the interval's billing period, and the stored bounds and amount stay as they are`, () => {
      const at = S + 900;
      const row = sealed(PERIOD, each.cut, at);
      expect(row).toEqual({ start: S, end_at: each.end(at), timestamp: S / 1000, amount: 1, carry_out: 2_345, identifier: `${ACCOUNT}:${S / 1000}` });
      expect(inPeriod(Number(row.timestamp), each.period(at))).toBe(true);
    });

    test(`${each.name} one and two seconds after the interval's start: the event is stamped with the second before the interval's end, inside its billing period`, () => {
      const stamps = [S + 1_900, S + 2_900].map((at) => {
        const row = sealed(PERIOD, each.cut, at);
        return [row.end_at, row.timestamp, inPeriod(Number(row.timestamp), each.period(at))];
      });
      expect(stamps).toEqual([
        [each.end(S + 1_900), Math.floor((each.end(S + 1_900) - 1) / 1000), true],
        [each.end(S + 2_900), Math.floor((each.end(S + 2_900) - 1) / 1000), true],
      ]);
    });
  }

  test("an interval that starts at its billing period's start and is cut by a delete in that second is stamped with the period's first second, not in the period before", () => {
    const period = { start: S, end: S + 30 * DAY };
    const row = sealed(period, (sql, at) => metering.cut(sql, "delete", at, at), S + 900);
    expect([row.start, row.end_at, row.timestamp]).toEqual([S, S, S / 1000]);
    expect(inPeriod(Number(row.timestamp), period)).toBe(true);
    expect(inPeriod(Number(row.timestamp), { start: S - 30 * DAY, end: S })).toBe(false);
  });
});
