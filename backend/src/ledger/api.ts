import type { Cap, Micro, Ms, Sql, Tier } from "../types.ts";

export interface Price {
  input: Micro;
  output: Micro;
}
export interface Usage {
  inputTokens: number;
  outputTokens: number;
}
export type HoldState = "open" | "settled" | "voided" | "expired";
export interface Receipt {
  requestId: string;
  amount: Micro;
  now: Ms;
  expiresAt: Ms;
}
export interface CapRead {
  cap: Cap;
  spent: Micro;
  held: Micro;
  periodEnd: Ms;
}
export type ReserveResult =
  | { outcome: "admitted"; receipt: Receipt }
  | { outcome: "repeat"; state: HoldState; dispatched: boolean; receipt: Receipt }
  | { outcome: "refused"; reason: "not-active" | "no-cap" | "over-cap" | "other-arguments" };
export type EndResult =
  | { outcome: "settled"; cost: Micro; overrun: Micro; repeat: boolean }
  | { outcome: "expired"; cost: Micro; overrun: Micro }
  | { outcome: "voided" }
  | { outcome: "refused"; reason: "unknown-request" | "already-ended" | "dispatched" };
export interface Ledger {
  init(sql: Sql): void;
  setCap(sql: Sql, cap: Cap, now: Ms, anchor?: Ms): void;
  cap(sql: Sql, now: Ms): CapRead | null;
  reserve(
    sql: Sql,
    req: { requestId: string; tier: Tier; amount: Micro; price: Price },
    now: Ms,
    facts: { active: boolean; interval: Ms },
  ): ReserveResult;
  dispatch(sql: Sql, requestId: string, now: Ms): "go" | "stop";
  settle(sql: Sql, requestId: string, usage: Usage, now: Ms): EndResult;
  void(sql: Sql, requestId: string, now: Ms): EndResult;
  expire(sql: Sql, now: Ms): number;
  intervalTotals(sql: Sql, interval: Ms): { reported: Micro; unreported: Micro; open: number };
  openHolds(sql: Sql): number;
  nextDue(sql: Sql): Ms | null;
}
