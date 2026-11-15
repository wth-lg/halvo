import { describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { ALARM_RETRY_MS, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms, Sql, Step } from "../../src/types.ts";
import { HASH, HOUR, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

type Kind = "cancelled" | "nothing-to-cancel" | "failed";

const events = (w: World) =>
  w.lines.map((line) => {
    try {
      return JSON.parse(line).event as string;
    } catch {
      return "?";
    }
  });

/** One instance over the world's host, whose named module functions throw while their switch is on. */
function own(w: World, faults: Record<string, () => boolean> = {}) {
  const wrap = <T extends object>(prefix: string, target: T): T => {
    const out: Record<string, unknown> = { ...(target as Record<string, unknown>) };
    for (const [name, on] of Object.entries(faults)) {
      const [module, fn] = name.split(".") as [string, string];
      if (module !== prefix) continue;
      const inner = (target as Record<string, (...a: unknown[]) => unknown>)[fn] as (...a: unknown[]) => unknown;
      out[fn] = (...a: unknown[]) => {
        if (on()) throw new RangeError(`${name} throws`);
        return inner(...a);
      };
    }
    return out as T;
  };
  return makeEntries(w.host, {
    accounts: wrap("accounts", makeAccounts()),
    ledger: wrap("ledger", makeLedger({ holdLifeMs: w.host.config.holdLifeMs })),
    metering: wrap("metering", makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds })),
  });
}

function cancelPort(w: World, answer: () => Kind) {
  const calls: Ms[] = [];
  w.host.port.cancelSubscription = async () => {
    calls.push(w.now());
    return { kind: answer() } as never;
  };
  return calls;
}

/** The count step's exit read of the mark (the first read of the mark in countFailure) throws while `on` is set. */
function exitReadThrows(w: World) {
  const state = { on: false, readsInTheCount: 0 };
  const inner = w.host.step;
  w.host.step = (<T>(run: (sql: Sql) => T): T =>
    inner((sql) =>
      run({
        exec: (query: string, ...bindings: (string | number | null)[]) => {
          if (state.on && query.startsWith("SELECT destroying_since") && String(new Error().stack).includes("countFailure")) {
            state.readsInTheCount += 1;
            if (state.readsInTheCount === 1) throw new RangeError("the exit's read of the mark throws");
          }
          return sql.exec(query, ...bindings);
        },
      } as Sql),
    )) as Step;
  return state;
}

const skeleton = async (w: World) => (await w.tables()).skeleton?.[0];
const nextTry = async (w: World): Promise<Ms | null> => {
  const at = (await skeleton(w))?.cancel_next_at;
  return at === null || at === undefined ? null : Number(at);
};

/** Runs the alarm at its due time with `entries`; answers the run's time, the arm after it and the stored next try. */
async function runAt(w: World, entries: ReturnType<typeof makeEntries>, before: () => void = () => {}) {
  const due = await w.host.getAlarm();
  if (due === null) throw new Error("no alarm stands");
  w.at(Math.max(due, w.now()));
  const at = w.now();
  before();
  await quiet(w.lines, () => entries.alarm());
  return { at, armed: await w.host.getAlarm(), next: await nextTry(w), failures: Number((await skeleton(w))?.alarm_failures) };
}

async function failingRunsWithNoMark(w: World, a: ReturnType<typeof makeEntries>, n: number, each: () => void = () => {}) {
  for (let i = 0; i < n; i += 1) await runAt(w, a, each);
}

describe("the fourth closed list of P7-115 (unit 5.6 part 1) A1: the count of failing runs restarts at the mark", () => {
  test("P6: four failing runs with no mark (ledger.nextDue throws), a delete during the fifth run's send: the exit arms no later than the delete's own arm, and the next pause is 60 s or the cancel's next try", async () => {
    const w = await world({ testClock: false });
    let nextDue = false;
    cancelPort(w, () => "failed");
    const a = own(w, { "ledger.nextDue": () => nextDue });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    nextDue = true;
    await quiet(w.lines, () => a.rearm());
    await failingRunsWithNoMark(w, a, 4);
    const failuresBefore = Number((await skeleton(w))?.alarm_failures);
    await w.use("r0", 10_000);
    const send = w.host.port.sendUsage.bind(w.host.port);
    let destroyArm: Ms | null = null;
    let hooked = false;
    w.host.port.sendUsage = async (args) => {
      if (!hooked) {
        hooked = true;
        await quiet(w.lines, () => a.destroy(HASH));
        destroyArm = await w.host.getAlarm();
      }
      return send(args);
    };
    w.at(START + HOUR + 1000);
    const runAtTime = w.now();
    await quiet(w.lines, () => a.alarm());
    const exitArm = await w.host.getAlarm();
    const second = await runAt(w, a);
    w.close();
    expect([failuresBefore, hooked]).toEqual([4, true]);
    expect(destroyArm).toBe(runAtTime + ALARM_RETRY_MS);
    expect(exitArm).toBe(runAtTime + ALARM_RETRY_MS);
    expect(second.at).toBe(runAtTime + ALARM_RETRY_MS);
    expect(second.failures).toBe(2);
    expect(second.armed).toBe(Math.max(second.at + 2 * ALARM_RETRY_MS, second.next ?? 0));
    expect(events(w)).not.toContain("alarm-stopped");
  });

  for (const stored of [4, 6] as const) {
    test(`the mirror: ${stored} failing runs stored from before a delete made before the run (${stored === 4 ? "runs whose exit read works" : "runs whose exit read throws, so the count passes 5"}): the first failing exit arms at the later of now plus ALARM_RETRY_MS and the delete term, the stored count is 1, and the second gap is twice ALARM_RETRY_MS or the next try`, async () => {
      const w = await world({ testClock: false });
      let nextDue = false;
      const cancels = cancelPort(w, () => "failed");
      const reads = exitReadThrows(w);
      const a = own(w, { "ledger.nextDue": () => nextDue });
      await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
      w.at(START + 1000);
      nextDue = true;
      await quiet(w.lines, () => a.rearm());
      await failingRunsWithNoMark(w, a, stored, () => {
        reads.on = stored === 6;
        reads.readsInTheCount = 0;
      });
      reads.on = false;
      const failuresBefore = Number((await skeleton(w))?.alarm_failures);
      w.at(w.now() + 1000);
      await quiet(w.lines, () => a.destroy(HASH));
      const first = await runAt(w, a);
      const second = await runAt(w, a);
      w.close();
      expect(failuresBefore).toBe(stored);
      expect(cancels.length).toBeGreaterThan(0);
      expect(first.failures).toBe(1);
      expect(first.armed).toBe(Math.max(first.at + ALARM_RETRY_MS, first.next ?? 0));
      expect(second.failures).toBe(2);
      expect((second.armed as Ms) - second.at).toBe(Math.max(2 * ALARM_RETRY_MS, (second.next ?? 0) - second.at));
    });
  }

  test("a count kept in memory under no mark is dropped when the count step's own read of the mark gives another time: six runs of A whose exit read throws, a delete answered by instance B, then A's failing run whose exit read throws pauses ALARM_RETRY_MS", async () => {
    const w = await world({ testClock: false });
    let nextDue = false;
    cancelPort(w, () => "failed");
    const reads = exitReadThrows(w);
    const a = own(w, { "ledger.nextDue": () => nextDue });
    const b = own(w, { "ledger.nextDue": () => nextDue });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    nextDue = true;
    await quiet(w.lines, () => a.rearm());
    const each = () => {
      reads.on = true;
      reads.readsInTheCount = 0;
    };
    await failingRunsWithNoMark(w, a, 6, each);
    reads.on = false;
    w.at(w.now() + 1000);
    await quiet(w.lines, () => b.destroy(HASH));
    const run = await runAt(w, a, each);
    w.close();
    expect(run.failures).toBe(1);
    expect(run.armed).toBe(Math.max(run.at + ALARM_RETRY_MS, run.next ?? 0));
  });
});
