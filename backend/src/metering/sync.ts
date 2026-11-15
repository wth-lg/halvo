import type { UsageRefusal } from "../billing/port.ts";
import { PROVIDER_CALL_TIMEOUT_MS } from "../billing/stripe/timeout.ts";
import type { BillingPeriod, Micro, Ms, Sql } from "../types.ts";
import { MICRO_PER_MINOR } from "../types.ts";
import { LATE_READ_SPAN_MS, type Metering, type Next, REFUSAL_LIMIT, RETRY_DELAY_CAP_MS, SEND_LIMIT_MS, type UsageAnswer, type UsageCall } from "./api.ts";

export const SYNC_RETRY_MS = 60_000;
export const UNIQUENESS_WINDOW_MS = 86_400_000;

type Row = Record<string, string | number | null>;
type Outcome = "acknowledged" | "no-call" | "written-off" | "not-billable";
type Closing = UsageRefusal | "send_limit";
type CutKind = "cancel" | "delete";

const OUTCOMES: readonly Outcome[] = ["acknowledged", "no-call", "written-off", "not-billable"];
const REFUSALS: readonly UsageRefusal[] = ["amount_not_whole_minor_units", "timestamp_too_old", "timestamp_in_future", "unknown_account"];
const CUT_KINDS: readonly CutKind[] = ["cancel", "delete"];
const MICRO = BigInt(MICRO_PER_MINOR);

interface Account {
  accountId: string;
  origin: Ms;
  period: BillingPeriod | null;
  carry: Micro;
  cutKind: CutKind | null;
  cutAt: Ms | null;
  writtenOff: Micro | null;
}

interface Interval {
  start: Ms;
  end: Ms;
  sealed: boolean;
  amount: number;
  identifier: string;
  timestamp: number;
  firstAttempt: Ms | null;
  nextAttempt: Ms | null;
  delay: Ms | null;
  refusals: number;
  unansweredAt: Ms | null;
  answerDueFor: Ms | null;
  pendingRefusal: UsageRefusal | null;
  outcome: Outcome | null;
}

function broken(what: string): never {
  const error = new Error(`a metering row holds a value of ${what} that the module never writes`);
  error.name = "MeteringRowError";
  throw error;
}

function whole(row: Row, key: string): number {
  const value = row[key];
  return typeof value === "number" && Number.isSafeInteger(value) ? value : broken(key);
}

function wholeOrNull(row: Row, key: string): number | null {
  return row[key] === null ? null : whole(row, key);
}

function text(row: Row, key: string): string {
  const value = row[key];
  return typeof value === "string" ? value : broken(key);
}

function timeWhileSet(row: Row, flag: string, key: string): Ms | null {
  const set = whole(row, flag);
  if (set !== 0 && set !== 1) broken(flag);
  if (set === 0 && row[key] !== null) broken(key);
  return set === 1 ? whole(row, key) : null;
}

function oneOfOrNull<T extends string>(row: Row, key: string, values: readonly T[]): T | null {
  const value = row[key];
  if (value === null) return null;
  return values.find((each) => each === value) ?? broken(key);
}

function seconds(at: Ms): number {
  return Math.floor(at / 1000);
}

function wholeSecond(at: Ms): Ms {
  return seconds(at) * 1000;
}

const ACCOUNT_COLUMNS = "account_id, origin, period_start, period_end, carry, written_off, cut_kind, cut_at";
const INTERVAL_COLUMNS =
  "start, end_at, sealed_at, amount, identifier, timestamp, first_attempt, next_attempt, delay, refusals, unanswered, unanswered_at, answer_due, sent_at, pending_refusal, outcome";

function accountOf(row: Row): Account {
  const start = wholeOrNull(row, "period_start");
  const end = wholeOrNull(row, "period_end");
  if ((start === null) !== (end === null)) broken("period_start");
  return {
    accountId: text(row, "account_id"),
    origin: whole(row, "origin"),
    period: start === null || end === null ? null : { start, end },
    carry: whole(row, "carry"),
    cutKind: oneOfOrNull(row, "cut_kind", CUT_KINDS),
    cutAt: wholeOrNull(row, "cut_at"),
    writtenOff: wholeOrNull(row, "written_off"),
  };
}

function intervalOf(row: Row): Interval {
  const sealed = row.sealed_at !== null;
  return {
    start: whole(row, "start"),
    end: whole(row, "end_at"),
    sealed,
    amount: sealed ? whole(row, "amount") : 0,
    identifier: sealed ? text(row, "identifier") : "",
    timestamp: sealed ? whole(row, "timestamp") : 0,
    firstAttempt: wholeOrNull(row, "first_attempt"),
    nextAttempt: wholeOrNull(row, "next_attempt"),
    delay: wholeOrNull(row, "delay"),
    refusals: whole(row, "refusals"),
    unansweredAt: timeWhileSet(row, "unanswered", "unanswered_at"),
    answerDueFor: timeWhileSet(row, "answer_due", "sent_at"),
    pendingRefusal: oneOfOrNull(row, "pending_refusal", REFUSALS),
    outcome: oneOfOrNull(row, "outcome", OUTCOMES),
  };
}

function readAccount(sql: Sql): Account | null {
  const row = sql.exec(`SELECT ${ACCOUNT_COLUMNS} FROM metering_account`)[0];
  return row === undefined ? null : accountOf(row);
}

function account(sql: Sql): Account {
  return readAccount(sql) ?? broken("metering_account");
}

function newest(sql: Sql): Interval | null {
  const row = sql.exec(`SELECT ${INTERVAL_COLUMNS} FROM metering_interval ORDER BY start DESC LIMIT 1`)[0];
  return row === undefined ? null : intervalOf(row);
}

function oldestUnsealedRow(sql: Sql): Interval | null {
  const row = sql.exec(`SELECT ${INTERVAL_COLUMNS} FROM metering_interval WHERE sealed_at IS NULL ORDER BY start LIMIT 1`)[0];
  return row === undefined ? null : intervalOf(row);
}

function oldestPending(sql: Sql): Interval | null {
  const row = sql.exec(
    `SELECT ${INTERVAL_COLUMNS} FROM metering_interval WHERE sealed_at IS NOT NULL AND outcome IS NULL ORDER BY start LIMIT 1`,
  )[0];
  return row === undefined ? null : intervalOf(row);
}

function intervalAtStart(sql: Sql, start: Ms): Interval | null {
  const row = sql.exec(`SELECT ${INTERVAL_COLUMNS} FROM metering_interval WHERE start = ?`, start)[0];
  return row === undefined ? null : intervalOf(row);
}

function setEnd(sql: Sql, start: Ms, end: Ms): void {
  sql.exec("UPDATE metering_interval SET end_at = ? WHERE start = ?", end, start);
}

function close(sql: Sql, interval: Interval, outcome: Outcome, closing: Closing | null, now: Ms): void {
  sql.exec(
    "UPDATE metering_interval SET outcome = ?, refusal = ?, pending_refusal = ?, answer_due = ?, sent_at = ?, closed_at = ? WHERE start = ?",
    outcome,
    closing,
    null,
    0,
    null,
    now,
    interval.start,
  );
}

function slot(a: Account, now: Ms, length: number): { start: Ms; end: Ms } {
  const period = a.period;
  if (period === null) {
    const start = a.origin + Math.floor((now - a.origin) / length) * length;
    return { start, end: start + length };
  }
  if (now >= period.end) {
    const start = period.end + Math.floor((now - period.end) / length) * length;
    return { start, end: start + length };
  }
  const start = period.start + Math.floor((now - period.start) / length) * length;
  const bound = now < period.start ? period.start : period.end;
  return { start, end: Math.min(start + length, bound) };
}

function endUnsealedAt(sql: Sql, at: Ms): void {
  sql.exec("UPDATE metering_interval SET end_at = MAX(start, ?) WHERE sealed_at IS NULL AND end_at > ?", at, at);
}

function writeOffCarry(sql: Sql): void {
  sql.exec("UPDATE metering_account SET written_off = COALESCE(written_off, 0) + carry, carry = ?", 0);
}

export function makeMetering(config: { intervalSeconds: number; marginSeconds: number }): Metering {
  const length = config.intervalSeconds * 1000;
  const margin = config.marginSeconds * 1000;
  if (!Number.isSafeInteger(length) || length < 1000) throw new RangeError("the sync interval is a whole number of seconds, 1 or more");
  if (!Number.isSafeInteger(margin) || margin <= PROVIDER_CALL_TIMEOUT_MS || margin >= UNIQUENESS_WINDOW_MS) {
    throw new RangeError("the sync margin is a whole number of seconds above the provider call's timeout and under the uniqueness window");
  }
  const lateReadSpan = Math.min(LATE_READ_SPAN_MS, UNIQUENESS_WINDOW_MS - margin);

  function inFirstWindow(interval: Interval, now: Ms): boolean {
    return interval.firstAttempt === null || now < interval.firstAttempt + UNIQUENESS_WINDOW_MS - margin;
  }

  function beforeTheLatePoint(interval: Interval, now: Ms): boolean {
    return interval.unansweredAt !== null && now < interval.unansweredAt + lateReadSpan;
  }

  function nextTry(interval: Interval, unansweredAt: Ms | null, now: Ms, delay: Ms): Ms {
    const bounds = [interval.timestamp * 1000 + SEND_LIMIT_MS, ...(unansweredAt === null ? [] : [unansweredAt + lateReadSpan])];
    return Math.min(now + delay, ...bounds.filter((bound) => bound > now));
  }

  function afterAChange(sql: Sql, start: Ms, now: Ms): void {
    const interval = intervalAtStart(sql, start);
    if (interval === null) return;
    sql.exec("UPDATE metering_interval SET next_attempt = ?, delay = ? WHERE start = ?", nextTry(interval, interval.unansweredAt, now, SYNC_RETRY_MS), SYNC_RETRY_MS, start);
  }

  function call(interval: Interval, kind: "send" | "read"): UsageCall {
    if (kind === "send") {
      return { kind: "send", interval: interval.start, identifier: interval.identifier, amountMinor: interval.amount, timestamp: interval.timestamp };
    }
    return { kind: "read", interval: interval.start, from: interval.timestamp, to: interval.timestamp + 1 };
  }

  function plan(sql: Sql, now: Ms, notBillable: number, forced: "send" | "read" | null): Next {
    let closed = notBillable;
    for (;;) {
      const interval = oldestPending(sql);
      if (interval === null) return { call: null, notBillable: closed };
      if (now >= interval.timestamp * 1000 + SEND_LIMIT_MS) {
        close(sql, interval, "not-billable", "send_limit", now);
        closed += 1;
        continue;
      }
      if (forced === null && interval.nextAttempt !== null && now < interval.nextAttempt) return { call: null, notBillable: closed };
      const inWindow = inFirstWindow(interval, now);
      const kind = forced ?? (interval.pendingRefusal === null && inWindow ? "send" : "read");
      const unansweredAt = interval.answerDueFor ?? interval.unansweredAt;
      const delay = interval.delay === null ? SYNC_RETRY_MS : Math.min(2 * interval.delay, RETRY_DELAY_CAP_MS);
      sql.exec(
        "UPDATE metering_interval SET first_attempt = ?, next_attempt = ?, delay = ?, attempts = attempts + 1, unanswered = ?, unanswered_at = ?, answer_due = ?, sent_at = ? WHERE start = ?",
        interval.firstAttempt ?? (kind === "send" ? now : null),
        nextTry(interval, unansweredAt, now, delay),
        delay,
        unansweredAt === null ? 0 : 1,
        unansweredAt,
        kind === "send" ? 1 : 0,
        kind === "send" ? now : null,
        interval.start,
      );
      return { call: call(interval, kind), notBillable: closed };
    }
  }

  function refuse(sql: Sql, interval: Interval, reason: UsageRefusal, now: Ms, chain: boolean): Next {
    if (reason === "timestamp_too_old") {
      close(sql, interval, "not-billable", reason, now);
      return plan(sql, now, 1, null);
    }
    const refusals = interval.refusals + 1;
    sql.exec("UPDATE metering_interval SET refusals = ?, pending_refusal = ? WHERE start = ?", refusals, null, interval.start);
    afterAChange(sql, interval.start, now);
    if (refusals >= REFUSAL_LIMIT) {
      close(sql, interval, "not-billable", reason, now);
      return plan(sql, now, 1, null);
    }
    return chain ? plan(sql, now, 0, "send") : { call: null, notBillable: 0 };
  }

  function othersIn(sql: Sql, interval: Interval, from: number, to: number): number {
    let minor = 0;
    for (const row of sql.exec(
      "SELECT start, amount, timestamp FROM metering_interval WHERE outcome = ? AND timestamp >= ? AND timestamp < ?",
      "acknowledged",
      from,
      to,
    )) {
      if (whole(row, "start") !== interval.start) minor += whole(row, "amount");
    }
    return minor;
  }

  return {
    init(sql) {
      sql.exec(
        "CREATE TABLE metering_account (account_id TEXT NOT NULL, origin INTEGER NOT NULL, period_start INTEGER, period_end INTEGER, carry INTEGER NOT NULL, written_off INTEGER, cut_kind TEXT CHECK (cut_kind IN ('cancel', 'delete')), cut_at INTEGER)",
      );
      sql.exec(
        "CREATE TABLE metering_interval (start INTEGER PRIMARY KEY, end_at INTEGER NOT NULL, sealed_at INTEGER, reported INTEGER, carry_in INTEGER, amount INTEGER, carry_out INTEGER, identifier TEXT, timestamp INTEGER, first_attempt INTEGER, next_attempt INTEGER, delay INTEGER, attempts INTEGER NOT NULL, refusals INTEGER NOT NULL, unanswered INTEGER NOT NULL CHECK (unanswered IN (0, 1)), unanswered_at INTEGER, answer_due INTEGER NOT NULL CHECK (answer_due IN (0, 1)), sent_at INTEGER, pending_refusal TEXT, outcome TEXT CHECK (outcome IN ('acknowledged', 'no-call', 'written-off', 'not-billable')), refusal TEXT, closed_at INTEGER)",
      );
    },

    begin(sql, accountId, now) {
      if (readAccount(sql) !== null) return;
      sql.exec(
        "INSERT INTO metering_account (account_id, origin, period_start, period_end, carry, written_off, cut_kind, cut_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        accountId,
        wholeSecond(now),
        null,
        null,
        0,
        null,
        null,
        null,
      );
    },

    billingPeriod(sql, period) {
      const a = account(sql);
      if (a.period !== null && a.period.start === period.start && a.period.end === period.end) return;
      sql.exec("UPDATE metering_account SET period_start = ?, period_end = ?", period.start, period.end);
      const open = newest(sql);
      if (open === null || open.sealed) return;
      let end = open.end;
      for (const edge of [period.start, period.end]) if (open.start < edge && edge < end) end = edge;
      if (end !== open.end) setEnd(sql, open.start, end);
    },

    intervalAt(sql, now) {
      const a = account(sql);
      const last = newest(sql);
      if (last !== null && last.start <= now && now < last.end) return last.start;
      const grid = slot(a, now, length);
      let start = last === null ? grid.start : Math.max(grid.start, last.end);
      if (last !== null && start === last.start) {
        if (!last.sealed) return last.start;
        start = last.start + 1000;
      }
      sql.exec(
        "INSERT INTO metering_interval (start, end_at, attempts, refusals, unanswered, answer_due) VALUES (?, ?, ?, ?, ?, ?)",
        start,
        Math.max(grid.end, start + 1000),
        0,
        0,
        0,
        0,
      );
      return start;
    },

    cut(sql, kind, at) {
      const a = readAccount(sql);
      if (a === null) return;
      const second = wholeSecond(at);
      if (a.cutKind === null) {
        endUnsealedAt(sql, second);
        sql.exec("UPDATE metering_account SET cut_kind = ?, cut_at = ?", kind, second);
        const open = newest(sql);
        if (open === null || open.sealed) writeOffCarry(sql);
        return;
      }
      if (kind === "delete") {
        endUnsealedAt(sql, second);
        sql.exec("UPDATE metering_account SET cut_kind = ?", "delete");
        return;
      }
      if (a.cutAt !== null && second >= a.cutAt) return;
      endUnsealedAt(sql, second);
      sql.exec("UPDATE metering_account SET cut_at = ?", second);
    },

    oldestUnsealed(sql) {
      const interval = oldestUnsealedRow(sql);
      return interval === null ? null : { interval: interval.start, end: interval.end };
    },

    seal(sql, start, reported, now) {
      const interval = oldestUnsealedRow(sql);
      if (interval === null || interval.start !== start || interval.end > now) return;
      if (!Number.isSafeInteger(reported) || reported < 0) throw new RangeError("the reported total of an interval is a whole number of micro-units, 0 or more");
      const a = account(sql);
      const total = BigInt(reported) + BigInt(a.carry);
      const amount = Number(total / MICRO);
      const carryOut = Number(total % MICRO);
      const last = newest(sql);
      const terminal = a.cutKind !== null && a.writtenOff === null && last !== null && last.start === interval.start;
      sql.exec(
        "UPDATE metering_interval SET sealed_at = ?, reported = ?, carry_in = ?, amount = ?, carry_out = ?, identifier = ?, timestamp = ?, next_attempt = ? WHERE start = ?",
        now,
        reported,
        a.carry,
        amount,
        carryOut,
        `${a.accountId}:${seconds(interval.start)}`,
        Math.max(seconds(interval.start), seconds(interval.end - 1)),
        now,
        interval.start,
      );
      sql.exec("UPDATE metering_account SET carry = ?", carryOut);
      if (terminal) writeOffCarry(sql);
      if (amount === 0) close(sql, interval, terminal ? "written-off" : "no-call", null, now);
    },

    send(sql, now) {
      return plan(sql, now, 0, null);
    },

    record(sql, made, answer: UsageAnswer, now) {
      const interval = intervalAtStart(sql, made.interval);
      if (interval === null || !interval.sealed || interval.outcome !== null) return { call: null, notBillable: 0 };
      if (made.kind === "send") {
        if (answer.kind === "recorded" || answer.kind === "duplicate") {
          close(sql, interval, "acknowledged", null, now);
          return plan(sql, now, 0, null);
        }
        if (answer.kind === "refused") {
          sql.exec("UPDATE metering_interval SET answer_due = ?, sent_at = ? WHERE start = ?", 0, null, interval.start);
          if (interval.unansweredAt === null) return refuse(sql, interval, answer.reason, now, false);
          if (interval.pendingRefusal === answer.reason) return { call: null, notBillable: 0 };
          sql.exec("UPDATE metering_interval SET pending_refusal = ? WHERE start = ?", answer.reason, interval.start);
          afterAChange(sql, interval.start, now);
          return interval.pendingRefusal === null ? plan(sql, now, 0, "read") : { call: null, notBillable: 0 };
        }
        const unansweredAt = interval.answerDueFor ?? interval.unansweredAt ?? now;
        sql.exec("UPDATE metering_interval SET unanswered = ?, unanswered_at = ?, answer_due = ?, sent_at = ? WHERE start = ?", 1, unansweredAt, 0, null, interval.start);
        if (interval.unansweredAt === null) afterAChange(sql, interval.start, now);
        return { call: null, notBillable: 0 };
      }
      if (answer.kind !== "total") return { call: null, notBillable: 0 };
      const others = othersIn(sql, interval, answer.from, answer.to);
      if (answer.totalMinor === others + interval.amount) {
        close(sql, interval, "acknowledged", null, now);
        return plan(sql, now, 0, null);
      }
      if (answer.totalMinor !== others) return { call: null, notBillable: 0 };
      if (beforeTheLatePoint(interval, now)) {
        const roundSend = inFirstWindow(interval, now) && interval.pendingRefusal !== "timestamp_too_old";
        return roundSend ? plan(sql, now, 0, "send") : { call: null, notBillable: 0 };
      }
      sql.exec("UPDATE metering_interval SET unanswered = ?, unanswered_at = ?, answer_due = ?, sent_at = ? WHERE start = ?", 0, null, 0, null, interval.start);
      if (interval.unansweredAt !== null) afterAChange(sql, interval.start, now);
      if (interval.pendingRefusal !== null) return refuse(sql, interval, interval.pendingRefusal, now, true);
      return plan(sql, now, 0, "send");
    },

    nextSend(sql) {
      return oldestPending(sql)?.nextAttempt ?? null;
    },

    settled(sql) {
      const row = sql.exec("SELECT COUNT(*) AS n FROM metering_interval WHERE sealed_at IS NULL OR outcome IS NULL")[0] ?? broken("the count of open intervals");
      return whole(row, "n") === 0;
    },
  };
}
