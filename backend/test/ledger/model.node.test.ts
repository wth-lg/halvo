import fc from "fast-check";
import { expect, test, vi } from "vitest";
import { type Call, HOLD_LIFE_MS, type Step, seededRig } from "./harness.ts";
import { Model } from "./model.ts";

vi.setConfig({ testTimeout: 19_828 });

const WEEK = 604_800_000;
const START = 1_893_456_000_000;
const HIGH = { input: 15_500_000, output: 78_500_000 };

const id = fc.constantFrom("r0", "r1", "r2", "r3", "r4");
const usage = fc.record({ inputTokens: fc.integer({ min: 0, max: 3000 }), outputTokens: fc.integer({ min: 0, max: 3000 }) });
const call: fc.Arbitrary<Call> = fc.oneof(
  fc
    .record({ requestId: id, amount: fc.constantFrom(5_000, 10_000, 15_000), high: fc.boolean() })
    .map(({ requestId, amount, high }): Call => ({ kind: "reserve", requestId, amount, ...(high ? { tier: "frontier" as const, price: HIGH } : {}) })),
  id.map((requestId): Call => ({ kind: "dispatch", requestId })),
  fc.record({ requestId: id, usage }).map(({ requestId, usage: used }): Call => ({ kind: "settle", requestId, usage: used })),
  id.map((requestId): Call => ({ kind: "void", requestId })),
  fc.constant<Call>({ kind: "alarm" }),
  fc.integer({ min: 1, max: 4 }).map((amount): Call => ({ kind: "capSet", cap: { amount, period: "week" } })),
  fc.constant<Call>({ kind: "capGet" }),
  fc.constant<Call>({ kind: "restart" }),
);
const gap = fc.constantFrom(0, 1, 700, 1500, 2999, 3000, 3001, WEEK);
const plan = fc.array(fc.tuple(gap, call), { maxLength: 40 });

test("the ledger behind the object's entries answers every call as the reference model does, and its totals and holds agree after every step", async () => {
  await fc.assert(
    fc.asyncProperty(plan, async (pairs) => {
      const rig = await seededRig({ cap: { amount: 2, period: "week" }, at: START });
      const model = new Model(START, 2, HOLD_LIFE_MS);
      let at = START;
      let endedSeqs = new Map<string, unknown>();
      try {
        for (const [wait, each] of pairs) {
          at += wait;
          const step: Step = { at, call: each };
          const got = await rig.run(step);
          const want = model.run(step);
          expect({ step, got }).toEqual({ step, got: want });
          const read = rig.read();
          const periods = Object.fromEntries(read.periods.map((row) => [row.period_start, { reported: row.reported, unreported: row.unreported, open: row.open }]));
          expect({ step, periods }).toEqual({ step, periods: Object.fromEntries(model.periods) });
          const states = Object.fromEntries(read.holds.map((row) => [row.request_id, row.state]));
          expect({ step, states }).toEqual({ step, states: Object.fromEntries([...model.holds].map(([key, hold]) => [key, hold.state])) });
          const open = read.holds.filter((row) => row.state === "open");
          if (each.kind !== "restart") expect({ step, overdue: open.filter((row) => Number(row.expires_at) <= at).length }).toEqual({ step, overdue: 0 });
          if (open.length > 0) {
            const earliest = Math.min(...open.map((row) => Number(row.expires_at)));
            expect({ step, alarmLate: (rig.alarmAt() ?? Number.POSITIVE_INFINITY) > earliest }).toEqual({ step, alarmLate: false });
          }
          const next = new Map(read.holds.map((row) => [String(row.request_id), row.ended_seq]));
          for (const [key, seq] of endedSeqs) if (seq !== null) expect({ step, key, seq: next.get(key) }).toEqual({ step, key, seq });
          endedSeqs = next;
        }
      } finally {
        rig.close();
      }
    }),
    { numRuns: 300, seed: 20261111 },
  );
});

test("a replay of a fixed trace through the entries gives the model's answers, a restart among them", async () => {
  const trace: Step[] = [
    { at: START + 1, call: { kind: "reserve", requestId: "a", amount: 15_000 } },
    { at: START + 2, call: { kind: "dispatch", requestId: "a" } },
    { at: START + 3, call: { kind: "reserve", requestId: "b", amount: 10_000 } },
    { at: START + 4, call: { kind: "restart" } },
    { at: START + 5, call: { kind: "reserve", requestId: "b", amount: 5_000 } },
    { at: START + 6, call: { kind: "capSet", cap: { amount: 1, period: "week" } } },
    { at: START + 7, call: { kind: "settle", requestId: "a", usage: { inputTokens: 1000, outputTokens: 1000 } } },
    { at: START + 8, call: { kind: "reserve", requestId: "c", amount: 5_000 } },
    { at: START + 3010, call: { kind: "alarm" } },
    { at: START + WEEK, call: { kind: "reserve", requestId: "c", amount: 5_000 } },
    { at: START + WEEK + 1, call: { kind: "capGet" } },
  ];
  const rig = await seededRig({ cap: { amount: 2, period: "week" }, at: START });
  const model = new Model(START, 2, HOLD_LIFE_MS);
  const got = [];
  for (const step of trace) got.push(await rig.run(step));
  rig.close();
  expect(got).toEqual(trace.map((step) => model.run(step)));
  expect(got[6]).toEqual({ outcome: "settled", cost: 18_800, overrun: 3_800, repeat: false });
  expect(got[7]).toEqual({ outcome: "refused", reason: "over-cap" });
  expect(got[9]).toEqual({ outcome: "admitted", receipt: { requestId: "c", amount: 5_000, now: START + WEEK, expiresAt: START + WEEK + 3000 } });
});
