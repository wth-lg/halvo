import { describe, expect, test } from "vitest";
import { ACCOUNT, CUSTOMER, DAY, START, subscriptionEvent, world } from "./world.ts";
import { cancels, destroy, held, holds, listed, MINUTE, periodsAfter, skeleton, started, statusOf, subscribed, tick, alarmAt } from "./u56p2-support.ts";

describe("the customer's list at a delete (unit 5.6 part 2; rulings P7-86 E and P7-117)", () => {
  test("case H (P7-110): a delete before the subscription event, with no usage, ends the subscription that stands on the customer, and no period begins in 90 days", async () => {
    const w = await listed({});
    w.at(START + MINUTE);
    const mark = w.now();
    const answer = await destroy(w);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_late_sub", providerTime: START + MINUTE + 1000 }));
    expect({ answer, cancels: cancels(w), sub_1: statusOf(w, "sub_1"), periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      answer: { result: "deleted" },
      cancels: [[null, CUSTOMER, "cancelled"]],
      sub_1: "canceled",
      periods: 0,
    });
    w.close();
  });

  test("a second subscription on the customer: the delete ends both, and no period of either begins after the mark", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }] });
    await subscribed(w);
    w.at(START + MINUTE);
    const mark = w.now();
    const answer = await destroy(w);
    expect({ answer, cancels: cancels(w), sub_1: statusOf(w, "sub_1"), sub_2: statusOf(w, "sub_2"), periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      answer: { result: "deleted" },
      cancels: [["sub_1", CUSTOMER, "cancelled"]],
      sub_1: "canceled",
      sub_2: "canceled",
      periods: 0,
    });
    w.close();
  });

  test("a list that gives no answer: the stored id is cancelled all the same, nothing is stored, the storage stays, and the next try asks again and ends the second subscription", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }], faults: [{ kind: "list-no-answer" }] });
    await subscribed(w);
    w.at(START + MINUTE);
    const mark = w.now();
    const first = await destroy(w);
    const after = { holds: await holds(w), sub_1: statusOf(w, "sub_1"), sub_2: statusOf(w, "sub_2"), row: await skeleton(w) };
    expect({ first, holds: after.holds, sub_1: after.sub_1, sub_2: after.sub_2, answeredAt: after.row === null ? "gone" : after.row.cancel_answered_at }).toEqual({
      first: { result: "not-finished" },
      holds: true,
      sub_1: "canceled",
      sub_2: "active",
      answeredAt: null,
    });
    await alarmAt(w, Number(after.row?.cancel_next_at));
    expect({ holds: await holds(w), sub_2: statusOf(w, "sub_2"), cancels: cancels(w), periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      holds: false,
      sub_2: "canceled",
      cancels: [
        ["sub_1", CUSTOMER, "failed", 0],
        ["sub_1", CUSTOMER, "cancelled"],
      ],
      periods: 0,
    });
    w.close();
  });

  test("a delete called twice, with a second subscription on the customer: the first call ends both and stores the answer, the second makes no call", async () => {
    const w = await world({ faults: [{ kind: "request-lost" }], listed: { plans: [{ id: "sub_2" }] } });
    await subscribed(w);
    await w.use("r0", 10_000);
    await w.clock(START + MINUTE);
    const mark = START + MINUTE;
    const first = await destroy(w);
    const second = await destroy(w);
    expect({ first, second, cancels: cancels(w), answeredFor: (await skeleton(w))?.cancel_answered_for, periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      first: { result: "not-finished" },
      second: { result: "not-finished" },
      cancels: [["sub_1", CUSTOMER, "cancelled"]],
      answeredFor: "sub_1",
      periods: 0,
    });
    w.close();
  });

  test("two deletes in flight, with a second subscription on the customer: one call ends both, the second delete meets the record and calls nothing, and the answer is stored once", async () => {
    const hold = held();
    const w = await listed({ plans: [{ id: "sub_2" }], faults: [hold.fault] }, { faults: [{ kind: "request-lost" }] });
    await subscribed(w);
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const ids = started(w);
    w.at(START + MINUTE);
    const mark = w.now();
    const first = destroy(w);
    await tick();
    w.at(START + MINUTE + 10_000);
    const second = await destroy(w);
    hold.release();
    expect({ first: await first, second, ids, cancels: cancels(w), answeredAt: (await skeleton(w))?.cancel_answered_at, periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      first: { result: "not-finished" },
      second: { result: "not-finished" },
      ids: ["sub_1"],
      cancels: [["sub_1", CUSTOMER, "cancelled"]],
      answeredAt: START + MINUTE + 10_000,
      periods: 0,
    });
    w.close();
  });

  test("U1: a stored id that the provider does not know and a customer that it does not know (a key of the other mode): the delete does not finish, nothing is stored and the storage stays", async () => {
    const w = await listed({ known: false }, { subscribed: false });
    await subscribed(w, "sub_gone");
    w.at(START + MINUTE);
    const answer = await destroy(w);
    const row = await skeleton(w);
    expect({ answer, holds: await holds(w), answeredAt: row === null ? "gone" : row.cancel_answered_at, cancels: cancels(w) }).toEqual({
      answer: { result: "not-finished" },
      holds: true,
      answeredAt: null,
      cancels: [["sub_gone", CUSTOMER, "failed", 0]],
    });
    w.close();
  });

  test("U1: the stored id refused and read as ended, with a second subscription that has not ended on the list: the second is cancelled, and no period begins after the mark", async () => {
    const w = await listed({ plans: [{ id: "sub_2" }] });
    await subscribed(w);
    w.meter.endSubscription(ACCOUNT, Math.floor(START / 1000) + 100, "sub_1");
    w.at(START + MINUTE);
    const mark = w.now();
    const answer = await destroy(w);
    expect({ answer, sub_2: statusOf(w, "sub_2"), cancels: cancels(w), periods: periodsAfter(w, mark, mark + 90 * DAY) }).toEqual({
      answer: { result: "deleted" },
      sub_2: "canceled",
      cancels: [["sub_1", CUSTOMER, "cancelled"]],
      periods: 0,
    });
    w.close();
  });
});

describe("guards that hold at e9c45eec and at the head", () => {
  test("a stored id that the provider no longer knows, with the expected customer and an empty list: the delete ends", async () => {
    const w = await listed({}, { subscribed: false });
    await subscribed(w, "sub_gone");
    w.at(START + MINUTE);
    expect(await destroy(w)).toEqual({ result: "deleted" });
    w.close();
  });

  test("a refused cancel of the stored id whose read shows it ended, with an empty list: the delete ends", async () => {
    const w = await listed({});
    await subscribed(w);
    w.meter.endSubscription(ACCOUNT, Math.floor(START / 1000) + 100, "sub_1");
    w.at(START + MINUTE);
    expect(await destroy(w)).toEqual({ result: "deleted" });
    w.close();
  });
});
