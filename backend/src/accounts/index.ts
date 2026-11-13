import type { CompletedSession, ProviderEvent, SubscriptionState } from "../billing/port.ts";
import type { BillingPeriod, Cap, Ms, Period, Sql } from "../types.ts";
import {
  type Accounts,
  type Applied,
  EVENT_ID_KEEP_MS,
  type Look,
  PENDING_DROP_MS,
  PENDING_KEEP_MS,
  type PendingCheckout,
  SESSION_LENGTH_MS,
  type Subscription,
} from "./api.ts";

export * from "./api.ts";

type Row = Record<string, string | number | null>;
type SubscriptionEvent = Extract<ProviderEvent, { kind: "subscription" }>;
type Made = Extract<Applied, { kind: "account-made" }>;

interface Standing {
  state: SubscriptionState;
  providerTime: Ms | null;
  periodEnd: Ms | null;
}

interface CancelTime {
  endedAt: Ms | null;
  first: Ms | null;
}

function keptTime(c: CancelTime): Ms | null {
  return c.endedAt ?? c.first;
}

function withCancel(c: CancelTime, event: SubscriptionEvent): CancelTime {
  if (event.state !== "cancelled") return c;
  const endedAt = c.endedAt === null || event.endedAt === null ? (c.endedAt ?? event.endedAt) : Math.min(c.endedAt, event.endedAt);
  return { endedAt, first: c.first === null ? event.providerTime : Math.min(c.first, event.providerTime) };
}

function cancelOf(row: Row): CancelTime {
  return { endedAt: wholeOrNull(row.ended_at), first: wholeOrNull(row.cancel_first) };
}

const RANK: Record<SubscriptionState, number> = { "payment-failed": 0, active: 1, cancelled: 2 };
const STATES: readonly string[] = ["active", "payment-failed", "cancelled"];
const PERIODS: readonly string[] = ["week", "month"];

function broken(): never {
  const error = new Error("the accounts tables hold what the module never writes");
  error.name = "ContractBrokenError";
  throw error;
}

function text(value: string | number | null | undefined): string {
  return typeof value === "string" ? value : broken();
}

function textOrNull(value: string | number | null | undefined): string | null {
  return value === null ? null : text(value);
}

function whole(value: string | number | null | undefined): number {
  return typeof value === "number" && Number.isInteger(value) ? value : broken();
}

function wholeOrNull(value: string | number | null | undefined): number | null {
  return value === null ? null : whole(value);
}

function stateOf(value: string | number | null | undefined): SubscriptionState {
  return typeof value === "string" && STATES.includes(value) ? (value as SubscriptionState) : broken();
}

function periodOf(start: string | number | null | undefined, end: string | number | null | undefined): BillingPeriod | null {
  const from = wholeOrNull(start);
  const to = wholeOrNull(end);
  if (from === null && to === null) return null;
  if (from === null || to === null) return broken();
  return { start: from, end: to };
}

function capOf(row: Row): Cap {
  const period = row.cap_period;
  if (typeof period !== "string" || !PERIODS.includes(period)) return broken();
  return { amount: whole(row.cap_amount), period: period as Period };
}

function isNewer(event: Standing, stored: Standing): boolean {
  if (event.providerTime === null) return broken();
  if (stored.providerTime === null) return true;
  if (event.providerTime !== stored.providerTime) return event.providerTime > stored.providerTime;
  if (event.state !== stored.state) return RANK[event.state] > RANK[stored.state];
  if (event.periodEnd === null) return false;
  return stored.periodEnd === null || event.periodEnd > stored.periodEnd;
}

function accountRow(sql: Sql): Row | null {
  const rows = sql.exec(
    "SELECT account_id, credential_hash, state, customer_id, subscription_id, period_start, period_end, provider_time, ended_at, cancel_first FROM account",
  );
  if (rows.length > 1) return broken();
  return rows[0] ?? null;
}

function pendingRow(sql: Sql): Row | null {
  const rows = sql.exec(
    "SELECT account_id, credential_hash, cap_amount, cap_period, session_id, url, expires_at, next_read FROM pending_checkout",
  );
  if (rows.length > 1) return broken();
  return rows[0] ?? null;
}

function applySubscription(sql: Sql, event: SubscriptionEvent): Applied {
  const incoming: Standing = { state: event.state, providerTime: event.providerTime, periodEnd: event.period?.end ?? null };
  const account = accountRow(sql);
  if (account !== null) {
    const subscriptionId = textOrNull(account.subscription_id);
    if (subscriptionId !== null && subscriptionId !== event.subscriptionId) return { kind: "none" };
    const was = stateOf(account.state);
    const stored = cancelOf(account);
    const cancel = withCancel(stored, event);
    if (!isNewer(incoming, { state: was, providerTime: wholeOrNull(account.provider_time), periodEnd: wholeOrNull(account.period_end) })) {
      if (cancel === stored) return { kind: "none" };
      sql.exec("UPDATE account SET ended_at = ?, cancel_first = ?", cancel.endedAt, cancel.first);
      const before = keptTime(stored);
      const after = keptTime(cancel);
      if (was === "cancelled" && after !== null && before !== null && after < before) return { kind: "cancel-time", at: after };
      return { kind: "none" };
    }
    sql.exec(
      "UPDATE account SET state = ?, subscription_id = ?, period_start = ?, period_end = ?, provider_time = ?, ended_at = ?, cancel_first = ?",
      event.state,
      event.subscriptionId,
      event.period?.start ?? null,
      event.period?.end ?? null,
      event.providerTime,
      cancel.endedAt,
      cancel.first,
    );
    const before = keptTime(stored);
    const after = keptTime(cancel);
    if (was === "cancelled" && event.state === "cancelled" && after !== null && before !== null && after < before) return { kind: "cancel-time", at: after };
    return { kind: "state", state: event.state, was, period: event.period };
  }
  const pending = pendingRow(sql);
  if (pending === null) return { kind: "none" };
  const kept = sql.exec("SELECT state, provider_time, period_end, ended_at, cancel_first FROM kept_event WHERE subscription_id = ?", event.subscriptionId)[0];
  if (kept !== undefined && !isNewer(incoming, { state: stateOf(kept.state), providerTime: whole(kept.provider_time), periodEnd: wholeOrNull(kept.period_end) })) {
    const stored = cancelOf(kept);
    const cancel = withCancel(stored, event);
    if (cancel !== stored) {
      sql.exec("UPDATE kept_event SET ended_at = ?, cancel_first = ? WHERE subscription_id = ?", cancel.endedAt, cancel.first, event.subscriptionId);
    }
    return { kind: "none" };
  }
  if (kept === undefined) {
    const cancel = withCancel({ endedAt: null, first: null }, event);
    sql.exec(
      "INSERT INTO kept_event (subscription_id, state, period_start, period_end, provider_time, kept_order, ended_at, cancel_first) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
      event.subscriptionId,
      event.state,
      event.period?.start ?? null,
      event.period?.end ?? null,
      event.providerTime,
      sql.exec("SELECT subscription_id FROM kept_event").length,
      cancel.endedAt,
      cancel.first,
    );
  } else {
    const cancel = withCancel(cancelOf(kept), event);
    sql.exec(
      "UPDATE kept_event SET state = ?, period_start = ?, period_end = ?, provider_time = ?, ended_at = ?, cancel_first = ? WHERE subscription_id = ?",
      event.state,
      event.period?.start ?? null,
      event.period?.end ?? null,
      event.providerTime,
      cancel.endedAt,
      cancel.first,
      event.subscriptionId,
    );
  }
  return { kind: "kept" };
}

function complete(sql: Sql, s: CompletedSession): Extract<Applied, { kind: "account-made" | "none" }> {
  if (accountRow(sql) !== null) return { kind: "none" };
  const pending = pendingRow(sql);
  if (pending === null || text(pending.session_id) !== s.sessionId) return { kind: "none" };
  const keptRows = sql.exec("SELECT subscription_id, state, period_start, period_end, provider_time, kept_order FROM kept_event");
  const kept =
    s.subscriptionId === null
      ? keptRows.reduce<Row | null>((first, row) => (first === null || whole(row.kept_order) < whole(first.kept_order) ? row : first), null)
      : (keptRows.find((row) => row.subscription_id === s.subscriptionId) ?? null);
  const state = kept !== null ? stateOf(kept.state) : s.payment === "paid" ? "active" : "payment-failed";
  const period = kept !== null ? periodOf(kept.period_start, kept.period_end) : null;
  const providerTime = kept !== null ? whole(kept.provider_time) : null;
  const subscriptionId = s.subscriptionId ?? (kept !== null ? text(kept.subscription_id) : null);
  const keptCancel = kept !== null ? sql.exec("SELECT ended_at, cancel_first FROM kept_event WHERE subscription_id = ?", text(kept.subscription_id))[0] : undefined;
  const cancel = keptCancel !== undefined ? cancelOf(keptCancel) : { endedAt: null, first: null };
  sql.exec("DELETE FROM kept_event");
  sql.exec("DELETE FROM pending_checkout");
  sql.exec(
    "INSERT INTO account (account_id, credential_hash, state, customer_id, subscription_id, period_start, period_end, provider_time, ended_at, cancel_first) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    text(pending.account_id),
    text(pending.credential_hash),
    state,
    s.customerId,
    subscriptionId,
    period?.start ?? null,
    period?.end ?? null,
    providerTime,
    cancel.endedAt,
    cancel.first,
  );
  const made: Made = { kind: "account-made", cap: capOf(pending), period };
  return made;
}

export function makeAccounts(): Accounts {
  return {
    init(sql) {
      sql.exec(
        "CREATE TABLE account (account_id TEXT NOT NULL, credential_hash TEXT NOT NULL, state TEXT NOT NULL, customer_id TEXT, subscription_id TEXT, period_start INTEGER, period_end INTEGER, provider_time INTEGER, ended_at INTEGER, cancel_first INTEGER)",
      );
      sql.exec(
        "CREATE TABLE pending_checkout (account_id TEXT NOT NULL, credential_hash TEXT NOT NULL, cap_amount INTEGER NOT NULL, cap_period TEXT NOT NULL, session_id TEXT NOT NULL, url TEXT NOT NULL, expires_at INTEGER NOT NULL, next_read INTEGER NOT NULL)",
      );
      sql.exec(
        "CREATE TABLE kept_event (subscription_id TEXT NOT NULL, state TEXT NOT NULL, period_start INTEGER, period_end INTEGER, provider_time INTEGER NOT NULL, kept_order INTEGER NOT NULL, ended_at INTEGER, cancel_first INTEGER)",
      );
      sql.exec("CREATE TABLE processed_event (event_id TEXT NOT NULL, received_at INTEGER NOT NULL)");
    },

    look(sql, credentialHash): Look {
      const held = accountRow(sql) ?? pendingRow(sql);
      if (held === null) return "nothing";
      if (text(held.credential_hash) !== credentialHash) return "mismatch";
      return accountRow(sql) === null ? "pending" : "account";
    },

    pending(sql): PendingCheckout | null {
      const row = pendingRow(sql);
      if (row === null) return null;
      return { sessionId: text(row.session_id), url: text(row.url), expiresAt: whole(row.expires_at), cap: capOf(row) };
    },

    subscription(sql): Subscription | null {
      const row = accountRow(sql);
      if (row === null) return null;
      const subscription: Subscription = {
        accountId: text(row.account_id),
        state: stateOf(row.state),
        customerId: textOrNull(row.customer_id),
        subscriptionId: textOrNull(row.subscription_id),
      };
      const cancelledAt = keptTime(cancelOf(row));
      if (subscription.state === "cancelled" && cancelledAt !== null) subscription.cancelledAt = cancelledAt;
      return subscription;
    },

    isActive(sql) {
      return accountRow(sql)?.state === "active";
    },

    keepPending(sql, p) {
      if (accountRow(sql) !== null || pendingRow(sql) !== null) return broken();
      sql.exec(
        "INSERT INTO pending_checkout (account_id, credential_hash, cap_amount, cap_period, session_id, url, expires_at, next_read) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        p.accountId,
        p.credentialHash,
        p.cap.amount,
        p.cap.period,
        p.sessionId,
        p.url,
        p.expiresAt,
        p.expiresAt,
      );
    },

    replacePendingCap(sql, cap) {
      if (pendingRow(sql) === null) return broken();
      sql.exec("UPDATE pending_checkout SET cap_amount = ?, cap_period = ?", cap.amount, cap.period);
    },

    applyEvent(sql, event, now) {
      if (event.kind === "ignored") return { kind: "none" };
      if (sql.exec("SELECT event_id FROM processed_event WHERE event_id = ?", event.eventId).length > 0) return { kind: "none" };
      sql.exec("INSERT INTO processed_event (event_id, received_at) VALUES (?, ?)", event.eventId, now);
      if (event.kind === "checkout-completed") {
        return complete(sql, {
          sessionId: event.sessionId,
          customerId: event.customerId,
          subscriptionId: event.subscriptionId,
          payment: event.payment,
        });
      }
      return applySubscription(sql, event);
    },

    complete,

    seed(sql, a) {
      if (accountRow(sql) !== null || pendingRow(sql) !== null) return broken();
      sql.exec(
        "INSERT INTO account (account_id, credential_hash, state, customer_id, subscription_id, period_start, period_end, provider_time, ended_at, cancel_first) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        a.accountId,
        a.credentialHash,
        a.state,
        a.customerId,
        null,
        null,
        null,
        null,
        null,
        null,
      );
    },

    nextDue(sql) {
      const times: Ms[] = [];
      const pending = pendingRow(sql);
      if (pending !== null) times.push(whole(pending.next_read));
      for (const row of sql.exec("SELECT received_at FROM processed_event")) times.push(whole(row.received_at) + EVENT_ID_KEEP_MS);
      return times.length === 0 ? null : Math.min(...times);
    },

    wake(sql, now) {
      const pending = pendingRow(sql);
      if (pending === null || whole(pending.next_read) > now) return null;
      return { sessionId: text(pending.session_id) };
    },

    record(sql, sessionId, read, now) {
      const pending = pendingRow(sql);
      if (pending === null || text(pending.session_id) !== sessionId) return "stale";
      const expiresAt = whole(pending.expires_at);
      if (read === "expired") return "expired-unpaid";
      if ((read === "open" || read === "unknown") && now >= expiresAt + PENDING_DROP_MS) return "dropped";
      if (read === "failed" && now >= expiresAt + PENDING_KEEP_MS) return "unanswered";
      sql.exec("UPDATE pending_checkout SET next_read = ?", now + SESSION_LENGTH_MS);
      return "kept";
    },

    forget(sql, now) {
      const before = now - EVENT_ID_KEEP_MS;
      const old = sql.exec("SELECT event_id FROM processed_event WHERE received_at <= ?", before);
      if (old.length > 0) sql.exec("DELETE FROM processed_event WHERE received_at <= ?", before);
      return old.length;
    },
  };
}
