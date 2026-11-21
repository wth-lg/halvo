import { describe, expect, test } from "vitest";
import { DAY, HOUR, world } from "./world.ts";
import { count, cutPair, END, foreign, kinds, NOT_BILLABLE, open, SPAN, stopAfterTheNextCommit, tries, UNEXPLAINED } from "./unexplained-support.ts";

const MINUTE = { readGranularitySeconds: 60 };
const NONE = { kind: "none" } as const;
const LOST = { kind: "request-lost" } as const;

function reads(n: number, total: number): string[] {
  return Array.from({ length: n }, () => `read total ${total}`);
}

describe("one usage-total-unexplained line for an interval's first unexplained total (unit 5.7, ruling P7-86 C)", { timeout: 120_000 }, () => {
  test("the first unexplained read writes one usage-total-unexplained line and sets the row's flag unexplained_logged", async () => {
    const w = await world({ meter: MINUTE, faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    const before = [count(w.lines, UNEXPLAINED), (await w.intervals())[0]?.unexplained_logged];
    await w.clock(END + DAY);
    const [row] = await w.intervals();
    expect(before).toEqual([0, 0]);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "read total 8"]);
    expect([count(w.lines, UNEXPLAINED), row?.unexplained_logged]).toEqual([1, 1]);
    w.close();
  });

  test("every later unexplained read of that interval writes none: 29 unexplained reads to the close, one line, and the flag stays set after the close", async () => {
    const w = await world({ meter: MINUTE, faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    await w.clock(END + DAY);
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed after recorded", ...reads(29, 8)]);
    expect([row?.outcome, row?.refusal, row?.unexplained_logged]).toEqual(["not-billable", "unexplained-total", 1]);
    expect([count(w.lines, UNEXPLAINED), count(w.lines, NOT_BILLABLE)]).toEqual([1, 1]);
    w.close();
  });

  test("unexplained, explained and unexplained again: still one line, and the flag is never cleared", async () => {
    const latePoint = END + 40_000 + SPAN;
    const w = await world({
      meter: { ...MINUTE, readLagSeconds: 30 * 3600 },
      faults: [NONE, LOST, ...Array.from({ length: 12 }, () => NONE), LOST],
    });
    await cutPair(w);
    foreign(w, 5, END + 40_000 + 2 * HOUR);
    await w.clock(END + 40_000);
    await w.clock(latePoint);
    const first = (await w.intervals())[1];
    let explained: unknown = null;
    await tries(w, (rows) => {
      if (explained === null && kinds(w.calls).includes("read total 3")) explained = [rows[1]?.unexplained_since, rows[1]?.unexplained_logged];
      return !kinds(w.calls).includes("read total 8");
    });
    const [, row] = await w.intervals();
    expect([first?.unexplained_since, first?.unexplained_logged]).toEqual([latePoint, 1]);
    expect(explained).toEqual([null, 1]);
    expect(kinds(w.calls)).toEqual(["send recorded", "send failed", ...reads(11, 0), "read total 3", "send failed", ...reads(5, 3), "read total 8"]);
    expect([row?.outcome, row?.unexplained_since, row?.unexplained_logged]).toEqual([null, END + 40_000 + 115_200_000, 1]);
    expect(count(w.lines, UNEXPLAINED)).toBe(1);
    w.close();
  });

  test("a stop after the step's commit and before the line: no line after the restart, and the flag is set", async () => {
    const w = await world({ meter: MINUTE });
    const stop = stopAfterTheNextCommit(w);
    w.faults.push({ kind: "response-lost" }, { kind: "during", run: async () => stop(), after: NONE });
    await w.use("r1", 30_000);
    foreign(w, 5);
    await w.clock(END);
    await w.clock(END + DAY);
    const afterTheStop = [count(w.lines, UNEXPLAINED), (await w.intervals())[0]?.unexplained_logged];
    w.restart();
    await tries(w, open(0));
    const [row] = await w.intervals();
    expect(afterTheStop).toEqual([0, 1]);
    expect([row?.outcome, row?.refusal, row?.unexplained_logged]).toEqual(["not-billable", "unexplained-total", 1]);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", ...reads(29, 8)]);
    expect([count(w.lines, UNEXPLAINED), count(w.lines, NOT_BILLABLE)]).toEqual([0, 1]);
    w.close();
  });

  test("two intervals of one minute behind one foreign event: two lines, one for each interval", async () => {
    const w = await world({ meter: MINUTE, faults: [LOST, ...Array.from({ length: 29 }, () => NONE), LOST] });
    await cutPair(w);
    foreign(w, 5);
    await w.clock(END + 40_000);
    await w.clock(END + 40_000 + DAY);
    await tries(w, open(0));
    const [first] = await w.intervals();
    await w.clock(Number(first?.closed_at) + DAY);
    await tries(w, open(1));
    const rows = await w.intervals();
    expect(rows.map((row) => [row.outcome, row.refusal, row.unexplained_logged])).toEqual([
      ["not-billable", "unexplained-total", 1],
      ["not-billable", "unexplained-total", 1],
    ]);
    expect([count(w.lines, UNEXPLAINED), count(w.lines, NOT_BILLABLE)]).toEqual([2, 2]);
    w.close();
  });
});
