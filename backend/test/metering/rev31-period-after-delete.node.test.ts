import { expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, DAY, HASH, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

const MINUTE = 60_000;
const DELETE_AT = START + MINUTE;

test("REVIEW period: an answer stored for no id, an id that arrives during the call, a recurring error and one failed cancel: no period begins after the delete", async () => {
  let w: World;
  const script = ["nothing-to-cancel", "failed", "cancelled"] as const;
  let n = 0;
  const cancelTimes: Ms[] = [];
  let during: (() => Promise<void>) | null = null;
  const cancel = async () => {
    cancelTimes.push(w.now());
    const kind = script[Math.min(n, script.length - 1)] as (typeof script)[number];
    n += 1;
    if (kind === "cancelled") w.meter.endSubscription(ACCOUNT, Math.floor(w.now() / 1000));
    const hook = during;
    during = null;
    if (hook !== null) await hook();
    return { kind };
  };
  w = await world({ testClock: false, cancel, subscribed: false });
  w.meter.subscribe(ACCOUNT, Math.floor(DELETE_AT / 1000) - 30 * 86_400 + 7200);
  let on = false;
  const accounts = makeAccounts();
  const forget = accounts.forget;
  const own = makeEntries(w.host, {
    accounts: {
      ...accounts,
      forget: (...a: Parameters<typeof forget>) => {
        if (on) throw new RangeError("accounts.forget throws");
        return forget(...a);
      },
    },
    ledger: makeLedger({ holdLifeMs: w.host.config.holdLifeMs }),
    metering: makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds }),
  });
  w.at(START + 1000);
  await w.use("r0", 10_000);
  w.at(DELETE_AT);
  during = async () => {
    await own.webhookEvent(subscriptionEvent({ eventId: "evt_sub_during", providerTime: DELETE_AT + 500 }));
  };
  const answer = await quiet(w.lines, () => own.destroy(HASH)).then((a) => a.result, (e: Error) => `threw ${e.name}`);
  const stored = (await w.tables()).skeleton?.[0];
  on = true;
  let failedRunAt: Ms | null = null;
  let armedAfter: Ms | null = null;
  for (let i = 0; i < 400; i += 1) {
    if ((await w.tables()).skeleton?.[0] === undefined) break;
    const due = await w.host.getAlarm();
    if (due === null || due > DELETE_AT + 3 * DAY) break;
    w.at(Math.max(due, w.now()));
    const before = cancelTimes.length;
    await quiet(w.lines, () => own.alarm());
    if (failedRunAt === null && cancelTimes.length > before && n === 2) {
      failedRunAt = w.now();
      armedAfter = await w.host.getAlarm();
    }
  }
  const periods = w.meter.periods(ACCOUNT, Math.floor((DELETE_AT + 3 * DAY) / 1000)).filter((p) => p.start * 1000 > DELETE_AT);
  process.stdout.write(`REVIEW destroy=${answer} storedFor=${String(stored?.cancel_answered_for)} cancelsAfterDelete=${JSON.stringify(cancelTimes.map((t) => t - DELETE_AT))} periodsBegunAfterDelete=${JSON.stringify(periods.map((p) => p.start * 1000 - DELETE_AT))} gone=${(await w.tables()).skeleton === undefined} armedAfterTheFailedTry=${String(armedAfter)}\n`);
  w.close();
  expect(periods).toEqual([]);
});
