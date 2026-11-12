import { routeTable } from "../../contract/routes.ts";
import type { BillingPeriod, Ms } from "../../types.ts";
import type { CompletedSession, ProviderEvent, SubscriptionState } from "../port.ts";

export const ACCOUNT_METADATA_KEY = "halvo_account";
export const COMPLETED_TYPE = "checkout.session.completed";
export const SUBSCRIPTION_TYPES: readonly string[] = [
  "customer.subscription.created",
  "customer.subscription.updated",
  "customer.subscription.deleted",
];
export const ENDED_STATUSES: readonly string[] = ["canceled", "incomplete_expired"];

const ACCOUNT_PATTERN = new RegExp(routeTable.headers["halvo-account"].pattern);
const PAYMENTS = { paid: "paid", unpaid: "unpaid", no_payment_required: "no-payment-required" } as const;

type Fields = Record<string, unknown>;

function isFields(value: unknown): value is Fields {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isWhole(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function nonEmpty(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

function idOf(value: unknown): string | null {
  if (isFields(value)) return nonEmpty(value.id);
  return nonEmpty(value);
}

function accountIdOf(value: unknown): string | null {
  return typeof value === "string" && ACCOUNT_PATTERN.test(value) ? value : null;
}

export function stateOfStatus(status: string): SubscriptionState {
  if (status === "active") return "active";
  if (ENDED_STATUSES.includes(status)) return "cancelled";
  return "payment-failed";
}

function periodOf(subscription: Fields): BillingPeriod | null {
  const items = isFields(subscription.items) ? subscription.items.data : undefined;
  const first: unknown = Array.isArray(items) ? items[0] : undefined;
  if (!isFields(first) || !isWhole(first.current_period_start) || !isWhole(first.current_period_end)) return null;
  return { start: first.current_period_start * 1000, end: first.current_period_end * 1000 };
}

export function completedSessionOf(session: unknown): CompletedSession | null {
  if (!isFields(session)) return null;
  const sessionId = nonEmpty(session.id);
  const customerId = idOf(session.customer);
  const subscriptionId = session.subscription === null ? null : idOf(session.subscription);
  const payment = typeof session.payment_status === "string" && Object.hasOwn(PAYMENTS, session.payment_status)
    ? PAYMENTS[session.payment_status as keyof typeof PAYMENTS]
    : null;
  if (sessionId === null || customerId === null || payment === null) return null;
  if (session.subscription !== null && subscriptionId === null) return null;
  return { sessionId, customerId, subscriptionId, payment };
}

export function reduceEvent(value: unknown): ProviderEvent | null {
  if (!isFields(value) || !isFields(value.data) || !isFields(value.data.object)) return null;
  const eventId = nonEmpty(value.id);
  const type = nonEmpty(value.type);
  if (eventId === null || type === null || !isWhole(value.created)) return null;
  const providerTime: Ms = value.created * 1000;
  const object = value.data.object;
  if (type === COMPLETED_TYPE) {
    const accountId = accountIdOf(object.client_reference_id);
    const session = completedSessionOf(object);
    if (accountId === null || session === null) return { kind: "ignored" };
    return { kind: "checkout-completed", eventId, providerTime, accountId, ...session };
  }
  if (SUBSCRIPTION_TYPES.includes(type)) {
    const accountId = isFields(object.metadata) ? accountIdOf(object.metadata[ACCOUNT_METADATA_KEY]) : null;
    const subscriptionId = nonEmpty(object.id);
    const customerId = idOf(object.customer);
    const status = nonEmpty(object.status);
    if (accountId === null || subscriptionId === null || customerId === null || status === null) return { kind: "ignored" };
    return {
      kind: "subscription",
      eventId,
      providerTime,
      accountId,
      subscriptionId,
      customerId,
      state: stateOfStatus(status),
      period: periodOf(object),
    };
  }
  return { kind: "ignored" };
}
