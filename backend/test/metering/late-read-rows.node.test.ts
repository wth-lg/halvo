import { describe, expect, test } from "vitest";
import type { UsageCall } from "../../src/metering/api.ts";
import { LATE_READ_SPAN_MS, makeMetering, SYNC_RETRY_MS } from "../../src/metering/index.ts";
import { openSql } from "../../test-kit/sql/node.ts";

type Row = Record<string, string | number | null>;

const ACCOUNT = "00000000-0000-4000-8000-0000000000a1";
const T0 = 1_800_000_017_000;
const HOUR = 3_600_000;
const SENT = T0 + HOUR + 2000;

function sealed() {
  const db = openSql();
  const metering = makeMetering({ intervalSeconds: 3600, marginSeconds: 300 });
  const start = db.step((sql) => {
    metering.init(sql);
    metering.begin(sql, ACCOUNT, T0);
    return metering.intervalAt(sql, T0 + 1000);
  });
  db.step((sql) => metering.seal(sql, start, 70_000, T0 + HOUR + 1000));
  return {
    db,
    send: (now: number) => db.step((sql) => metering.send(sql, now)),
    record: (call: UsageCall, answer: Parameters<typeof metering.record>[2], now: number) => db.step((sql) => metering.record(sql, call, answer, now)),
    row: (): Row => db.step((sql) => sql.exec("SELECT * FROM metering_interval WHERE start = ?", start)[0] as Row),
  };
}

function named(call: UsageCall | null): UsageCall {
  if (call === null) throw new Error("the step named no call");
  return call;
}

function readOf(call: UsageCall | null): Extract<UsageCall, { kind: "read" }> {
  if (call === null || call.kind !== "read") throw new Error(`the step named ${call === null ? "no call" : call.kind}, not a read`);
  return call;
}

describe("the rows that a late read leaves (unit 5.4; ruling P7-100, J1 to J3)", () => {
  test("J3: two sends in a row whose answers are lost at a stop count the late point from the newer send, and a read at the older send's late point names no send", () => {
    const m = sealed();
    const first = named(m.send(SENT).call);
    expect(first.kind).toBe("send");
    const second = named(m.send(SENT + SYNC_RETRY_MS).call);
    expect(second.kind).toBe("send");
    const afterTheSecond = m.row();
    expect({ sent_at: afterTheSecond.sent_at, answer_due: afterTheSecond.answer_due, unanswered_at: afterTheSecond.unanswered_at }).toEqual({
      sent_at: SENT + SYNC_RETRY_MS,
      answer_due: 1,
      unanswered_at: SENT,
    });
    const olderLatePoint = SENT + LATE_READ_SPAN_MS;
    const read = readOf(m.send(olderLatePoint).call);
    expect(m.row().unanswered_at).toBe(SENT + SYNC_RETRY_MS);
    const after = m.record(read, { kind: "total", from: read.from, to: read.to, totalMinor: 0 }, olderLatePoint);
    expect(after.call).toBeNull();
    m.db.close();
  });

  test("J1: a deciding read that clears the mark sets the delay back, so the resend that it names leaves the next try 2 x SYNC_RETRY_MS after that step", () => {
    const m = sealed();
    const first = named(m.send(SENT).call);
    m.record(first, { kind: "failed" }, SENT);
    let now = SENT + LATE_READ_SPAN_MS;
    for (let unanswered = 0; unanswered < 6; unanswered += 1) {
      const read = readOf(m.send(now).call);
      m.record(read, { kind: "failed" }, now);
      now = Number(m.row().next_attempt);
    }
    expect(Number(m.row().delay)).toBe(3_600_000);
    const deciding = readOf(m.send(now).call);
    const resend = m.record(deciding, { kind: "total", from: deciding.from, to: deciding.to, totalMinor: 0 }, now);
    expect(resend.call?.kind).toBe("send");
    const row = m.row();
    expect({ unanswered: row.unanswered, pending_refusal: row.pending_refusal, delay: row.delay, next_attempt: row.next_attempt }).toEqual({
      unanswered: 0,
      pending_refusal: null,
      delay: 2 * SYNC_RETRY_MS,
      next_attempt: now + 2 * SYNC_RETRY_MS,
    });
    m.db.close();
  });

  test("J2: a refusal with another reason while one is pending, recorded 50 s before the late point, leaves the next try at the late point", () => {
    const m = sealed();
    const first = named(m.send(SENT).call);
    m.record(first, { kind: "failed" }, SENT);
    const second = named(m.send(SENT + SYNC_RETRY_MS).call);
    const afterTheRefusal = m.record(second, { kind: "refused", reason: "unknown_account" }, SENT + SYNC_RETRY_MS);
    expect(afterTheRefusal.call?.kind).toBe("read");
    const now = SENT + LATE_READ_SPAN_MS - 50_000;
    const read = readOf(m.send(now).call);
    const resend = named(m.record(read, { kind: "total", from: read.from, to: read.to, totalMinor: 0 }, now).call);
    expect(resend.kind).toBe("send");
    m.record(resend, { kind: "refused", reason: "amount_not_whole_minor_units" }, now);
    const row = m.row();
    expect({ pending_refusal: row.pending_refusal, unanswered_at: row.unanswered_at, next_attempt: row.next_attempt }).toEqual({
      pending_refusal: "amount_not_whole_minor_units",
      unanswered_at: SENT,
      next_attempt: SENT + LATE_READ_SPAN_MS,
    });
    m.db.close();
  });
});
