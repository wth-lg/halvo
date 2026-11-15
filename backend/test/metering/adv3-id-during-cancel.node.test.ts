import { expect, test } from "vitest";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, HASH, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

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

test("adv3: a subscription id that arrives during a cancel call made for no id, with the metering settled: the storage stays until the cancel for that id is answered, and that id is cancelled", async () => {
  const d = await deleting({ script: ["nothing-to-cancel"] });
  const w = d.w;
  await w.use("r0", 10_000);
  await w.clock(START + MINUTE);
  d.during = async () => {
    await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub_during", providerTime: START + MINUTE + 500 }));
  };
  const answer = await quiet(w.lines, () => w.entries().destroy(HASH));
  const afterTheCall = { answer, holds: await holds(w), stored: (await skeleton(w))?.cancel_answered_for ?? "no row", status: await w.entries().status(HASH) };
  console.log("ADV3 after the call:", JSON.stringify(afterTheCall));
  await w.clock(START + MINUTE + 1000);
  const cancels = w.calls.filter((each) => each.kind === "cancel").map((each) => [each.subscriptionId, each.answer]);
  console.log("ADV3 cancel calls:", JSON.stringify(cancels), "holds:", await holds(w));
  expect(afterTheCall.holds).toBe(true);
  expect(cancels).toEqual([
    [null, "nothing-to-cancel"],
    ["sub_1", "cancelled"],
  ]);
  expect(await holds(w)).toBe(false);
  w.close();
});
