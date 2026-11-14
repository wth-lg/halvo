import { describe, expect, test } from "vitest";
import { SYNC_RETRY_MS } from "../../src/metering/index.ts";
import { ACCOUNT, HASH, HOUR, START, subscriptionEvent, world } from "./world.ts";

const MINUTE = 60_000;
const END_OF_FIRST = START + HOUR;

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

describe("the cut at a billing-period end (unit 5.1)", () => {
  test("a billing period cuts the open interval at its start, slots count from its start, none spans its end, and after its end they count from that end", async () => {
    const w = await world();
    await w.use("r0", 10_000);
    const period = { start: START + 30 * MINUTE, end: START + 100 * MINUTE };
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_period", providerTime: START, period }));
    await w.clock(START + 40 * MINUTE);
    await w.use("r1", 10_000);
    await w.clock(START + 95 * MINUTE);
    await w.use("r2", 10_000);
    await w.clock(START + 105 * MINUTE);
    await w.use("r3", 10_000);
    await w.clock(START + 160 * MINUTE);
    const rows = (await w.intervals()).map((row) => [row.start, row.end_at, row.timestamp, row.outcome]);
    expect(rows).toEqual([
      [START, period.start, period.start / 1000 - 1, "acknowledged"],
      [period.start, START + 90 * MINUTE, (START + 90 * MINUTE) / 1000 - 1, "acknowledged"],
      [START + 90 * MINUTE, period.end, period.end / 1000 - 1, "acknowledged"],
      [period.end, period.end + HOUR, (period.end + HOUR) / 1000 - 1, "acknowledged"],
    ]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.amountMinor)).toEqual([1, 1, 1, 1]);
    w.close();
  });

  test("the same billing period given twice changes nothing", async () => {
    const w = await world();
    await w.use("r0", 10_000);
    const period = { start: START + 30 * MINUTE, end: START + 100 * MINUTE };
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_1", providerTime: START, period }));
    const once = await w.tables();
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_2", providerTime: START + 1000, period }));
    const twice = await w.tables();
    expect([twice.metering_account, twice.metering_interval]).toEqual([once.metering_account, once.metering_interval]);
    w.close();
  });
});

describe("the cut at the destroy step (unit 5.1)", () => {
  test("the destroy step cuts the open interval at its own time, sends what is unsent before it cancels, and sends the remainder under one minor unit nowhere", async () => {
    const w = await world();
    await w.use("r1", 25_500);
    await w.clock(START + 10 * MINUTE);
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    expect(kinds(w.calls)).toEqual(["send recorded", "cancel nothing-to-cancel"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor, each.timestamp])).toEqual([
      [`${ACCOUNT}:${START / 1000}`, 2, (START + 10 * MINUTE) / 1000 - 1],
    ]);
    expect(await w.entries().testInspect()).toEqual({ result: "no-such-account" });
    w.close();
  });

  test("a destroy whose send gets no answer answers not-finished and makes no cancel call; the next destroy sends the same identifier, then cancels", async () => {
    const w = await world({ faults: [{ kind: "request-lost" }] });
    await w.use("r1", 25_500);
    await w.clock(START + 10 * MINUTE);
    const first = await w.entries().destroy(HASH);
    const afterFirst = kinds(w.calls);
    await w.clock(START + 10 * MINUTE + SYNC_RETRY_MS);
    const second = await w.entries().destroy(HASH);
    expect([first, afterFirst, second]).toEqual([{ result: "not-finished" }, ["send failed"], { result: "deleted" }]);
    expect(kinds(w.calls)).toEqual(["send failed", "send recorded", "cancel nothing-to-cancel"]);
    expect(new Set(w.calls.flatMap((each) => (each.kind === "send" ? [each.identifier] : []))).size).toBe(1);
    w.close();
  });

  test("a destroy while a hold of the cut interval is open answers not-finished, and after the hold ends the next destroy seals, sends and cancels", async () => {
    const w = await world();
    await w.use("r0", 10_000);
    await w.reserve("r1");
    const first = await w.entries().destroy(HASH);
    await w.settle("r1", 20_000);
    const second = await w.entries().destroy(HASH);
    expect([first, second]).toEqual([{ result: "not-finished" }, { result: "deleted" }]);
    expect(kinds(w.calls)).toEqual(["send recorded", "cancel nothing-to-cancel"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.amountMinor)).toEqual([3]);
    w.close();
  });
});

describe("the cut at the destroy step after a cancel (unit 5.1)", () => {
  test("an interval that a refused reserve opened after the cancel's cut is cut at the destroy step too, so the delete finishes", async () => {
    const w = await world();
    await w.use("r1", 10_000);
    await w.clock(START + 5 * MINUTE);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: START + 5 * MINUTE, state: "cancelled" }));
    await w.clock(START + 10 * MINUTE);
    expect(await w.reserve("r2")).toEqual({ outcome: "refused", reason: "not-active" });
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    expect(kinds(w.calls)).toEqual(["send recorded", "cancel nothing-to-cancel"]);
    w.close();
  });
});

describe("the remainder at a cancel (unit 5.1)", () => {
  test("a cancel after the last interval was sealed writes the carried remainder off at once, and nothing of it is sent", async () => {
    const w = await world();
    await w.use("r1", 4_000);
    await w.clock(END_OF_FIRST);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: END_OF_FIRST + 10 * MINUTE, state: "cancelled", endedAt: END_OF_FIRST + 5 * MINUTE }));
    const account = (await w.tables()).metering_account ?? [];
    expect(account.map((row) => [row.cut_kind, row.carry, row.written_off])).toEqual([["cancel", 0, 4_000]]);
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    expect(w.meter.recordedEvents(ACCOUNT)).toEqual([]);
    w.close();
  });

  test("an older interval sealed after the cut carries its remainder into the cut interval, so one remainder under one minor unit is written off", async () => {
    const w = await world();
    await w.use("r0", 6_000);
    await w.clock(END_OF_FIRST - 1000);
    await w.reserve("r1");
    await w.clock(END_OF_FIRST + 500);
    await w.use("r2", 6_000);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: END_OF_FIRST + 700, state: "cancelled", endedAt: END_OF_FIRST + 600 }));
    await w.settle("r1", 0);
    await w.clock(END_OF_FIRST + 2000);
    const tables = await w.tables();
    expect((tables.metering_interval ?? []).map((row) => [row.start, row.amount, row.carry_in, row.carry_out, row.outcome])).toEqual([
      [START, 0, 0, 6_000, "no-call"],
      [END_OF_FIRST, 1, 6_000, 2_000, "acknowledged"],
    ]);
    expect((tables.metering_account ?? []).map((row) => [row.carry, row.written_off])).toEqual([[0, 2_000]]);
    expect(w.meter.exactTotal(ACCOUNT, 0, Number.MAX_SAFE_INTEGER)).toBe(1);
    w.close();
  });
});

describe("a cut at or before the open interval's start (unit 5.1)", () => {
  test("after a cancel cut that ends the open interval at its start, a reserve in the same second and one after the seal are refused as not active, and the delete finishes", async () => {
    const w = await world();
    await w.use("r0", 10_000);
    await w.clock(START + 500);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: START + 400, state: "cancelled", endedAt: START - 5 * MINUTE }));
    const before = await w.reserve("r1");
    await w.clock(START + 1000);
    const sealed = (await w.intervals()).map((row) => [row.start, row.end_at, row.outcome]);
    const after = await w.reserve("r2");
    await w.clock(START + 2500);
    const later = await w.reserve("r3");
    const destroy = await w.entries().destroy(HASH);
    expect({ before, sealed, after, later, destroy }).toEqual({
      before: { outcome: "refused", reason: "not-active" },
      sealed: [[START, START, "acknowledged"]],
      after: { outcome: "refused", reason: "not-active" },
      later: { outcome: "refused", reason: "not-active" },
      destroy: { result: "deleted" },
    });
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor])).toEqual([[`${ACCOUNT}:${START / 1000}`, 1]]);
    w.close();
  });
});

describe("the cut at a cancel (unit 5.1)", () => {
  test("a cancel ends the open interval at the cancel's time, and the remainder under one minor unit is written off with the last acknowledgement", async () => {
    const w = await world();
    await w.use("r1", 25_500);
    await w.clock(START + 20 * MINUTE);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: START + 10 * MINUTE, state: "cancelled" }));
    await w.clock(START + 21 * MINUTE);
    const tables = await w.tables();
    expect((tables.metering_interval ?? []).map((row) => [row.start, row.end_at, row.amount, row.carry_out, row.outcome])).toEqual([
      [START, START + 10 * MINUTE, 2, 5_500, "acknowledged"],
    ]);
    expect((tables.metering_account ?? []).map((row) => [row.cut_kind, row.carry, row.written_off])).toEqual([["cancel", 0, 5_500]]);
    expect(w.meter.exactTotal(ACCOUNT, 0, Number.MAX_SAFE_INTEGER)).toBe(2);
    w.close();
  });
});
