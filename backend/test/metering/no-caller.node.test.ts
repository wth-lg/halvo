import { describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { ALARM_PAUSE_CAP_MS, DELETE_REMOVAL_BOUND_MS, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms, Sql, Step } from "../../src/types.ts";
import { ACCOUNT, DAY, HASH, quiet, START, type World, world } from "./world.ts";

const MINUTE = 60_000;
const DELETE_AT = START + 10 * MINUTE;

type Failing =
  | "ledger.nextDue"
  | "accounts.nextDue"
  | "metering.nextSend"
  | "metering.oldestUnsealed"
  | "ledger.intervalTotals"
  | "ledger.openHolds"
  | "metering.settled"
  | "metering.closeOpen"
  | "metering.cut"
  | "the mark's read and the count step";

interface Ending {
  secondDelete: string;
  cancels: Ms[];
  answeredAt: Ms | null;
  goneAt: Ms | null;
  periodsAfterTheAnswer: number;
  stopped: number;
  runs: number;
}

function throwing(on: () => boolean, name: string, real: (...a: never[]) => unknown) {
  return (...a: never[]) => {
    if (on()) throw new RangeError(`${name} throws`);
    return real(...a);
  };
}

async function ninetyDays(failing: Failing, sends: "answered" | "lost"): Promise<Ending> {
  const cancels: Ms[] = [];
  let w: World;
  let ended = false;
  let answeredAt: Ms | null = null;
  const cancel = async () => {
    cancels.push(w.now());
    if (cancels.length <= 3) return { kind: "failed" as const };
    answeredAt = answeredAt ?? w.now();
    if (ended) return { kind: "nothing-to-cancel" as const };
    ended = true;
    w.meter.endSubscription(ACCOUNT, Math.floor(w.now() / 1000));
    return { kind: "cancelled" as const };
  };
  w = await world({ testClock: false, cancel, ...(sends === "lost" ? { always: { kind: "request-lost" as const } } : {}) });
  let on = false;
  const isOn = () => on;
  const accounts = makeAccounts();
  const ledger = makeLedger({ holdLifeMs: w.host.config.holdLifeMs });
  const metering = makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds });
  const [module, fn] = failing.split(".") as [string, string];
  const wrap = <T extends object>(name: string, target: T): T =>
    module === name && fn in target ? { ...target, [fn]: throwing(isOn, failing, (target as Record<string, (...a: never[]) => unknown>)[fn] as (...a: never[]) => unknown) } : target;
  const own = makeEntries(w.host, { accounts: wrap("accounts", accounts), ledger: wrap("ledger", ledger), metering: wrap("metering", metering) });
  let inAlarm = false;
  let alarmSteps = 0;
  if (failing === "the mark's read and the count step") {
    const inner = w.host.step;
    w.host.step = (<T>(run: (sql: Sql) => T): T => {
      if (on && inAlarm && alarmSteps++ === 0) throw new RangeError("the mark's read throws");
      return inner((sql) =>
        run({
          exec: (query: string, ...bindings: unknown[]) => {
            if (on && inAlarm && query.startsWith("UPDATE skeleton SET last_now = ?, alarm_failures = ?")) throw new RangeError("the count step throws");
            return sql.exec(query, ...(bindings as (string | number | null)[]));
          },
        } as Sql),
      );
    }) as Step;
  }
  w.at(START + 1000);
  await w.use("r0", 10_000);
  w.at(DELETE_AT);
  on = true;
  const answer = (run: () => Promise<{ result: string }>) => quiet(w.lines, run).then((a) => a.result, (e: Error) => `threw ${e.name}`);
  await answer(() => own.destroy(HASH));
  const secondDelete = await answer(() => own.destroy(HASH));
  let goneAt: Ms | null = null;
  let runs = 0;
  const until = DELETE_AT + 90 * DAY;
  for (let i = 0; i < 2000; i += 1) {
    const read = (await w.tables()).skeleton?.[0];
    if (read === undefined) {
      goneAt = goneAt ?? w.now();
      break;
    }
    const due = await w.host.getAlarm();
    if (due === null || due > until) break;
    w.at(Math.max(due, w.now()));
    inAlarm = true;
    alarmSteps = 0;
    await quiet(w.lines, () => own.alarm());
    inAlarm = false;
    runs += 1;
  }
  if (goneAt === null && (await w.tables()).skeleton === undefined) goneAt = w.now();
  const periodsAfterTheAnswer =
    answeredAt === null ? -1 : w.meter.periods(ACCOUNT, Math.floor(until / 1000)).filter((period) => period.start * 1000 > (answeredAt as Ms)).length;
  const stopped = w.lines.filter((line) => JSON.parse(line).event === "alarm-stopped").length;
  w.close();
  return { secondDelete, cancels, answeredAt, goneAt, periodsAfterTheAnswer, stopped, runs };
}

function holdsTheRule(ending: Ending): void {
  expect(ending.secondDelete.startsWith("threw")).toBe(false);
  expect(ending.answeredAt).not.toBeNull();
  expect(ending.periodsAfterTheAnswer).toBe(0);
  expect(ending.stopped).toBe(0);
  expect(ending.goneAt).not.toBeNull();
  expect((ending.goneAt ?? Number.POSITIVE_INFINITY) - (ending.answeredAt ?? 0)).toBeLessThanOrEqual(DELETE_REMOVAL_BOUND_MS + ALARM_PAUSE_CAP_MS);
}

describe("90 days with no caller after the delete: the owed cancel is made by the object, no period begins after its answer, and the storage goes by the bound (P7-110 L1 to L3)", () => {
  for (const failing of ["ledger.nextDue", "accounts.nextDue", "metering.nextSend", "metering.oldestUnsealed", "ledger.intervalTotals", "ledger.openHolds"] as const) {
    test(`a due-time read, ${failing}, throws on every call from the delete on; the person's second delete answers`, async () => {
      holdsTheRule(await ninetyDays(failing, "answered"));
    });
  }

  test("metering.settled throws on every call from the delete on, every send lost: the answer is stored and the storage goes at the bound", async () => {
    holdsTheRule(await ninetyDays("metering.settled", "lost"));
  });

  test("metering.closeOpen throws at the bound, every send lost: the storage goes all the same", async () => {
    holdsTheRule(await ninetyDays("metering.closeOpen", "lost"));
  });

  test("metering.cut throws at the delete and in every run after it", async () => {
    holdsTheRule(await ninetyDays("metering.cut", "answered"));
  });

  test("the mark's read and the count step throw in every alarm run", async () => {
    holdsTheRule(await ninetyDays("the mark's read and the count step", "answered"));
  });
});
