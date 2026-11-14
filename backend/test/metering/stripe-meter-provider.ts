import { FakeMeter, type FakeMeterConfig, type RefusalReason } from "../../test-kit/fake-meter/fake-meter.ts";
import type { Recorded } from "../billing/servers.ts";

export const PROVIDER_CODE_OF: Record<RefusalReason, string> = {
  amount_not_whole_minor_units: "meter_event_invalid_value",
  timestamp_too_old: "timestamp_too_far_in_past",
  timestamp_in_future: "timestamp_in_future",
  unknown_account: "meter_event_customer_not_found",
};

export const METER_CONFIG: FakeMeterConfig = {
  uniquenessWindowSeconds: 24 * 3600,
  duplicateAnswer: "same-as-recorded",
  graceSeconds: 3600,
  invoicePeriodSeconds: 30 * 24 * 3600,
  readLagSeconds: 0,
  readGranularitySeconds: 60,
};

function refusal(message: string, code?: string) {
  return { status: 400, body: JSON.stringify({ error: { type: "invalid_request_error", message, ...(code === undefined ? {} : { code }) } }) };
}

function whole(text: string | undefined): number {
  return text !== undefined && /^-?[0-9]+$/.test(text) ? Number(text) : Number.NaN;
}

export function providerOver(meter: FakeMeter, clock: { now: number }, names: { meterId: string; eventName: string }) {
  return (request: Recorded): { status: number; body: string } => {
    const values = request.values;
    if (request.method === "POST" && request.path === "/v1/billing/meter_events") {
      if (values.event_name !== names.eventName) return refusal("no meter has this event name");
      const event = {
        account: values["payload[stripe_customer_id]"] ?? "",
        identifier: values.identifier ?? "",
        amountMinor: whole(values["payload[value]"]),
        timestamp: whole(values.timestamp),
      };
      const result = meter.send(event, clock.now);
      if (result.status === "refused") return refusal(result.reason, PROVIDER_CODE_OF[result.reason]);
      if (result.status === "duplicate") return refusal("an event with this identifier exists");
      return {
        status: 200,
        body: JSON.stringify({
          object: "billing.meter_event",
          created: clock.now,
          event_name: values.event_name,
          identifier: event.identifier,
          livemode: false,
          payload: { stripe_customer_id: event.account, value: values["payload[value]"] },
          timestamp: event.timestamp,
        }),
      };
    }
    const summaries = /^\/v1\/billing\/meters\/([^/]+)\/event_summaries$/.exec(request.path);
    if (request.method === "GET" && summaries !== null) {
      if (decodeURIComponent(summaries[1] ?? "") !== names.meterId) {
        return { status: 404, body: JSON.stringify({ error: { type: "invalid_request_error", code: "resource_missing", message: "No such meter" } }) };
      }
      const from = whole(values.start_time);
      const to = whole(values.end_time);
      if (!(Number.isSafeInteger(from) && Number.isSafeInteger(to) && from < to && from % 60 === 0 && to % 60 === 0)) return refusal("not a range of whole minutes");
      const read = meter.read(values.customer ?? "", from, to, clock.now);
      return {
        status: 200,
        body: JSON.stringify({
          object: "list",
          data: [
            {
              id: "mtrusg_test_1",
              object: "billing.meter_event_summary",
              aggregated_value: read.totalMinor,
              start_time: read.widenedFrom,
              end_time: read.widenedTo,
              livemode: false,
              meter: names.meterId,
            },
          ],
          has_more: false,
          url: request.path,
        }),
      };
    }
    return { status: 404, body: JSON.stringify({ error: { type: "invalid_request_error", message: "Unrecognized request URL" } }) };
  };
}

export function fakeMeter(config: Partial<FakeMeterConfig> = {}): FakeMeter {
  return new FakeMeter({ ...METER_CONFIG, ...config });
}
