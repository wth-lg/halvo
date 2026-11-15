import fc from "fast-check";
import { describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { ALARM_RETRY_MS, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms, Sql, Step } from "../../src/types.ts";
import { HASH, HOUR, quiet, START, subscriptionEvent, UNIT, type World, world } from "./world.ts";

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

/** While `on` is set, the first `upTo` reads of the mark in countFailure throw: 1 is the exit's read, 2 adds the count step's. */
function exitReadThrows(w: World) {
  const state = { on: false, readsInTheCount: 0, upTo: 1 };
  const inner = w.host.step;
  w.host.step = (<T>(run: (sql: Sql) => T): T =>
    inner((sql) =>
      run({
        exec: (query: string, ...bindings: (string | number | null)[]) => {
          if (state.on && query.startsWith("SELECT destroying_since") && String(new Error().stack).includes("countFailure")) {
            state.readsInTheCount += 1;
            if (state.readsInTheCount <= state.upTo) throw new RangeError("the count step's read of the mark throws");
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

type Before = 0 | 1 | 4 | 5 | 6 | 7;
type Failure = "pass" | "due-time" | "send";
type ExitThrow = "none" | "exit-read" | "count-write" | "last-now" | "owed-step" | "due-in-owed";
type Arms = "one" | "destroy-arms-differ" | "rearm-first";
interface Cell {
  before: Before;
  failure: Failure;
  exitThrow: ExitThrow;
  exitAfterTheArm: boolean;
  arms: Arms;
}

const stackHas = (name: string) => String(new Error().stack).includes(name);

/**
 * One cell of the drawn test: `before` failing runs with no mark (ledger.nextDue throwing; 5 ends in the stop and a status
 * call that arms again; 6 and 7 by runs whose exit read throws), then a run whose send is held while another entry arms
 * (a delete, and with `rearm-first` a rearm before it), the exit 5 s or 120 s after the run began, a throw at one step of
 * the exit, and the runs after it until the first cancel call. Every arm made by an entry other than the run is recorded.
 */
async function cell(c: Cell) {
  const w = await world({ testClock: false });
  const cancels: Ms[] = [];
  w.host.port.cancelSubscription = async () => {
    cancels.push(w.now());
    return { kind: "failed" } as never;
  };
  const f = { due: false, exit: "none" as ExitThrow, exitReadOnly: false, readsInTheCount: 0, inOther: false, inDestroy: false, threwOnce: false };
  const otherArms: Ms[] = [];
  const setAlarm = w.host.setAlarm.bind(w.host);
  w.host.setAlarm = (at) => {
    if (f.inOther && at !== null) otherArms.push(at);
    setAlarm(at);
  };
  const inner = w.host.step;
  w.host.step = (<T>(run: (sql: Sql) => T): T =>
    inner((sql) =>
      run({
        exec: (query: string, ...bindings: (string | number | null)[]) => {
          if (f.inDestroy && c.arms === "destroy-arms-differ" && query.startsWith("UPDATE skeleton SET last_now")) w.at(w.now() + 2000);
          const inCount = stackHas("countFailure");
          if (inCount && query.startsWith("SELECT destroying_since")) {
            f.readsInTheCount += 1;
            if ((f.exitReadOnly || f.exit === "exit-read") && f.readsInTheCount === 1) throw new RangeError("the exit's read of the mark throws");
          }
          if (inCount && !f.threwOnce && f.exit === "count-write" && query.startsWith("UPDATE skeleton SET alarm_failures")) {
            f.threwOnce = true;
            throw new RangeError("the count's write throws");
          }
          if (inCount && !f.threwOnce && f.exit === "last-now" && query.startsWith("UPDATE skeleton SET last_now")) {
            f.threwOnce = true;
            throw new RangeError("the write of last_now throws");
          }
          return sql.exec(query, ...bindings);
        },
      } as Sql),
    )) as Step;
  const a = own(w, {
    "ledger.nextDue": () => f.due || (f.exit === "due-in-owed" && stackHas("countFailure") && stackHas("dueOf")),
    "accounts.subscription": () => f.exit === "owed-step" && stackHas("countFailure"),
  });
  await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
  w.at(START + 1000);
  f.due = true;
  await quiet(w.lines, () => a.rearm());
  const failing = c.before === 5 ? 5 : c.before;
  for (let i = 0; i < failing; i += 1) {
    const due = await w.host.getAlarm();
    if (due === null) break;
    w.at(Math.max(due, w.now()));
    f.exitReadOnly = c.before >= 6;
    f.readsInTheCount = 0;
    await quiet(w.lines, () => a.alarm());
    f.exitReadOnly = false;
  }
  const stoppedBefore = events(w).filter((e) => e === "alarm-stopped").length;
  if (c.before === 5) {
    w.at(w.now() + 1000);
    await quiet(w.lines, () => a.status(HASH));
  }
  const failuresBefore = Number((await skeleton(w))?.alarm_failures);
  w.at(w.now() + 1000);
  await w.use("r0", 10_000);
  const end = START + Math.ceil((w.now() - START + 1) / HOUR) * HOUR;
  const send = w.host.port.sendUsage.bind(w.host.port);
  const runAt = end + 1000;
  const exitAt = runAt + (c.exitAfterTheArm ? 120_000 : 5_000);
  let deleteAt: Ms | null = null;
  let hooked = false;
  w.host.port.sendUsage = async (args) => {
    if (!hooked) {
      hooked = true;
      w.at(runAt + 1000);
      f.inOther = true;
      if (c.arms === "rearm-first") {
        await quiet(w.lines, () => a.rearm());
        w.at(w.now() + 2000);
      }
      deleteAt = w.now();
      f.inDestroy = true;
      await quiet(w.lines, () => a.destroy(HASH));
      f.inDestroy = false;
      f.inOther = false;
      w.at(exitAt);
      if (c.failure === "send") throw new RangeError("the held send throws");
    }
    return send(args);
  };
  w.at(runAt);
  f.due = c.failure === "due-time";
  f.exit = c.exitThrow;
  f.readsInTheCount = 0;
  await quiet(w.lines, () => a.alarm());
  f.exit = "none";
  const armed = await w.host.getAlarm();
  const row = await skeleton(w);
  const underTheMark = row !== undefined && row.destroying_since !== null;
  const storedAfter = Number(row?.alarm_failures);
  let firstCancel: Ms | null = cancels.find((at) => deleteAt !== null && at >= deleteAt) ?? null;
  for (let n = 0; n < 30 && firstCancel === null; n += 1) {
    if ((await skeleton(w)) === undefined) break;
    const due = await w.host.getAlarm();
    if (due === null) break;
    w.at(Math.max(due, w.now()));
    await quiet(w.lines, () => a.alarm());
    firstCancel = cancels.find((at) => deleteAt !== null && at >= deleteAt) ?? null;
  }
  w.close();
  const earliest = otherArms.length === 0 ? null : Math.min(...otherArms);
  return { hooked, stoppedBefore, failuresBefore, exitAt, armed, underTheMark, storedAfter, earliest, otherArms, firstCancel };
}

function holdsTheBound(c: Cell, r: Awaited<ReturnType<typeof cell>>) {
  const name = JSON.stringify(c);
  expect(r.hooked, name).toBe(true);
  if (c.before === 5) expect(r.stoppedBefore, name).toBe(1);
  expect(r.underTheMark, name).toBe(true);
  expect(r.armed, name).not.toBeNull();
  const bound = r.earliest !== null && r.earliest > r.exitAt ? r.earliest : r.exitAt + ALARM_RETRY_MS;
  expect(r.armed as Ms, `${name} arms ${JSON.stringify(r.otherArms)} exit ${r.exitAt}`).toBeLessThanOrEqual(bound);
  expect(r.armed as Ms, name).toBeGreaterThan(r.exitAt);
  if (c.failure !== "pass" && c.exitThrow !== "count-write") expect(r.storedAfter, name).toBe(1);
  expect(r.firstCancel, name).not.toBeNull();
  expect(r.firstCancel as Ms, name).toBeLessThanOrEqual(Math.max(r.armed as Ms, r.exitAt + ALARM_RETRY_MS));
}

const FIXED: Cell[] = (["none", "exit-read", "count-write", "last-now", "owed-step"] as const).flatMap((exitThrow) =>
  ([false, true] as const).map((exitAfterTheArm) => ({ before: 4 as const, failure: "due-time" as const, exitThrow, exitAfterTheArm, arms: "one" as const })),
);

const cells = fc
  .record({
    before: fc.constantFrom<Before>(0, 1, 4, 5, 6, 7),
    failure: fc.constantFrom<Failure>("pass", "due-time", "send"),
    exitThrow: fc.constantFrom<ExitThrow>("none", "exit-read", "count-write", "last-now", "owed-step", "due-in-owed"),
    exitAfterTheArm: fc.boolean(),
    arms: fc.constantFrom<Arms>("one", "destroy-arms-differ", "rearm-first"),
  })
  .map((c): Cell => (c.failure === "pass" ? { ...c, exitThrow: "none" } : c));

describe("the fourth closed list of P7-115 (unit 5.6 part 1) A2 and A3: an exit never arms later than an arm that another entry set during its run", () => {
  test("the drawn test, seed 115, 60 drawn cells and ten fixed ones: after the exit the alarm stands under the mark, at or before the earliest arm of another entry when that lies after the exit, else at or before the exit plus ALARM_RETRY_MS; the first cancel call no later than the later of that arm and the exit plus ALARM_RETRY_MS", async () => {
    const drawn = fc.sample(cells, { seed: 115, numRuns: 60 });
    for (const c of [...FIXED, ...drawn]) holdsTheBound(c, await cell(c));
  });

  test("the drawn test's column with no delete: five failing runs with no mark end with alarm-stopped and no alarm (P7-111 M2)", async () => {
    const w = await world({ testClock: false });
    let due = false;
    const a = own(w, { "ledger.nextDue": () => due });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    due = true;
    await quiet(w.lines, () => a.rearm());
    const arms: (Ms | null)[] = [];
    for (let i = 0; i < 5; i += 1) arms.push((await runAt(w, a)).armed);
    w.close();
    expect(arms.slice(0, 4).every((at) => at !== null)).toBe(true);
    expect(arms[4]).toBeNull();
    expect(events(w).filter((e) => e === "alarm-stopped")).toHaveLength(1);
  });

  test("the drawn test's column under a mark set before the runs, nothing owed (the cancel throws): the pause doubles from the mark, 30 s, 60 s, 120 s, 240 s (P7-110 L3 (b))", async () => {
    const w = await world({ testClock: false });
    let due = false;
    w.host.port.cancelSubscription = async () => {
      throw new RangeError("the cancel call throws");
    };
    const a = own(w, { "ledger.nextDue": () => due });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    due = true;
    await quiet(w.lines, () => a.rearm());
    for (let i = 0; i < 4; i += 1) await runAt(w, a);
    w.at(w.now() + 1000);
    await quiet(w.lines, () => a.destroy(HASH)).catch(() => undefined);
    const gaps: Ms[] = [];
    for (let i = 0; i < 4; i += 1) {
      const run = await runAt(w, a);
      gaps.push((run.armed as Ms) - run.at);
    }
    w.close();
    expect(gaps).toEqual([1, 2, 4, 8].map((x) => x * ALARM_RETRY_MS));
  });

  test("two instances over one storage: four failing runs of A with no mark, a delete answered by B during A's send with ledger.nextDue throwing: A's exit arms no later than its exit plus ALARM_RETRY_MS", async () => {
    const w = await world({ testClock: false });
    let due = false;
    cancelPort(w, () => "failed");
    const a = own(w, { "ledger.nextDue": () => due });
    const b = own(w, { "ledger.nextDue": () => due });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    due = true;
    await quiet(w.lines, () => a.rearm());
    await failingRunsWithNoMark(w, a, 4);
    await w.use("r0", 10_000);
    const send = w.host.port.sendUsage.bind(w.host.port);
    let hooked = false;
    const exitAt = START + HOUR + 6000;
    w.host.port.sendUsage = async (args) => {
      if (!hooked) {
        hooked = true;
        w.at(START + HOUR + 2000);
        await quiet(w.lines, () => b.destroy(HASH));
        w.at(exitAt);
      }
      return send(args);
    };
    w.at(START + HOUR + 1000);
    await quiet(w.lines, () => a.alarm());
    const armed = await w.host.getAlarm();
    w.close();
    expect(hooked).toBe(true);
    expect(armed).not.toBeNull();
    expect(armed as Ms).toBeLessThanOrEqual(exitAt + ALARM_RETRY_MS);
  });
});

const r1 = { requestId: "r1", tier: "low_cost_hosted" as const, amount: 100, price: UNIT };

/** A run at the end of the first interval whose send is held while `during` runs, with the clock at `exitAt` after it. */
async function heldRun(w: World, a: ReturnType<typeof makeEntries>, during: (runAt: Ms) => Promise<void>, exitAt: (runAt: Ms) => Ms, sendThrows: boolean) {
  const send = w.host.port.sendUsage.bind(w.host.port);
  const runAt = START + HOUR + 1000;
  let hooked = false;
  w.host.port.sendUsage = async (args) => {
    if (!hooked) {
      hooked = true;
      await during(runAt);
      w.at(exitAt(runAt));
      if (sendThrows) throw new RangeError("the held send throws");
    }
    return send(args);
  };
  w.at(runAt);
  await quiet(w.lines, () => a.alarm());
  return { hooked, runAt, armed: await w.host.getAlarm() };
}

function armsOfTheInstance(w: World) {
  const arms: Ms[] = [];
  const state = { on: false };
  const setAlarm = w.host.setAlarm.bind(w.host);
  w.host.setAlarm = (at) => {
    if (state.on && at !== null) arms.push(at);
    setAlarm(at);
  };
  return { arms, state };
}

describe("the fourth closed list of P7-115 (unit 5.6 part 1): tests for the copies of my own that every test passed", () => {
  test("A3 at the run's own steps: a passing end under a delete made during the run arms no later than a hold's expiry that a reserve of the same instance armed during the run, the hold settled before the run's end", async () => {
    const w = await world({ testClock: false });
    cancelPort(w, () => "failed");
    const a = own(w);
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const recorded = armsOfTheInstance(w);
    const errorsBefore = events(w).filter((e) => e === "alarm-error").length;
    const run = await heldRun(
      w,
      a,
      async (runAt) => {
        w.at(runAt + 1000);
        recorded.state.on = true;
        await quiet(w.lines, () => a.reserve(HASH, r1));
        w.at(runAt + 1500);
        await quiet(w.lines, () => a.settle("r1", { inputTokens: 1000, outputTokens: 0 }));
        await quiet(w.lines, () => a.destroy(HASH));
        recorded.state.on = false;
      },
      (runAt) => runAt + 2000,
      false,
    );
    const marked = (await skeleton(w))?.destroying_since;
    w.close();
    expect(run.hooked).toBe(true);
    expect(marked).not.toBeNull();
    expect(events(w).filter((e) => e === "alarm-error").length).toBe(errorsBefore);
    expect(Math.min(...recorded.arms)).toBe(run.runAt + 4000);
    expect(run.armed).toBe(run.runAt + 4000);
  });

  test("A3 at the failing exit outside the mark: a run whose held send throws, with no delete, arms no later than a hold's expiry that a reserve of the same instance armed during the run", async () => {
    const w = await world({ testClock: false });
    const a = own(w);
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const run = await heldRun(
      w,
      a,
      async (runAt) => {
        w.at(runAt + 1000);
        await quiet(w.lines, () => a.reserve(HASH, r1));
      },
      (runAt) => runAt + 2000,
      true,
    );
    const row = await skeleton(w);
    w.close();
    expect(run.hooked).toBe(true);
    expect([row?.destroying_since, Number(row?.alarm_failures)]).toEqual([null, 1]);
    expect(run.armed).toBe(run.runAt + 4000);
  });

  test("T1 (d)'s term in A3's form: instance B reserves a hold and answers a delete during A's held send, which throws; A, which recorded none of B's arms, arms at the hold's expiry, 2 s after its exit, and not at its exit plus ALARM_RETRY_MS", async () => {
    const w = await world({ testClock: false });
    cancelPort(w, () => "failed");
    const a = own(w);
    const b = own(w);
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    await w.use("r0", 10_000);
    const run = await heldRun(
      w,
      a,
      async (runAt) => {
        w.at(runAt + 1000);
        await quiet(w.lines, () => b.reserve(HASH, r1));
        await quiet(w.lines, () => b.destroy(HASH));
      },
      (runAt) => runAt + 2000,
      true,
    );
    const marked = (await skeleton(w))?.destroying_since;
    w.close();
    expect(run.hooked).toBe(true);
    expect(marked).not.toBeNull();
    expect(run.armed).toBe(run.runAt + 4000);
  });

  test("A1 in destroy's own entry: six runs whose exit read throws keep a count of 6 in memory with no mark; the same instance's delete drops it, so a failing run whose exit read and count step both throw pauses ALARM_RETRY_MS", async () => {
    const w = await world({ testClock: false });
    let nextDue = false;
    cancelPort(w, () => "failed");
    const reads = exitReadThrows(w);
    const a = own(w, { "ledger.nextDue": () => nextDue });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    nextDue = true;
    await quiet(w.lines, () => a.rearm());
    await failingRunsWithNoMark(w, a, 6, () => {
      reads.on = true;
      reads.readsInTheCount = 0;
    });
    reads.on = false;
    w.at(w.now() + 1000);
    await quiet(w.lines, () => a.destroy(HASH));
    reads.upTo = 2;
    const run = await runAt(w, a, () => {
      reads.on = true;
      reads.readsInTheCount = 0;
    });
    w.close();
    expect(run.armed).toBe(Math.max(run.at + ALARM_RETRY_MS, run.next ?? 0));
  });

  test("A1: a later destroy call, which finds the mark set, resets nothing: after three failing runs under the mark (the cancel throws, nothing owed) and a second destroy call, the fourth failing run, in an instance new after the restart of the object, pauses 240 s", async () => {
    const w = await world({ testClock: false });
    let due = false;
    w.host.port.cancelSubscription = async () => {
      throw new RangeError("the cancel call throws");
    };
    const a = own(w, { "ledger.nextDue": () => due });
    await a.webhookEvent(subscriptionEvent({ eventId: "evt_sub_1", providerTime: START + 500, subscriptionId: "sub_1" }));
    w.at(START + 1000);
    due = true;
    await quiet(w.lines, () => a.destroy(HASH)).catch(() => undefined);
    for (let i = 0; i < 3; i += 1) await runAt(w, a);
    w.at(w.now() + 1000);
    await quiet(w.lines, () => a.destroy(HASH)).catch(() => undefined);
    const run = await runAt(w, own(w, { "ledger.nextDue": () => due }));
    w.close();
    expect(run.failures).toBe(4);
    expect((run.armed as Ms) - run.at).toBe(8 * ALARM_RETRY_MS);
  });
});
