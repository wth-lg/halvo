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

const TARGETS = "../../stress/targets.mjs";
const TEST_ENTRY: unknown = (await import(TARGETS)).testEntryApi.ledger;

function endedAtExpiry(state: "voided" | "expired", dispatched: 0 | 1) {
  const run = record(4_000);
  const row = { ...run.final.holds[0], state, dispatched };
  const unreported = state === "expired" ? 100 : 0;
  return { ...run, ledger: TEST_ENTRY, final: { ...run.final, holds: [row], periods: [{ period_start: ANCHOR, reported: 0, unreported, open: 0 }] } };
}

test("unit 4.5 on the test entry: the judge passes a hold without the dispatch mark that ended voided at its expiry, and one with the mark that expired at its amount", async () => {
  const { judge } = await import(RULE);
  const voided = judge(endedAtExpiry("voided", 0));
  expect(voided.violations).toEqual([]);
  expect(voided.facts).toMatchObject({ voided: 0, expired: 0, voidedAtExpiry: 1 });
  expect(judge(endedAtExpiry("expired", 1)).violations).toEqual([]);
});

test("unit 4.5 on the test entry: the judge flags a hold without the mark that expired at its amount, and a hold with the mark that ended voided", async () => {
  const { judge } = await import(RULE);
  expect(judge(endedAtExpiry("expired", 0)).violations).toEqual([
    { clause: "voided-at-expiry", id: "r", detail: "expired at its amount of 100 without the dispatch mark: it had no upstream call" },
  ]);
  expect(judge(endedAtExpiry("voided", 1)).violations).toEqual([
    { clause: "voided-at-expiry", id: "r", detail: "voided with the dispatch mark: it may have had an upstream call" },
  ]);
});

test("unit 4.5 on the test entry, from the review: the judge flags a hold whose request reached the upstream without the dispatch mark, also when it settled", async () => {
  const { judge } = await import(RULE);
  const run = record(4_000);
  const row = { ...run.final.holds[0], state: "settled", dispatched: 0, cost: 50, ended_at: 2_000, ended_by: "settle" };
  const settled = {
    ...run,
    ledger: TEST_ENTRY,
    price: { input: 10_000_000, output: 40_000_000 },
    requests: [{ ...run.requests[0], trailer: { settled: true, outcome: "settled", cost: 50, settle_ms: 1 } }],
    mockEvents: [
      { id: "r", ev: "request", t: 1 },
      { id: "r", ev: "usage", final: true, input_tokens: 1, output_tokens: 1, t: 2 },
      { id: "r", ev: "end", clean: true, t: 3 },
    ],
    final: { ...run.final, holds: [row], periods: [{ period_start: ANCHOR, reported: 50, unreported: 0, open: 0 }] },
  };
  expect(judge(settled).violations).toEqual([
    { clause: "voided-at-expiry", id: "r", detail: "the upstream was called for this request, and its hold carries no dispatch mark" },
  ]);
});

function stopped(state: "voided" | "expired", dispatched: 0 | 1, headersAt: number | null = 4_050) {
  const run = endedAtExpiry(state, dispatched);
  return { ...run, requests: [{ ...run.requests[0], status: 409, body: '{"result":"stopped"}', headersAt, trailer: null }] };
}

test("ruling B-34 line 1: a 409 stopped whose answer reached the driver at or after the row's expiry is no violation, and one whose answer came before the expiry, or at no recorded time, is", async () => {
  const { judge } = await import(RULE);
  const tooSoon = [{ clause: "integrity", id: "r", detail: "the request ended with status 409 and error undefined" }];
  expect(judge(stopped("voided", 0, 4_000)).violations).toEqual([]);
  expect(judge(stopped("voided", 0, 4_050)).violations).toEqual([]);
  expect(judge(stopped("voided", 0, 3_999)).violations).toEqual(tooSoon);
  expect(judge(stopped("voided", 0, null)).violations).toEqual(tooSoon);
});

test("unit 4.5 on the test entry, from the review: a 409 stopped whose hold ended voided at its expiry without the mark and without an upstream call is no violation, and one whose hold expired is", async () => {
  const { judge } = await import(RULE);
  expect(judge(stopped("voided", 0)).violations).toEqual([]);
  expect(judge(stopped("expired", 1)).violations).toEqual([{ clause: "integrity", id: "r", detail: "the request ended with status 409 and error undefined" }]);
  expect(judge(stopped("expired", 0)).violations).toEqual([
    { clause: "integrity", id: "r", detail: "the request ended with status 409 and error undefined" },
    { clause: "voided-at-expiry", id: "r", detail: "expired at its amount of 100 without the dispatch mark: it had no upstream call" },
  ]);
});

const API: { chatStatus: unknown; ledger: unknown } = (await import(TARGETS)).testEntryApi;

function reviewed(over: { row?: object; request?: object; top?: object } = {}) {
  const row = {
    request_id: "r",
    period_start: ANCHOR,
    amount: 100,
    expires_at: 4_000,
    state: "voided",
    dispatched: 0,
    created_at: 1_000,
    created_seq: 1,
    ended_at: 4_000,
    ended_seq: 2,
    ended_by: "expire",
    input_tokens: null,
    output_tokens: null,
    cost: null,
    overrun: 0,
    late_at: null,
    late_seq: null,
    ...over.row,
  };
  const request = {
    id: "r",
    status: 409,
    body: '{"result":"stopped"}',
    sentAt: 1_000,
    headersAt: null,
    endedAt: 4_100,
    hold: 100,
    receipt: null,
    trailer: null,
    abortedAt: null,
    lost: [],
    behavior: "normal",
    fault: "usage-last",
    ...over.request,
  };
  return {
    cell: { name: "usage-last@1", kind: "matrix", level: 1, switch: "usage-last" },
    cap: 10_000,
    price: { input: 3_100_000, output: 15_700_000 },
    requests: [request],
    mockEvents: [],
    final: { now: 9_000, account: { cap: 10_000, clock_ms: 9_000, seq: 2 }, periods: [{ period_start: ANCHOR, reported: 0, unreported: 0, open: 0 }], holds: [row], alarm: null, databaseSize: null },
    afterRestart: null,
    kill: null,
    firstLedgerReadAt: 9_000,
    restartReadyAt: null,
    serverErrors: [],
    anchor: ANCHOR,
    periodMs: PERIOD,
    chatStatus: API.chatStatus,
    ledger: API.ledger,
    ...over.top,
  };
}

const flaggedAsIntegrity = [{ clause: "integrity", id: "r", detail: "the request ended with status 409 and error undefined" }];
const reviewCases = [
  {
    name: "C4, a 409 stopped for an id sent once whose row a void step ended before its expiry, without the mark and the upstream, is a violation (B-34 line 4)",
    record: reviewed({ row: { ended_at: 2_000, ended_by: "void" }, request: { headersAt: 4_050 } }),
    expected: { counted: true, violations: flaggedAsIntegrity },
  },
  {
    name: "C5, a 409 repeat for an id sent once whose row was voided at its expiry is a violation (B-34 line 4)",
    record: reviewed({ request: { body: '{"result":"repeat","state":"voided","dispatched":false}', headersAt: 4_050 } }),
    expected: { counted: true, violations: flaggedAsIntegrity },
  },
  {
    name: "C6, a quiet cell whose kill left a hold open that was never dispatched, and that the alarm voided at its expiry before the first read, is no violation",
    record: reviewed({
      request: { status: null, error: "ECONNRESET", endedAt: 2_000, body: null },
      top: { cell: { name: "quiet@1", kind: "quiet", level: 1, switch: null }, kill: { at: 2_000, inFlight: 1 }, firstLedgerReadAt: 5_000, restartReadyAt: 3_000 },
    }),
    expected: { counted: false, violations: [] },
  },
  {
    name: "C7, a void cell whose hold the gateway voided before its expiry counts",
    record: reviewed({
      request: { status: 503, body: '{"result":"voided"}', endedAt: 2_000 },
      row: { ended_at: 2_000, ended_by: "void" },
      top: { cell: { name: "void@1", kind: "void", level: 1, switch: null } },
    }),
    expected: { counted: true, violations: [] },
  },
];

for (const each of reviewCases) {
  test(`unit 4.5, the review's B1: ${each.name}`, async () => {
    const { judge } = await import(RULE);
    const verdict = judge(structuredClone(each.record));
    expect({ counted: verdict.counted, violations: verdict.violations }).toEqual(each.expected);
  });
}

test("the S1 spike's ledger, which has no dispatch mark, keeps S1's rule: a void after the expiry is flagged", async () => {
  const { judge } = await import(RULE);
  const run = record(4_000);
  const spike = { ...run, final: { ...run.final, holds: [{ ...run.final.holds[0], state: "voided" }], periods: [{ period_start: ANCHOR, reported: 0, unreported: 0, open: 0 }] } };
  expect(judge(spike).violations).toEqual([{ clause: "no-open-after-expiry", id: "r", detail: "voided 0 ms after its expiry" }]);
});

function rollover(oldEndsAfterTheNewAdmission: boolean) {
  const run = record(4_000);
  const boundary = ANCHOR + PERIOD;
  const base = { ...run.final.holds[0], state: "settled", dispatched: 1, cost: 50, expires_at: boundary + 9_000, ended_by: "settle" };
  const old = oldEndsAfterTheNewAdmission
    ? { ...base, request_id: "old", period_start: ANCHOR, created_at: boundary - 1_000, created_seq: 1, ended_at: boundary + 2_000, ended_seq: 3 }
    : { ...base, request_id: "old", period_start: ANCHOR, created_at: boundary - 1_000, created_seq: 1, ended_at: boundary - 500, ended_seq: 2 };
  const young = {
    ...base,
    request_id: "new",
    period_start: boundary,
    created_at: boundary + 1_000,
    created_seq: oldEndsAfterTheNewAdmission ? 2 : 3,
    ended_at: boundary + 3_000,
    ended_seq: 4,
  };
  const request = (id: string, sentAt: number) => ({ ...run.requests[0], id, sentAt, trailer: { settled: true, outcome: "settled", cost: 50, settle_ms: 1 } });
  const usage = (id: string) => [{ id, ev: "request", t: 1 }, { id, ev: "usage", final: true, input_tokens: 1, output_tokens: 1, t: 2 }, { id, ev: "end", clean: true, t: 3 }];
  return {
    ...run,
    cell: { name: "rollover@20", kind: "rollover", level: 20, switch: "usage-last" },
    price: { input: 10_000_000, output: 40_000_000 },
    requests: [request("old", boundary - 1_000), request("new", boundary + 1_000)],
    mockEvents: [...usage("old"), ...usage("new")],
    ledger: TEST_ENTRY,
    final: {
      ...run.final,
      now: boundary + 20_000,
      holds: [old, young],
      periods: [
        { period_start: ANCHOR, reported: 50, unreported: 0, open: 0 },
        { period_start: boundary, reported: 50, unreported: 0, open: 0 },
      ],
    },
  };
}

test("ruling B-15: the judge returns crossed among the facts of a rollover cell, true when a hold of the old period was open at an admission of the new one, and false when it had ended", async () => {
  const { judge } = await import(RULE);
  const open = judge(rollover(true));
  expect([open.violations, open.facts.crossed, open.counted]).toEqual([[], true, true]);
  const ended = judge(rollover(false));
  expect([ended.violations, ended.facts.crossed, ended.counted]).toEqual([[], false, false]);
  expect(judge(record(4_000)).facts.crossed).toBeNull();
});

test("the judge flags a hold that the ledger expired before its expiry time, and passes one expired at it", async () => {
  const { judge } = await import(RULE);
  const early = judge(record(3_999)).violations;
  expect(early).toEqual([{ clause: "others-full-hold", id: "r", detail: "expired 1 ms before its expiry time" }]);
  expect(judge(record(4_000)).violations).toEqual([]);
});
