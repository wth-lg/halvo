import { expect, test } from "vitest";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, DAY, HASH, quiet, START, type World, world } from "./world.ts";

const MINUTE = 60_000;
type Answer = "cancelled" | "nothing-to-cancel" | "failed";
type Script = Answer | "provider";
interface Deleting {
  w: World;
  script: Script[];
  during: (() => void | Promise<void>) | null;
  cancelTimes: Ms[];
}
async function deleting(o: Parameters<typeof world>[0] & { script?: Script[] } = {}): Promise<Deleting> {
  let ended = false;
  const d = { script: o.script ?? [], during: null, cancelTimes: [] } as unknown as Deleting;
  const cancel = async (): Promise<{ kind: Answer }> => {
    d.cancelTimes.push(d.w.now());
    const next = d.script.shift() ?? "provider";
    const answer: Answer = next === "provider" ? (ended ? "nothing-to-cancel" : "cancelled") : next;
    if (next === "provider" && !ended) {
      ended = true;
      d.w.meter.endSubscription(ACCOUNT, Math.floor(d.w.now() / 1000));
    }
    const hook = d.during;
    d.during = null;
    if (hook !== null) await hook();
    return { kind: answer };
  };
  d.w = await world({ ...o, cancel });
  return d;
}
const holds = async (w: World) => (await w.entries().testInspect()).result === "snapshot";
const skeleton = async (w: World) => (await w.tables()).skeleton?.[0] ?? null;


test("adv3: a stop after the last step committed the removal and before deleteAll ran: after the restart the settled object deletes in its next run, not at answeredAt + 24 h", async () => {
  const d = await deleting();
  const w = d.w;
  await w.use("r0", 10_000);
  await w.clock(START + MINUTE);
  const real = w.host.deleteAll;
  w.host.deleteAll = async () => {
    w.host.deleteAll = real;
    const error = new Error("the object stopped");
    error.name = "ObjectStoppedError";
    throw error;
  };
  const stopped = await quiet(w.lines, () => w.entries().destroy(HASH)).then(
    (answer) => JSON.stringify(answer),
    (error: Error) => error.name,
  );
  const afterTheStop = await skeleton(w);
  w.restart();
  await w.entries().rearm();
  const due = await w.entries().testInspect();
  console.log("ADV3 stop:", stopped, "skeleton:", JSON.stringify(afterTheStop), "due:", due.result === "snapshot" ? Number(due.due) - START : due.result);
  await w.clock(START + 2 * MINUTE);
  const twoMinutes = await holds(w);
  await w.clock(START + MINUTE + DAY - 1000);
  const justBeforeTheBound = await holds(w);
  await w.clock(START + MINUTE + DAY);
  console.log("ADV3 holds at +2 min:", twoMinutes, "at the bound less 1 s:", justBeforeTheBound, "at the bound:", await holds(w));
  expect(stopped).toBe("ObjectStoppedError");
  expect(afterTheStop).toEqual(expect.objectContaining({ cancel_answered_at: START + MINUTE }));
  expect(twoMinutes).toBe(false);
  w.close();
});
