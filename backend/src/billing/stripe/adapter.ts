import { URL_MAX_LENGTH } from "../../contract/schemas.ts";
import { log } from "../../log.ts";
import type { ProviderPort } from "../port.ts";
import { ACCOUNT_METADATA_KEY, completedSessionOf, ENDED_STATUSES, reduceEvent } from "./reduce.ts";
import { SIGNATURE_HEADER, signatureHolds } from "./signature.ts";
import { PROVIDER_CALL_TIMEOUT_MS } from "./timeout.ts";

export const API_VERSION = "2026-09-30.endive";
export const ERROR_TYPES: readonly string[] = ["api_error", "card_error", "idempotency_error", "invalid_request_error"];
/** The most requests that one call of `cancelSubscription` makes (contract 3.6). */
export const CALL_REQUEST_LIMIT = 20;
/** No request of `cancelSubscription` starts with less than this much of the call's one deadline left (contract 3.6). */
export const REQUEST_START_MARGIN_MS = 500;
/** The page size of the customer's list of subscriptions. */
export const LIST_PAGE_SIZE = 100;

export interface StripeConfig {
  baseUrl: string;
  apiKey: string;
  signingSecret: string;
  priceId: string;
}

type Answer = { status: number; body: unknown } | null;
type Fields = Record<string, unknown>;
type Cancel = Awaited<ReturnType<ProviderPort["cancelSubscription"]>>;
type CancelOutcome = "ended" | "needs-no-cancel" | "unanswered" | "stopped";
type CustomerRead = "goes-on" | "deleted" | "unknown" | "fault";
type Page = { entries: { id: string; status: string }[]; hasMore: boolean };
type ListRead = { kind: "whole"; notEnded: number } | { kind: "fault" } | { kind: "stopped" };

const LIVE_KEY = /^[rs]k_live_/;
const TEST_KEY = /^[rs]k_test_/;

const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

function isFields(value: unknown): value is Fields {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function providerErrorOf(answer: Answer): Fields | null {
  if (answer === null || !isFields(answer.body) || !isFields(answer.body.error)) return null;
  const type = answer.body.error.type;
  return typeof type === "string" && ERROR_TYPES.includes(type) ? answer.body.error : null;
}

function isUnknownId(answer: Answer): boolean {
  return answer !== null && answer.status === 404 && providerErrorOf(answer)?.code === "resource_missing";
}

function succeeded(answer: Answer): answer is { status: number; body: Fields } {
  return answer !== null && answer.status >= 200 && answer.status < 300 && isFields(answer.body);
}

function usableUrl(value: unknown): string | null {
  return typeof value === "string" && value !== "" && value.length <= URL_MAX_LENGTH ? value : null;
}

function parse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function endedSubscription(answer: Answer, subscriptionId: string): boolean {
  return (
    succeeded(answer) && answer.body.id === subscriptionId && typeof answer.body.status === "string" && ENDED_STATUSES.includes(answer.body.status)
  );
}

function keyIsLive(apiKey: string): boolean | null {
  if (LIVE_KEY.test(apiKey)) return true;
  return TEST_KEY.test(apiKey) ? false : null;
}

function customerRead(answer: Answer, customerId: string, live: boolean | null): CustomerRead {
  if (answer === null) return "fault";
  if (answer.status === 404) return "unknown";
  if (!succeeded(answer) || answer.body.id !== customerId || answer.body.object !== "customer") return "fault";
  if (answer.body.deleted === true) return "deleted";
  if (typeof answer.body.livemode !== "boolean") return "fault";
  return answer.body.livemode === live ? "goes-on" : "unknown";
}

function pageOf(answer: Answer, customerId: string): Page | null {
  if (!succeeded(answer) || answer.body.object !== "list" || !Array.isArray(answer.body.data) || typeof answer.body.has_more !== "boolean") return null;
  const entries: Page["entries"] = [];
  for (const entry of answer.body.data) {
    if (!isFields(entry) || typeof entry.id !== "string" || entry.id === "" || typeof entry.status !== "string" || entry.customer !== customerId) return null;
    entries.push({ id: entry.id, status: entry.status });
  }
  if (answer.body.has_more && entries.length === 0) return null;
  return { entries, hasMore: answer.body.has_more };
}

/**
 * The Stripe adapter of the provider port. `clock` is the time that the bound of one cancel call reads: the platform's
 * clock unless a test gives another.
 */
export function makeStripePort(config: StripeConfig, clock: () => number = () => Date.now()): ProviderPort {
  const ready = config.baseUrl !== "" && config.apiKey !== "";
  const live = keyIsLive(config.apiKey);

  async function call(method: "GET" | "POST" | "DELETE", path: string, params: [string, string][] | null, signal: AbortSignal): Promise<Answer> {
    const headers: Record<string, string> = { authorization: `Bearer ${config.apiKey}`, "stripe-version": API_VERSION };
    if (params !== null) headers["content-type"] = "application/x-www-form-urlencoded";
    try {
      const response = await fetch(`${config.baseUrl}${path}`, {
        method,
        headers,
        body: params === null ? undefined : new URLSearchParams(params).toString(),
        signal,
      });
      const bytes = new Uint8Array(await response.arrayBuffer());
      let text: string;
      try {
        text = decoder.decode(bytes);
      } catch {
        text = "";
      }
      return { status: response.status, body: parse(text) };
    } catch {
      return null;
    }
  }

  const deadline = () => AbortSignal.timeout(PROVIDER_CALL_TIMEOUT_MS);

  async function cancelAll(subscriptionId: string | null, customerId: string | null): Promise<Cancel> {
    const endsAt = clock() + PROVIDER_CALL_TIMEOUT_MS;
    const signal = deadline();
    let made = 0;
    let counted = 0;
    let endedOne = false;
    let storedUnanswered = false;
    let listedCancel = false;
    const failed = (): Cancel => ({ kind: "failed", cancelled: counted });

    async function ask(method: "GET" | "DELETE", path: string, params: [string, string][] | null): Promise<Answer | "stopped"> {
      if (made >= CALL_REQUEST_LIMIT || endsAt - clock() < REQUEST_START_MARGIN_MS) return "stopped";
      made += 1;
      return call(method, path, params, signal);
    }

    async function cancelOne(id: string): Promise<CancelOutcome> {
      const path = `/v1/subscriptions/${encodeURIComponent(id)}`;
      const cancelled = await ask("DELETE", path, [["invoice_now", "true"]]);
      if (cancelled === "stopped") return "stopped";
      if (cancelled === null) return "unanswered";
      if (succeeded(cancelled)) return endedSubscription(cancelled, id) ? "ended" : "unanswered";
      const read = await ask("GET", path, null);
      if (read === "stopped") return "stopped";
      return isUnknownId(read) || endedSubscription(read, id) ? "needs-no-cancel" : "unanswered";
    }

    async function readList(customer: string, cancelling: boolean): Promise<ListRead> {
      let after: string | null = null;
      let notEnded = 0;
      for (;;) {
        const query: [string, string][] = [
          ["customer", customer],
          ["limit", String(LIST_PAGE_SIZE)],
        ];
        if (after !== null) query.push(["starting_after", after]);
        const answer = await ask("GET", `/v1/subscriptions?${new URLSearchParams(query).toString()}`, null);
        if (answer === "stopped") return { kind: "stopped" };
        const page = pageOf(answer, customer);
        if (page === null) {
          log({ event: "cancel-list-fault" });
          return { kind: "fault" };
        }
        for (const entry of page.entries) {
          if (ENDED_STATUSES.includes(entry.status)) continue;
          notEnded += 1;
          if (!cancelling) continue;
          listedCancel = true;
          const outcome = await cancelOne(entry.id);
          if (outcome === "stopped") return { kind: "stopped" };
          if (outcome === "ended") {
            counted += 1;
            endedOne = true;
          }
        }
        const last = page.entries.at(-1);
        if (!page.hasMore || last === undefined) return { kind: "whole", notEnded };
        after = last.id;
      }
    }

    const end = (): Cancel => {
      if (storedUnanswered) return failed();
      return endedOne ? { kind: "cancelled" } : { kind: "nothing-to-cancel" };
    };

    if (subscriptionId !== null) {
      const outcome = await cancelOne(subscriptionId);
      if (outcome === "stopped") return failed();
      endedOne = outcome === "ended";
      storedUnanswered = outcome === "unanswered";
    }
    if (customerId === null) return failed();
    const customer = await ask("GET", `/v1/customers/${encodeURIComponent(customerId)}`, null);
    if (customer === "stopped") return failed();
    const read = customerRead(customer, customerId, live);
    if (read === "unknown") log({ event: "cancel-customer-unknown" });
    if (read === "unknown" || read === "fault") return failed();
    if (read === "deleted") return end();
    const walked = await readList(customerId, true);
    if (walked.kind !== "whole") return failed();
    if (!listedCancel) return end();
    const last = await readList(customerId, false);
    return last.kind === "whole" && last.notEnded === 0 ? end() : failed();
  }

  return {
    async startCheckout({ accountId, successUrl, expiresAt }) {
      if (!ready || config.priceId === "") return { kind: "failed" };
      const answer = await call(
        "POST",
        "/v1/checkout/sessions",
        [
          ["mode", "subscription"],
          ["line_items[0][price]", config.priceId],
          ["success_url", successUrl],
          ["client_reference_id", accountId],
          [`subscription_data[metadata][${ACCOUNT_METADATA_KEY}]`, accountId],
          ["expires_at", String(Math.ceil(expiresAt / 1000))],
          ["allowed_payment_method_types[0]", "card"],
        ],
        deadline(),
      );
      if (!succeeded(answer)) return { kind: "failed" };
      const sessionId = answer.body.id;
      const url = usableUrl(answer.body.url);
      const stored = answer.body.expires_at;
      if (typeof sessionId !== "string" || sessionId === "" || url === null || typeof stored !== "number" || !Number.isSafeInteger(stored)) {
        return { kind: "failed" };
      }
      return { kind: "session", sessionId, url, expiresAt: stored * 1000 };
    },

    async verifyWebhook(rawBody, headers, now) {
      if (!(await signatureHolds(rawBody, headers.get(SIGNATURE_HEADER), config.signingSecret, now))) return { kind: "rejected" };
      let text: string;
      try {
        text = decoder.decode(rawBody);
      } catch {
        return { kind: "rejected" };
      }
      const event = reduceEvent(parse(text));
      return event === null ? { kind: "rejected" } : { kind: "event", event };
    },

    async readSession(sessionId) {
      if (!ready) return { kind: "failed" };
      const answer = await call("GET", `/v1/checkout/sessions/${encodeURIComponent(sessionId)}`, null, deadline());
      if (isUnknownId(answer)) return { kind: "unknown" };
      if (!succeeded(answer) || answer.body.id !== sessionId) return { kind: "failed" };
      if (answer.body.status === "open") return { kind: "open" };
      if (answer.body.status === "expired") return { kind: "expired" };
      if (answer.body.status !== "complete") return { kind: "failed" };
      const session = completedSessionOf(answer.body);
      return session === null ? { kind: "failed" } : { kind: "complete", session };
    },

    async openManagePage({ customerId, returnUrl }) {
      if (!ready) return { kind: "failed" };
      const answer = await call(
        "POST",
        "/v1/billing_portal/sessions",
        [
          ["customer", customerId],
          ["return_url", returnUrl],
        ],
        deadline(),
      );
      const url = succeeded(answer) ? usableUrl(answer.body.url) : null;
      return url === null ? { kind: "failed" } : { kind: "page", url };
    },

    async cancelSubscription(subscriptionId, customerId) {
      if (subscriptionId === null && customerId === null) return { kind: "nothing-to-cancel" };
      if (!ready) return { kind: "failed", cancelled: 0 };
      return cancelAll(subscriptionId, customerId);
    },

    async sendUsage() {
      return { kind: "failed" };
    },

    async readUsageTotal() {
      return { kind: "failed" };
    },
  };
}
