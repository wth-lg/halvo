import { DurableObject } from "cloudflare:workers";

export type Micro = number & { readonly __unit: "micro-units" };
export type EpochMs = number & { readonly __unit: "epoch-ms" };

export const micro = (value: number): Micro => {
  if (!Number.isSafeInteger(value) || value < 0) throw new RangeError(`not a micro-unit amount: ${value}`);
  return value as Micro;
};

export interface Price {
  readonly input: Micro;
  readonly output: Micro;
}
export interface Usage {
  readonly inputTokens: number;
  readonly outputTokens: number;
}

const TOKENS_PER_PRICE_UNIT = 1_000_000;
export const costOf = (price: Price, usage: Usage): Micro =>
  micro(Math.ceil((usage.inputTokens * price.input + usage.outputTokens * price.output) / TOKENS_PER_PRICE_UNIT));

export type Admission = "counts-open-holds" | "ignores-open-holds";
export type HoldState = "open" | "settled" | "voided" | "expired";

export interface Receipt {
  readonly requestId: string;
  readonly amount: Micro;
  readonly expiresAt: EpochMs;
  readonly periodStart: EpochMs;
}
export type ReserveResult =
  | { readonly outcome: "admitted"; readonly receipt: Receipt }
  | { readonly outcome: "repeat"; readonly state: HoldState; readonly receipt: Receipt }
  | { readonly outcome: "refused"; readonly reason: "no-cap" | "over-cap" | "other-arguments" };
export type EndResult =
  | { readonly outcome: "settled"; readonly cost: Micro; readonly overrun: Micro; readonly repeat: boolean }
  | { readonly outcome: "expired"; readonly cost: Micro; readonly overrun: Micro }
  | { readonly outcome: "voided" }
  | { readonly outcome: "refused"; readonly reason: "unknown-request" | "already-ended" };

type StepName = "reserve" | "settle" | "void" | "setCap" | "alarm";

type AccountRow = {
  cap: number | null;
  period_ms: number;
  anchor_ms: number;
  clock_ms: number;
  seq: number;
};
type PeriodRow = { period_start: number; reported: number; unreported: number; open: number };
type HoldRow = {
  request_id: string;
  period_start: number;
  amount: number;
  input_price: number;
  output_price: number;
  expires_at: number;
  state: HoldState;
  created_at: number;
  created_seq: number;
  ended_at: number | null;
  ended_seq: number | null;
  ended_by: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cost: number | null;
  overrun: number;
  late_at: number | null;
  late_seq: number | null;
};

export interface Snapshot {
  readonly now: number;
  readonly admission: Admission;
  readonly holdLifeMs: number;
  readonly account: AccountRow;
  readonly periods: PeriodRow[];
  readonly holds: HoldRow[];
  readonly alarm: number | null;
  readonly databaseSize: number;
}

export interface LedgerEnv {
  readonly HOLD_LIFE_MS: string;
  readonly ADMISSION: string;
}

const DEFAULT_PERIOD_MS = 7 * 24 * 60 * 60 * 1000;
const DEFAULT_ANCHOR_MS = 0;
const SCHEMA = [
  `CREATE TABLE IF NOT EXISTS account (
     id INTEGER PRIMARY KEY CHECK (id = 1),
     cap INTEGER,
     period_ms INTEGER NOT NULL,
     anchor_ms INTEGER NOT NULL,
     clock_ms INTEGER NOT NULL,
     seq INTEGER NOT NULL)`,
  `CREATE TABLE IF NOT EXISTS periods (
     period_start INTEGER PRIMARY KEY,
     reported INTEGER NOT NULL,
     unreported INTEGER NOT NULL,
     open INTEGER NOT NULL)`,
  `CREATE TABLE IF NOT EXISTS holds (
     request_id TEXT PRIMARY KEY,
     period_start INTEGER NOT NULL,
     amount INTEGER NOT NULL,
     input_price INTEGER NOT NULL,
     output_price INTEGER NOT NULL,
     expires_at INTEGER NOT NULL,
     state TEXT NOT NULL CHECK (state IN ('open', 'settled', 'voided', 'expired')),
     created_at INTEGER NOT NULL,
     created_seq INTEGER NOT NULL,
     ended_at INTEGER,
     ended_seq INTEGER,
     ended_by TEXT,
     input_tokens INTEGER,
     output_tokens INTEGER,
     cost INTEGER,
     overrun INTEGER NOT NULL DEFAULT 0,
     late_at INTEGER,
     late_seq INTEGER)`,
  `CREATE INDEX IF NOT EXISTS holds_open ON holds (expires_at) WHERE state = 'open'`,
  `INSERT OR IGNORE INTO account (id, cap, period_ms, anchor_ms, clock_ms, seq) VALUES (1, NULL, ${DEFAULT_PERIOD_MS}, ${DEFAULT_ANCHOR_MS}, 0, 0)`,
];

function parseAdmission(value: string): Admission {
  if (value === "counts-open-holds" || value === "ignores-open-holds") return value;
  throw new Error(`ADMISSION must be counts-open-holds or ignores-open-holds, not ${value}`);
}

function parseMs(name: string, value: string): number {
  const ms = Number(value);
  if (!Number.isSafeInteger(ms) || ms <= 0) throw new Error(`${name} must be a positive integer of milliseconds`);
  return ms;
}

export class Ledger extends DurableObject<LedgerEnv> {
  private readonly sql: SqlStorage;
  private readonly admission: Admission;
  private readonly holdLifeMs: number;

  constructor(ctx: DurableObjectState, env: LedgerEnv) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.admission = parseAdmission(env.ADMISSION);
    this.holdLifeMs = parseMs("HOLD_LIFE_MS", env.HOLD_LIFE_MS);
    ctx.storage.transactionSync(() => {
      for (const statement of SCHEMA) this.sql.exec(statement);
    });
  }

  async setCap(cap: Micro, period?: { readonly periodMs: number; readonly anchorMs: number }): Promise<void> {
    this.step("setCap", () => {
      micro(cap);
      this.sql.exec("UPDATE account SET cap = ? WHERE id = 1", cap);
      if (!period) return;
      if (!Number.isSafeInteger(period.periodMs) || period.periodMs <= 0 || !Number.isSafeInteger(period.anchorMs)) {
        throw new RangeError("the period needs a positive length and an integer anchor");
      }
      const holds = this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM holds").one().n;
      if (holds > 0) throw new Error("the spike sets the period only before the first hold");
      this.sql.exec("UPDATE account SET period_ms = ?, anchor_ms = ? WHERE id = 1", period.periodMs, period.anchorMs);
    });
    await this.armAlarmInSameTurn();
  }

  async reserve(requestId: string, amount: Micro, price: Price): Promise<ReserveResult> {
    const result = this.step("reserve", (now, seq): ReserveResult => {
      micro(amount);
      const existing = this.hold(requestId);
      if (existing) {
        const same =
          existing.amount === amount && existing.input_price === price.input && existing.output_price === price.output;
        if (!same) return { outcome: "refused", reason: "other-arguments" };
        return { outcome: "repeat", state: existing.state, receipt: receiptOf(existing) };
      }
      const account = this.account();
      if (account.cap === null) return { outcome: "refused", reason: "no-cap" };
      const periodStart =
        account.anchor_ms + Math.floor((now - account.anchor_ms) / account.period_ms) * account.period_ms;
      this.sql.exec(
        "INSERT OR IGNORE INTO periods (period_start, reported, unreported, open) VALUES (?, 0, 0, 0)",
        periodStart,
      );
      const period = this.sql.exec<PeriodRow>("SELECT * FROM periods WHERE period_start = ?", periodStart).one();
      const counted =
        period.reported + period.unreported + (this.admission === "counts-open-holds" ? period.open : 0);
      if (counted + amount > account.cap) return { outcome: "refused", reason: "over-cap" };
      const expiresAt = now + this.holdLifeMs;
      this.sql.exec(
        `INSERT INTO holds (request_id, period_start, amount, input_price, output_price, expires_at, state,
           created_at, created_seq) VALUES (?, ?, ?, ?, ?, ?, 'open', ?, ?)`,
        requestId,
        periodStart,
        amount,
        price.input,
        price.output,
        expiresAt,
        now,
        seq,
      );
      this.sql.exec("UPDATE periods SET open = open + ? WHERE period_start = ?", amount, periodStart);
      return {
        outcome: "admitted",
        receipt: { requestId, amount, expiresAt: expiresAt as EpochMs, periodStart: periodStart as EpochMs },
      };
    });
    await this.armAlarmInSameTurn();
    return result;
  }

  async settle(requestId: string, usage: Usage): Promise<EndResult> {
    const result = this.step("settle", (now, seq): EndResult => {
      const hold = this.hold(requestId);
      if (!hold) return { outcome: "refused", reason: "unknown-request" };
      const cost = costOf({ input: micro(hold.input_price), output: micro(hold.output_price) }, usage);
      const overrun = micro(Math.max(0, cost - hold.amount));
      switch (hold.state) {
        case "open":
          this.sql.exec(
            `UPDATE holds SET state = 'settled', ended_at = ?, ended_seq = ?, ended_by = 'settle',
               input_tokens = ?, output_tokens = ?, cost = ?, overrun = ? WHERE request_id = ?`,
            now,
            seq,
            usage.inputTokens,
            usage.outputTokens,
            cost,
            overrun,
            requestId,
          );
          this.sql.exec(
            "UPDATE periods SET open = open - ?, reported = reported + ? WHERE period_start = ?",
            hold.amount,
            cost,
            hold.period_start,
          );
          return { outcome: "settled", cost, overrun, repeat: false };
        case "settled":
          return { outcome: "settled", cost: micro(hold.cost ?? 0), overrun: micro(hold.overrun), repeat: true };
        case "expired":
          if (hold.late_at !== null) {
            return { outcome: "expired", cost: micro(hold.cost ?? 0), overrun: micro(hold.overrun) };
          }
          this.sql.exec(
            `UPDATE holds SET late_at = ?, late_seq = ?, input_tokens = ?, output_tokens = ?, cost = ?, overrun = ?
               WHERE request_id = ?`,
            now,
            seq,
            usage.inputTokens,
            usage.outputTokens,
            cost,
            overrun,
            requestId,
          );
          this.sql.exec(
            "UPDATE periods SET reported = reported + ? WHERE period_start = ?",
            overrun,
            hold.period_start,
          );
          return { outcome: "expired", cost, overrun };
        case "voided":
          return { outcome: "refused", reason: "already-ended" };
      }
    });
    await this.armAlarmInSameTurn();
    return result;
  }

  async void(requestId: string): Promise<EndResult> {
    const result = this.step("void", (now, seq): EndResult => {
      const hold = this.hold(requestId);
      if (!hold) return { outcome: "refused", reason: "unknown-request" };
      if (hold.state === "voided") return { outcome: "voided" };
      if (hold.state !== "open") return { outcome: "refused", reason: "already-ended" };
      this.sql.exec(
        "UPDATE holds SET state = 'voided', ended_at = ?, ended_seq = ?, ended_by = 'void' WHERE request_id = ?",
        now,
        seq,
        requestId,
      );
      this.sql.exec("UPDATE periods SET open = open - ? WHERE period_start = ?", hold.amount, hold.period_start);
      return { outcome: "voided" };
    });
    await this.armAlarmInSameTurn();
    return result;
  }

  override async alarm(): Promise<void> {
    this.step("alarm", () => undefined);
    await this.armAlarmInSameTurn();
  }

  async inspect(): Promise<Snapshot> {
    return {
      now: Date.now(),
      admission: this.admission,
      holdLifeMs: this.holdLifeMs,
      account: this.account(),
      periods: this.sql.exec<PeriodRow>("SELECT * FROM periods ORDER BY period_start").toArray(),
      holds: this.sql.exec<HoldRow>("SELECT * FROM holds ORDER BY created_seq").toArray(),
      alarm: await this.ctx.storage.getAlarm(),
      databaseSize: this.sql.databaseSize,
    };
  }

  private step<T>(by: StepName, work: (now: EpochMs, seq: number) => T): T {
    return this.ctx.storage.transactionSync(() => {
      const account = this.account();
      const now = Math.max(Date.now(), account.clock_ms) as EpochMs;
      const seq = account.seq + 1;
      this.sql.exec("UPDATE account SET clock_ms = ?, seq = ? WHERE id = 1", now, seq);
      const due = this.sql
        .exec<Pick<HoldRow, "request_id" | "amount" | "period_start">>(
          "SELECT request_id, amount, period_start FROM holds WHERE state = 'open' AND expires_at <= ?",
          now,
        )
        .toArray();
      for (const hold of due) {
        this.sql.exec(
          "UPDATE holds SET state = 'expired', ended_at = ?, ended_seq = ?, ended_by = ? WHERE request_id = ?",
          now,
          seq,
          by,
          hold.request_id,
        );
        this.sql.exec(
          "UPDATE periods SET open = open - ?, unreported = unreported + ? WHERE period_start = ?",
          hold.amount,
          hold.amount,
          hold.period_start,
        );
      }
      return work(now, seq);
    });
  }

  private async armAlarmInSameTurn(): Promise<void> {
    const next = this.sql
      .exec<{ at: number | null }>("SELECT MIN(expires_at) AS at FROM holds WHERE state = 'open'")
      .one().at;
    if (next === null) await this.ctx.storage.deleteAlarm();
    else await this.ctx.storage.setAlarm(next);
  }

  private account(): AccountRow {
    return this.sql
      .exec<AccountRow>("SELECT cap, period_ms, anchor_ms, clock_ms, seq FROM account WHERE id = 1")
      .one();
  }

  private hold(requestId: string): HoldRow | undefined {
    return this.sql.exec<HoldRow>("SELECT * FROM holds WHERE request_id = ?", requestId).toArray()[0];
  }
}

const receiptOf = (hold: HoldRow): Receipt => ({
  requestId: hold.request_id,
  amount: micro(hold.amount),
  expiresAt: hold.expires_at as EpochMs,
  periodStart: hold.period_start as EpochMs,
});
