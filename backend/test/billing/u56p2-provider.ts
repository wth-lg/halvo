import type { StripeConfig } from "../../src/billing/stripe/adapter.ts";
import type { Recorded } from "./servers.ts";

export const KEY = "sk_test_halvolocal";
export const CUSTOMER = "cus_1";
export const MISSING = JSON.stringify({ error: { type: "invalid_request_error", code: "resource_missing", message: "No such object", param: "id" } });
export const REFUSED = JSON.stringify({ error: { type: "invalid_request_error", message: "refused" } });

export type Reply = { status: number; body: string } | "drop" | "hang";
export type Sub = { id: string; status: string; customer: string };

/**
 * A provider on a loopback recorder, scripted for the customer's list: `subs` in the order that the list gives them, a
 * cancel that ends a live subscription and refuses an ended one, `refused` cancels that the provider refuses while the
 * subscription stays live, `endsOnRefusal` cancels refused because the subscription ended at that moment, `dropped`
 * cancels that get no answer, `page` to change the answer of the list's n-th request, `onCancel` after each cancel that
 * went through, and `tick` before each request.
 */
export interface Model {
  subs: Sub[];
  customer?: Reply;
  refused?: string[];
  endsOnRefusal?: string[];
  dropped?: string[];
  page?: (index: number, reply: Reply) => Reply;
  onCancel?: (id: string, subs: Sub[]) => void;
  tick?: () => void;
}

export function json(value: unknown, status = 200): Reply {
  return { status, body: JSON.stringify(value) };
}

export function live(id: string, status = "active", customer = CUSTOMER): Sub {
  return { id, status, customer };
}

export function provider(model: Model): (request: Recorded) => Reply {
  let pages = 0;
  return (request) => {
    model.tick?.();
    const subscription = /^\/v1\/subscriptions\/([^/]+)$/.exec(request.path);
    if (subscription !== null) {
      const id = decodeURIComponent(subscription[1] as string);
      const found = model.subs.find((each) => each.id === id);
      if (request.method === "GET") return found === undefined ? { status: 404, body: MISSING } : json(found);
      if ((model.dropped ?? []).includes(id)) return "drop";
      if (found === undefined) return { status: 404, body: MISSING };
      if (found.status === "canceled" || found.status === "incomplete_expired") return { status: 400, body: REFUSED };
      if ((model.endsOnRefusal ?? []).includes(id)) {
        found.status = "canceled";
        return { status: 400, body: REFUSED };
      }
      if ((model.refused ?? []).includes(id)) return { status: 400, body: REFUSED };
      found.status = "canceled";
      model.onCancel?.(id, model.subs);
      return json(found);
    }
    if (request.path === `/v1/customers/${CUSTOMER}`) return model.customer ?? json({ id: CUSTOMER, object: "customer", livemode: false });
    if (request.path === "/v1/subscriptions") {
      const limit = Number(request.values.limit ?? "10");
      const after = request.values.starting_after;
      const from = after === undefined ? 0 : model.subs.findIndex((each) => each.id === after) + 1;
      const status = request.values.status;
      const shown = model.subs
        .slice(from)
        .filter((each) => each.customer === request.values.customer)
        .filter((each) => (status === undefined ? each.status !== "canceled" : status === "all" || each.status === status));
      const reply = json({ object: "list", url: "/v1/subscriptions", has_more: shown.length > limit, data: shown.slice(0, limit) });
      const index = pages;
      pages += 1;
      return model.page === undefined ? reply : model.page(index, reply);
    }
    return { status: 500, body: "" };
  };
}

export function config(baseUrl: string, over: Partial<StripeConfig> = {}): StripeConfig {
  return { baseUrl, apiKey: KEY, signingSecret: "whsec_test", priceId: "price_test", ...over };
}

export function line(request: Recorded): string {
  const after = request.values.starting_after;
  return `${request.method} ${request.path}${after === undefined ? "" : ` after ${after}`}`;
}
