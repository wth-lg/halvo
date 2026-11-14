import { describe, expect, test } from "vitest";
import { REFUSAL_LIMIT, SEND_LIMIT_MS, SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS } from "../../src/metering/index.ts";
import { ACCOUNT, DAY, HASH, HOUR, START, world } from "./world.ts";

const END = START + HOUR;
const IDENTIFIER = `${ACCOUNT}:${START / 1000}`;
const TIMESTAMP = END / 1000 - 1;
const MARGIN = 300_000;
const NOT_BILLABLE = '{"event":"usage-not-billable"}';

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

describe("the seal (unit 5.1)", () => {
  test("the seal waits for the interval's open holds: an alarm after the interval's end with a hold open seals nothing, and the seal after the hold ends takes its cost", async () => {
    const w = await world();
    await w.use("r0", 30_000);
    await w.clock(END - 1000);
    await w.reserve("r1");
    await w.clock(END + 500);
    await w.entries().alarm();
    const whileOpen = { intervals: (await w.intervals()).map((row) => row.sealed_at), calls: w.calls.length };
    await w.settle("r1", 25_000);
    await w.clock(END + 1000);
    const [sealed] = await w.intervals();
    expect(whileOpen).toEqual({ intervals: [null], calls: 0 });
    expect(sealed).toEqual(expect.objectContaining({ start: START, end_at: END, reported: 55_000, amount: 5, carry_out: 5_000, outcome: "acknowledged" }));
    expect(kinds(w.calls)).toEqual(["send recorded"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor, each.timestamp])).toEqual([[IDENTIFIER, 5, TIMESTAMP]]);
    w.close();
  });

  test("an interval whose amount is 0 is acknowledged with no call, and the remainder under one minor unit is carried into the next interval", async () => {
    const w = await world();
    await w.use("r1", 4_000);
    await w.clock(END);
    await w.clock(END + 1000);
    await w.use("r2", 7_000);
    await w.clock(END + HOUR);
    const rows = (await w.intervals()).map((row) => [row.start, row.amount, row.carry_in, row.carry_out, row.outcome]);
    expect(rows).toEqual([
      [START, 0, 0, 4_000, "no-call"],
      [END, 1, 4_000, 1_000, "acknowledged"],
    ]);
    expect(kinds(w.calls)).toEqual(["send recorded"]);
    w.close();
  });
});

describe("one identifier for every attempt (unit 5.1)", () => {
  test("a send that gets no answer is sent again under the same identifier, and the meter holds one event", async () => {
    const w = await world({ faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    expect(w.calls.map((each) => (each.kind === "send" ? [each.identifier, each.amountMinor, each.timestamp, each.answer] : each.kind))).toEqual([
      [IDENTIFIER, 3, TIMESTAMP, "failed after recorded"],
      [IDENTIFIER, 3, TIMESTAMP, "duplicate"],
    ]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    expect((await w.intervals())[0]?.outcome).toBe("acknowledged");
    w.close();
  });

  test("inside the window every retry sends the same identifier again with no read, up to the last retry before the margin", async () => {
    const w = await world({ always: { kind: "request-lost" } });
    await w.use("r1", 30_000);
    for (const at of [END, END + SYNC_RETRY_MS, END + 3 * SYNC_RETRY_MS, END + UNIQUENESS_WINDOW_MS - MARGIN - 1]) await w.clock(at);
    expect(w.calls.map((each) => (each.kind === "send" ? each.identifier : each.kind))).toEqual([IDENTIFIER, IDENTIFIER, IDENTIFIER, IDENTIFIER]);
    w.close();
  });
});

describe("the first attempt (unit 5.1)", () => {
  test("the time of the first attempt is stored before the first call, and a crash after the call keeps it, so the window counts from it", async () => {
    let seen: unknown = "no call";
    const w = await world({
      faults: [
        {
          kind: "during",
          run: async () => {
            seen = (await w.intervals())[0]?.first_attempt;
          },
          after: { kind: "request-lost" },
        },
      ],
    });
    await w.use("r1", 30_000);
    await w.clock(END);
    expect(seen).toBe(END);
    w.restart();
    await w.clock(END + UNIQUENESS_WINDOW_MS - MARGIN);
    expect(kinds(w.calls)).toEqual(["send failed", "read total 0", "send recorded"]);
    expect((await w.intervals())[0]?.first_attempt).toBe(END);
    w.close();
  });
});

describe("the uniqueness window (unit 5.1)", () => {
  test("after the window a read comes first, and the meter that holds the amount gets no second send", async () => {
    const w = await world({ faults: [{ kind: "response-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + UNIQUENESS_WINDOW_MS);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "read total 3"]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
    expect((await w.intervals())[0]?.outcome).toBe("acknowledged");
    w.close();
  });

  test("after the window a resend follows the read only when the provider-side total lacks the amount", async () => {
    const w = await world({ faults: [{ kind: "request-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + UNIQUENESS_WINDOW_MS);
    expect(kinds(w.calls)).toEqual(["send failed", "read total 0", "send recorded"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.identifier)).toEqual([IDENTIFIER]);
    w.close();
  });

  test("the margin: a retry 300 s and 1 ms before the window ends still sends, and one 300 s before it reads first", async () => {
    const early = await world({ faults: [{ kind: "request-lost" }, { kind: "request-lost" }] });
    await early.use("r1", 30_000);
    await early.clock(END);
    await early.clock(END + UNIQUENESS_WINDOW_MS - MARGIN - 1);
    const late = await world({ faults: [{ kind: "request-lost" }, { kind: "request-lost" }] });
    await late.use("r1", 30_000);
    await late.clock(END);
    await late.clock(END + UNIQUENESS_WINDOW_MS - MARGIN);
    expect([kinds(early.calls), kinds(late.calls)]).toEqual([
      ["send failed", "send failed"],
      ["send failed", "read failed"],
    ]);
    early.close();
    late.close();
  });

  test("the window counts from the first attempt and never again from a later one, so a resend past the meter's window follows a read", async () => {
    const w = await world({ faults: [{ kind: "response-lost" }, { kind: "request-lost" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + UNIQUENESS_WINDOW_MS - MARGIN - 100_000);
    await w.clock(END + UNIQUENESS_WINDOW_MS + 100_000);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "send failed", "read total 3"]);
    expect(w.meter.exactTotal(ACCOUNT, 0, Number.MAX_SAFE_INTEGER)).toBe(3);
    w.close();
  });
});

describe("a read whose range holds another interval (unit 5.1, S9)", () => {
  async function wideRead(second: "response-lost" | "request-lost") {
    const w = await world({ meter: { readGranularitySeconds: 86_400 }, faults: [{ kind: "none" }, { kind: second }] });
    await w.use("r1", 10_000);
    await w.clock(END);
    await w.clock(END + 1000);
    await w.use("r2", 30_000);
    await w.clock(END + HOUR);
    await w.clock(END + HOUR + UNIQUENESS_WINDOW_MS);
    return w;
  }

  test("the acknowledged intervals inside the read's range are subtracted before the compare: the provider that holds the amount acknowledges it", async () => {
    const w = await wideRead("response-lost");
    expect(kinds(w.calls)).toEqual(["send recorded", "send failed after recorded", "read total 4"]);
    expect((await w.intervals()).map((row) => row.outcome)).toEqual(["acknowledged", "acknowledged"]);
    expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(2);
    w.close();
  });

  test("the acknowledged intervals inside the read's range are subtracted before the compare: the provider that lacks the amount gets the resend", async () => {
    const w = await wideRead("request-lost");
    expect(kinds(w.calls)).toEqual(["send recorded", "send failed", "read total 1", "send recorded"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.amountMinor)).toEqual([1, 3]);
    w.close();
  });
});

describe("refusals (unit 5.1)", () => {
  test("a refusal after an unanswered attempt stays pending and uncounted through a read before the late point that lacks the amount, and inside the window that read is followed by one send, whose refusal ends the round", async () => {
    const w = await world({
      faults: [{ kind: "request-lost" }, { kind: "refuse", reason: "unknown_account" }, { kind: "none" }, { kind: "refuse", reason: "unknown_account" }],
    });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed", "send refused unknown_account", "read total 0", "send refused unknown_account"]);
    expect([row?.refusals, row?.pending_refusal, row?.unanswered, row?.outcome]).toEqual([0, "unknown_account", 1, null]);
    w.close();
  });

  test("a refusal that follows an unanswered attempt is stored only after one more read: the read that shows the amount acknowledges", async () => {
    const w = await world({ faults: [{ kind: "response-lost" }, { kind: "refuse", reason: "unknown_account" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "send refused unknown_account", "read total 3"]);
    expect([row?.outcome, row?.refusals, row?.refusal]).toEqual(["acknowledged", 0, null]);
    w.close();
  });

  test("a refusal that follows an unanswered attempt is not counted by a read before the late point that lacks the amount, and the send of the round follows in the same chain", async () => {
    const w = await world({ faults: [{ kind: "request-lost" }, { kind: "refuse", reason: "unknown_account" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const [row] = await w.intervals();
    expect(kinds(w.calls)).toEqual(["send failed", "send refused unknown_account", "read total 0", "send recorded"]);
    expect([row?.outcome, row?.refusals]).toEqual(["acknowledged", 0]);
    w.close();
  });

  test("an old-timestamp refusal after an unanswered attempt is not stored when the read shows the provider holds the amount", async () => {
    const w = await world({ faults: [{ kind: "response-lost" }, { kind: "refuse", reason: "timestamp_too_old" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "send refused timestamp_too_old", "read total 3"]);
    expect((await w.intervals())[0]?.outcome).toBe("acknowledged");
    w.close();
  });

  test("an old-timestamp refusal with no unanswered attempt closes the interval as not billable at once, with one line", async () => {
    const w = await world({ faults: [{ kind: "refuse", reason: "timestamp_too_old" }] });
    await w.use("r1", 30_000);
    await w.clock(END);
    const [row] = await w.intervals();
    expect([row?.outcome, row?.refusal]).toEqual(["not-billable", "timestamp_too_old"]);
    expect(w.lines).toEqual([NOT_BILLABLE]);
    w.close();
  });
});

describe("the two bounds of the sync (unit 5.1, contract 3.6; check 55 reads these two tests by name)", () => {
  test("bound REFUSAL_LIMIT: a meter that refuses with unknown_account three times, one retry delay apart, closes the interval as not billable, logs one line, sends nowhere, and the account delete then finishes", async () => {
    const w = await world({ subscribed: false });
    await w.use("r1", 30_000);
    const refusals: unknown[] = [];
    for (let each = 0; each < REFUSAL_LIMIT; each += 1) {
      await w.clock(END + each * SYNC_RETRY_MS);
      refusals.push((await w.intervals())[0]?.refusals);
    }
    const [row] = await w.intervals();
    const spend = await w.entries().capGet(HASH);
    expect(refusals).toEqual([1, 2, 3]);
    expect([row?.outcome, row?.refusal, row?.amount]).toEqual(["not-billable", "unknown_account", 3]);
    expect(kinds(w.calls)).toEqual(Array(REFUSAL_LIMIT).fill("send refused unknown_account"));
    expect(w.lines).toEqual([NOT_BILLABLE]);
    expect(w.meter.recordedEvents(ACCOUNT)).toEqual([]);
    expect(spend).toEqual(expect.objectContaining({ spend: expect.objectContaining({ spent: 30_000 }) }));
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    expect(kinds(w.calls).slice(REFUSAL_LIMIT)).toEqual(["cancel nothing-to-cancel"]);
    w.close();
  });

  test("bound SEND_LIMIT_MS: a meter that never answers gets one send, the first clock call past the interval's timestamp plus SEND_LIMIT_MS closes the interval as not billable with no further call, logs one line, and the account delete then finishes", async () => {
    const w = await world({ always: { kind: "request-lost" } });
    await w.use("r1", 30_000);
    await w.clock(END);
    const before = w.calls.length;
    await w.clock(TIMESTAMP * 1000 + SEND_LIMIT_MS);
    const [row] = await w.intervals();
    expect(before).toBe(1);
    expect(kinds(w.calls)).toEqual(["send failed"]);
    expect([row?.outcome, row?.refusal]).toEqual(["not-billable", "send_limit"]);
    expect(w.lines).toEqual([NOT_BILLABLE]);
    expect(w.lines.some((line) => line.includes(ACCOUNT))).toBe(false);
    expect(await w.entries().destroy(HASH)).toEqual({ result: "deleted" });
    expect(kinds(w.calls)).toEqual(["send failed", "cancel nothing-to-cancel"]);
    w.close();
  });
});

describe("a sync wake never delays a hold's expiry (unit 5.1; check 55 reads this test by name)", () => {
  test("a hold whose expiry falls due while a sync wake waits on the provider ends at its expiry time", async () => {
    let during: unknown = "no call";
    const w = await world({
      testClock: false,
      faults: [
        {
          kind: "during",
          run: async () => {
            w.at(END + 500 + 3000);
            during = await w.settle("r2", 1_000);
          },
        },
      ],
    });
    await w.use("r1", 30_000);
    w.at(END + 500);
    await w.reserve("r2");
    await w.entries().dispatch("r2");
    w.at(END + 1000);
    await w.entries().alarm();
    w.host.config.testClock = true;
    const tables = await w.tables();
    const r2 = (tables.ledger_holds ?? []).find((row) => row.request_id === "r2");
    expect(during).toEqual({ outcome: "expired", cost: 1_000, overrun: 900 });
    expect([r2?.state, r2?.expires_at, r2?.ended_at]).toEqual(["expired", END + 3500, END + 3500]);
    expect(kinds(w.calls)).toEqual(["send recorded"]);
    w.close();
  });
});

describe("a hold that no call followed, whose expiry falls due while a sync wake waits on the provider (units 5.1 and 4.5)", () => {
  test("it ends voided at its expiry time, a settle after its expiry answers already-ended, and its interval counts and sends nothing of it", async () => {
    let during: unknown = "no call";
    const w = await world({
      testClock: false,
      faults: [
        {
          kind: "during",
          run: async () => {
            w.at(END + 500 + 3000);
            during = await w.settle("r2", 1_000);
          },
        },
      ],
    });
    await w.use("r1", 30_000);
    w.at(END + 500);
    await w.reserve("r2");
    w.at(END + 1000);
    await w.entries().alarm();
    w.host.config.testClock = true;
    const tables = await w.tables();
    const r2 = (tables.ledger_holds ?? []).find((row) => row.request_id === "r2");
    expect(during).toEqual({ outcome: "refused", reason: "already-ended" });
    expect([r2?.state, r2?.expires_at, r2?.ended_at]).toEqual(["voided", END + 3500, END + 3500]);
    expect(kinds(w.calls)).toEqual(["send recorded"]);
    w.at(END + HOUR + 1000);
    await w.entries().alarm();
    const next = (await w.intervals()).find((row) => row.start === END);
    expect(next).toEqual(expect.objectContaining({ start: END, end_at: END + HOUR, reported: 0, amount: 0, carry_in: 0, carry_out: 0, outcome: "no-call" }));
    expect(kinds(w.calls)).toEqual(["send recorded"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor, each.timestamp])).toEqual([[IDENTIFIER, 3, TIMESTAMP]]);
    w.close();
  });
});

test("the retry delay and the window are the constants that the tests read", () => {
  expect([SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS, DAY]).toEqual([60_000, 86_400_000, 86_400_000]);
});
