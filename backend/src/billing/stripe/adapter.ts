import { URL_MAX_LENGTH } from "../../contract/schemas.ts";
import type { ProviderPort } from "../port.ts";
import { ACCOUNT_METADATA_KEY, completedSessionOf, ENDED_STATUSES, reduceEvent } from "./reduce.ts";
import { SIGNATURE_HEADER, signatureHolds } from "./signature.ts";
import { PROVIDER_CALL_TIMEOUT_MS } from "./timeout.ts";

export const API_VERSION = "2026-09-30.endive";
export const ERROR_TYPES: readonly string[] = ["api_error", "card_error", "idempotency_error", "invalid_request_error"];

export interface StripeConfig {
  baseUrl: string;
  apiKey: string;
  signingSecret: string;
  priceId: string;
}

type Answer = { status: number; body: unknown } | null;
type Fields = Record<string, unknown>;

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

export function makeStripePort(config: StripeConfig): ProviderPort {
  const ready = config.baseUrl !== "" && config.apiKey !== "";

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

    async cancelSubscription(subscriptionId) {
      if (subscriptionId === null) return { kind: "nothing-to-cancel" };
      if (!ready) return { kind: "failed" };
      const signal = deadline();
      const path = `/v1/subscriptions/${encodeURIComponent(subscriptionId)}`;
      const ended = (answer: Answer) =>
        succeeded(answer) && answer.body.id === subscriptionId && typeof answer.body.status === "string" && ENDED_STATUSES.includes(answer.body.status);
      const cancelled = await call("DELETE", path, [["invoice_now", "true"]], signal);
      if (cancelled === null) return { kind: "failed" };
      if (succeeded(cancelled)) return ended(cancelled) ? { kind: "cancelled" } : { kind: "failed" };
      const read = await call("GET", path, null, signal);
      if (isUnknownId(read) || ended(read)) return { kind: "nothing-to-cancel" };
      return { kind: "failed" };
    },

    async sendUsage() {
      return { kind: "failed" };
    },

    async readUsageTotal() {
      return { kind: "failed" };
    },
  };
}
