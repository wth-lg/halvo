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
    const before = metering.settled(sql);
    const closed = metering.closeOpen(sql, T0 + 3 * H);
    const again = metering.closeOpen(sql, T0 + 3 * H + 1);
    return {
      before,
      closed,
      again,
      after: metering.settled(sql),
      intervals: sql.exec("SELECT start, sealed_at, amount, identifier, timestamp, outcome, refusal, closed_at FROM metering_interval ORDER BY start").map((row) => ({ ...row })),
    };
  });
}

export const DELETE_BOUND_EXPECTED = {
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
