import type { UsageRefusal } from "../billing/port.ts";
import type { BillingPeriod, Micro, Minor, Ms, Seconds, Sql } from "../types.ts";

export type UsageCall =
  | { kind: "send"; interval: Ms; identifier: string; amountMinor: Minor; timestamp: Seconds }
  | { kind: "read"; interval: Ms; from: Seconds; to: Seconds };
export type UsageAnswer =
  | { kind: "recorded" }
  | { kind: "duplicate" }
  | { kind: "refused"; reason: UsageRefusal }
  | { kind: "total"; from: Seconds; to: Seconds; totalMinor: Minor }
  | { kind: "failed" };
export const SEND_LIMIT_MS = 3_024_000_000;
export const REFUSAL_LIMIT = 3;
export const LATE_READ_SPAN_MS = 86_100_000;
export const RETRY_DELAY_CAP_MS = 3_600_000;
export interface Next {
  call: UsageCall | null;
  notBillable: number;
}
export interface Metering {
  init(sql: Sql): void;
  begin(sql: Sql, accountId: string, now: Ms): void;
  billingPeriod(sql: Sql, period: BillingPeriod, now: Ms): void;
  intervalAt(sql: Sql, now: Ms): Ms;
  cut(sql: Sql, kind: "cancel" | "delete", at: Ms, now: Ms): void;
  oldestUnsealed(sql: Sql): { interval: Ms; end: Ms } | null;
  seal(sql: Sql, interval: Ms, reported: Micro, now: Ms): void;
  send(sql: Sql, now: Ms): Next;
  record(sql: Sql, call: UsageCall, answer: UsageAnswer, now: Ms): Next;
  nextSend(sql: Sql): Ms | null;
  settled(sql: Sql): boolean;
}
