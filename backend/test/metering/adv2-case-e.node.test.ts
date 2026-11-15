import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, HASH, quiet, START, type World, world } from "./world.ts";

const out = (line: string) => appendFileSync(process.env.ADV2_OUT ?? "/dev/null", `${line}\n`);
const MINUTE = 60_000;
type Answer = "cancelled" | "nothing-to-cancel" | "failed";

async function setup(o: { script: Answer[]; invoiceDays?: number }) {
  const cancelTimes: Ms[] = [];
  let w: World;
  const script = o.script.slice();
  let ended = false;
  const cancel = async (): Promise<{ kind: Answer }> => {
    cancelTimes.push(w.now());
    const next = script.shift() ?? "cancelled";
    if (next !== "failed" && !ended) {
      ended = true;
      w.meter.endSubscription(ACCOUNT, Math.floor(w.now() / 1000));
    }
    return { kind: next };
  };
  w = await world({ testClock: false, cancel, meter: { invoicePeriodSeconds: (o.invoiceDays ?? 30) * 86_400 } });
  return { w, cancelTimes };
}

async function markWithUsage(w: World): Promise<{ mark: Ms; thrown: string }> {
  w.at(START + 1000);
  await w.use("r0", 10_000);
  w.at(START + 10 * MINUTE);
  const mark = w.now();
  const thrown = await quiet(w.lines, () => w.entries().destroy(HASH)).then(
    (a) => JSON.stringify(a),
    (e: Error) => e.name,
  );
  return { mark, thrown };
}

test("ADV2-E: a stop that lasts the rest of one alarm run (every later step of that run throws, as after an object reset), the provider's cancel failing: runs until the alarm is deleted", async () => {
  const { w, cancelTimes } = await setup({ script: Array(500).fill("failed") });
  const { mark } = await markWithUsage(w);
  const inner = w.host.step;
  let dead = false;
  w.host.step = ((run: Parameters<typeof inner>[0]) => {
    if (dead) {
      const e = new Error("the object stopped");
      e.name = "ObjectStoppedError";
      throw e;
    }
    return inner(run);
  }) as typeof inner;
  const atDestroy = cancelTimes.length;
  const due = await w.host.getAlarm();
  w.at(due ?? mark);
  dead = true;
  await quiet(w.lines, () => w.entries().alarm());
  dead = false;
  const events = w.lines.map((l) => JSON.parse(l).event);
  out(JSON.stringify({ case: "E", runs: 1, alarmAfter: await w.host.getAlarm(), cancelsInRun: cancelTimes.length - atDestroy, events }));
  expect(await w.host.getAlarm()).not.toBeNull();
  w.close();
});
