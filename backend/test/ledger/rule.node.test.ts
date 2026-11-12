import { expect, test } from "vitest";

const RULE = "../../stress/rule.mjs";
const ANCHOR = 1_000;
const PERIOD = 604_800_000;

function record(endedAt: number) {
  const row = {
    request_id: "r",
    period_start: ANCHOR,
    amount: 100,
    expires_at: 4_000,
    state: "expired",
    created_at: 1_000,
    created_seq: 1,
    ended_at: endedAt,
    ended_seq: 2,
    ended_by: "expire",
    input_tokens: null,
    output_tokens: null,
    cost: null,
    overrun: 0,
    late_at: null,
    late_seq: null,
  };
  return {
    cell: { name: "usage-last@1", kind: "matrix", level: 1, switch: "usage-last" },
    cap: 10_000,
    price: { input: 3_100_000, output: 15_700_000 },
    requests: [{ id: "r", status: 200, sentAt: 1_000, headersAt: 1_001, endedAt: 1_100, hold: 100, receipt: null, trailer: { settled: false, why: "the stream ended with no final usage" }, abortedAt: null, lost: [], behavior: "normal", fault: "usage-last" }],
    mockEvents: [],
    final: { now: 9_000, account: { cap: 10_000, clock_ms: 9_000, seq: 2 }, periods: [{ period_start: ANCHOR, reported: 0, unreported: 100, open: 0 }], holds: [row], alarm: null, databaseSize: null },
    afterRestart: null,
    kill: null,
    firstLedgerReadAt: 9_000,
    restartReadyAt: null,
    serverErrors: [],
    anchor: ANCHOR,
    periodMs: PERIOD,
    chatStatus: { refused: 403, voided: 503, upstreamFailed: 502 },
    ledger: { alarmMark: "expire", lateSettleAddsOverrun: false },
  };
}

test("the judge flags a hold that the ledger expired before its expiry time, and passes one expired at it", async () => {
  const { judge } = await import(RULE);
  const early = judge(record(3_999)).violations;
  expect(early).toEqual([{ clause: "others-full-hold", id: "r", detail: "expired 1 ms before its expiry time" }]);
  expect(judge(record(4_000)).violations).toEqual([]);
});
