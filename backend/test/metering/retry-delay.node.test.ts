import { describe, expect, test } from "vitest";
import * as metering from "../../src/metering/index.ts";
import { SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS } from "../../src/metering/index.ts";
import { HOUR, START, world } from "./world.ts";

const END = START + HOUR;
const MARGIN = 300_000;
const SPAN = UNIQUENESS_WINDOW_MS - MARGIN;
const WINDOW_END = END + SPAN;
const CAP = 3_600_000;

function doublingOffsets(until: number): number[] {
  const offsets = [0];
  let gap = SYNC_RETRY_MS / 1000;
  for (;;) {
    const next = (offsets.at(-1) ?? 0) + gap;
    if (next >= until) return offsets;
    offsets.push(next);
    gap = Math.min(2 * gap, CAP / 1000);
  }
}

test("the cap of the delay between the tries of one interval is 3,600 s", () => {
  expect(metering.RETRY_DELAY_CAP_MS).toBe(CAP);
});

describe("the delay between the tries of one interval (unit 5.4, ruling P7-77, part 4; contract 3.6)", { timeout: 300_000 }, () => {
  test("case 11: a meter that never answers gets 29 sends in the first 86,100 s, at a delay that doubles from the retry delay to RETRY_DELAY_CAP_MS; past the window it gets reads alone, the last one before the late point exactly at it, and the try after a change comes one retry delay later", async () => {
    const w = await world({ always: { kind: "request-lost" } });
    await w.use("r1", 30_000);
    await w.clock(END);
    const nextTry = async () => Number((await w.intervals())[0]?.next_attempt);
    let lastSend = END;
    for (let tries = 0; tries < 5_000; tries += 1) {
      const at = await nextTry();
      if (at >= lastSend + SPAN) break;
      await w.clock(at);
      const last = w.calls.at(-1);
      if (last?.kind === "send") lastSend = last.at;
    }
    const latePoint = lastSend + SPAN;
    const sends = w.calls.filter((each) => each.kind === "send").map((each) => (each.at - END) / 1000);
    const readsBeforeTheLatePoint = w.calls.filter((each) => each.kind === "read").map((each) => each.at);
    expect(sends).toEqual(doublingOffsets(SPAN / 1000));
    expect(sends).toHaveLength(29);
    expect(readsBeforeTheLatePoint.every((at) => at >= WINDOW_END && at < latePoint)).toBe(true);
    expect(await nextTry()).toBe(latePoint);
    w.faults.push({ kind: "none" });
    await w.clock(latePoint);
    const [row] = await w.intervals();
    expect(w.calls.slice(-2).map((each) => [each.kind, each.at, each.answer])).toEqual([
      ["read", latePoint, "total 0"],
      ["send", latePoint, "failed"],
    ]);
    expect([row?.next_attempt, row?.delay, row?.unanswered_at]).toEqual([latePoint + SYNC_RETRY_MS, SYNC_RETRY_MS, latePoint]);
    await w.clock(latePoint + SYNC_RETRY_MS);
    const [after] = await w.intervals();
    expect(w.calls.at(-1)?.kind).toBe("read");
    expect([after?.next_attempt, after?.delay]).toEqual([latePoint + SYNC_RETRY_MS + 2 * SYNC_RETRY_MS, 2 * SYNC_RETRY_MS]);
    w.close();
  });

  test("the delay stays at RETRY_DELAY_CAP_MS while the tries change nothing", async () => {
    const w = await world({ always: { kind: "request-lost" } });
    await w.use("r1", 30_000);
    await w.clock(END);
    const gaps: number[] = [];
    let at = END;
    for (let tries = 0; tries < 12; tries += 1) {
      const next = Number((await w.intervals())[0]?.next_attempt);
      gaps.push(next - at);
      at = next;
      await w.clock(at);
    }
    expect(gaps).toEqual([60_000, 120_000, 240_000, 480_000, 960_000, 1_920_000, CAP, CAP, CAP, CAP, CAP, CAP]);
    expect((await w.intervals())[0]?.delay).toBe(CAP);
    w.close();
  });

  test("a refusal stored for the first time sets the delay back to the retry delay, and the same refusal again while it is pending changes nothing, so the delay doubles", async () => {
    const w = await world({ subscribed: false, faults: [{ kind: "request-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const [row] = await w.intervals();
    expect(w.calls.map((each) => `${each.kind} ${each.answer}`)).toEqual(["send failed", "send refused unknown_account", "read total 0", "send refused unknown_account"]);
    expect([row?.pending_refusal, row?.refusals, row?.delay, row?.next_attempt]).toEqual(["unknown_account", 0, 4 * SYNC_RETRY_MS, END + SYNC_RETRY_MS + 4 * SYNC_RETRY_MS]);
    w.close();
  });

  test("a meter that never answers: the close at SEND_LIMIT_MS comes at the bound itself, since no try is set later than it, after about 820 tries at the capped delay", async () => {
    const w = await world({ always: { kind: "request-lost" } });
    await w.use("r1", 30_000);
    await w.clock(END);
    const limit = END - 1000 + metering.SEND_LIMIT_MS;
    let at = END;
    for (let tries = 0; tries < 2_000; tries += 1) {
      const [row] = await w.intervals();
      if (row?.outcome !== null) break;
      at = Number(row?.next_attempt);
      await w.clock(at);
    }
    const [row] = await w.intervals();
    expect([row?.outcome, row?.refusal, row?.closed_at, at]).toEqual(["not-billable", "send_limit", limit, limit]);
    expect(w.calls.length).toBeGreaterThan(800);
    expect(w.calls.length).toBeLessThan(900);
    w.close();
  });
});
