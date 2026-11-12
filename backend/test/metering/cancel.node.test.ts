import { describe, expect, test } from "vitest";
import type { ProviderEvent } from "../../src/billing/port.ts";
import { reduceEvent } from "../../src/billing/stripe/reduce.ts";
import { ACCOUNT, HASH, HOUR, START, subscriptionEvent, world } from "./world.ts";

const MINUTE = 60_000;
const ENDED = START + 10 * MINUTE;
const SESSION = { sessionId: "cs_cancel", customerId: "cus_1", subscriptionId: "sub_1", payment: "paid" as const };
const COMPLETED: ProviderEvent = { kind: "checkout-completed", eventId: "evt_completed", providerTime: START + 2 * MINUTE, accountId: ACCOUNT, ...SESSION };
const CHECKOUT = { sessionId: SESSION.sessionId, url: "http://127.0.0.1:1/x" };

function orders<T>(items: T[]): T[][] {
  if (items.length <= 1) return [items];
  return items.flatMap((item, index) => orders([...items.slice(0, index), ...items.slice(index + 1)]).map((rest) => [item, ...rest]));
}

async function cutAfter(events: ProviderEvent[]): Promise<unknown> {
  const w = await world();
  await w.use("r1", 25_500);
  await w.clock(START + 30 * MINUTE);
  for (const event of events) await w.entries().webhookEvent(event);
  const tables = await w.tables();
  w.close();
  return {
    intervals: (tables.metering_interval ?? []).map((row) => [row.start, row.end_at]),
    cut: (tables.metering_account ?? []).map((row) => [row.cut_kind, row.cut_at]),
  };
}

describe("the cancel's time reaches the entries (unit 5.1, the Revision of 2026-11-11)", () => {
  test("a subscription event's ended_at reaches the provider event as endedAt in milliseconds, and an object that states none gives null", () => {
    const body = (object: Record<string, unknown>) => ({
      id: "evt_1",
      type: "customer.subscription.deleted",
      created: 1_800_000_100,
      data: { object: { id: "sub_1", customer: "cus_1", status: "canceled", metadata: { halvo_account: ACCOUNT }, ...object } },
    });
    expect(reduceEvent(body({ ended_at: 1_800_000_050 }))).toEqual(expect.objectContaining({ kind: "subscription", state: "cancelled", endedAt: 1_800_000_050_000 }));
    expect(reduceEvent(body({ ended_at: null }))).toEqual(expect.objectContaining({ kind: "subscription", endedAt: null }));
    expect(reduceEvent(body({}))).toEqual(expect.objectContaining({ kind: "subscription", endedAt: null }));
  });

  test("the cut at a cancel takes the subscription's end that the object states, over the event's provider time", async () => {
    const cut = await cutAfter([subscriptionEvent({ eventId: "evt_c", providerTime: START + 20 * MINUTE, state: "cancelled", endedAt: ENDED })]);
    expect(cut).toEqual({ intervals: [[START, ENDED]], cut: [["cancel", ENDED]] });
  });
});

describe("one cut in every order of arrival (unit 5.1, the coordinator's point PA-2)", () => {
  test("the same cancel events, each stating the subscription's end, give one cut at that end in all six orders", async () => {
    const events = [
      subscriptionEvent({ eventId: "evt_active", providerTime: START + 5 * MINUTE, state: "active" }),
      subscriptionEvent({ eventId: "evt_updated", providerTime: START + 15 * MINUTE, state: "cancelled", endedAt: ENDED }),
      subscriptionEvent({ eventId: "evt_deleted", providerTime: START + 16 * MINUTE, state: "cancelled", endedAt: ENDED }),
    ];
    const seen = [];
    for (const order of orders(events)) seen.push(await cutAfter(order));
    expect(seen).toEqual(Array(6).fill({ intervals: [[START, ENDED]], cut: [["cancel", ENDED]] }));
  });

  test("where no object states the end, the earliest provider time of the cancel events gives the cut in all six orders", async () => {
    const events = [
      subscriptionEvent({ eventId: "evt_active", providerTime: START + 5 * MINUTE, state: "active" }),
      subscriptionEvent({ eventId: "evt_updated", providerTime: START + 15 * MINUTE, state: "cancelled" }),
      subscriptionEvent({ eventId: "evt_deleted", providerTime: START + 16 * MINUTE, state: "cancelled" }),
    ];
    const seen = [];
    for (const order of orders(events)) seen.push(await cutAfter(order));
    expect(seen).toEqual(Array(6).fill({ intervals: [[START, START + 15 * MINUTE]], cut: [["cancel", START + 15 * MINUTE]] }));
  });

  test("a cancel event that is not newer but earlier moves the cut back while the cut interval is not sealed, and changes nothing after its seal", async () => {
    const w = await world();
    await w.use("r1", 25_500);
    await w.clock(START + 30 * MINUTE);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_16", providerTime: START + 16 * MINUTE, state: "cancelled" }));
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_15", providerTime: START + 15 * MINUTE, state: "cancelled" }));
    const moved = (await w.intervals()).map((row) => [row.end_at, row.sealed_at]);
    await w.clock(START + 31 * MINUTE);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_14", providerTime: START + 14 * MINUTE, state: "cancelled" }));
    const after = (await w.intervals()).map((row) => [row.end_at, row.outcome]);
    expect(moved).toEqual([[START + 15 * MINUTE, null]]);
    expect(after).toEqual([[START + 15 * MINUTE, "acknowledged"]]);
    w.close();
  });
});

describe("an account made in the state cancelled (unit 5.1, contract 3.4)", () => {
  async function madeCancelled(events: ProviderEvent[]): Promise<unknown> {
    const w = await world({ seed: false, checkout: CHECKOUT });
    expect(await w.entries().start(ACCOUNT, HASH, { amount: 1000, period: "month" })).toEqual({ result: "session", url: CHECKOUT.url });
    for (const event of events) await w.entries().webhookEvent(event);
    const tables = await w.tables();
    w.close();
    return {
      state: (tables.account ?? []).map((row) => row.state),
      cut: (tables.metering_account ?? []).map((row) => [row.cut_kind, row.cut_at]),
    };
  }

  test("the step that makes an account in the state cancelled cuts at the cancel's time, as the cancel event does after the account is made", async () => {
    const cancel = subscriptionEvent({ eventId: "evt_cancel", providerTime: START + 20 * MINUTE, state: "cancelled", endedAt: ENDED });
    const seen = [await madeCancelled([cancel, COMPLETED]), await madeCancelled([COMPLETED, cancel])];
    expect(seen).toEqual(Array(2).fill({ state: ["cancelled"], cut: [["cancel", ENDED]] }));
  });

  test("an account that the alarm makes from a read in the state cancelled gets the same cut in that step", async () => {
    const w = await world({ seed: false, checkout: CHECKOUT, session: SESSION });
    await w.entries().start(ACCOUNT, HASH, { amount: 1000, period: "month" });
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", providerTime: START + 20 * MINUTE, state: "cancelled", endedAt: ENDED }));
    w.at(START + HOUR);
    await w.entries().alarm();
    const tables = await w.tables();
    expect((tables.account ?? []).map((row) => row.state)).toEqual(["cancelled"]);
    expect((tables.metering_account ?? []).map((row) => [row.cut_kind, row.cut_at])).toEqual([["cancel", ENDED]]);
    w.close();
  });
});
