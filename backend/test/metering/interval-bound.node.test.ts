import { expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import type { Metering } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import { makeTestHost } from "../../test-kit/sql/node.ts";

const WEEK = 604_800_000;
const OLDER = 0;
const ACROSS = WEEK - 1000;
const ACCOUNT = "00000000-0000-4000-8000-000000000001";
const HASH = "a".repeat(64);
const PRICE = { input: 1_000_000, output: 1_000_000 };
const HALF_PAST_THE_RANGE = { inputTokens: 4_710_000_000_000_000, outputTokens: 0 };

function scriptedMetering(oldest: () => Ms): Metering {
  return {
    init() {},
    begin() {},
    billingPeriod() {},
    intervalAt: () => ACROSS,
    cut() {},
    oldestUnsealed: () => ({ interval: oldest(), end: oldest() + 3_600_000 }),
    seal() {},
    send: () => ({ call: null, notBillable: 0 }),
    record: () => ({ call: null, notBillable: 0 }),
    nextSend: () => null,
    settled: () => true,
  };
}

async function outcome(run: () => Promise<unknown>): Promise<unknown> {
  try {
    return await run();
  } catch (error) {
    return { threw: (error as Error).name };
  }
}

test("B-17: a settle that would take its sync interval past 2^53 - 1 while an older interval is unsealed is refused, and then the account still answers and its delete finishes", async () => {
  const port = {
    async cancelSubscription() {
      return { kind: "nothing-to-cancel" as const };
    },
  } as unknown as ProviderPort;
  let oldest = OLDER;
  let now = 0;
  const { host, close } = makeTestHost({ port, clock: () => now });
  const entries = makeEntries(host, { accounts: makeAccounts(), ledger: makeLedger({ holdLifeMs: 3000 }), metering: scriptedMetering(() => oldest) });
  await entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", customerId: "cus_1", cap: { amount: 10, period: "week" }, anchor: 0 });
  now = WEEK - 500;
  const a = await outcome(() => entries.reserve(HASH, { requestId: "a", tier: "low_cost_hosted", amount: 100, price: PRICE }));
  const settledA = await outcome(() => entries.settle("a", HALF_PAST_THE_RANGE));
  now = WEEK + 500;
  const b = await outcome(() => entries.reserve(HASH, { requestId: "b", tier: "low_cost_hosted", amount: 100, price: PRICE }));
  const settledB = await outcome(() => entries.settle("b", HALF_PAST_THE_RANGE));
  oldest = ACROSS;
  const after = {
    capGet: await outcome(() => entries.capGet(HASH)),
    status: await outcome(() => entries.status(HASH)),
  };
  now = WEEK + 10_000;
  const destroy = await outcome(() => entries.destroy(HASH));
  const inspect = await outcome(() => entries.testInspect());
  close();
  expect({ a, settledA, b, settledB, after, destroy, inspect }).toEqual({
    a: expect.objectContaining({ outcome: "admitted" }),
    settledA: { outcome: "settled", cost: 4_710_000_000_000_000, overrun: 4_709_999_999_999_900, repeat: false },
    b: expect.objectContaining({ outcome: "admitted" }),
    settledB: { threw: "RangeError" },
    after: {
      capGet: { result: "cap", cap: { amount: 10, period: "week" }, spend: { spent: 0, held: 100, period_end: 2 * WEEK } },
      status: { result: "active" },
    },
    destroy: { result: "deleted" },
    inspect: { result: "no-such-account" },
  });
});
