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
const skeleton = async (w: World) => (await w.tables()).skeleton?.[0] ?? null;


test("adv3: failed cancels grow the next try, an outside destroy stores the answer for no id before that next try, then the id arrives: the cancel for the id is made at the next run, not at the stale next try", async () => {
  const d = await deleting({ always: { kind: "request-lost" }, script: ["failed", "failed", "failed", "nothing-to-cancel"] });
  const w = d.w;
  await w.use("r0", 10_000);
  const t0 = START + MINUTE;
  await w.clock(t0);
  await quiet(w.lines, () => w.entries().destroy(HASH));
  await w.clock(t0 + 60_000);
  await w.clock(t0 + 180_000);
  const beforeTheAnswer = await skeleton(w);
  await w.clock(t0 + 200_000);
  await quiet(w.lines, () => w.entries().destroy(HASH));
  const afterTheAnswer = await skeleton(w);
  await w.clock(t0 + 210_000);
  await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_sub_late", providerTime: t0 + 210_000 }));
  await w.clock(t0 + 220_000);
  const byTwoTwenty = w.calls.filter((each) => each.kind === "cancel").map((each) => [each.subscriptionId, each.answer]);
  console.log("ADV3 before the answer:", JSON.stringify(beforeTheAnswer));
  console.log("ADV3 after the answer:", JSON.stringify(afterTheAnswer));
  console.log("ADV3 cancel calls by t0+220 s:", JSON.stringify(byTwoTwenty), "times:", JSON.stringify(d.cancelTimes.map((at) => (at - t0) / 1000)));
  expect(byTwoTwenty).toEqual([
    [null, "failed"],
    [null, "failed"],
    [null, "failed"],
    [null, "nothing-to-cancel"],
    ["sub_1", "cancelled"],
  ]);
  w.close();
});
