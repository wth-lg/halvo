import { expect, test } from "vitest";
import { openSql } from "../../test-kit/sql/node.ts";
import { expiryCases } from "./expiry-cases.ts";
import { type Rig, seededRig } from "./harness.ts";

const T = Date.UTC(2031, 1, 3);
const WEEK_MS = 604_800_000;
const USAGE = { inputTokens: 100, outputTokens: 100 };

for (const each of expiryCases) {
  test(`${each.name} (node:sqlite)`, () => {
    const db = openSql();
    let seen: unknown;
    try {
      seen = each.run(db.step);
    } finally {
      db.close();
    }
    expect(seen).toEqual(each.expected);
  });
}

function endsOf(rig: Rig) {
  return Object.fromEntries(rig.read().holds.map((row) => [row.request_id, { state: row.state, dispatched: row.dispatched, ended_by: row.ended_by }]));
}

for (const path of ["the alarm on the host clock", "a clock call under the test clock", "the alarm after a restart"] as const) {
  test(`through the object's entries, ${path} voids at its expiry the hold that was never dispatched and expires the dispatched one at its full amount`, async () => {
    const rig = await seededRig({ cap: { amount: 10, period: "week" }, at: T });
    try {
      if (path === "a clock call under the test clock") expect(await rig.entries().testSetClock(T)).toEqual({ result: "clock-set" });
      await rig.run({ at: T, call: { kind: "reserve", requestId: "unmarked", amount: 40_000 } });
      await rig.run({ at: T, call: { kind: "reserve", requestId: "marked", amount: 50_000 } });
      expect(await rig.run({ at: T, call: { kind: "dispatch", requestId: "marked" } })).toBe("go");
      if (path === "a clock call under the test clock") {
        expect(await rig.entries().testSetClock(T + 3000)).toEqual({ result: "clock-set" });
      } else {
        if (path === "the alarm after a restart") await rig.run({ at: T + 1, call: { kind: "restart" } });
        expect(rig.alarmAt()).toBe(T + 3000);
        await rig.run({ at: T + 3000, call: { kind: "alarm" } });
      }
      expect(endsOf(rig)).toEqual({
        unmarked: { state: "voided", dispatched: 0, ended_by: "expire" },
        marked: { state: "expired", dispatched: 1, ended_by: "expire" },
      });
      expect(rig.read().periods).toEqual([expect.objectContaining({ reported: 0, unreported: 50_000, open: 0 })]);
      expect(await rig.run({ at: T + 3001, call: { kind: "capGet" } })).toEqual({
        result: "cap",
        cap: { amount: 10, period: "week" },
        spend: { spent: 50_000, held: 0, period_end: T + WEEK_MS },
      });
      expect(await rig.run({ at: T + 3002, call: { kind: "void", requestId: "unmarked" } })).toEqual({ outcome: "refused", reason: "already-ended" });
      expect(await rig.run({ at: T + 3003, call: { kind: "settle", requestId: "unmarked", usage: USAGE } })).toEqual({ outcome: "refused", reason: "already-ended" });
      expect(await rig.run({ at: T + 3004, call: { kind: "void", requestId: "marked" } })).toEqual({ outcome: "refused", reason: "already-ended" });
    } finally {
      rig.close();
    }
  });
}
