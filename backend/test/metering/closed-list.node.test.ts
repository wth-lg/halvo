import { describe, expect, test } from "vitest";
import { PROVIDER_CALL_TIMEOUT_MS } from "../../src/billing/index.ts";
import type { ProviderEvent } from "../../src/billing/port.ts";
import { makeMetering, SYNC_RETRY_MS } from "../../src/metering/index.ts";
import { ACCOUNT, HOUR, START, subscriptionEvent, world } from "./world.ts";

const MINUTE = 60_000;
const END = START + HOUR;
const NOT_BILLABLE = '{"event":"usage-not-billable"}';

function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

describe("an answer that a stop of the object lost (unit 5.1, B-29)", () => {
  for (const reason of ["unknown_account", "timestamp_too_old"] as const) {
    test(`B-29: after a stop between a send that the meter took and its record step, a ${reason} refusal waits for a read, and the meter that holds the amount acknowledges the interval`, async () => {
      const w = await world();
      w.faults.push({ kind: "during", run: async () => w.stopNextStep("before"), after: { kind: "none" } }, { kind: "refuse", reason });
      await w.use("r1", 30_000);
      await w.clock(END);
      const afterTheStop = (await w.intervals()).map((row) => [row.outcome, row.answer_due]);
      w.restart();
      await w.clock(END + SYNC_RETRY_MS);
      const [row] = await w.intervals();
      expect(afterTheStop).toEqual([[null, 1]]);
      expect(kinds(w.calls)).toEqual(["send recorded", `send refused ${reason}`, "read total 3"]);
      expect([row?.outcome, row?.refusals, row?.refusal, row?.unanswered, row?.answer_due]).toEqual(["acknowledged", 0, null, 1, 0]);
      expect(w.meter.recordedEvents(ACCOUNT)).toHaveLength(1);
      expect(w.lines.filter((line) => line === NOT_BILLABLE)).toEqual([]);
      w.close();
    });
  }
});

describe("two cancel events that state different ends (unit 5.1, B-44)", () => {
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
      kept: (tables.account ?? []).map((row) => [row.ended_at, row.cancel_first]),
    };
  }

  test("B-44: the earlier of two stated ends gives the one cut, in both orders, whether the newer or the older event states it", async () => {
    const at = (minutes: number) => START + minutes * MINUTE;
    const pairs: [ProviderEvent, ProviderEvent][] = [
      [
        subscriptionEvent({ eventId: "evt_older_late_end", providerTime: at(15), state: "cancelled", endedAt: at(12) }),
        subscriptionEvent({ eventId: "evt_newer_early_end", providerTime: at(16), state: "cancelled", endedAt: at(10) }),
      ],
      [
        subscriptionEvent({ eventId: "evt_older_early_end", providerTime: at(15), state: "cancelled", endedAt: at(10) }),
        subscriptionEvent({ eventId: "evt_newer_late_end", providerTime: at(16), state: "cancelled", endedAt: at(12) }),
      ],
    ];
    const seen = [];
    for (const [first, second] of pairs) {
      seen.push(await cutAfter([first, second]));
      seen.push(await cutAfter([second, first]));
    }
    expect(seen).toEqual(Array(4).fill({ intervals: [[START, at(10)]], cut: [["cancel", at(10)]], kept: [[at(10), at(15)]] }));
  });
});

describe("the margin (unit 5.1, B-32)", () => {
  test("B-32: the factory refuses a margin that is not above the provider call's timeout", () => {
    const made = (marginSeconds: number) => {
      try {
        makeMetering({ intervalSeconds: 3600, marginSeconds });
        return "made";
      } catch (error) {
        return (error as Error).name;
      }
    };
    const atTheTimeout = PROVIDER_CALL_TIMEOUT_MS / 1000;
    expect([made(0), made(atTheTimeout), made(atTheTimeout + 1), made(300)]).toEqual(["RangeError", "RangeError", "made", "made"]);
  });
});
