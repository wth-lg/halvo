import type { CompletedSession, ProviderEvent, SubscriptionState } from "../billing/port.ts";
import type { BillingPeriod, Cap, Ms, Sql } from "../types.ts";

export const SESSION_LENGTH_MS = 1_860_000;
export const PENDING_DROP_MS = 90_000_000;
export const EVENT_ID_KEEP_MS = 2_592_000_000;
export const PENDING_KEEP_MS = EVENT_ID_KEEP_MS;
export type Look = "nothing" | "mismatch" | "pending" | "account";
export interface PendingCheckout {
  sessionId: string;
  url: string;
  expiresAt: Ms;
  cap: Cap;
}
export interface Subscription {
  accountId: string;
  state: SubscriptionState;
  customerId: string | null;
  subscriptionId: string | null;
  cancelledAt?: Ms;
}
export type Applied =
  | { kind: "account-made"; cap: Cap; period: BillingPeriod | null }
  | { kind: "state"; state: SubscriptionState; was: SubscriptionState; period: BillingPeriod | null; overrode: boolean }
  | { kind: "kept"; overrode: boolean }
  | { kind: "cancel-time"; at: Ms }
  | { kind: "none"; overrode: boolean };
export interface Accounts {
  init(sql: Sql): void;
  look(sql: Sql, credentialHash: string): Look;
  pending(sql: Sql): PendingCheckout | null;
  subscription(sql: Sql): Subscription | null;
  isActive(sql: Sql): boolean;
  keepPending(
    sql: Sql,
    p: { accountId: string; credentialHash: string; cap: Cap; sessionId: string; url: string; expiresAt: Ms },
    now: Ms,
  ): void;
  replacePendingCap(sql: Sql, cap: Cap): void;
  applyEvent(sql: Sql, event: ProviderEvent, now: Ms): Applied;
  complete(sql: Sql, s: CompletedSession): Extract<Applied, { kind: "account-made" | "none" }>;
  seed(sql: Sql, a: { accountId: string; credentialHash: string; state: SubscriptionState; customerId: string | null }, now: Ms): void;
  nextDue(sql: Sql): Ms | null;
  wake(sql: Sql, now: Ms): { sessionId: string } | null;
  record(
    sql: Sql,
    sessionId: string,
    read: "open" | "expired" | "unknown" | "failed",
    now: Ms,
  ): "expired-unpaid" | "dropped" | "unanswered" | "kept" | "stale";
  forget(sql: Sql, now: Ms): number;
}
