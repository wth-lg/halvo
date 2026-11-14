import { describe, expect, test } from "vitest";
import * as metering from "../../src/metering/index.ts";
import { SEND_LIMIT_MS, SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS } from "../../src/metering/index.ts";
import { ACCOUNT, HASH, HOUR, START, world } from "./world.ts";

const END = START + HOUR;
const TIMESTAMP = END / 1000 - 1;
const MARGIN = 300_000;
const SPAN = UNIQUENESS_WINDOW_MS - MARGIN;
const WINDOW_END = END + SPAN;
const LAG_23_HOURS = 82_800;
const NOT_BILLABLE = '{"event":"usage-not-billable"}';

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

async function state(w: Awaited<ReturnType<typeof world>>): Promise<unknown[]> {
  const [row] = await w.intervals();
  return [row?.outcome, row?.refusal, row?.refusals, row?.pending_refusal, row?.unanswered];
}

test("the late point's span is the uniqueness window less the shipped margin of 300 s", () => {
  expect(metering.LATE_READ_SPAN_MS).toBe(86_100_000);
  expect(metering.LATE_READ_SPAN_MS).toBe(UNIQUENESS_WINDOW_MS - MARGIN);
});

test("with a margin above 300 s the late point is never later than the uniqueness window less the margin: with a margin of 600 s the read 85,800 s after the unanswered send decides", async () => {
  const w = await world({ config: { marginSeconds: 600 }, faults: [{ kind: "request-lost" }, { kind: "refuse", reason: "unknown_account" }, { kind: "none" }, { kind: "refuse", reason: "unknown_account" }] });
  await w.use("r1", 30_000);
  await w.clock(END);
  await w.clock(END + SYNC_RETRY_MS);
  await w.clock(END + UNIQUENESS_WINDOW_MS - 600_000);
  expect(kinds(w.calls)).toEqual(["send failed", "send refused unknown_account", "read total 0", "send refused unknown_account", "read total 0", "send recorded"]);
  expect(await state(w)).toEqual(["acknowledged", null, 1, null, 0]);
  w.close();
});

describe("a read made before the late point decides nothing (unit 5.4, ruling P7-77, part 7)", () => {
  test("case 7: a send recorded late in the window of the first attempt, its answer lost, then refusals: the late point counts from that send, the reads before it only acknowledge, and the interval is acknowledged once the lag has passed", async () => {
    const w = await world({
      meter: { readLagSeconds: LAG_23_HOURS },
      faults: [
        { kind: "request-lost" },
        { kind: "response-lost" },
        { kind: "refuse", reason: "unknown_account" },
        { kind: "none" },
        { kind: "refuse", reason: "unknown_account" },
        { kind: "none" },
        { kind: "refuse", reason: "unknown_account" },
      ],
    });
    await w.use("r1", 30_000);
    const recordedAt = END + 80_000_000;
    for (const at of [END, recordedAt, recordedAt + HOUR, END + 87_000_000, END + 90_000_000, recordedAt + LAG_23_HOURS * 1000 + 200_000]) await w.clock(at);
    expect(kinds(w.calls)).toEqual([
      "send failed",
      "send failed after recorded",
      "send refused unknown_account",
      "read total 0",
      "send refused unknown_account",
      "read total 0",
      "read failed",
      "read total 3",
    ]);
    expect(await state(w)).toEqual(["acknowledged", null, 0, null, 1]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toEqual([]);
    w.close();
  });

  test("case 8: wholly past the window of the first attempt, a resend that the meter records with its answer lost sets a new late point; the reads before it name no send, no refusal arrives, and the interval is acknowledged once the lag has passed", async () => {
    const w = await world({
      meter: { readLagSeconds: LAG_23_HOURS },
      faults: [
        { kind: "request-lost" },
        { kind: "none" },
        { kind: "response-lost" },
        { kind: "none" },
        { kind: "refuse", reason: "unknown_account" },
        { kind: "none" },
        { kind: "refuse", reason: "unknown_account" },
        { kind: "none" },
        { kind: "refuse", reason: "unknown_account" },
      ],
    });
    await w.use("r1", 30_000);
    const resentAt = END + UNIQUENESS_WINDOW_MS;
    for (const at of [END, resentAt, resentAt + HOUR, resentAt + 2 * HOUR, resentAt + 3 * HOUR, resentAt + 83_000_000, resentAt + 84_000_000]) await w.clock(at);
    expect(kinds(w.calls)).toEqual([
      "send failed",
      "read total 0",
      "send failed after recorded",
      "read total 0",
      "read failed",
      "read total 0",
      "read failed",
      "read total 3",
    ]);
    expect(await state(w)).toEqual(["acknowledged", null, 0, null, 1]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toEqual([]);
    w.close();
  });

  test("case 10: a request truly lost, then a refusal that stays: rounds inside the window count nothing, the late read counts the refusal, and two more refused sends one retry delay apart close the interval as not billable, after which the account delete finishes", async () => {
    const w = await world({ subscribed: false, faults: [{ kind: "request-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    await w.clock(END + 2 * HOUR);
    const inTheWindow = await state(w);
    await w.clock(WINDOW_END);
    const afterTheLateRead = await state(w);
    await w.clock(WINDOW_END + SYNC_RETRY_MS);
    const sends = w.calls.filter((each) => each.kind === "send").map((each) => each.at);
    expect(inTheWindow).toEqual([null, null, 0, "unknown_account", 1]);
    expect(afterTheLateRead).toEqual([null, null, 2, null, 0]);
    expect(await state(w)).toEqual(["not-billable", "unknown_account", 3, null, 0]);
    expect(sends.slice(-2)).toEqual([WINDOW_END, WINDOW_END + SYNC_RETRY_MS]);
    expect(kinds(w.calls)).toEqual([
      "send failed",
      "send refused unknown_account",
      "read total 0",
      "send refused unknown_account",
      "read total 0",
      "send refused unknown_account",
      "read total 0",
      "send refused unknown_account",
      "read total 0",
      "send refused unknown_account",
    ]);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toEqual([NOT_BILLABLE]);
    expect(w.meter.recordedEvents(ACCOUNT)).toEqual([]);
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    w.close();
  });

  test("a refusal with another reason while one is pending replaces the pending reason, counts nothing, ends the round and sets the delay back to the retry delay", async () => {
    const w = await world({
      faults: [{ kind: "request-lost" }, { kind: "refuse", reason: "unknown_account" }, { kind: "none" }, { kind: "refuse", reason: "amount_not_whole_minor_units" }],
    });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed", "send refused unknown_account", "read total 0", "send refused amount_not_whole_minor_units"]);
    expect(await state(w)).toEqual([null, null, 0, "amount_not_whole_minor_units", 1]);
    expect([row?.delay, row?.next_attempt]).toEqual([SYNC_RETRY_MS, END + 2 * SYNC_RETRY_MS]);
    w.close();
  });

  test("an age refusal after an unanswered attempt is stored as pending and not counted: no send follows it, and the late read that lacks the amount closes the interval as not billable", async () => {
    const w = await world({ faults: [{ kind: "request-lost" }, { kind: "refuse", reason: "timestamp_too_old" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    await w.clock(END + 2 * HOUR);
    const beforeTheLatePoint = { state: await state(w), lines: [...w.lines] };
    await w.clock(WINDOW_END);
    expect(beforeTheLatePoint).toEqual({ state: [null, null, 0, "timestamp_too_old", 1], lines: [] });
    expect(kinds(w.calls)).toEqual(["send failed", "send refused timestamp_too_old", "read total 0", "read total 0", "read total 0"]);
    expect(await state(w)).toEqual(["not-billable", "timestamp_too_old", 0, null, 0]);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toEqual([NOT_BILLABLE]);
    w.close();
  });

  test("the time of the newest send is stored before the call, so after a stop that loses the answer of a send that the meter took, the late point counts from that send", async () => {
    let seen: unknown = "no call";
    const sentAt = END + 80_000_000;
    const w = await world({ meter: { readLagSeconds: LAG_23_HOURS } });
    w.faults.push(
      { kind: "request-lost" },
      {
        kind: "during",
        run: async () => {
          const [row] = await w.intervals();
          seen = [row?.sent_at, row?.answer_due];
          w.stopNextStep("before");
        },
        after: { kind: "none" },
      },
      { kind: "refuse", reason: "unknown_account" },
      { kind: "none" },
      { kind: "refuse", reason: "unknown_account" },
      { kind: "none" },
      { kind: "refuse", reason: "unknown_account" },
    );
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(sentAt);
    w.restart();
    for (const at of [sentAt + HOUR, END + 87_000_000, END + 90_000_000, sentAt + LAG_23_HOURS * 1000 + 200_000]) await w.clock(at);
    expect(seen).toEqual([sentAt, 1]);
    expect(kinds(w.calls)).toEqual([
      "send failed",
      "send recorded",
      "send refused unknown_account",
      "read total 0",
      "send refused unknown_account",
      "read total 0",
      "read failed",
      "read total 3",
    ]);
    expect(await state(w)).toEqual(["acknowledged", null, 0, null, 1]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    w.close();
  });
});

describe("what the late read leaves as it was (unit 5.4, ruling P7-77, part 7)", () => {
  test("case 12: the close at SEND_LIMIT_MS makes no read before it, also with an age refusal pending, and keeps the mark on the row while the meter holds the amount", async () => {
    const w = await world({ always: { kind: "response-lost" }, faults: [{ kind: "response-lost" }, { kind: "refuse", reason: "timestamp_too_old" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const before = kinds(w.calls);
    await w.clock(TIMESTAMP * 1000 + SEND_LIMIT_MS);
    expect(before).toEqual(["send failed after recorded", "send refused timestamp_too_old", "read failed"]);
    expect(kinds(w.calls)).toEqual(before);
    expect(await state(w)).toEqual(["not-billable", "send_limit", 0, null, 1]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    expect(w.lines).toEqual([NOT_BILLABLE]);
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    w.close();
  });

  test("case 13, the premise: an amount that reaches the meter's reads one second before the late point is acknowledged by the late read, with no second send", async () => {
    const w = await world({ meter: { readLagSeconds: SPAN / 1000 - 1 }, faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(WINDOW_END);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "read total 3"]);
    expect(await state(w)).toEqual(["acknowledged", null, 0, null, 1]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    w.close();
  });

  for (const [amount, outcome, last] of [
    [5, null, "read total 5"],
    [3, "acknowledged", null],
  ] as const) {
    test(`case 14: an event at the meter that no interval of the module owns, of ${amount} minor units in the interval's minute, ${outcome === null ? "leaves the total unexplained, so the interval is read again and never sent" : "with the interval's own amount, acknowledges the interval with no send"}`, async () => {
      const w = await world({ meter: { readGranularitySeconds: 60 }, faults: [{ kind: "request-lost" }] });
      await w.use("r1", 30_000);
      w.meter.send({ account: ACCOUNT, identifier: `${ACCOUNT}:foreign`, amountMinor: amount, timestamp: TIMESTAMP - 10 }, END / 1000);
      await w.clock(END);
      await w.clock(END + UNIQUENESS_WINDOW_MS);
      await w.clock(END + UNIQUENESS_WINDOW_MS + HOUR);
      expect(kinds(w.calls)).toEqual(["send failed", `read total ${amount}`, ...(last === null ? [] : [last])]);
      expect((await w.intervals())[0]?.outcome).toBe(outcome);
      w.close();
    });
  }
});
