import { PROVIDER_CALL_TIMEOUT_MS } from "../billing/index.ts";
import type { ProviderPort, UsageRefusal } from "../billing/port.ts";
import { API_VERSION } from "../billing/stripe/adapter.ts";

export interface StripeMeterConfig {
  baseUrl: string;
  apiKey: string;
  meterId: string;
  eventName: string;
}

export type StripeMeter = Pick<ProviderPort, "sendUsage" | "readUsageTotal">;

type Answer = { status: number; body: unknown } | null;
type Fields = Record<string, unknown>;

const MINUTE = 60;
const LONGEST = 100;
const ANSWER_LIMIT = 1_048_576;
const METER_ID = /^[A-Za-z0-9_]{1,100}$/;
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

export const REFUSAL_OF_CODE: ReadonlyMap<string, UsageRefusal> = new Map([
  ["timestamp_too_far_in_past", "timestamp_too_old"],
  ["timestamp_in_future", "timestamp_in_future"],
  ["meter_event_customer_not_found", "unknown_account"],
  ["meter_event_invalid_value", "amount_not_whole_minor_units"],
  ["meter_event_value_too_many_digits", "amount_not_whole_minor_units"],
]);

function refusalOf(answer: { status: number; body: unknown }): UsageRefusal | null {
  if (answer.status !== 400 || !isFields(answer.body) || !isFields(answer.body.error)) return null;
  const { type, code } = answer.body.error;
  if (type !== "invalid_request_error" || typeof code !== "string") return null;
  return REFUSAL_OF_CODE.get(code) ?? null;
}

function isId(value: unknown): value is string {
  if (typeof value !== "string" || value === "") return false;
  try {
    encodeURIComponent(value);
    return true;
  } catch {
    return false;
  }
}

function isWhole(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value);
}

function isFields(value: unknown): value is Fields {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function bodyOf(response: Response): Promise<Uint8Array | null> {
  if (response.body === null) return new Uint8Array();
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > ANSWER_LIMIT) {
      reader.cancel().catch(() => undefined);
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.byteLength;
  }
  return bytes;
}

function parse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

export function makeStripeMeter(config: StripeMeterConfig): StripeMeter {
  function missingProviderValue(): boolean {
    return config.baseUrl === "" || config.apiKey === "" || !METER_ID.test(config.meterId) || config.eventName === "" || config.eventName.length > LONGEST;
  }

  async function call(method: "GET" | "POST", path: string, params: [string, string][]): Promise<Answer> {
    const signal = AbortSignal.timeout(PROVIDER_CALL_TIMEOUT_MS);
    try {
      const form = new URLSearchParams(params).toString();
      const headers: Record<string, string> = { authorization: `Bearer ${config.apiKey}`, "stripe-version": API_VERSION };
      if (method === "POST") headers["content-type"] = "application/x-www-form-urlencoded";
      const response = await fetch(method === "GET" ? `${config.baseUrl}${path}?${form}` : `${config.baseUrl}${path}`, {
        method,
        headers,
        body: method === "POST" ? form : undefined,
        signal,
        redirect: "manual",
      });
      const bytes = await bodyOf(response);
      if (bytes === null) return null;
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

  return {
    async sendUsage({ customerId, identifier, amountMinor, timestamp }) {
      if (missingProviderValue() || !isId(customerId) || !isId(identifier) || identifier.length > LONGEST) return { kind: "failed" };
      if (!isWhole(amountMinor) || amountMinor < 1) return { kind: "refused", reason: "amount_not_whole_minor_units" };
      if (!isWhole(timestamp)) return { kind: "refused", reason: "timestamp_too_old" };
      const answer = await call("POST", "/v1/billing/meter_events", [
        ["event_name", config.eventName],
        ["payload[stripe_customer_id]", customerId],
        ["payload[value]", String(amountMinor)],
        ["identifier", identifier],
        ["timestamp", String(timestamp)],
      ]);
      if (answer === null) return { kind: "failed" };
      if (answer.status >= 200 && answer.status < 300 && isFields(answer.body) && answer.body.object === "billing.meter_event" && answer.body.identifier === identifier) return { kind: "recorded" };
      const reason = refusalOf(answer);
      return reason === null ? { kind: "failed" } : { kind: "refused", reason };
    },

    async readUsageTotal({ customerId, from, to }) {
      if (missingProviderValue() || !isId(customerId)) return { kind: "failed" };
      if (!(Number.isSafeInteger(from) && Number.isSafeInteger(to) && from < to)) return { kind: "failed" };
      const start = Math.floor(from / MINUTE) * MINUTE;
      const end = Math.ceil(to / MINUTE) * MINUTE;
      if (!isWhole(start) || !isWhole(end)) return { kind: "failed" };
      const answer = await call("GET", `/v1/billing/meters/${config.meterId}/event_summaries`, [
        ["customer", customerId],
        ["start_time", String(start)],
        ["end_time", String(end)],
      ]);
      if (answer === null || answer.status < 200 || answer.status >= 300 || !isFields(answer.body)) return { kind: "failed" };
      if (answer.body.object !== "list" || Object.hasOwn(answer.body, "error")) return { kind: "failed" };
      const { data, has_more } = answer.body;
      if (has_more !== false || !Array.isArray(data) || data.length > 1) return { kind: "failed" };
      const [only] = data;
      if (only === undefined) return { kind: "total", from: start, to: end, totalMinor: 0 };
      if (!isFields(only) || only.meter !== config.meterId) return { kind: "failed" };
      const { start_time, end_time, aggregated_value } = only;
      if (!isWhole(aggregated_value)) return { kind: "failed" };
      if (start_time !== start || end_time !== end || aggregated_value < 0) return { kind: "failed" };
      return { kind: "total", from: start, to: end, totalMinor: aggregated_value };
    },
  };
}
