import { describe, expect, test } from "vitest";
import { LATE_READ_SPAN_MS } from "../../src/metering/index.ts";
import { ACCOUNT, DAY, HASH, HOUR, world } from "./world.ts";
import { count, cutPair, END, foreign, kinds, NOT_BILLABLE, open, SPAN, stopAfterTheNextCommit, tries } from "./unexplained-support.ts";

type Row = Record<string, string | number | null>;

const MINUTE = { readGranularitySeconds: 60 };
const NONE = { kind: "none" } as const;
const LOST = { kind: "request-lost" } as const;
const UNKNOWN = { kind: "refuse", reason: "unknown_account" } as const;

function state(row: Row | undefined): unknown[] {
  return [row?.outcome, row?.refusal, row?.refusals, row?.pending_refusal, row?.unanswered, row?.unexplained_since, row?.unexplained_total, row?.unexplained_neighbours];
}

function reads(n: number, total: number): string[] {
  return Array.from({ length: n }, () => `read total ${total}`);
}

describe("a run of unexplained totals is closed as not billable with the cause unexplained-total (unit 5.7, ruling P7-86 B)", { timeout: 120_000 }, () => {
  test("a foreign event in the interval's minute, the first send recorded and its answer lost: 29 reads and no send, then the close with the cause unexplained-total, the two sums kept and one line, and the fake meter holds one event of the interval", async () => {
    const w = await world({ meter: MINUTE, faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + DAY;
    await w.clock(since);
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed after recorded", ...reads(29, 8)]);
    expect(state(row)).toEqual(["not-billable", "unexplained-total", 0, null, 1, since, 8, 0]);
    expect(row?.closed_at).toBe(since + SPAN);
    expect(count(w.lines, NOT_BILLABLE)).toBe(1);
    expect(w.meter.recordedEvents(ACCOUNT).filter((each) => each.identifier === row?.identifier)).toHaveLength(1);
    w.close();
  });

  test("with a margin of 3,900 s the run's bound stays LATE_READ_SPAN_MS and not the late point's span, the window less the margin: the read 82,920 s after the run's first closes nothing, and the try at 86,100 s closes", async () => {
    const w = await world({ meter: MINUTE, faults: [{ kind: "response-lost" }], config: { marginSeconds: 3_900 } });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + DAY;
    await w.clock(since);
    let at82920: unknown = null;
    await tries(w, (rows) => {
      if (w.calls.at(-1)?.at === since + 82_920_000) at82920 = [rows[0]?.outcome, rows[0]?.unexplained_since];
      return rows[0]?.outcome === null;
    });
    const [row] = await w.intervals();
    expect(at82920).toEqual([null, since]);
    expect([row?.outcome, row?.refusal, row?.closed_at]).toEqual(["not-billable", "unexplained-total", since + SPAN]);
    w.close();
  });

  test("a send recorded with its answer lost four hours before the bound, in a run whose stored time is older (written into the row): the read at the bound is before the late point and closes nothing; the read at the late point closes", async () => {
    const w = await world({ meter: MINUTE, faults: [LOST, LOST] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const sent = END + 20 * HOUR;
    await w.clock(sent);
    const bound = sent + 4 * HOUR;
    const since = bound - LATE_READ_SPAN_MS;
    w.host.step((sql) => sql.exec("UPDATE metering_interval SET unexplained_since = ?", since));
    await w.clock(bound);
    const atTheBound = state((await w.intervals())[0]);
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect(kinds(w.calls).slice(0, 3)).toEqual(["send failed", "send failed", "read total 5"]);
    expect(atTheBound).toEqual([null, null, 0, null, 1, since, null, null]);
    expect(state(row)).toEqual(["not-billable", "unexplained-total", 0, null, 1, since, 5, 0]);
    expect(row?.closed_at).toBe(sent + SPAN);
    expect(kinds(w.calls).filter((each) => each.startsWith("send"))).toHaveLength(2);
    w.close();
  });

  test("one lagging unexplained read, then the object down for a day, then a fresh read that shows both amounts of a cut pair: acknowledged and not closed", async () => {
    const w = await world({ meter: { ...MINUTE, readLagSeconds: 23 * 3600 }, faults: [NONE, { kind: "response-lost" }, UNKNOWN] });
    await cutPair(w);
    await w.clock(END + 40_000);
    await w.clock(END + 100_000);
    const lagging = state((await w.intervals())[1]);
    await w.clock(END + 100_000 + DAY);
    const [, row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send recorded", "send failed after recorded", "send refused unknown_account", "read total 0", "read total 5"]);
    expect(lagging).toEqual([null, null, 0, "unknown_account", 1, END + 100_000, null, null]);
    expect(state(row)).toEqual(["acknowledged", null, 0, null, 1, null, null, null]);
    expect(count(w.lines, NOT_BILLABLE)).toBe(0);
    w.close();
  });

  test("unexplained by a lag of 30 hours and then explained: the stored time is cleared and the late read resends", async () => {
    const w = await world({ meter: { ...MINUTE, readLagSeconds: 30 * 3600 }, faults: [NONE, LOST] });
    await cutPair(w);
    await w.clock(END + 40_000);
    const latePoint = END + 40_000 + SPAN;
    await w.clock(latePoint);
    const first = state((await w.intervals())[1]);
    await tries(w, open(1));
    const [, row] = await w.intervals();
    expect(first).toEqual([null, null, 0, null, 1, latePoint, null, null]);
    expect(kinds(w.calls)).toEqual(["send recorded", "send failed", ...reads(11, 0), "read total 3", "send recorded"]);
    expect(state(row)).toEqual(["acknowledged", null, 0, null, 0, null, null, null]);
    w.close();
  });

  for (const [amount, outcomes] of [
    [5, ["not-billable", "not-billable"]],
    [2, ["not-billable", "acknowledged"]],
  ] as const) {
    test(`two intervals of one minute behind a foreign event of ${amount} minor units: ${amount === 2 ? "the pin: the second one's own amount acknowledges it with no send after its first" : "each waits its own span and each is closed with the cause"}`, async () => {
      const w = await world({ meter: MINUTE, faults: [LOST, ...Array.from({ length: 29 }, () => NONE), LOST] });
      await cutPair(w);
      foreign(w, amount);
      await w.clock(END + 40_000);
      const sinceFirst = END + 40_000 + DAY;
      await w.clock(sinceFirst);
      await tries(w, open(0));
      const [first] = await w.intervals();
      const sinceSecond = Number(first?.closed_at) + DAY;
      await w.clock(sinceSecond);
      await tries(w, open(1));
      const [x, y] = await w.intervals();
      expect([x?.outcome, y?.outcome]).toEqual(outcomes);
      expect(state(x)).toEqual(["not-billable", "unexplained-total", 0, null, 1, sinceFirst, amount, 0]);
      expect(x?.closed_at).toBe(sinceFirst + SPAN);
      if (amount === 2) {
        expect(kinds(w.calls)).toEqual(["send failed", ...reads(29, 2), "send failed", "read total 2"]);
        expect(state(y)).toEqual(["acknowledged", null, 0, null, 1, null, null, null]);
        expect(count(w.lines, NOT_BILLABLE)).toBe(1);
      } else {
        expect(kinds(w.calls)).toEqual(["send failed", ...reads(29, 5), "send failed", ...reads(29, 5)]);
        expect(state(y)).toEqual(["not-billable", "unexplained-total", 0, null, 1, sinceSecond, 5, 0]);
        expect(y?.closed_at).toBe(sinceSecond + SPAN);
        expect(count(w.lines, NOT_BILLABLE)).toBe(2);
      }
      w.close();
    });
  }

  test("an interval none of whose sends lacks a definite answer, unexplained past its first window: it closes one span after its first unexplained total, at the first read at or after it", async () => {
    const w = await world({ meter: MINUTE, faults: [UNKNOWN] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + DAY;
    await w.clock(since);
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send refused unknown_account", ...reads(29, 5)]);
    expect(state(row)).toEqual(["not-billable", "unexplained-total", 1, null, 0, since, 5, 0]);
    expect(Number(row?.closed_at) - since).toBe(SPAN);
    expect(count(w.lines, NOT_BILLABLE)).toBe(1);
    w.close();
  });

  test("a stop between the step that records the read and the next step: the stored time is as that step left it, and the close comes once", async () => {
    const w = await world({ meter: MINUTE });
    const stop = stopAfterTheNextCommit(w);
    w.faults.push({ kind: "response-lost" }, { kind: "during", run: async () => stop(), after: NONE });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + DAY;
    await w.clock(since);
    const afterTheStop = state((await w.intervals())[0]);
    w.restart();
    await tries(w, open(0));
    const closed = (await w.intervals())[0];
    await w.clock(Number(closed?.closed_at) + DAY);
    const [row] = await w.intervals();
    expect(afterTheStop).toEqual([null, null, 0, null, 1, since, null, null]);
    expect(state(row)).toEqual(["not-billable", "unexplained-total", 0, null, 1, since, 8, 0]);
    expect(row?.closed_at).toBe(since + SPAN);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", ...reads(29, 8)]);
    expect(count(w.lines, NOT_BILLABLE)).toBe(1);
    w.close();
  });

  test("a pending refusal at the close: the stored cause is unexplained-total, the pending refusal is dropped and not counted", async () => {
    const w = await world({ meter: MINUTE, faults: [LOST, UNKNOWN] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + 60_000;
    await w.clock(since);
    const pending = state((await w.intervals())[0]);
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect(pending).toEqual([null, null, 0, "unknown_account", 1, since, null, null]);
    expect(state(row)).toEqual(["not-billable", "unexplained-total", 0, null, 1, since, 5, 0]);
    expect(Number(row?.closed_at)).toBeGreaterThanOrEqual(since + SPAN);
    expect(kinds(w.calls).filter((each) => each.startsWith("send"))).toEqual(["send failed", "send refused unknown_account"]);
    expect(count(w.lines, NOT_BILLABLE)).toBe(1);
    w.close();
  });

  test("a try is set at the run's bound: the run of a pending refusal closes LATE_READ_SPAN_MS after its first unexplained total and not at the doubling delay's next read", async () => {
    const w = await world({ meter: MINUTE, faults: [LOST, UNKNOWN] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + 60_000;
    await w.clock(since);
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect([row?.outcome, row?.refusal, row?.unexplained_since]).toEqual(["not-billable", "unexplained-total", since]);
    expect(Number(row?.closed_at) - since).toBe(LATE_READ_SPAN_MS);
    w.close();
  });

  test("a delete answered at the run's first unexplained total, with an interval of 500 minor units queued behind the run: the run closes at its bound, before the removal's, and the meter gets the 500", async () => {
    const w = await world({ meter: MINUTE, faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    await w.use("r2", 5_000_000);
    await w.clock(END + DAY);
    await w.entries().destroy(HASH);
    let due: number | null = null;
    for (let step = 0; step < 100; step += 1) {
      const snapshot = await w.entries().testInspect();
      if (snapshot.result !== "snapshot" || snapshot.due === null || snapshot.due === due) break;
      due = snapshot.due;
      await w.clock(due);
    }
    const own = w.meter.recordedEvents(ACCOUNT).filter((each) => !each.identifier.endsWith(":foreign"));
    expect((await w.entries().testInspect()).result).toBe("no-such-account");
    expect(own.map((each) => each.amountMinor)).toEqual([3, 500]);
    w.close();
  });

  test("an explained read one span after the run's first unexplained read resends the interval and does not close it", async () => {
    const w = await world({ meter: { ...MINUTE, readLagSeconds: 170_000 }, faults: [NONE, LOST] });
    await cutPair(w);
    await w.clock(END + 40_000);
    const since = END + 40_000 + SPAN;
    await w.clock(since);
    await tries(w, open(1));
    const [, row] = await w.intervals();
    expect([row?.outcome, row?.refusal, row?.unexplained_since, row?.unexplained_total, Number(row?.closed_at) - since]).toEqual(["acknowledged", null, null, null, SPAN]);
    expect(kinds(w.calls).slice(-3)).toEqual(["read total 0", "read total 3", "send recorded"]);
    expect(count(w.lines, NOT_BILLABLE)).toBe(0);
    w.close();
  });

  test("a failed read inside a run keeps the stored time of its first unexplained total", async () => {
    const w = await world({ meter: MINUTE, faults: [{ kind: "response-lost" }, ...Array.from({ length: 8 }, () => NONE), LOST, LOST] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const since = END + DAY;
    await w.clock(since);
    let afterTheFailedReads: unknown = null;
    await tries(w, (rows) => {
      if (afterTheFailedReads === null && kinds(w.calls).filter((each) => each === "read failed").length === 2) afterTheFailedReads = rows[0]?.unexplained_since;
      return rows[0]?.outcome === null;
    });
    const [row] = await w.intervals();
    expect(afterTheFailedReads).toBe(since);
    expect(kinds(w.calls).filter((each) => each === "read failed")).toHaveLength(2);
    expect([row?.outcome, row?.refusal, row?.unexplained_since, Number(row?.closed_at) - since]).toEqual(["not-billable", "unexplained-total", since, SPAN]);
    w.close();
  });

  test("the close keeps a non-zero neighbours' sum", async () => {
    const w = await world({ meter: MINUTE, faults: [NONE, { kind: "response-lost" }] });
    await cutPair(w);
    foreign(w, 5);
    await w.clock(END + 40_000);
    const since = END + 40_000 + DAY;
    await w.clock(since);
    await tries(w, open(1));
    const [x, y] = await w.intervals();
    expect(x?.outcome).toBe("acknowledged");
    expect(state(y)).toEqual(["not-billable", "unexplained-total", 0, null, 1, since, 10, 3]);
    w.close();
  });
});
