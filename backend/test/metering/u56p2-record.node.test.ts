import { describe, expect, test } from "vitest";
import { CALL_IN_FLIGHT_MS } from "../../src/do/entries.ts";
import { SYNC_RETRY_MS } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import type { CancelFault } from "../../test-kit/fake-meter/port.ts";
import { ACCOUNT, CUSTOMER, DAY, HASH, quiet, START, type World } from "./world.ts";
import { alarmAt, CAP_MS, callRecord, cancels, destroy, events, held, holds, listed, MINUTE, periodsAfter, secondInstance, skeleton, started, statusOf, subscribed, tick } from "./u56p2-support.ts";

describe("the record of the call in flight, one per account (P7-117 T6; P7-111 M3)", () => {
  test("S5: a list call is in flight for an account with no stored id and an id arrives: no second call starts while the record stands, and the next call, once the record is gone, finds the plan ended", async () => {
    const hold = held();
    const w = await listed({ faults: [hold.fault] }, { faults: [{ kind: "request-lost" }] });
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const ids = started(w);
    w.at(START + MINUTE);
    const mark = w.now();
    const first = destroy(w);
    await tick();
    await subscribed(w, "sub_1", START + MINUTE + 500);
    w.at(START + MINUTE + 10_000);
    const second = await destroy(w);
    const whileInFlight = [...ids];
    hold.release();
    await first;
    await destroy(w);
    expect({ second, whileInFlight, ids, periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      second: { result: "not-finished" },
      whileInFlight: [null],
      ids: [null, "sub_1"],
      periods: 0,
    });
    w.close();
  });

  for (const who of ["a second instance over the same storage", "the same object after a restart", "the alarm due at the mark"] as const) {
    test(`P7-113: while a list call made for no id is in flight, ${who} makes no call before the record's expiry, and the first call's answer is stored once`, async () => {
      const hold = held();
      const w = await listed({ faults: [hold.fault] }, { faults: [{ kind: "request-lost" }] });
      w.at(START + 1000);
      await w.use("r0", 10_000);
      const ids = started(w);
      w.at(START + MINUTE);
      const first = destroy(w);
      await tick();
      w.at(START + MINUTE + 10_000);
      if (who === "a second instance over the same storage") await quiet(w.lines, () => secondInstance(w).destroy(HASH));
      if (who === "the same object after a restart") {
        w.restart();
        await destroy(w);
      }
      if (who === "the alarm due at the mark") await quiet(w.lines, () => w.entries().alarm());
      const whileInFlight = [...ids];
      const due = await w.host.getAlarm();
      hold.release();
      await first;
      expect({ whileInFlight, due, answeredAt: (await skeleton(w))?.cancel_answered_at, cancels: cancels(w) }).toEqual({
        whileInFlight: [null],
        due: START + MINUTE + CALL_IN_FLIGHT_MS,
        answeredAt: START + MINUTE + 10_000,
        cancels: [[null, CUSTOMER, "cancelled"]],
      });
      w.close();
    });
  }

  test("the count step: a failing run of the alarm while the record of a call for no id stands arms at the record's expiry, not at the pause", async () => {
    const hold = held();
    const w = await listed({ faults: [hold.fault] }, { faults: [{ kind: "request-lost" }] });
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const ids = started(w);
    w.at(START + MINUTE);
    const first = destroy(w);
    await tick();
    w.at(START + MINUTE + 1000);
    w.stopNextStep("before");
    await quiet(w.lines, () => w.entries().alarm());
    const armed = await w.host.getAlarm();
    const whileInFlight = [...ids];
    hold.release();
    await first;
    expect({ armed, whileInFlight, failures: (await skeleton(w))?.alarm_failures }).toEqual({ armed: START + MINUTE + CALL_IN_FLIGHT_MS, whileInFlight: [null], failures: 1 });
    w.close();
  });

  test("the bound of a call made for no id (P7-111 M3 (c)): cancel-call-unsettled at the bound, no second call while the record stands, and the late answer clears only its own record", async () => {
    const first = held();
    const later = held();
    const w = await listed({ faults: [first.fault, later.fault] }, { faults: [{ kind: "request-lost" }] });
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const ids = started(w);
    w.at(START + MINUTE);
    const unsettled = destroy(w);
    await tick();
    w.endPauses();
    const answer = await unsettled;
    const recordAtTheBound = await callRecord(w);
    const delayAtTheBound = (await skeleton(w))?.cancel_delay;
    w.at(START + MINUTE + 10_000);
    const second = await destroy(w);
    const beforeTheExpiry = [...ids];
    w.at(START + MINUTE + CALL_IN_FLIGHT_MS);
    const third = destroy(w);
    await tick();
    first.release();
    await tick();
    await tick();
    const recordAfterTheLateAnswer = await callRecord(w);
    later.release();
    await third;
    expect({ answer, unsettled: events(w).filter((each) => each === "cancel-call-unsettled").length, recordAtTheBound, delayAtTheBound, second, beforeTheExpiry, recordAfterTheLateAnswer }).toEqual({
      answer: { result: "not-finished" },
      unsettled: 1,
      recordAtTheBound: [{ subscription_id: null, began_at: START + MINUTE }],
      delayAtTheBound: 2 * SYNC_RETRY_MS,
      second: { result: "not-finished" },
      beforeTheExpiry: [null],
      recordAfterTheLateAnswer: [{ subscription_id: null, began_at: START + MINUTE + CALL_IN_FLIGHT_MS }],
    });
    w.close();
  });
});

describe("the pace of the tries after a failed call (P7-117 T2, T8 and T9; Addition 3, U3 and U6)", () => {
  async function tries(w: World, count: number): Promise<{ wait: Ms; delay: Ms | null; unanswered: number }[]> {
    const seen: { wait: Ms; delay: Ms | null; unanswered: number }[] = [];
    for (let each = 0; each < count; each += 1) {
      const row = await skeleton(w);
      if (row === null || row.cancel_next_at === null) break;
      await alarmAt(w, Number(row.cancel_next_at));
      const after = await skeleton(w);
      if (after === null || after.cancel_next_at === null) break;
      seen.push({
        wait: Number(after.cancel_next_at) - w.now(),
        delay: after.cancel_delay === null ? null : Number(after.cancel_delay),
        unanswered: events(w).filter((event) => event === "cancel-unanswered").length,
      });
    }
    return seen;
  }

  test("a failed call whose only success was the stored id's cancel doubles the pause; a failed call that ended a listed subscription sets the next try at 60 s and the delay back to 60 s, also at the cap, where it writes no cancel-unanswered", async () => {
    const lost: CancelFault[] = Array.from({ length: 7 }, () => ({ kind: "list-no-answer" }));
    const w = await listed({ plans: [{ id: "sub_2" }, { id: "sub_3" }], perCall: 1, faults: lost });
    await subscribed(w);
    w.at(START + MINUTE);
    const mark = w.now();
    expect(await destroy(w)).toEqual({ result: "not-finished" });
    const firstRow = await skeleton(w);
    const seen = await tries(w, 8);
    await alarmAt(w, Number((await skeleton(w))?.cancel_next_at));
    expect({
      first: { wait: Number(firstRow?.cancel_next_at) - mark, delay: firstRow?.cancel_delay },
      seen,
      unanswered: events(w).filter((each) => each === "cancel-unanswered").length,
      answers: cancels(w).map((each) => each.slice(2).join(" ")),
      holds: await holds(w),
    }).toEqual({
      first: { wait: SYNC_RETRY_MS, delay: 2 * SYNC_RETRY_MS },
      seen: [
        { wait: 120_000, delay: 240_000, unanswered: 0 },
        { wait: 240_000, delay: 480_000, unanswered: 0 },
        { wait: 480_000, delay: 960_000, unanswered: 0 },
        { wait: 960_000, delay: 1_920_000, unanswered: 0 },
        { wait: 1_920_000, delay: CAP_MS, unanswered: 0 },
        { wait: CAP_MS, delay: CAP_MS, unanswered: 1 },
        { wait: SYNC_RETRY_MS, delay: SYNC_RETRY_MS, unanswered: 1 },
      ],
      unanswered: 1,
      answers: ["failed 0", "failed 0", "failed 0", "failed 0", "failed 0", "failed 0", "failed 0", "failed 1", "cancelled"],
      holds: false,
    });
    w.close();
  });

  test("T8 with X5: a listed subscription that the provider refuses on every try keeps the delete open with its data; the walk ends every other subscription, the pause doubles after the first try, and the second count writes cancel-unanswered at the first try, beside its progress, then at most once per RETRY_DELAY_CAP_MS, the time of the last line in its column", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }, { id: "sub_3" }], refused: ["sub_2"] });
    await subscribed(w);
    w.at(START + MINUTE);
    expect(await destroy(w)).toEqual({ result: "not-finished" });
    const firstRow = await skeleton(w);
    const firstLines = events(w).filter((each) => each === "cancel-unanswered").length;
    const seen = await tries(w, 9);
    expect({
      first: { wait: Number(firstRow?.cancel_next_at) - (START + MINUTE), delay: firstRow?.cancel_delay, lines: firstLines },
      waits: seen.map((each) => each.wait),
      linesAfterEachTry: seen.map((each) => each.unanswered),
      lastLineAt: Number((await skeleton(w))?.cancel_unanswered_at) - (START + MINUTE),
      plans: [statusOf(w, "sub_1"), statusOf(w, "sub_2"), statusOf(w, "sub_3")],
      holds: await holds(w),
      answeredAt: (await skeleton(w))?.cancel_answered_at,
    }).toEqual({
      first: { wait: SYNC_RETRY_MS, delay: SYNC_RETRY_MS, lines: 1 },
      waits: [60_000, 120_000, 240_000, 480_000, 960_000, 1_920_000, CAP_MS, CAP_MS, CAP_MS],
      linesAfterEachTry: [1, 1, 1, 1, 1, 1, 2, 3, 4],
      lastLineAt: 11_040_000,
      plans: ["canceled", "active", "canceled"],
      holds: true,
      answeredAt: null,
    });
    w.close();
  });
});

describe("X1 in the object: the column of the settled stored id (P7-119 Addition 1)", () => {
  const settledArguments = (w: World) => w.calls.flatMap((each) => (each.kind === "cancel" ? [[each.subscriptionId, each.stored]] : []));

  test("a failed call that settles the stored id writes it into cancel_stored_settled_for, and the next try passes the stored id as settled", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }, { id: "sub_3" }], perCall: 1 });
    await subscribed(w);
    w.at(START + MINUTE);
    expect(await destroy(w)).toEqual({ result: "not-finished" });
    const afterFirst = (await skeleton(w))?.cancel_stored_settled_for;
    await alarmAt(w, Number((await skeleton(w))?.cancel_next_at));
    expect({ afterFirst, calls: settledArguments(w), holds: await holds(w) }).toEqual({
      afterFirst: "sub_1",
      calls: [
        ["sub_1", "unsettled"],
        ["sub_1", "settled"],
      ],
      holds: false,
    });
    w.close();
  });

  test("an id that arrives after a call made for no id is not settled: the column stays empty and the next try puts the new id first", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }, { id: "sub_3" }], perCall: 1 }, { subscribed: false });
    w.at(START + MINUTE);
    expect(await destroy(w)).toEqual({ result: "not-finished" });
    const afterFirst = (await skeleton(w))?.cancel_stored_settled_for ?? null;
    w.meter.subscribe(ACCOUNT, Math.floor((START + MINUTE) / 1000), "sub_1");
    await subscribed(w, "sub_1", START + MINUTE + 1000);
    await alarmAt(w, Number((await skeleton(w))?.cancel_next_at));
    expect({ afterFirst, calls: settledArguments(w) }).toEqual({
      afterFirst: null,
      calls: [
        [null, "unsettled"],
        ["sub_1", "unsettled"],
      ],
    });
    w.close();
  });
});

describe("R2 of P7-120: e26, a call that does not settle within its bound", () => {
  test("the bound settles nothing: after a kept first unknown, a call held past CANCEL_CALL_BOUND_MS starts the count again, its late answer writes nothing, and the next call is asked with the stored id unsettled", async () => {
    const late = held();
    const w = await listed({ faults: [{ kind: "none" }, late.fault] }, { subscribed: false });
    await subscribed(w, "sub_gone");
    w.at(START + MINUTE);
    const first = await destroy(w);
    const afterFirst = await skeleton(w);
    w.at(Number(afterFirst?.cancel_next_at));
    const second = quiet(w.lines, () => w.entries().alarm());
    await tick();
    w.endPauses();
    await second;
    const afterBound = await skeleton(w);
    late.release();
    await tick();
    await tick();
    const afterLate = await skeleton(w);
    await alarmAt(w, Number(afterLate?.cancel_next_at));
    const column = (row: Record<string, string | number | null> | null) => [row?.cancel_stored_settled_for ?? null, row?.cancel_stored_unknown_for ?? null];
    expect({
      first,
      afterFirst: column(afterFirst),
      afterBound: column(afterBound),
      afterLate: column(afterLate),
      calls: w.calls.flatMap((each) => (each.kind === "cancel" ? [[each.subscriptionId, each.stored]] : [])),
    }).toEqual({
      first: { result: "not-finished" },
      afterFirst: [null, "sub_gone"],
      afterBound: [null, null],
      afterLate: [null, null],
      calls: [
        ["sub_gone", "unsettled"],
        ["sub_gone", "unknown-once"],
        ["sub_gone", "unsettled"],
      ],
    });
    w.close();
  });
});

describe("item (5) of P7-119: the record after a failed call with progress", () => {
  test("adv3 c4: a failed call that ended one listed subscription clears its record in the step after its answer; the person's second delete 30 s later makes the next call", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }, { id: "sub_3" }], perCall: 1 });
    await subscribed(w);
    w.at(START + MINUTE);
    const first = await destroy(w);
    const recordAfter = await callRecord(w);
    w.at(START + MINUTE + 30_000);
    await destroy(w);
    expect({ first, recordAfter, callsAfterSecond: cancels(w).length }).toEqual({ first: { result: "not-finished" }, recordAfter: [], callsAfterSecond: 2 });
    w.close();
  });
});

describe("manage under the mark (P7-117 T7)", () => {
  test("T7: a manage page asked before the delete and answered after the mark is written answers unavailable", async () => {
    const w = await listed({ faults: [{ kind: "list-no-answer" }] });
    await subscribed(w);
    let answer: () => void = () => {};
    const answered = new Promise<void>((resolve) => {
      answer = resolve;
    });
    w.host.port.openManagePage = async () => {
      await answered;
      return { kind: "page", url: "https://billing.example.test/session" };
    };
    w.at(START + MINUTE);
    const manage = w.entries().manage(HASH);
    await tick();
    expect(await destroy(w)).toEqual({ result: "not-finished" });
    answer();
    expect(await manage).toEqual({ result: "unavailable" });
    w.close();
  });

  test("T7: under the mark manage answers unavailable and opens no page, and start answers account-exists", async () => {
    const w = await listed({ faults: [{ kind: "list-no-answer" }] });
    await subscribed(w);
    w.at(START + MINUTE);
    expect(await destroy(w)).toEqual({ result: "not-finished" });
    let opened = 0;
    const open = w.host.port.openManagePage.bind(w.host.port);
    w.host.port.openManagePage = (a) => {
      opened += 1;
      return open(a);
    };
    const manage = await w.entries().manage(HASH);
    const start = await w.entries().start(ACCOUNT, HASH, { amount: 100_000_000, period: "month" });
    expect({ manage, opened, start }).toEqual({ manage: { result: "unavailable" }, opened: 0, start: { result: "account-exists" } });
    w.close();
  });
});
