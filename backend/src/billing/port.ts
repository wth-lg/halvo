import type { BillingPeriod, Minor, Ms, Seconds } from "../types.ts";

export type SubscriptionState = "active" | "payment-failed" | "cancelled";

export type ProviderEvent =
  | {
      kind: "checkout-completed";
      eventId: string;
      providerTime: Ms;
      accountId: string;
      sessionId: string;
      customerId: string;
      subscriptionId: string | null;
      payment: "paid" | "unpaid" | "no-payment-required";
    }
  | {
      kind: "subscription";
      eventId: string;
      providerTime: Ms;
      accountId: string;
      subscriptionId: string;
      customerId: string;
      state: SubscriptionState;
      period: BillingPeriod | null;
      endedAt: Ms | null;
    }
  | { kind: "ignored" };

export interface CompletedSession {
  sessionId: string;
  customerId: string;
  subscriptionId: string | null;
  payment: "paid" | "unpaid" | "no-payment-required";
}

export type UsageRefusal = "amount_not_whole_minor_units" | "timestamp_too_old" | "timestamp_in_future" | "unknown_account";

/**
 * The stored subscription id's state that the account object keeps between the tries of a delete (ruling P7-120,
 * Addition 1, N1 and N2): `unknown-once` after one counted unknown answer, `settled` when no try asks for it again.
 */
export type StoredIdState = "unsettled" | "unknown-once" | "settled";

export interface ProviderPort {
  startCheckout(a: {
    accountId: string;
    successUrl: string;
    expiresAt: Ms;
  }): Promise<{ kind: "session"; sessionId: string; url: string; expiresAt: Ms } | { kind: "failed" }>;
  verifyWebhook(rawBody: Uint8Array, headers: Headers, now: Ms): Promise<{ kind: "event"; event: ProviderEvent } | { kind: "rejected" }>;
  readSession(
    sessionId: string,
  ): Promise<
    | { kind: "open" }
    | { kind: "complete"; session: CompletedSession }
    | { kind: "expired" }
    | { kind: "unknown" }
    | { kind: "failed" }
  >;
  openManagePage(a: { customerId: string; returnUrl: string }): Promise<{ kind: "page"; url: string } | { kind: "failed" }>;
  /**
   * Ends every subscription of the customer that has not ended (contract 2): the stored id's cancel, read after a
   * refusal, unless `stored` is `settled`; one read of the customer; its list read page by page with each page's
   * cancels before the next page; and a whole list read after the last cancel (with its first page read again when it
   * had more than one). The call ends when the customer's read shows it deleted, or on that last whole list.
   * `stored` is what the object keeps for the stored id between tries (ruling P7-120, Addition 1, N1 and N2):
   * `settled` once a call ended it, read it ended, or got a second counted unknown answer for it in a later try than the
   * first; `unknown-once` after the first counted unknown answer, an answer that the provider does not know the id given
   * in a call whose read of the customer went on or showed the customer deleted; else `unsettled`.
   * `failed` carries `cancelled`, the listed subscriptions that this call ended and that its last whole list no longer
   * shows as not ended (a call that stops before a last whole list counts every cancel that it made); `unended`, the
   * subscriptions that this call read as not ended and could not end; and `stored`, the stored id's state after this call.
   */
  cancelSubscription(
    subscriptionId: string | null,
    customerId: string | null,
    stored: StoredIdState,
  ): Promise<{ kind: "cancelled" } | { kind: "nothing-to-cancel" } | { kind: "failed"; cancelled: number; unended: number; stored: StoredIdState }>;
  sendUsage(e: {
    accountId: string;
    customerId: string;
    identifier: string;
    amountMinor: Minor;
    timestamp: Seconds;
  }): Promise<{ kind: "recorded" } | { kind: "duplicate" } | { kind: "refused"; reason: UsageRefusal } | { kind: "failed" }>;
  readUsageTotal(a: {
    accountId: string;
    customerId: string;
    from: Seconds;
    to: Seconds;
  }): Promise<{ kind: "total"; from: Seconds; to: Seconds; totalMinor: Minor } | { kind: "failed" }>;
}
