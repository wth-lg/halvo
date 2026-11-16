import { describe, expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { DELETE_REMOVAL_BOUND_MS, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering, SYNC_RETRY_MS } from "../../src/metering/index.ts";
import type { Ms, Sql, Step } from "../../src/types.ts";
import type { CancelFault } from "../../test-kit/fake-meter/port.ts";
import { ACCOUNT, DAY, HASH, quiet, START, world } from "./world.ts";

const MINUTE = 60_000;
const DELETE_AT = START + 10 * MINUTE;

type Failing =
  | "ledger.expire"
  | "metering.seal"
  | "accounts.forget"
  | "ledger.nextDue"
  | "accounts.nextDue"
  | "metering.nextSend"
  | "metering.oldestUnsealed"
  | "ledger.intervalTotals"
  | "due:accounts.subscription"
  | "metering.settled"
  | "metering.closeOpen"
  | "metering.cut"
  | "the mark's read and the count step";

interface Ending {
  secondDelete: string;
  answers: string[];
  answeredAt: Ms | null;
  goneAt: Ms | null;
  periodsAfterTheAnswer: number;
  plans: string[];
  stopped: number;
}

function throwing(on: () => boolean, name: string, real: (...a: never[]) => unknown, only?: string) {
  return (...a: never[]) => {
    if (on() && (only === undefined || String(new Error().stack).includes(only))) throw new RangeError(`${name} throws`);
    return real(...a);
  };
}

async function ninetyDays(failing: Failing, sends: "answered" | "lost", o: { rejectTheRemovalOnce?: boolean } = {}): Promise<Ending> {
  const lost: CancelFault[] = [{ kind: "list-no-answer" }, { kind: "list-no-answer" }, { kind: "list-no-answer" }];
  const w = await world({ testClock: false, listed: { plans: [{ id: "sub_2" }], faults: lost }, ...(sends === "lost" ? { always: { kind: "request-lost" as const } } : {}) });
  const answers: string[] = [];
  let answeredAt: Ms | null = null;
  const port = w.host.port.cancelSubscription.bind(w.host.port);
  w.host.port.cancelSubscription = async (id, customerId) => {
    const answer = await port(id, customerId);
    answers.push(answer.kind);
    if (answer.kind !== "failed") answeredAt = answeredAt ?? w.now();
    return answer;
  };
  let on = false;
  const isOn = () => on;
  const accounts = makeAccounts();
  const ledger = makeLedger({ holdLifeMs: w.host.config.holdLifeMs });
  const metering = makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds });
  const dueAlone = failing.startsWith("due:");
  const [module, fn] = failing.replace("due:", "").split(".") as [string, string];
  const wrap = <T extends object>(name: string, target: T): T =>
    module === name && fn in target
      ? { ...target, [fn]: throwing(isOn, failing, (target as Record<string, (...a: never[]) => unknown>)[fn] as (...a: never[]) => unknown, dueAlone ? "dueOf" : undefined) }
      : target;
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
            if (on && inAlarm && query.startsWith("UPDATE skeleton SET alarm_failures = ?")) throw new RangeError("the count step throws");
            return sql.exec(query, ...(bindings as (string | number | null)[]));
          },
        } as Sql),
      );
    }) as Step;
  }
  if (o.rejectTheRemovalOnce === true) {
    const deleteAll = w.host.deleteAll;
    let rejected = false;
    w.host.deleteAll = async () => {
      if (!rejected) {
        rejected = true;
        throw new RangeError("the removal is rejected");
      }
      return deleteAll();
    };
  }
  w.at(START + 1000);
  await w.use("r0", 10_000);
  w.at(DELETE_AT);
  on = true;
  const answer = (run: () => Promise<{ result: string }>) => quiet(w.lines, run).then((a) => a.result, (e: Error) => `threw ${e.name}`);
  await answer(() => own.destroy(HASH));
  const secondDelete = await answer(() => own.destroy(HASH));
  let goneAt: Ms | null = null;
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
  }
  if (goneAt === null && (await w.tables()).skeleton === undefined) goneAt = w.now();
  const answered = answeredAt as Ms | null;
  const periodsAfterTheAnswer = answered === null ? -1 : w.meter.periods(ACCOUNT, Math.floor(until / 1000)).filter((period) => period.start * 1000 > answered).length;
  const stopped = w.lines.filter((line) => JSON.parse(line).event === "alarm-stopped").length;
  const plans = w.meter.plans(ACCOUNT).map((plan) => `${plan.id} ${plan.status}`);
  w.close();
  return { secondDelete, answers, answeredAt: answered, goneAt, periodsAfterTheAnswer, plans, stopped };
}

function holdsTheRule(ending: Ending): void {
  expect(ending.secondDelete.startsWith("threw")).toBe(false);
  expect(ending.answers.slice(0, 3)).toEqual(["failed", "failed", "failed"]);
  expect(ending.answeredAt).not.toBeNull();
  expect(ending.plans).toEqual(["sub_1 canceled", "sub_2 canceled"]);
  expect(ending.periodsAfterTheAnswer).toBe(0);
  expect(ending.stopped).toBe(0);
  expect(ending.goneAt).not.toBeNull();
  expect((ending.goneAt ?? Number.POSITIVE_INFINITY) - (ending.answeredAt ?? 0)).toBeLessThanOrEqual(DELETE_REMOVAL_BOUND_MS + SYNC_RETRY_MS);
}

describe("90 days with no caller after the delete, over the customer's list (unit 5.6 part 2): no stored id, a second subscription on the customer and a list with no answer at the first three tries; the object ends both plans by itself, no period begins after its answer, and the storage goes by the bound", () => {
  for (const failing of ["ledger.nextDue", "accounts.nextDue", "metering.nextSend", "metering.oldestUnsealed", "ledger.intervalTotals", "due:accounts.subscription"] as const) {
    test(`a due-time read, ${failing}, throws on every call from the delete on; the person's second delete answers`, async () => {
      holdsTheRule(await ninetyDays(failing, "answered"));
    });
  }

  for (const failing of ["ledger.expire", "metering.seal", "accounts.forget"] as const) {
    test(`a call of the alarm's first step, ${failing}, throws on every call from the delete on; the person's second delete answers`, async () => {
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

  test("accounts.forget throws on every call, every send lost, and the removal at the bound is rejected once: the storage goes within SYNC_RETRY_MS after it", async () => {
    holdsTheRule(await ninetyDays("accounts.forget", "lost", { rejectTheRemovalOnce: true }));
  });

  test("the mark's read and the count step throw in every alarm run", async () => {
    holdsTheRule(await ninetyDays("the mark's read and the count step", "answered"));
  });
});
