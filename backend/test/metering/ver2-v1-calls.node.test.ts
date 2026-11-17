// Verifier-interrogate-2, row V1: one of the three calls of the alarm's first step throws on every call from the delete on.
// Built on adversary 2's case C (the real modules, one function wrapped, entries made on the world's host) with the
// real-clock host: the alarm runs at the time that the host holds, and nobody calls an entry unless the case says so.
import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, DAY, HASH, quiet, START, type World, world } from "./world.ts";

const out = (o: unknown) => appendFileSync(process.env.VER_OUT ?? "/dev/null", `${JSON.stringify(o)}\n`);
const MINUTE = 60_000;
type Call = "ledger.expire" | "metering.seal" | "accounts.forget";
type Ending = "owed" | "answered";

async function setup(call: Call, ending: Ending) {
  const cancelAt: Ms[] = [];
  let w: World;
  let ended = false;
  const cancel = async () => {
    cancelAt.push(w.now());
    if (ended) return { kind: "nothing-to-cancel" as const };
    ended = true;
    w.meter.endSubscription(ACCOUNT, Math.floor(w.now() / 1000));
    return { kind: "cancelled" as const };
  };
  // "answered": every send is lost, so the metering never settles and only the 24-hour bound removes the storage.
  w = await world({ testClock: false, cancel, ...(ending === "answered" ? { always: { kind: "request-lost" as const } } : {}) });
  let on = false;
  const thrown: Ms[] = [];
  const fail = () => {
    thrown.push(w.now());
    const e = new RangeError(`${call} throws`);
    throw e;
  };
  const accounts = makeAccounts();
  const ledger = makeLedger({ holdLifeMs: w.host.config.holdLifeMs });
  const metering = makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds });
  const mods = {
    accounts: call === "accounts.forget" ? { ...accounts, forget: (...a: Parameters<typeof accounts.forget>) => (on ? fail() : accounts.forget(...a)) } : accounts,
    ledger: call === "ledger.expire" ? { ...ledger, expire: (...a: Parameters<typeof ledger.expire>) => (on ? fail() : ledger.expire(...a)) } : ledger,
    metering: call === "metering.seal" ? { ...metering, seal: (...a: Parameters<typeof metering.seal>) => (on ? fail() : metering.seal(...a)) } : metering,
  };
  const own = makeEntries(w.host, mods as Parameters<typeof makeEntries>[1]);
  return { w, own, cancelAt, thrown, turnOn: () => (on = true) };
}

for (const call of ["ledger.expire", "metering.seal", "accounts.forget"] as const) {
  for (const ending of ["owed", "answered"] as const) {
    test(`V1 ${call} ${ending}: the call throws on every call from the delete on; 90 days of alarm runs with no caller, then one destroy from outside`, async () => {
      const { w, own, cancelAt, thrown, turnOn } = await setup(call, ending);
      w.at(START + 1000);
      await w.use("r0", 10_000);
      w.at(START + 10 * MINUTE);
      if (ending === "owed") await w.reserve("r1"); // a request in flight at the delete: destroy makes no cancel
      const mark = w.now();
      turnOn();
      const destroyAnswer = await quiet(w.lines, () => own.destroy(HASH)).then((a) => a.result as string, (e: Error) => `threw ${e.name}`);
      const atDestroy = { answer: destroyAnswer, cancels: cancelAt.length, alarm: await w.host.getAlarm() };
      const runs: { dt: number; cancels: number; events: string[] }[] = [];
      const until = mark + 90 * DAY;
      for (let i = 0; i < 400; i += 1) {
        const due = await w.host.getAlarm();
        if (due === null || due > until) break;
        w.at(Math.max(due, w.now()));
        const before = cancelAt.length;
        const lines: string[] = [];
        await quiet(lines, () => own.alarm());
        runs.push({ dt: (w.now() - mark) / 1000, cancels: cancelAt.length - before, events: lines.map((l) => JSON.parse(l).event) });
      }
      w.at(until);
      const sk = (await w.tables()).skeleton?.[0] ?? null;
      const atDay90 = {
        alarm: await w.host.getAlarm(),
        holds: sk !== null,
        cancelsInAlarmRuns: runs.reduce((n, r) => n + r.cancels, 0),
        stopped: runs.filter((r) => r.events.includes("alarm-stopped")).length,
        periodsBegunAfterMark: w.meter.periods(ACCOUNT, Math.floor(until / 1000)).filter((p) => p.start * 1000 > mark).map((p) => Math.round((p.start * 1000 - mark) / DAY)),
        skeleton: sk,
      };
      const status = await quiet(w.lines, () => own.status(HASH)).then((a) => a.result as string, (e: Error) => `threw ${e.name}`);
      // what ends the state: the person deletes again from the app (one call of destroy from outside), the error still there
      const before = cancelAt.length;
      const again = await quiet(w.lines, () => own.destroy(HASH)).then((a) => a.result as string, (e: Error) => `threw ${e.name}`);
      const afterAgain = { answer: again, cancels: cancelAt.length - before, holds: (await w.tables()).skeleton !== undefined };
      out({ case: `V1 ${call} ${ending}`, atDestroy, runs: runs.length, runsDetail: runs.slice(0, 6), thrownCount: thrown.length, atDay90, statusAtDay90: status, destroyAtDay90: afterAgain });
      // what contract 3.4 asks: the owed cancel is made by the alarm also after an error of the seal and send part, and the
      // storage goes at the latest 24 hours after the answered cancel
      expect({ cancelled: cancelAt.length > 0, holdsAtDay90: atDay90.holds }).toEqual({ cancelled: true, holdsAtDay90: false });
      w.close();
    });
  }
}
