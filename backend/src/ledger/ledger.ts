import type { Cap, Micro, Ms, Period, Sql, Tier } from "../types.ts";
import { MICRO_PER_MINOR } from "../types.ts";
import type { CapRead, EndResult, HoldState, Ledger, Price, ReserveResult, Usage } from "./api.ts";
import { costOf } from "./cost.ts";
import { type CapPeriod, periodAt } from "./periods.ts";

type Row = Record<string, string | number | null>;
type EndedBy = "expire" | "reserve" | "dispatch" | "settle" | "void" | "cap" | "setCap";

const HOLD_STATES: readonly HoldState[] = ["open", "settled", "voided", "expired"];
const PERIODS: readonly Period[] = ["week", "month"];
const TIERS: readonly Tier[] = ["low_cost_hosted", "frontier"];
const COUNTED_SPEND_MAX = BigInt(Number.MAX_SAFE_INTEGER);

interface CapRow {
  amount: number;
  period: Period;
  anchor: Ms;
  nextPeriod: Period | null;
  switchAt: Ms | null;
}

interface Hold {
  requestId: string;
  tier: Tier;
  amount: Micro;
  price: Price;
  periodStart: Ms;
  expiresAt: Ms;
  state: HoldState;
  dispatched: boolean;
  cost: Micro | null;
  overrun: Micro;
  lateAt: Ms | null;
}

interface Totals {
  reported: Micro;
  unreported: Micro;
  open: Micro;
}

interface Call {
  sql: Sql;
  now: Ms;
  by: EndedBy;
  seq: number;
  readonly seqAtStart: number;
  expired: number;
}

function rowBroken(what: string): never {
  const error = new Error(`a ledger row holds a value of ${what} that the ledger never writes`);
  error.name = "LedgerRowError";
  throw error;
}

function inputBroken(what: string): never {
  throw new RangeError(`${what} is not a whole number of 0 or more`);
}

function whole(name: string, value: number): number {
  return Number.isSafeInteger(value) && value >= 0 ? value : inputBroken(name);
}

function int(row: Row, key: string): number {
  const value = row[key];
  return typeof value === "number" && Number.isSafeInteger(value) ? value : rowBroken(key);
}

function intOrNull(row: Row, key: string): number | null {
  return row[key] === null ? null : int(row, key);
}

function oneOf<T extends string>(row: Row, key: string, values: readonly T[]): T {
  const value = row[key];
  return values.find((each) => each === value) ?? rowBroken(key);
}

function oneOfOrNull<T extends string>(row: Row, key: string, values: readonly T[]): T | null {
  return row[key] === null ? null : oneOf(row, key, values);
}

function capOf(row: Row): CapRow {
  return {
    amount: int(row, "amount"),
    period: oneOf(row, "period", PERIODS),
    anchor: int(row, "anchor"),
    nextPeriod: oneOfOrNull(row, "next_period", PERIODS),
    switchAt: intOrNull(row, "switch_at"),
  };
}

function holdOf(row: Row): Hold {
  const requestId = row.request_id;
  if (typeof requestId !== "string") return rowBroken("request_id");
  return {
    requestId,
    tier: oneOf(row, "tier", TIERS),
    amount: int(row, "amount"),
    price: { input: int(row, "input_price"), output: int(row, "output_price") },
    periodStart: int(row, "period_start"),
    expiresAt: int(row, "expires_at"),
    state: oneOf(row, "state", HOLD_STATES),
    dispatched: int(row, "dispatched") === 1,
    cost: intOrNull(row, "cost"),
    overrun: int(row, "overrun"),
    lateAt: intOrNull(row, "late_at"),
  };
}

function readCap(sql: Sql): CapRow | null {
  const row = sql.exec("SELECT amount, period, anchor, next_period, switch_at FROM ledger_cap")[0];
  return row === undefined ? null : capOf(row);
}

function readHold(sql: Sql, requestId: string): Hold | null {
  const row = sql.exec(
    "SELECT request_id, tier, amount, input_price, output_price, period_start, expires_at, state, dispatched, cost, overrun, late_at FROM ledger_holds WHERE request_id = ?",
    requestId,
  )[0];
  return row === undefined ? null : holdOf(row);
}

function readTotals(sql: Sql, periodStart: Ms): Totals | null {
  const row = sql.exec("SELECT reported, unreported, open FROM ledger_periods WHERE period_start = ?", periodStart)[0];
  return row === undefined ? null : { reported: int(row, "reported"), unreported: int(row, "unreported"), open: int(row, "open") };
}

function periodOf(cap: CapRow, now: Ms): CapPeriod {
  return periodAt(cap.period, cap.anchor, now);
}

function nextSeq(call: Call): number {
  call.seq += 1;
  return call.seq;
}

function switchPeriodType(call: Call): void {
  const cap = readCap(call.sql);
  if (cap === null || cap.nextPeriod === null || cap.switchAt === null || cap.switchAt > call.now) return;
  call.sql.exec("UPDATE ledger_cap SET period = ?, anchor = ?, next_period = ?, switch_at = ?", cap.nextPeriod, cap.switchAt, null, null);
}

function expireDue(call: Call): void {
  const due = call.sql.exec(
    "SELECT request_id, amount, period_start FROM ledger_holds WHERE state = ? AND expires_at <= ? ORDER BY expires_at, request_id",
    "open",
    call.now,
  );
  for (const row of due) {
    const requestId = row.request_id;
    if (typeof requestId !== "string") rowBroken("request_id");
    const amount = int(row, "amount");
    call.sql.exec(
      "UPDATE ledger_holds SET state = ?, ended_at = ?, ended_seq = ?, ended_by = ? WHERE request_id = ?",
      "expired",
      call.now,
      nextSeq(call),
      call.by,
      requestId,
    );
    call.sql.exec(
      "UPDATE ledger_periods SET open = open - ?, unreported = unreported + ? WHERE period_start = ?",
      amount,
      amount,
      int(row, "period_start"),
    );
    call.expired += 1;
  }
}

function within<T>(sql: Sql, now: Ms, by: EndedBy, run: (call: Call) => T): T {
  const stateRow = sql.exec("SELECT seq FROM ledger_state")[0] ?? rowBroken("seq");
  const seq = int(stateRow, "seq");
  const call: Call = { sql, now: whole("now", now), by, seq, seqAtStart: seq, expired: 0 };
  switchPeriodType(call);
  expireDue(call);
  const value = run(call);
  if (call.seq !== call.seqAtStart) sql.exec("UPDATE ledger_state SET seq = ?", call.seq);
  return value;
}

function samePrice(a: Price, b: Price): boolean {
  return a.input === b.input && a.output === b.output;
}

function settleOpen(call: Call, hold: Hold, usage: Usage): EndResult {
  const cost = costOf(hold.price, usage);
  const totals = readTotals(call.sql, hold.periodStart) ?? rowBroken("the totals of the hold's period");
  if (BigInt(totals.reported) + BigInt(totals.unreported) + BigInt(totals.open) - BigInt(hold.amount) + BigInt(cost) > COUNTED_SPEND_MAX) {
    throw new RangeError("the cost would take the counted spend of its period above the largest integer that a number holds exactly");
  }
  const overrun = Math.max(0, cost - hold.amount);
  call.sql.exec(
    "UPDATE ledger_holds SET state = ?, ended_at = ?, ended_seq = ?, ended_by = ?, input_tokens = ?, output_tokens = ?, cost = ?, overrun = ? WHERE request_id = ?",
    "settled",
    call.now,
    nextSeq(call),
    "settle",
    usage.inputTokens,
    usage.outputTokens,
    cost,
    overrun,
    hold.requestId,
  );
  call.sql.exec("UPDATE ledger_periods SET open = open - ?, reported = reported + ? WHERE period_start = ?", hold.amount, cost, hold.periodStart);
  return { outcome: "settled", cost, overrun, repeat: false };
}

function settleExpired(call: Call, hold: Hold, usage: Usage): EndResult {
  if (hold.lateAt !== null) return { outcome: "expired", cost: hold.cost ?? rowBroken("cost"), overrun: hold.overrun };
  const cost = costOf(hold.price, usage);
  const overrun = Math.max(0, cost - hold.amount);
  call.sql.exec(
    "UPDATE ledger_holds SET late_at = ?, late_seq = ?, input_tokens = ?, output_tokens = ?, cost = ?, overrun = ? WHERE request_id = ?",
    call.now,
    nextSeq(call),
    usage.inputTokens,
    usage.outputTokens,
    cost,
    overrun,
    hold.requestId,
  );
  return { outcome: "expired", cost, overrun };
}

export function makeLedger(config: { holdLifeMs: number }): Ledger {
  const holdLifeMs = whole("holdLifeMs", config.holdLifeMs);

  return {
    init(sql) {
      sql.exec("CREATE TABLE ledger_state (seq INTEGER NOT NULL)");
      sql.exec("INSERT INTO ledger_state (seq) VALUES (?)", 0);
      sql.exec(
        "CREATE TABLE ledger_cap (amount INTEGER NOT NULL, period TEXT NOT NULL CHECK (period IN ('week', 'month')), anchor INTEGER NOT NULL, next_period TEXT CHECK (next_period IN ('week', 'month')), switch_at INTEGER)",
      );
      sql.exec(
        "CREATE TABLE ledger_periods (period_start INTEGER PRIMARY KEY, period_end INTEGER NOT NULL, reported INTEGER NOT NULL, unreported INTEGER NOT NULL, open INTEGER NOT NULL)",
      );
      sql.exec(
        "CREATE TABLE ledger_holds (request_id TEXT PRIMARY KEY, tier TEXT NOT NULL, amount INTEGER NOT NULL, input_price INTEGER NOT NULL, output_price INTEGER NOT NULL, period_start INTEGER NOT NULL, sync_interval INTEGER NOT NULL, created_at INTEGER NOT NULL, created_seq INTEGER NOT NULL, expires_at INTEGER NOT NULL, state TEXT NOT NULL CHECK (state IN ('open', 'settled', 'voided', 'expired')), dispatched INTEGER NOT NULL CHECK (dispatched IN (0, 1)), ended_at INTEGER, ended_seq INTEGER, ended_by TEXT, input_tokens INTEGER, output_tokens INTEGER, cost INTEGER, overrun INTEGER NOT NULL, late_at INTEGER, late_seq INTEGER, UNIQUE (state, expires_at, request_id), UNIQUE (sync_interval, request_id))",
      );
    },

    setCap(sql, cap: Cap, now, anchor) {
      within(sql, now, "setCap", (call) => {
        const stored = readCap(sql);
        if (stored === null) {
          sql.exec(
            "INSERT INTO ledger_cap (amount, period, anchor, next_period, switch_at) VALUES (?, ?, ?, ?, ?)",
            cap.amount,
            cap.period,
            anchor === undefined ? call.now : whole("anchor", anchor),
            null,
            null,
          );
        } else if (cap.period === stored.period) {
          sql.exec("UPDATE ledger_cap SET amount = ?, next_period = ?, switch_at = ?", cap.amount, null, null);
        } else {
          sql.exec(
            "UPDATE ledger_cap SET amount = ?, next_period = ?, switch_at = ?",
            cap.amount,
            cap.period,
            stored.switchAt ?? periodOf(stored, call.now).end,
          );
        }
      });
    },

    cap(sql, now): CapRead | null {
      return within(sql, now, "cap", (call) => {
        const stored = readCap(sql);
        if (stored === null) return null;
        const period = periodOf(stored, call.now);
        const totals = readTotals(sql, period.start) ?? { reported: 0, unreported: 0, open: 0 };
        return {
          cap: { amount: stored.amount, period: stored.nextPeriod ?? stored.period },
          spent: totals.reported + totals.unreported,
          held: totals.open,
          periodEnd: period.end,
        };
      });
    },

    reserve(sql, req, now, facts): ReserveResult {
      return within(sql, now, "reserve", (call): ReserveResult => {
        const existing = readHold(sql, req.requestId);
        if (existing !== null) {
          if (existing.tier !== req.tier || existing.amount !== req.amount || !samePrice(existing.price, req.price)) {
            return { outcome: "refused", reason: "other-arguments" };
          }
          return {
            outcome: "repeat",
            state: existing.state,
            dispatched: existing.dispatched,
            receipt: { requestId: existing.requestId, amount: existing.amount, now: call.now, expiresAt: existing.expiresAt },
          };
        }
        if (!facts.active) return { outcome: "refused", reason: "not-active" };
        const stored = readCap(sql);
        if (stored === null) return { outcome: "refused", reason: "no-cap" };
        const amount = whole("amount", req.amount);
        const price = { input: whole("price.input", req.price.input), output: whole("price.output", req.price.output) };
        const period = periodOf(stored, call.now);
        const totals = readTotals(sql, period.start);
        const counted = totals === null ? 0 : totals.reported + totals.unreported + totals.open;
        if (counted + amount > stored.amount * MICRO_PER_MINOR) return { outcome: "refused", reason: "over-cap" };
        if (totals === null) {
          sql.exec(
            "INSERT INTO ledger_periods (period_start, period_end, reported, unreported, open) VALUES (?, ?, ?, ?, ?)",
            period.start,
            period.end,
            0,
            0,
            0,
          );
        }
        sql.exec("UPDATE ledger_periods SET open = open + ? WHERE period_start = ?", amount, period.start);
        const expiresAt = call.now + holdLifeMs;
        sql.exec(
          "INSERT INTO ledger_holds (request_id, tier, amount, input_price, output_price, period_start, sync_interval, created_at, created_seq, expires_at, state, dispatched, overrun) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
          req.requestId,
          req.tier,
          amount,
          price.input,
          price.output,
          period.start,
          whole("interval", facts.interval),
          call.now,
          nextSeq(call),
          expiresAt,
          "open",
          0,
          0,
        );
        return { outcome: "admitted", receipt: { requestId: req.requestId, amount, now: call.now, expiresAt } };
      });
    },

    dispatch(sql, requestId, now) {
      return within(sql, now, "dispatch", () => {
        const hold = readHold(sql, requestId);
        if (hold === null || hold.state !== "open" || hold.dispatched) return "stop";
        sql.exec("UPDATE ledger_holds SET dispatched = ? WHERE request_id = ?", 1, requestId);
        return "go";
      });
    },

    settle(sql, requestId, usage, now): EndResult {
      return within(sql, now, "settle", (call): EndResult => {
        const hold = readHold(sql, requestId);
        if (hold === null) return { outcome: "refused", reason: "unknown-request" };
        switch (hold.state) {
          case "open":
            return settleOpen(call, hold, usage);
          case "settled":
            return { outcome: "settled", cost: hold.cost ?? rowBroken("cost"), overrun: hold.overrun, repeat: true };
          case "expired":
            return settleExpired(call, hold, usage);
          case "voided":
            return { outcome: "refused", reason: "already-ended" };
        }
      });
    },

    void(sql, requestId, now): EndResult {
      return within(sql, now, "void", (call): EndResult => {
        const hold = readHold(sql, requestId);
        if (hold === null) return { outcome: "refused", reason: "unknown-request" };
        switch (hold.state) {
          case "voided":
            return { outcome: "voided" };
          case "settled":
          case "expired":
            return { outcome: "refused", reason: "already-ended" };
          case "open":
            if (hold.dispatched) return { outcome: "refused", reason: "dispatched" };
            sql.exec(
              "UPDATE ledger_holds SET state = ?, ended_at = ?, ended_seq = ?, ended_by = ? WHERE request_id = ?",
              "voided",
              call.now,
              nextSeq(call),
              "void",
              requestId,
            );
            sql.exec("UPDATE ledger_periods SET open = open - ? WHERE period_start = ?", hold.amount, hold.periodStart);
            return { outcome: "voided" };
        }
      });
    },

    expire(sql, now) {
      return within(sql, now, "expire", (call) => call.expired);
    },

    intervalTotals(sql, interval) {
      const row =
        sql.exec(
          "SELECT COALESCE(SUM(CASE WHEN state = ? THEN cost END), 0) AS reported, COALESCE(SUM(CASE WHEN state = ? THEN amount END), 0) AS unreported, COUNT(CASE WHEN state = ? THEN 1 END) AS open FROM ledger_holds WHERE sync_interval = ?",
          "settled",
          "expired",
          "open",
          interval,
        )[0] ?? rowBroken("the interval totals");
      return { reported: int(row, "reported"), unreported: int(row, "unreported"), open: int(row, "open") };
    },

    openHolds(sql) {
      const row = sql.exec("SELECT COUNT(*) AS n FROM ledger_holds WHERE state = ?", "open")[0] ?? rowBroken("the count of open holds");
      return int(row, "n");
    },

    nextDue(sql) {
      const row = sql.exec("SELECT MIN(expires_at) AS at FROM ledger_holds WHERE state = ?", "open")[0] ?? rowBroken("the next expiry");
      return intOrNull(row, "at");
    },
  };
}
