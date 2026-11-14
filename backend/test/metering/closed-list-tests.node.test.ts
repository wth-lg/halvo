import { describe, expect, test } from "vitest";
import { SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS } from "../../src/metering/index.ts";
import { ACCOUNT, HOUR, START, subscriptionEvent, world } from "./world.ts";

const MINUTE = 60_000;
const END = START + HOUR;
const NOT_BILLABLE = '{"event":"usage-not-billable"}';

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

describe("the two tests that B-32 names (unit 5.1)", () => {
  test("B-32: a read that a pending refusal named and that gets no answer is followed by a read again, not by a send", async () => {
    const w = await world({ faults: [{ kind: "response-lost" }, { kind: "refuse", reason: "unknown_account" }, { kind: "request-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    await w.clock(END + 3 * SYNC_RETRY_MS);
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "send refused unknown_account", "read failed", "read total 3"]);
    expect([row?.outcome, row?.refusals]).toEqual(["acknowledged", 0]);
    w.close();
  });

  test("B-32: the interval that a cancel cut and whose amount is 0 ends written-off, with no call, and its remainder is written off", async () => {
    const w = await world();
    await w.use("r1", 4_000);
    await w.clock(START + 20 * MINUTE);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: START + 10 * MINUTE, state: "cancelled", endedAt: START + 10 * MINUTE }));
    await w.clock(START + 21 * MINUTE);
    const tables = await w.tables();
    expect((tables.metering_interval ?? []).map((row) => [row.start, row.end_at, row.amount, row.outcome])).toEqual([[START, START + 10 * MINUTE, 0, "written-off"]]);
    expect((tables.metering_account ?? []).map((row) => [row.carry, row.written_off])).toEqual([[0, 4_000]]);
    expect(w.calls).toEqual([]);
    w.close();
  });
});

describe("the two rules that B-46 names (unit 5.1)", () => {
  test("B-46: a billing period whose end falls inside the open interval cuts that interval at the period's end", async () => {
    const w = await world();
    await w.use("r0", 10_000);
    const periodEnd = START + 20 * MINUTE;
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_period", providerTime: START, period: { start: START - 10 * 24 * HOUR, end: periodEnd } }));
    const cut = (await w.intervals()).map((row) => [row.start, row.end_at]);
    await w.clock(END);
    expect(cut).toEqual([[START, periodEnd]]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => [each.amountMinor, each.timestamp])).toEqual([[1, periodEnd / 1000 - 1]]);
    w.close();
  });

  test("B-46: the line usage-not-billable is written after its step commits, so a step that rolls back writes none", async () => {
    const w = await world();
    w.faults.push(
      { kind: "during", run: async () => w.stopNextStep("after"), after: { kind: "refuse", reason: "timestamp_too_old" } },
      { kind: "refuse", reason: "timestamp_too_old" },
    );
    await w.use("r1", 30_000);
    await w.clock(END);
    const afterTheRollback = { outcome: (await w.intervals())[0]?.outcome, lines: [...w.lines] };
    w.restart();
    await w.clock(END + SYNC_RETRY_MS);
    await w.clock(END + UNIQUENESS_WINDOW_MS - 300_000);
    const [row] = await w.intervals();
    expect(afterTheRollback.outcome).toBe(null);
    expect(afterTheRollback.lines.filter((line) => line === NOT_BILLABLE)).toEqual([]);
    expect(afterTheRollback.lines.some((line) => line.includes('"event":"alarm-error"'))).toBe(true);
    expect([row?.outcome, row?.refusal]).toEqual(["not-billable", "timestamp_too_old"]);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toEqual([NOT_BILLABLE]);
    w.close();
  });
});
