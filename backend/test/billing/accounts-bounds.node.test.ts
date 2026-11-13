import { afterEach, expect, test } from "vitest";
import { EVENT_ID_KEEP_MS, PENDING_DROP_MS, PENDING_KEEP_MS, SESSION_LENGTH_MS } from "../../src/accounts/index.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { type Entries, START_RESTARTS } from "../../src/do/entries.ts";
import {
  ACCOUNT,
  CAP,
  completed,
  HASH,
  OTHER_CAP,
  sessionPort,
  subscriptionEvent,
  tables,
  type World,
  world,
} from "./support.ts";

const EXPIRY = 10_000;
let current: World | null = null;

afterEach(() => {
  current?.close();
  current = null;
});

function open(port: ProviderPort): World {
  current = world(port);
  return current;
}

type Read = Awaited<ReturnType<ProviderPort["readSession"]>>;

function readsFrom(answers: () => Read): { port: ProviderPort & { made: number }; reads: string[] } {
  const reads: string[] = [];
  const port = sessionPort(EXPIRY, {
    async readSession(sessionId) {
      reads.push(sessionId);
      return answers();
    },
  });
  return { port, reads };
}

async function pendingStart(entries: Entries): Promise<void> {
  expect(await entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/c/1" });
  expect((await tables(entries))?.pending_checkout).toHaveLength(1);
}

test("start answers unavailable after START_RESTARTS restarts on stale reads", async () => {
  const reads: string[] = [];
  let depth = 0;
  let entries: Entries | null = null;
  const port = sessionPort(0, {
    async readSession(sessionId) {
      reads.push(sessionId);
      if (depth > 0) return { kind: "expired" };
      depth += 1;
      try {
        expect((await entries?.start(ACCOUNT, HASH, OTHER_CAP))?.result).toBe("session");
      } finally {
        depth -= 1;
      }
      return { kind: "open" };
    },
  });
  const w = open(port);
  entries = w.entries;
  expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/c/1" });
  reads.length = 0;

  expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "unavailable" });

  expect(reads).toEqual(["cs_1", "cs_1", "cs_2", "cs_2", "cs_3", "cs_3"]);
  expect(reads.filter((_, index) => index % 2 === 0)).toHaveLength(START_RESTARTS + 1);
  const pending = (await tables(w.entries))?.pending_checkout;
  expect(pending).toEqual([expect.objectContaining({ session_id: "cs_4", cap_amount: OTHER_CAP.amount, cap_period: OTHER_CAP.period })]);
});

test("a processed event id is gone after EVENT_ID_KEEP_MS and a redelivery changes nothing", async () => {
  const w = open(sessionPort(EXPIRY));
  await pendingStart(w.entries);
  await w.entries.webhookEvent(completed());
  await w.entries.webhookEvent(subscriptionEvent());
  const kept = await tables(w.entries);
  expect(kept?.processed_event?.map((row) => [row.event_id, row.received_at])).toEqual([
    ["evt_completed", 1000],
    ["evt_subscription", 1000],
  ]);
  const account = kept?.account;
  expect(account).toEqual([expect.objectContaining({ state: "active", provider_time: 3_000, subscription_id: "sub_1" })]);

  expect(await w.entries.testSetClock(1000 + EVENT_ID_KEEP_MS - 1)).toEqual({ result: "clock-set" });
  expect((await tables(w.entries))?.processed_event).toHaveLength(2);
  expect(await w.entries.testSetClock(1000 + EVENT_ID_KEEP_MS)).toEqual({ result: "clock-set" });
  expect((await tables(w.entries))?.processed_event).toEqual([]);

  await w.entries.webhookEvent(subscriptionEvent());
  await w.entries.webhookEvent(completed());
  const after = await tables(w.entries);
  expect(after?.account).toEqual(account);
  expect(after?.pending_checkout).toEqual([]);
  expect(await w.entries.status(HASH)).toEqual({ result: "active" });
});

test("a read that answers unknown at the first bound removes the pending checkout", async () => {
  const { port, reads } = readsFrom(() => ({ kind: "unknown" }));
  const w = open(port);
  await pendingStart(w.entries);

  await w.entries.testSetClock(EXPIRY);
  expect(reads).toEqual(["cs_1"]);
  expect((await tables(w.entries))?.pending_checkout).toEqual([expect.objectContaining({ next_read: EXPIRY + SESSION_LENGTH_MS })]);
  expect(w.logs).toEqual([]);

  await w.entries.testSetClock(EXPIRY + PENDING_DROP_MS);
  expect(reads).toEqual(["cs_1", "cs_1"]);
  expect(await w.entries.testInspect()).toEqual({ result: "no-such-account" });
  expect(w.logs).toEqual([{ event: "pending-dropped" }]);
  expect(await w.entries.status(HASH)).toEqual({ result: "no-such-account" });
});

test("a read that gives no answer keeps the pending checkout at the first bound and removes it at the second", async () => {
  const { port, reads } = readsFrom(() => ({ kind: "failed" }));
  const w = open(port);
  await pendingStart(w.entries);

  await w.entries.testSetClock(EXPIRY + PENDING_DROP_MS);
  expect(reads).toEqual(["cs_1"]);
  expect((await tables(w.entries))?.pending_checkout).toEqual([
    expect.objectContaining({ session_id: "cs_1", next_read: EXPIRY + PENDING_DROP_MS + SESSION_LENGTH_MS }),
  ]);
  expect(await w.entries.status(HASH)).toEqual({ result: "pending" });

  await w.entries.testSetClock(EXPIRY + PENDING_KEEP_MS - SESSION_LENGTH_MS);
  expect((await tables(w.entries))?.pending_checkout).toEqual([expect.objectContaining({ next_read: EXPIRY + PENDING_KEEP_MS })]);
  expect(w.logs).toEqual([]);

  await w.entries.testSetClock(EXPIRY + PENDING_KEEP_MS);
  expect(await w.entries.testInspect()).toEqual({ result: "no-such-account" });
  expect(w.logs).toEqual([{ event: "pending-unanswered" }]);
  expect(reads).toEqual(["cs_1", "cs_1", "cs_1"]);
});

test("a read that says open after reads that gave no answer removes the pending checkout at once", async () => {
  let answer: Read = { kind: "failed" };
  const { port, reads } = readsFrom(() => answer);
  const w = open(port);
  await pendingStart(w.entries);

  await w.entries.testSetClock(EXPIRY + PENDING_DROP_MS);
  expect((await tables(w.entries))?.pending_checkout).toHaveLength(1);
  answer = { kind: "open" };

  await w.entries.testSetClock(EXPIRY + PENDING_DROP_MS + SESSION_LENGTH_MS);
  expect(reads).toEqual(["cs_1", "cs_1"]);
  expect(await w.entries.testInspect()).toEqual({ result: "no-such-account" });
  expect(w.logs).toEqual([{ event: "pending-dropped" }]);
});

test("complete with no-payment-required makes the account in the state payment-failed", async () => {
  const { port, reads } = readsFrom(() => ({
    kind: "complete",
    session: { sessionId: "cs_1", customerId: "cus_1", subscriptionId: "sub_1", payment: "no-payment-required" },
  }));
  const w = open(port);
  await pendingStart(w.entries);

  await w.entries.testSetClock(EXPIRY);
  expect(reads).toEqual(["cs_1"]);
  const after = await tables(w.entries);
  expect(after?.pending_checkout).toEqual([]);
  expect(after?.account).toEqual([
    expect.objectContaining({ account_id: ACCOUNT, state: "payment-failed", customer_id: "cus_1", subscription_id: "sub_1", provider_time: null }),
  ]);
  expect(await w.entries.status(HASH)).toEqual({ result: "payment-failed" });

  await w.entries.webhookEvent(subscriptionEvent({ state: "active" }));
  expect(await w.entries.status(HASH)).toEqual({ result: "active" });
});
