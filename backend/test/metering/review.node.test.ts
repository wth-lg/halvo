import { describe, expect, test } from "vitest";
import type { ProviderEvent } from "../../src/billing/port.ts";
import { makeMetering, SEND_LIMIT_MS, SYNC_RETRY_MS } from "../../src/metering/index.ts";
import { openSql } from "../../test-kit/sql/node.ts";
import { ACCOUNT, HASH, HOUR, START, subscriptionEvent, type World, world } from "./world.ts";

const MINUTE = 60_000;
const LONG_HOLDS = { config: { holdLifeMs: 2 * HOUR } };

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

function sent(w: World): unknown {
  return w.meter.recordedEvents(ACCOUNT).map((each) => [each.identifier, each.amountMinor, each.timestamp]);
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

describe("the review's cases on one cut in every order (unit 5.1, PA-2)", () => {
  test("a newer cancel event that states an earlier end than the kept provider time moves the cut back to that end, as the other order cuts there", async () => {
    const noEnd = subscriptionEvent({ eventId: "evt_no_end", providerTime: START + 20 * MINUTE, state: "cancelled" });
    const stated = subscriptionEvent({ eventId: "evt_stated", providerTime: START + 25 * MINUTE, state: "cancelled", endedAt: START + 10 * MINUTE });
    const seen = [await cutAfter([noEnd, stated]), await cutAfter([stated, noEnd])];
    expect(seen).toEqual(Array(2).fill({ intervals: [[START, START + 10 * MINUTE]], cut: [["cancel", START + 10 * MINUTE]] }));
  });

  test("an earlier cancel time cuts the unsealed interval that holds it, also when the first cancel ended nothing and a refused reserve opened the next interval", async () => {
    const late = subscriptionEvent({ eventId: "evt_late", providerTime: START + HOUR + 2 * MINUTE, state: "cancelled" });
    const early = subscriptionEvent({ eventId: "evt_early", providerTime: START + HOUR + MINUTE, state: "cancelled", endedAt: START + 30 * MINUTE });
    const run = async (first: ProviderEvent, second: ProviderEvent) => {
      const w = await world(LONG_HOLDS);
      await w.use("r0", 25_500);
      await w.clock(START + 31 * MINUTE);
      await w.reserve("r1");
      await w.clock(START + HOUR + 5 * MINUTE);
      await w.entries().webhookEvent(first);
      const refused = await w.reserve("r2");
      await w.entries().webhookEvent(second);
      await w.settle("r1", 10_000);
      await w.clock(START + 3 * HOUR);
      const first_interval = (await w.intervals())[0];
      const out = { refused, first: [first_interval?.start, first_interval?.end_at], sent: sent(w) };
      w.close();
      return out;
    };
    const expected = {
      refused: { outcome: "refused", reason: "not-active" },
      first: [START, START + 30 * MINUTE],
      sent: [[`${ACCOUNT}:${START / 1000}`, 3, (START + 30 * MINUTE) / 1000 - 1]],
    };
    expect([await run(late, early), await run(early, late)]).toEqual([expected, expected]);
  });

  test("a delete between a cancel and an earlier cancel time still moves the cut back on the interval that the cancel cut", async () => {
    const at9 = subscriptionEvent({ eventId: "evt_9", providerTime: START + 9 * MINUTE, state: "cancelled" });
    const at5 = subscriptionEvent({ eventId: "evt_5", providerTime: START + 5 * MINUTE, state: "cancelled" });
    const run = async (deleteBeforeTheEarlierCancel: boolean) => {
      const w = await world(LONG_HOLDS);
      await w.use("r0", 25_500);
      await w.clock(START + MINUTE);
      await w.reserve("r1");
      await w.clock(START + 15 * MINUTE);
      if (!deleteBeforeTheEarlierCancel) await w.entries().webhookEvent(at5);
      await w.entries().webhookEvent(at9);
      await w.reserve("r2");
      const first = await w.entries().destroy(HASH);
      if (deleteBeforeTheEarlierCancel) await w.entries().webhookEvent(at5);
      await w.settle("r1", 10_000);
      const second = await w.entries().destroy(HASH);
      const out = { first, second, sent: sent(w) };
      w.close();
      return out;
    };
    const expected = {
      first: { result: "not-finished" },
      second: { result: "deleted" },
      sent: [[`${ACCOUNT}:${START / 1000}`, 3, (START + 5 * MINUTE) / 1000 - 1]],
    };
    expect([await run(true), await run(false)]).toEqual([expected, expected]);
  });
});

describe("the review's case on the remainder after a cut (unit 5.1)", () => {
  test("after a cancel, an active event stamped later than it changes nothing: the later uses are refused, the meter's total is the 1 minor unit used before the cut, and nothing is left to write off", async () => {
    const w = await world();
    await w.use("r0", 10_000);
    await w.clock(START + 10 * MINUTE);
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_c", providerTime: START + 10 * MINUTE, state: "cancelled", endedAt: START + 10 * MINUTE }));
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_a", providerTime: START + 11 * MINUTE, state: "active" }));
    await w.clock(START + 12 * MINUTE);
    const later = [(await w.use("r2", 15_000)) as { reserved: unknown }];
    await w.clock(START + 2 * HOUR);
    later.push((await w.use("r3", 5_000)) as { reserved: unknown });
    await w.clock(START + 4 * HOUR);
    const tables = await w.tables();
    expect(later.map((each) => each.reserved)).toEqual(Array(2).fill({ outcome: "refused", reason: "not-active" }));
    expect(w.meter.exactTotal(ACCOUNT, 0, Number.MAX_SAFE_INTEGER)).toBe(1);
    expect((tables.metering_account ?? []).map((row) => [row.carry, row.written_off])).toEqual([[0, 0]]);
    w.close();
  });
});

describe("the review's cases on the edges of the send limit and the retry delay (unit 5.1)", () => {
  test("one millisecond before the interval's timestamp plus SEND_LIMIT_MS the interval still gets a call, a read past the window, and the next wake after that closes it with no call", async () => {
    const w = await world({ always: { kind: "request-lost" } });
    await w.use("r1", 30_000);
    const limit = (START + HOUR - 1000) + SEND_LIMIT_MS;
    await w.clock(START + HOUR);
    await w.clock(limit - 1);
    const before = { calls: kinds(w.calls), outcome: (await w.intervals())[0]?.outcome };
    await w.clock(limit - 1 + SYNC_RETRY_MS);
    const after = { calls: kinds(w.calls), outcome: (await w.intervals())[0]?.outcome };
    expect({ before, after }).toEqual({
      before: { calls: ["send failed", "read failed"], outcome: null },
      after: { calls: ["send failed", "read failed"], outcome: "not-billable" },
    });
    w.close();
  });

  test("send names no call one millisecond before the stored next attempt, and names the retry at it", () => {
    const db = openSql();
    const metering = makeMetering({ intervalSeconds: 3600, marginSeconds: 300 });
    const seen = db.step((sql) => {
      metering.init(sql);
      metering.begin(sql, ACCOUNT, 0);
      metering.intervalAt(sql, 1000);
      metering.seal(sql, 0, 20_000, 3_600_000);
      const first = metering.send(sql, 3_600_000).call;
      if (first === null) throw new Error("no first call");
      metering.record(sql, first, { kind: "failed" }, 3_600_000);
      return {
        early: metering.send(sql, 3_600_000 + SYNC_RETRY_MS - 1).call,
        due: metering.send(sql, 3_600_000 + SYNC_RETRY_MS).call?.kind,
      };
    });
    db.close();
    expect(seen).toEqual({ early: null, due: "send" });
  });
});
