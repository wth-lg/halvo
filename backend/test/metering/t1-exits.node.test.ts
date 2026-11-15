import { describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { ALARM_RETRY_MS, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms, Sql, Step } from "../../src/types.ts";
import { ACCOUNT, DAY, HASH, quiet, START, subscriptionEvent, world } from "./world.ts";

const SECOND = 1000;
const ANSWER_TO_GONE: Ms = 86_460_000;

interface Faults {
  heldCall: "send" | "read";
  sendThrows?: boolean;
  expireThrows?: boolean;
  recordThrowsOnce?: boolean;
  nextDueThrowsInTheDueTime?: boolean;
  countWriteThrowsOnce?: boolean;
  exitReadThrows?: boolean;
  exitAfterSeconds?: number;
}

interface Exit {
  answer: string;
  armedByTheDelete: Ms | null;
  exitAt: Ms;
  armedAfterTheExit: Ms | null;
  earliestDeleteArm: Ms | null;
  cancels: Ms[];
  answeredAt: Ms | null;
  goneAt: Ms | null;
  periodsAfterTheDelete: number;
  stoppedUnderTheMark: number;
  nullArmsUnderTheMark: number;
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

async function exitWithTheDeleteInTheRun(f: Faults): Promise<Exit> {
  let ended = false;
  let endNow: () => void = () => {};
  const w = await world({
    testClock: false,
    cancel: async () => {
      if (ended) return { kind: "nothing-to-cancel" as const };
      ended = true;
      endNow();
      return { kind: "cancelled" as const };
    },
  });
  endNow = () => w.meter.endSubscription(ACCOUNT, Math.floor(w.now() / 1000));
  let on = false;
  let recordThrew = false;
  let countThrew = false;
  const accounts = makeAccounts();
  const ledger = makeLedger({ holdLifeMs: w.host.config.holdLifeMs });
  const metering = makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds });
  const own = makeEntries(w.host, {
    accounts,
    ledger: {
      ...ledger,
      expire: (...a: Parameters<typeof ledger.expire>) => {
        if (on && f.expireThrows) throw new RangeError("ledger.expire throws");
        return ledger.expire(...a);
      },
      nextDue: (...a: Parameters<typeof ledger.nextDue>) => {
        if (on && f.nextDueThrowsInTheDueTime && String(new Error().stack).includes("dueOf")) throw new RangeError("ledger.nextDue throws");
        return ledger.nextDue(...a);
      },
    },
    metering: {
      ...metering,
      record: (...a: Parameters<typeof metering.record>) => {
        if (on && f.recordThrowsOnce && !recordThrew) {
          recordThrew = true;
          throw new RangeError("metering.record throws once");
        }
        return metering.record(...a);
      },
    },
  });
  const inner = w.host.step;
  w.host.step = ((run) =>
    inner((sql) =>
      run({
        exec: (query: string, ...bindings: (string | number | null)[]) => {
          const inCount = String(new Error().stack).includes("countFailure");
          if (on && f.countWriteThrowsOnce && !countThrew && inCount && query.startsWith("UPDATE skeleton SET alarm_failures = ?")) {
            countThrew = true;
            throw new RangeError("the count's write throws once");
          }
          if (on && f.exitReadThrows && inCount && query.startsWith("SELECT destroying_since")) throw new RangeError("the mark's read at the exit throws");
          return sql.exec(query, ...bindings);
        },
      } as Sql),
    )) as Step;
  const markSet = () =>
    inner((sql) => sql.exec("SELECT name FROM sqlite_master WHERE type = ? AND name = ?", "table", "skeleton").length > 0 && sql.exec("SELECT destroying_since FROM skeleton")[0]?.destroying_since !== null);
  let nullArmsUnderTheMark = 0;
  let inDestroy = false;
  const deleteArms: Ms[] = [];
  const setAlarm = w.host.setAlarm.bind(w.host);
  w.host.setAlarm = (at) => {
    if (at === null && markSet()) nullArmsUnderTheMark += 1;
    if (inDestroy && at !== null) deleteArms.push(at);
    setAlarm(at);
  };
  const cancels: Ms[] = [];
  let answeredAt: Ms | null = null;
  const cancel = w.host.port.cancelSubscription;
  w.host.port.cancelSubscription = async (id) => {
    cancels.push(w.now());
    const answer = await cancel(id);
    if (answer.kind !== "failed" && answeredAt === null) answeredAt = w.now();
    return answer;
  };
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let entered = false;
  const holdOnce = (call: (...a: never[]) => Promise<unknown>) => {
    let first = true;
    return async (...a: never[]) => {
      if (first) {
        first = false;
        entered = true;
        await held;
        if (f.sendThrows) throw new RangeError("the port call throws");
      }
      return call(...a);
    };
  };
  const send = w.host.port.sendUsage;
  if (f.heldCall === "send") w.host.port.sendUsage = holdOnce(send as never) as typeof w.host.port.sendUsage;
  else {
    w.host.port.sendUsage = async () => ({ kind: "failed" });
    w.host.port.readUsageTotal = holdOnce(w.host.port.readUsageTotal as never) as typeof w.host.port.readUsageTotal;
  }
  await quiet(w.lines, () => own.webhookEvent(subscriptionEvent({ eventId: "evt_sub", providerTime: START })));
  w.at(START + 10 * SECOND);
  await quiet(w.lines, () => w.use("r0", 25_000));
  let run: Promise<void> | null = null;
  for (let n = 0; n < 500 && run === null; n += 1) {
    const due = await w.host.getAlarm();
    if (due === null) break;
    w.at(Math.max(due, w.now()));
    const running = quiet(w.lines, () => own.alarm());
    await tick();
    await tick();
    if (entered) run = running;
    else await running;
  }
  if (run === null) throw new Error("no alarm run made the held call");
  if (f.heldCall === "read") w.host.port.sendUsage = send;
  const runStart = w.now();
  const t0 = runStart + SECOND;
  w.at(t0);
  on = true;
  inDestroy = true;
  const answer = await quiet(w.lines, () => own.destroy(HASH)).then(
    (a) => a.result,
    (e: Error) => `threw ${e.name}`,
  );
  inDestroy = false;
  const armedByTheDelete = await w.host.getAlarm();
  const exitAt = t0 + (f.exitAfterSeconds ?? 5) * SECOND;
  w.at(exitAt);
  release();
  await run;
  const armedAfterTheExit = await w.host.getAlarm();
  let goneAt: Ms | null = null;
  for (let n = 0; n < 5000; n += 1) {
    if ((await w.tables()).skeleton === undefined) {
      goneAt = goneAt ?? w.now();
      break;
    }
    const due = await w.host.getAlarm();
    if (due === null || due > t0 + 2 * DAY) break;
    w.at(Math.max(due, w.now()));
    await quiet(w.lines, () => own.alarm());
  }
  if (goneAt === null && (await w.tables()).skeleton === undefined) goneAt = w.now();
  const stoppedUnderTheMark = w.lines.filter((line) => JSON.parse(line).event === "alarm-stopped").length;
  const periodsAfterTheDelete = w.meter.periods(ACCOUNT, Math.floor((t0 + 90 * DAY) / 1000)).filter((period) => period.start * 1000 > t0).length;
  w.close();
  const earliestDeleteArm = deleteArms.length === 0 ? null : Math.min(...deleteArms);
  return { answer, armedByTheDelete, exitAt, armedAfterTheExit, earliestDeleteArm, cancels, answeredAt, goneAt, periodsAfterTheDelete, stoppedUnderTheMark, nullArmsUnderTheMark };
}

function holdsTheRule(e: Exit): void {
  expect(e.answer).toBe("not-finished");
  expect(e.armedByTheDelete).not.toBeNull();
  expect(e.stoppedUnderTheMark).toBe(0);
  expect(e.nullArmsUnderTheMark).toBe(0);
  expect(e.armedAfterTheExit).not.toBeNull();
  expect(e.armedAfterTheExit as Ms).toBeGreaterThanOrEqual(e.earliestDeleteArm !== null && e.earliestDeleteArm > e.exitAt ? Math.min(e.earliestDeleteArm, e.exitAt + ALARM_RETRY_MS) : e.exitAt + ALARM_RETRY_MS);
  expect(e.armedAfterTheExit as Ms).toBeLessThanOrEqual(Math.max(e.armedByTheDelete as Ms, e.exitAt + ALARM_RETRY_MS));
  expect(e.cancels.length).toBeGreaterThan(0);
  expect(e.cancels[0] as Ms).toBeLessThanOrEqual((e.armedByTheDelete as Ms) + ALARM_RETRY_MS);
  expect(e.periodsAfterTheDelete).toBe(0);
  expect(e.answeredAt).not.toBeNull();
  expect(e.goneAt).not.toBeNull();
  expect((e.goneAt as Ms) - (e.answeredAt as Ms)).toBeLessThanOrEqual(ANSWER_TO_GONE);
}

describe("an alarm run that began before the delete exits after the mark is set: the alarm is never left unarmed under the mark (P7-113 T1)", () => {
  test("the verifier's form: ledger.expire throws from the delete on, and the count's write throws once", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", expireThrows: true, countWriteThrowsOnce: true }));
  });

  test("the verifier's form: metering.record throws once after the delete began, and the count's write throws once", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", recordThrowsOnce: true, countWriteThrowsOnce: true }));
  });

  test("the verifier's form with the exit 15 s after the delete, so that the floor of now plus ALARM_RETRY_MS lies after the delete's own arm: ledger.expire throws, and the count's write throws once", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", expireThrows: true, countWriteThrowsOnce: true, exitAfterSeconds: 15 }));
  });

  test("the exit by a send that throws, the count's write throwing once", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", sendThrows: true, countWriteThrowsOnce: true }));
  });

  test("the exit by a send that throws, the count's write working", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", sendThrows: true }));
  });

  test("the exit by a due-time error caught in the run, the count's write throwing once", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", nextDueThrowsInTheDueTime: true, countWriteThrowsOnce: true }));
  });

  test("the passing exit: the send returns and the run ends with no error", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send" }));
  });

  test("the exit whose read of the mark throws, with the send throwing", async () => {
    holdsTheRule(await exitWithTheDeleteInTheRun({ heldCall: "send", sendThrows: true, exitReadThrows: true }));
  });

  test("the removal-owed form: a usage read held open, destroy answered during it, the record step and the count's write each throwing once", async () => {
    const e = await exitWithTheDeleteInTheRun({ heldCall: "read", recordThrowsOnce: true, countWriteThrowsOnce: true });
    expect(e.stoppedUnderTheMark).toBe(0);
    expect(e.nullArmsUnderTheMark).toBe(0);
    expect(e.armedAfterTheExit).not.toBeNull();
    expect(e.answeredAt).not.toBeNull();
    expect(e.goneAt).not.toBeNull();
    expect((e.goneAt as Ms) - (e.answeredAt as Ms)).toBeLessThanOrEqual(ANSWER_TO_GONE);
  });
});
