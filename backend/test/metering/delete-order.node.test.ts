import { describe, expect, test } from "vitest";
import { DELETE_REMOVAL_BOUND_MS, makeEntries } from "../../src/do/entries.ts";
import { makeAccounts } from "../../src/accounts/index.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { LATE_READ_SPAN_MS, makeMetering, SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, DAY, HASH, HOUR, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

const MINUTE = 60_000;
const END = START + HOUR;
const NOT_BILLABLE = '{"event":"usage-not-billable"}';

type Answer = "cancelled" | "nothing-to-cancel" | "failed";
type Script = Answer | "provider";

interface Deleting {
  w: World;
  script: Script[];
  during: (() => void | Promise<void>) | null;
  cancelTimes: Ms[];
}

async function deleting(o: Parameters<typeof world>[0] & { script?: Script[] } = {}): Promise<Deleting> {
  let ended = false;
  const d = { script: o.script ?? [], during: null, cancelTimes: [] } as unknown as Deleting;
  const cancel = async (): Promise<{ kind: Answer }> => {
    d.cancelTimes.push(d.w.now());
    const next = d.script.shift() ?? "provider";
    const answer: Answer = next === "provider" ? (ended ? "nothing-to-cancel" : "cancelled") : next;
    if (next === "provider" && !ended) {
      ended = true;
      d.w.meter.endSubscription(ACCOUNT, Math.floor(d.w.now() / 1000));
    }
    const hook = d.during;
    d.during = null;
    if (hook !== null) await hook();
    return { kind: answer };
  };
  d.w = await world({ ...o, cancel });
  return d;
}

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

async function skeleton(w: World): Promise<Record<string, string | number | null> | null> {
  return (await w.tables()).skeleton?.[0] ?? null;
}

async function holds(w: World): Promise<boolean> {
  return (await w.entries().testInspect()).result === "snapshot";
}

function periodsBegunAfter(w: World, mark: Ms): number {
  return w.meter.periods(ACCOUNT, Math.floor(w.now() / 1000)).filter((period) => period.start * 1000 > mark).length;
}

function destroy(w: World) {
  return quiet(w.lines, () => w.entries().destroy(HASH));
}

describe("the cancel does not wait for the metering (unit 5.6, ruling P7-82 D1 and D2)", () => {
  test("a delete whose send gets no answer cancels in the same call; no period begins after the mark, the storage stays until the resend is acknowledged, and the object deletes by itself", async () => {
    const d = await deleting({ meter: { invoicePeriodSeconds: 4500 }, faults: [{ kind: "request-lost" }] });
    const w = d.w;
    await w.use("r1", 25_500);
    await w.clock(START + 10 * MINUTE);
    const mark = w.now();
    const answer = await destroy(w);
    const atTheCall = { answer, calls: kinds(w.calls), holds: await holds(w), answered: (await skeleton(w))?.cancel_answered_at };
    await w.clock(mark + SYNC_RETRY_MS);
    const afterTheResend = await holds(w);
    await w.clock(START + 2 * DAY);
    expect(atTheCall).toEqual({ answer: { result: "not-finished" }, calls: ["send failed", "cancel cancelled"], holds: true, answered: mark });
    expect(afterTheResend).toBe(false);
    expect(kinds(w.calls)).toEqual(["send failed", "cancel cancelled", "send recorded"]);
    expect(periodsBegunAfter(w, mark)).toBe(0);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor])).toEqual([[`${ACCOUNT}:${START / 1000}`, 2]]);
    w.close();
  });

  test("case 3: interval A sent with its answer lost, the cut interval B unsent and a period end inside the wait: the cancel is answered at the delete, no period begins after it, A is acknowledged and B sent later, and the object deletes by itself", async () => {
    const d = await deleting({ meter: { invoicePeriodSeconds: 4500 }, faults: [{ kind: "response-lost" }] });
    const w = d.w;
    await w.use("r0", 30_000);
    await w.clock(END);
    await w.clock(END + 1000);
    await w.use("r1", 20_000);
    await w.clock(END + 30_000);
    const mark = w.now();
    const answer = await destroy(w);
    const cancelAt = d.cancelTimes.slice();
    await w.clock(END + 2 * HOUR);
    expect(answer).toEqual({ result: "not-finished" });
    expect(cancelAt).toEqual([mark]);
    expect(periodsBegunAfter(w, mark)).toBe(0);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "cancel cancelled", "send duplicate", "send recorded"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.amountMinor)).toEqual([3, 2]);
    expect(await holds(w)).toBe(false);
    w.close();
  });

  test("case 5: a hold that ends between step 3 and step 4 leaves its interval unsealed, so the seal and send part runs once more and the cut interval is sent before the cancel", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.faults.push({ kind: "request-lost" });
    w.at(END);
    await quiet(w.lines, () => w.entries().alarm());
    w.at(END + SYNC_RETRY_MS - 1000);
    await w.reserve("r1");
    w.faults.push({ kind: "during", run: async () => void (await w.settle("r1", 20_000)) });
    w.at(END + SYNC_RETRY_MS);
    const answer = await destroy(w);
    expect(answer).toEqual({ result: "deleted" });
    expect(kinds(w.calls)).toEqual(["send failed", "send recorded", "send recorded", "cancel cancelled"]);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.amountMinor)).toEqual([1, 2]);
    w.close();
  });

  test("case 12: five intervals sealed at the delete with every send answered: step 3 sends three, the cancel follows in the same call, and the alarm sends the other two one retry delay later and deletes", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    for (let each = 0; each < 5; each += 1) {
      w.at(START + each * HOUR + 1000);
      await w.use(`r${each}`, 10_000);
    }
    w.at(START + 5 * HOUR + 1000);
    const answer = await destroy(w);
    const atTheCall = kinds(w.calls);
    w.at(START + 5 * HOUR + 1000 + SYNC_RETRY_MS);
    await quiet(w.lines, () => w.entries().alarm());
    expect(answer).toEqual({ result: "not-finished" });
    expect(atTheCall).toEqual(["send recorded", "send recorded", "send recorded", "cancel cancelled"]);
    expect(kinds(w.calls)).toEqual(["send recorded", "send recorded", "send recorded", "cancel cancelled", "send recorded", "send recorded"]);
    expect(await holds(w)).toBe(false);
    w.close();
  });

  test("case 12: a delete inside the oldest interval's retry delay sends nothing and cancels in the same call", async () => {
    const d = await deleting({ testClock: false, faults: [{ kind: "request-lost" }] });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.at(END);
    await quiet(w.lines, () => w.entries().alarm());
    w.at(END + 30_000);
    const answer = await destroy(w);
    expect(answer).toEqual({ result: "not-finished" });
    expect(kinds(w.calls)).toEqual(["send failed", "cancel cancelled"]);
    w.close();
  });

  test("a delete inside the first window of P7-77, with a refusal pending after a lost request: the cancel goes out at once, and the storage goes at the removal's bound", async () => {
    const d = await deleting({ faults: [{ kind: "request-lost" }], always: { kind: "refuse", reason: "unknown_account" } });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(END);
    await w.clock(END + SYNC_RETRY_MS);
    const mark = w.now();
    const answer = await destroy(w);
    expect([answer, d.cancelTimes]).toEqual([{ result: "not-finished" }, [mark]]);
    await w.clock(mark + DAY - 1000);
    const beforeTheBound = await holds(w);
    await w.clock(mark + DAY);
    expect(beforeTheBound).toBe(true);
    expect(await holds(w)).toBe(false);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toHaveLength(1);
    w.close();
  });
});

describe("the stored answer (unit 5.6, ruling P7-82 D2)", () => {
  test("two deletes in flight, the first called for no subscription id and the second for the id that arrived meanwhile: the late answer for no id leaves the answer stored for the row's id, status answers cancelled, and the alarm makes no further cancel call", async () => {
    const d = await deleting({ testClock: false, always: { kind: "request-lost" }, script: ["nothing-to-cancel", "provider"] });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.at(START + MINUTE);
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    d.during = () => held;
    const first = destroy(w);
    await new Promise((resolve) => setTimeout(resolve, 0));
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START + MINUTE + 500 }));
    w.at(START + 2 * MINUTE);
    const second = await destroy(w);
    w.at(START + 3 * MINUTE);
    release();
    expect([await first, second]).toEqual([{ result: "not-finished" }, { result: "not-finished" }]);
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: START + 2 * MINUTE, cancel_answered_for: "sub_1" }));
    expect(await w.entries().status(HASH)).toEqual({ result: "cancelled" });
    w.at(START + 4 * MINUTE);
    await quiet(w.lines, () => w.entries().alarm());
    expect(w.calls.filter((each) => each.kind === "cancel").map((each) => [each.subscriptionId, each.answer])).toEqual([
      ["sub_1", "cancelled"],
      [null, "nothing-to-cancel"],
    ]);
    w.close();
  });

  test("a delete called twice: the second call makes no cancel call, because an answer is stored for the row's id", async () => {
    const d = await deleting({ faults: [{ kind: "request-lost" }] });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const first = await destroy(w);
    const second = await destroy(w);
    expect([first, second]).toEqual([{ result: "not-finished" }, { result: "not-finished" }]);
    expect(kinds(w.calls).filter((each) => each.startsWith("cancel"))).toEqual(["cancel cancelled"]);
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: START + MINUTE, cancel_answered_for: null }));
    w.close();
  });

  test("two deletes in flight: one cancel call; the second meets the call in flight, answers not-finished and calls nothing, and the first stores the answer (P7-110 L4)", async () => {
    const d = await deleting({ testClock: false, faults: [{ kind: "request-lost" }, { kind: "request-lost" }] });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START + 2000 }));
    w.at(START + MINUTE);
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    d.during = () => held;
    const first = destroy(w);
    await new Promise((resolve) => setTimeout(resolve, 0));
    w.at(START + 2 * MINUTE);
    const second = await destroy(w);
    const stored = await skeleton(w);
    w.at(START + 3 * MINUTE);
    release();
    expect([await first, second]).toEqual([{ result: "not-finished" }, { result: "not-finished" }]);
    expect(stored).toEqual(expect.objectContaining({ cancel_answered_at: null }));
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: START + 3 * MINUTE, cancel_answered_for: "sub_1" }));
    expect(w.calls.filter((each) => each.kind === "cancel").map((each) => [each.subscriptionId, each.answer])).toEqual([["sub_1", "cancelled"]]);
    w.close();
  });

  test("a stop between the cancel call and the write of its answer: nothing is stored, and the next run of the alarm calls again, reads the end and stores", async () => {
    const d = await deleting();
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    d.during = () => w.stopNextStep("before");
    const stopped = await destroy(w).then(
      () => "answered",
      (error: Error) => error.name,
    );
    const afterTheStop = await skeleton(w);
    w.restart();
    await w.clock(START + MINUTE + 1000);
    expect(stopped).toBe("ObjectStoppedError");
    expect(afterTheStop).toEqual(expect.objectContaining({ destroying_since: START + MINUTE, cancel_answered_at: null }));
    expect(kinds(w.calls).filter((each) => each.startsWith("cancel"))).toEqual(["cancel cancelled", "cancel nothing-to-cancel"]);
    expect(await holds(w)).toBe(false);
    w.close();
  });

  test("case 13: an account that never subscribed has its null cancel stored, and a subscription id that arrives after it is cancelled by the next run", async () => {
    const d = await deleting({ faults: [{ kind: "request-lost" }, { kind: "request-lost" }], script: ["nothing-to-cancel"] });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const answer = await destroy(w);
    const storedForNull = await skeleton(w);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_late_sub", providerTime: START + MINUTE + 1000 }));
    await w.clock(START + MINUTE + 2000);
    const storedForTheId = await skeleton(w);
    expect(answer).toEqual({ result: "not-finished" });
    expect(storedForNull).toEqual(expect.objectContaining({ cancel_answered_at: START + MINUTE, cancel_answered_for: null }));
    expect(storedForTheId).toEqual(expect.objectContaining({ cancel_answered_at: START + MINUTE + 2000, cancel_answered_for: "sub_1" }));
    expect(w.calls.filter((each) => each.kind === "cancel").map((each) => [each.subscriptionId, each.answer])).toEqual([
      [null, "nothing-to-cancel"],
      ["sub_1", "cancelled"],
    ]);
    w.close();
  });

  test("case 14: the provider's own cancel event under the mark, with a stated end one second before the mark, moves no amount, and the row reads cancelled", async () => {
    const d = await deleting({ faults: [{ kind: "request-lost" }] });
    const w = d.w;
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START }));
    await w.use("r0", 25_000);
    await w.clock(START + 10 * MINUTE);
    const mark = w.now();
    await destroy(w);
    const before = (await w.intervals()).map((row) => [row.start, row.end_at, row.amount, row.timestamp]);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: mark + 1000, state: "cancelled", endedAt: mark - 1000 }));
    const after = await w.tables();
    expect((after.metering_interval ?? []).map((row) => [row.start, row.end_at, row.amount, row.timestamp])).toEqual(before);
    expect((after.account ?? []).map((row) => row.state)).toEqual(["cancelled"]);
    await w.clock(mark + SYNC_RETRY_MS);
    expect(await holds(w)).toBe(false);
    expect(w.meter.recordedEvents(ACCOUNT).map((each) => each.amountMinor)).toEqual([2]);
    w.close();
  });
});

describe("the object finishes a begun delete by itself (unit 5.6, ruling P7-82 D3 to D5; contract 3.6)", () => {
  test("bound of the object's own cancel: a port whose cancel answers failed ten times and then cancelled, with no call of destroy after the first: the tries come at the doubling delay, the answer is stored once, and the storage goes when the metering is settled", async () => {
    const d = await deleting({ script: Array(10).fill("failed") });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const first = await destroy(w);
    const whileFailing: boolean[] = [];
    for (let each = 0; each < 10; each += 1) {
      whileFailing.push(await holds(w));
      const snapshot = await w.entries().testInspect();
      if (snapshot.result !== "snapshot" || snapshot.due === null) break;
      await w.clock(snapshot.due);
    }
    const gaps = d.cancelTimes.slice(1).map((at, index) => (at - (d.cancelTimes[index] ?? 0)) / 1000);
    expect(first).toEqual({ result: "not-finished" });
    expect(whileFailing).toEqual(Array(10).fill(true));
    expect(gaps).toEqual([60, 120, 240, 480, 960, 1920, 3600, 3600, 3600, 3600]);
    expect(kinds(w.calls).filter((each) => each.startsWith("cancel"))).toEqual([...Array(10).fill("cancel failed"), "cancel cancelled"]);
    expect(await holds(w)).toBe(false);
    w.close();
  });

  test("the provider down for sends and for the cancel, with no second call from outside: the mark and the storage stay, and the alarm keeps asking", async () => {
    const d = await deleting({ always: { kind: "request-lost" }, script: Array(40).fill("failed") });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    await destroy(w);
    for (let hour = 1; hour <= 30; hour += 1) await w.clock(START + MINUTE + hour * HOUR);
    const stored = await skeleton(w);
    expect(stored).toEqual(expect.objectContaining({ destroying_since: START + MINUTE, cancel_answered_at: null }));
    expect(d.cancelTimes.length).toBeGreaterThanOrEqual(30);
    expect(w.lines.filter((line) => line.includes("alarm-stopped"))).toEqual([]);
    w.close();
  });

  test("bound DELETE_REMOVAL_BOUND_MS: a cancel answered and every later send answered failed: one clock call past the bound closes each open interval with the cause delete-bound, logs one line for each, and the object holds nothing", async () => {
    const d = await deleting({ always: { kind: "request-lost" } });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(END);
    await w.clock(END + 5 * MINUTE);
    await w.use("r1", 20_000);
    await w.clock(END + 10 * MINUTE);
    const answer = await destroy(w);
    const answered = (await skeleton(w))?.cancel_answered_at;
    let closed: (string | number | null | undefined)[][] = [];
    const deleteAll = w.host.deleteAll;
    w.host.deleteAll = async () => {
      closed = w.host.step((sql) => sql.exec("SELECT start, outcome, refusal FROM metering_interval ORDER BY start")).map((row) => [row.start, row.outcome, row.refusal]);
      await deleteAll();
    };
    const linesBefore = w.lines.length;
    await w.clock(END + 10 * MINUTE + DAY);
    expect([answer, answered]).toEqual([{ result: "not-finished" }, END + 10 * MINUTE]);
    expect(closed).toEqual([
      [START, "not-billable", "delete-bound"],
      [END, "not-billable", "delete-bound"],
    ]);
    expect(w.lines.slice(linesBefore)).toEqual([NOT_BILLABLE, NOT_BILLABLE]);
    expect(w.lines.some((line) => line.includes(ACCOUNT))).toBe(false);
    expect(await holds(w)).toBe(false);
    w.close();
  });

  test("the cancel answered and every later send refused, in the form that closes after three refusals: the object deletes by itself once the third refusal closes the interval", async () => {
    const d = await deleting({ faults: Array(3).fill({ kind: "refuse", reason: "unknown_account" }) });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const answer = await destroy(w);
    await w.clock(START + MINUTE + SYNC_RETRY_MS);
    const afterTwo = await holds(w);
    await w.clock(START + MINUTE + 2 * SYNC_RETRY_MS);
    expect([answer, afterTwo]).toEqual([{ result: "not-finished" }, true]);
    expect(kinds(w.calls)).toEqual(["send refused unknown_account", "cancel cancelled", "send refused unknown_account", "send refused unknown_account"]);
    expect(await holds(w)).toBe(false);
    expect(w.lines.filter((line) => line === NOT_BILLABLE)).toHaveLength(1);
    w.close();
  });

  test("usage that settles between the cancel and the removal: the storage stays until the interval is acknowledged, then goes in the same run", async () => {
    const d = await deleting({ faults: [{ kind: "response-lost" }] });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const answer = await destroy(w);
    await w.clock(START + MINUTE + SYNC_RETRY_MS - 1000);
    const beforeTheResend = await holds(w);
    await w.clock(START + MINUTE + SYNC_RETRY_MS);
    expect([answer, beforeTheResend]).toEqual([{ result: "not-finished" }, true]);
    expect(kinds(w.calls)).toEqual(["send failed after recorded", "cancel cancelled", "send duplicate"]);
    expect(await holds(w)).toBe(false);
    w.close();
  });

  test("the removal at the bound, then a new start under the same id: every identifier of the new account holds a start after the removal, and the meter records each identifier once", async () => {
    const d = await deleting({ always: { kind: "request-lost" } });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    await destroy(w);
    const removal = START + MINUTE + DAY;
    await w.clock(removal);
    const gone = !(await holds(w));
    const callsBefore = w.calls.length;
    const later = removal + HOUR;
    const seeded = await w.entries().testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", customerId: "cus_2", cap: { amount: 100_000_000, period: "month" }, time: later });
    await w.clock(later);
    await w.use("r9", 10_000);
    await w.clock(later + 2 * HOUR);
    const sends = w.calls.slice(callsBefore).flatMap((each) => (each.kind === "send" ? [each.identifier] : []));
    expect([gone, seeded.result]).toEqual([true, "seeded"]);
    expect(sends.length).toBeGreaterThan(0);
    expect(sends.every((identifier) => Number(identifier.split(":")[1]) * 1000 > removal)).toBe(true);
    const events = w.meter.recordedEvents(ACCOUNT).map((each) => each.identifier);
    expect(new Set(events).size).toBe(events.length);
    w.close();
  });

  test("throws in the seal and send part: destroy makes the cancel call and stores its answer before the error leaves", async () => {
    const d = await deleting();
    const w = d.w;
    await w.use("r0", 10_000);
    w.faults.push({ kind: "during", run: async () => w.stopNextStep("before") });
    await w.clock(START + MINUTE);
    const thrown = await destroy(w).then(
      () => "answered",
      (error: Error) => error.name,
    );
    expect(thrown).toBe("ObjectStoppedError");
    expect(kinds(w.calls).filter((each) => each.startsWith("cancel"))).toEqual(["cancel cancelled"]);
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: START + MINUTE }));
    w.close();
  });

  test("case 5 with an error: the second run of the seal and send part throws in destroy, and the cancel call is made and its answer stored before the error leaves", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.faults.push({ kind: "request-lost" });
    w.at(END);
    await quiet(w.lines, () => w.entries().alarm());
    w.at(END + SYNC_RETRY_MS - 1000);
    await w.reserve("r1");
    w.faults.push({ kind: "during", run: async () => void (await w.settle("r1", 20_000)) });
    w.faults.push({ kind: "during", run: async () => w.stopNextStep("before") });
    w.at(END + SYNC_RETRY_MS);
    const thrown = await destroy(w).then(
      () => "answered",
      (error: Error) => error.name,
    );
    expect(thrown).toBe("ObjectStoppedError");
    expect(kinds(w.calls)).toEqual(["send failed", "send recorded", "send recorded", "cancel cancelled"]);
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: END + SYNC_RETRY_MS }));
    w.close();
  });

  test("case 5 with an error in the alarm: a hold open at the delete ends during the alarm's send, the second run of the seal and send part throws, and the run makes the owed cancel call and stores its answer", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.faults.push({ kind: "request-lost" });
    w.at(END);
    await quiet(w.lines, () => w.entries().alarm());
    w.at(END + SYNC_RETRY_MS - 1000);
    await w.reserve("r1");
    const answer = await destroy(w);
    w.faults.push({ kind: "during", run: async () => void (await w.settle("r1", 20_000)) });
    w.faults.push({ kind: "during", run: async () => w.stopNextStep("before") });
    w.at(END + SYNC_RETRY_MS);
    await quiet(w.lines, () => w.entries().alarm());
    expect(answer).toEqual({ result: "not-finished" });
    expect(kinds(w.calls)).toEqual(["send failed", "send recorded", "send recorded", "cancel cancelled"]);
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: END + SYNC_RETRY_MS, alarm_failures: 1 }));
    w.close();
  });

  test("throws in the seal and send part of the alarm: the run makes the owed cancel call and stores its answer, and the error is counted as today", async () => {
    const d = await deleting({ faults: [{ kind: "request-lost" }], script: ["failed"] });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    await destroy(w);
    w.faults.push({ kind: "during", run: async () => w.stopNextStep("before") });
    await w.clock(START + MINUTE + SYNC_RETRY_MS);
    expect(kinds(w.calls)).toEqual(["send failed", "cancel failed", "send recorded", "cancel cancelled"]);
    expect(await skeleton(w)).toEqual(expect.objectContaining({ cancel_answered_at: START + MINUTE + SYNC_RETRY_MS, alarm_failures: 1 }));
    expect(w.lines.map((line) => JSON.parse(line).event)).toContain("alarm-error");
    w.close();
  });
});

describe("status under the mark (unit 5.6, rulings P7-84 and P7-82 part 9)", () => {
  test("a delete whose cancel is unanswered, then status, which answers closing and not active; after the stored answer status answers cancelled, also while the storage waits", async () => {
    const d = await deleting({ always: { kind: "request-lost" }, script: ["failed"] });
    const w = d.w;
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START }));
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const before = await w.entries().status(HASH);
    await destroy(w);
    const unanswered = await w.entries().status(HASH);
    await w.clock(START + MINUTE + SYNC_RETRY_MS);
    const answered = await w.entries().status(HASH);
    expect([before, unanswered, answered]).toEqual([{ result: "active" }, { result: "closing" }, { result: "cancelled" }]);
    expect(await holds(w)).toBe(true);
    w.close();
  });

  test("a subscription id that arrives after an answer stored for null turns status back to closing until its own cancel is answered", async () => {
    const d = await deleting({ always: { kind: "request-lost" }, script: ["nothing-to-cancel"] });
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    await destroy(w);
    const storedForNull = await w.entries().status(HASH);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_late_sub", providerTime: START + MINUTE + 1000 }));
    const owed = await w.entries().status(HASH);
    await w.clock(START + MINUTE + 2000);
    expect([storedForNull, owed, await w.entries().status(HASH)]).toEqual([{ result: "cancelled" }, { result: "closing" }, { result: "cancelled" }]);
    w.close();
  });
});

describe("the margin's bound (unit 5.6, ruling P7-96 point 9)", () => {
  test("makeMetering refuses a margin under the uniqueness window less LATE_READ_SPAN_MS, 300 s as shipped: 299 s is refused and 300 s is taken", () => {
    const made = (marginSeconds: number) => {
      try {
        makeMetering({ intervalSeconds: 3600, marginSeconds });
        return "made";
      } catch (error) {
        return (error as Error).name;
      }
    };
    expect((UNIQUENESS_WINDOW_MS - LATE_READ_SPAN_MS) / 1000).toBe(300);
    expect([made(299), made(300)]).toEqual(["RangeError", "made"]);
  });

  test("DELETE_REMOVAL_BOUND_MS is 24 hours, the provider's uniqueness window", () => {
    expect([DELETE_REMOVAL_BOUND_MS, DELETE_REMOVAL_BOUND_MS === UNIQUENESS_WINDOW_MS]).toEqual([DAY, true]);
  });
});

describe("a hold counts as open only before its stored expiry (unit 5.6, P7-110 L2)", () => {
  test("a hold past its stored end, with ledger.expire throwing on every call: the cancel is made once the hold's expiry has passed, and a hold inside its life still delays it", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    let failing = false;
    const ledger = makeLedger({ holdLifeMs: w.host.config.holdLifeMs });
    const own = makeEntries(w.host, {
      accounts: makeAccounts(),
      ledger: {
        ...ledger,
        expire: (...a: Parameters<typeof ledger.expire>) => {
          if (failing) throw new RangeError("expire throws");
          return ledger.expire(...a);
        },
      },
      metering: makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds }),
    });
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.at(START + MINUTE);
    await own.reserve(HASH, { requestId: "r1", tier: "low_cost_hosted", amount: 100, price: { input: 1_000_000, output: 1_000_000 } });
    failing = true;
    const inside = await quiet(w.lines, () => own.destroy(HASH));
    const cancelsInside = d.cancelTimes.length;
    w.at(START + MINUTE + w.host.config.holdLifeMs);
    const after = await quiet(w.lines, () => own.destroy(HASH));
    expect([inside, cancelsInside]).toEqual([{ result: "not-finished" }, 0]);
    expect(after.result === "deleted" || after.result === "not-finished").toBe(true);
    expect(d.cancelTimes).toEqual([START + MINUTE + w.host.config.holdLifeMs]);
    w.close();
  });
});

describe("one cancel call at a time for one id, and the cut interval's send first (unit 5.6, P7-110 L4)", () => {
  test("L4 (a): the alarm delivered during the cancel call of a destroy from outside makes no second cancel call", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START }));
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.at(START + 10 * MINUTE);
    d.during = () => quiet(w.lines, () => w.entries().alarm());
    const answer = await destroy(w);
    expect(answer).toEqual({ result: "deleted" });
    expect(w.calls.filter((each) => each.kind === "cancel").map((each) => [each.subscriptionId, each.answer])).toEqual([["sub_1", "cancelled"]]);
    w.close();
  });

  test("V2 and L4 (c): one destroy from outside, with the due alarm delivered at each of its port calls, makes one cancel call, after the cut interval's send", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START }));
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.at(START + 10 * MINUTE);
    const deliver = async () => void (await quiet(w.lines, () => w.entries().alarm()));
    w.faults.push({ kind: "during", run: deliver });
    d.during = deliver;
    const answer = await destroy(w);
    expect(answer).toEqual({ result: "deleted" });
    expect(kinds(w.calls)).toEqual(["send recorded", "cancel cancelled"]);
    w.close();
  });

  const beforeSend = (w: World) => w.lines.filter((line) => line === '{"event":"cancel-before-send"}').length;

  test("L4 (d): the call limit of the seal and send part forces the cancel with two intervals unsent: two cancel-before-send lines, which name no account", async () => {
    const d = await deleting({ testClock: false });
    const w = d.w;
    for (let each = 0; each < 5; each += 1) {
      w.at(START + each * HOUR + 1000);
      await w.use(`r${each}`, 10_000);
    }
    w.at(START + 5 * HOUR + 1000);
    await destroy(w);
    expect(beforeSend(w)).toBe(2);
    expect(w.lines.some((line) => line.includes(ACCOUNT))).toBe(false);
    w.close();
  });

  test("L4 (d): the oldest interval's next attempt in the future forces the cancel with one interval unsent: one cancel-before-send line", async () => {
    const d = await deleting({ testClock: false, faults: [{ kind: "request-lost" }] });
    const w = d.w;
    w.at(START + 1000);
    await w.use("r0", 10_000);
    w.at(END);
    await quiet(w.lines, () => w.entries().alarm());
    w.at(END + 30_000);
    await destroy(w);
    expect(beforeSend(w)).toBe(1);
    w.close();
  });

  for (const fault of [{ kind: "request-lost" as const }, { kind: "refuse" as const, reason: "unknown_account" as const }]) {
    test(`L4 (d): a ${fault.kind === "refuse" ? "refused" : "failed"} send forces the cancel with the cut interval unacknowledged: one cancel-before-send line`, async () => {
      const d = await deleting({ faults: [fault] });
      const w = d.w;
      await w.use("r0", 10_000);
      await w.clock(START + MINUTE);
      await destroy(w);
      expect([d.cancelTimes.length, beforeSend(w)]).toEqual([1, 1]);
      w.close();
    });
  }

  test("L4 (d): a cancel made with every interval acknowledged writes no cancel-before-send line", async () => {
    const d = await deleting();
    const w = d.w;
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    await destroy(w);
    expect([d.cancelTimes.length, beforeSend(w)]).toEqual([1, 0]);
    w.close();
  });
});
