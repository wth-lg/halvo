import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Step } from "../../src/types.ts";

const ACCOUNT = "00000000-0000-4000-8000-0000000000e6";
const H = 3_600_000;
const T0 = 1_800_000_017_000;

export const DELETE_BOUND_CASE = "closeOpen at the removal's bound: an interval left unsealed, one sealed and unsent, one acknowledged, and settled after it";

export function runDeleteBoundCase(step: Step): unknown {
  const metering = makeMetering({ intervalSeconds: 3600, marginSeconds: 300 });
  return step((sql) => {
    metering.init(sql);
    metering.begin(sql, ACCOUNT, T0);
    metering.intervalAt(sql, T0 + 10);
    metering.seal(sql, T0, 30_000, T0 + H);
    const sent = metering.send(sql, T0 + H);
    if (sent.call !== null) metering.record(sql, sent.call, { kind: "recorded" }, T0 + H);
    metering.intervalAt(sql, T0 + H + 10);
    metering.seal(sql, T0 + H, 20_000, T0 + 2 * H);
    metering.intervalAt(sql, T0 + 2 * H + 10);
    metering.cut(sql, "delete", T0 + 2 * H + 500_000, T0 + 2 * H + 500_000);
    const unacknowledged = metering.unacknowledged(sql);
    const before = metering.settled(sql);
    const closed = metering.closeOpen(sql, T0 + 3 * H);
    const again = metering.closeOpen(sql, T0 + 3 * H + 1);
    return {
      unacknowledged,
      before,
      closed,
      again,
      after: metering.settled(sql),
      intervals: sql.exec("SELECT start, sealed_at, amount, identifier, timestamp, outcome, refusal, closed_at FROM metering_interval ORDER BY start").map((row) => ({ ...row })),
    };
  });
}

export const DELETE_BOUND_EXPECTED = {
  unacknowledged: 2,
  before: false,
  closed: 2,
  again: 0,
  after: true,
  intervals: [
    {
      start: T0,
      sealed_at: T0 + H,
      amount: 3,
      identifier: `${ACCOUNT}:${T0 / 1000}`,
      timestamp: (T0 + H) / 1000 - 1,
      outcome: "acknowledged",
      refusal: null,
      closed_at: T0 + H,
    },
    {
      start: T0 + H,
      sealed_at: T0 + 2 * H,
      amount: 2,
      identifier: `${ACCOUNT}:${(T0 + H) / 1000}`,
      timestamp: (T0 + 2 * H) / 1000 - 1,
      outcome: "not-billable",
      refusal: "delete-bound",
      closed_at: T0 + 3 * H,
    },
    {
      start: T0 + 2 * H,
      sealed_at: T0 + 3 * H,
      amount: 0,
      identifier: `${ACCOUNT}:${(T0 + 2 * H) / 1000}`,
      timestamp: (T0 + 2 * H + 500_000) / 1000 - 1,
      outcome: "not-billable",
      refusal: "delete-bound",
      closed_at: T0 + 3 * H,
    },
  ],
};

export const HOLDS_AT_CASE = "openHoldsAt at the cancel's test: a hold counts as open only while its stored expiry is after the step's now";

export function runHoldsAtCase(step: Step): unknown {
  const ledger = makeLedger({ holdLifeMs: 3000 });
  return step((sql) => {
    ledger.init(sql);
    ledger.setCap(sql, { amount: 5, period: "week" }, T0);
    const reserved = ledger.reserve(sql, { requestId: "r1", tier: "low_cost_hosted", amount: 100, price: { input: 1, output: 1 } }, T0, { active: true, interval: T0 });
    return {
      reserved: reserved.outcome,
      before: ledger.openHoldsAt(sql, T0 + 2999),
      atTheExpiry: ledger.openHoldsAt(sql, T0 + 3000),
      stored: ledger.openHolds(sql),
    };
  });
}

export const HOLDS_AT_EXPECTED = { reserved: "admitted", before: 1, atTheExpiry: 0, stored: 1 };
