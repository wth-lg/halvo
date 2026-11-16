import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import type { Ms } from "../../src/types.ts";
import { ACCOUNT, CUSTOMER, DAY, HASH, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

export const MINUTE = 60_000;
const DEADLINE = 10_000;
const MISSING = { error: { type: "invalid_request_error", code: "resource_missing", message: "No such object", param: "id" } };
const REFUSED = { error: { type: "invalid_request_error", message: "refused" } };

/** One request that the stand-in provider was asked, with the try (from 1) that asked it. */
export interface Asked {
  attempt: number;
  method: string;
  path: string;
  after: string | null;
}

/**
 * A customer at a provider behind the real Stripe adapter inside the real account object. Every request takes `delayMs`
 * on a simulated clock and is cut, as the deadline's abort cuts it, when it would end after the call's 10,000 ms. The
 * provider lists newest first. `refused` cancels are refused while the subscription stays live; `stale` subscriptions go
 * on showing as active in the list after a cancel ended them; `down(attempt)` gives no answer to any request of that try;
 * `beforeList(attempt, n)` runs before the try's n-th list request (from 1) is answered.
 */
export interface Customer {
  plans: number;
  status?: "active" | "incomplete_expired";
  extra?: { id: string; status: "active" | "incomplete_expired" }[];
  stored: boolean | string;
  noFirstPlan?: boolean;
  delayMs?: number;
  refused?: string[];
  stale?: string[];
  down?: (attempt: number) => boolean;
  beforeList?: (attempt: number, n: number, made: (id: string) => void) => void;
  beforeTry?: (attempt: number, made: (id: string) => void) => void;
  days?: number;
}

export interface Ending {
  tries: number;
  answers: string[];
  goneAfterMs: Ms | null;
  lines: Record<string, number>;
  live: string[];
  periodsBegunAfterMark: number;
  asked: Asked[];
  waits: Ms[];
  storedSettledFor: string | number | null;
}

export async function driven(c: Customer): Promise<{ ending: Ending; w: World }> {
  const span = 30 * DAY;
  const plans = Array.from({ length: c.plans }, (_, k) => ({
    id: `sub_${String(k + 2).padStart(5, "0")}`,
    at: START - Math.floor(((c.plans - k) * span) / (c.plans + 1)),
    status: c.status ?? "active",
  }));
  for (const each of c.extra ?? []) plans.push({ id: each.id, at: START - 1000, status: each.status });
  const w = await world({ testClock: false, listed: { plans }, ...(c.noFirstPlan === true ? { subscribed: false } : {}) });
  const meter = w.meter;
  const sec = () => Math.floor(w.now() / 1000);
  const refused = new Set(c.refused ?? []);
  const stale = new Set(c.stale ?? []);
  const asked: Asked[] = [];
  let attempt = 0;
  let elapsed = 0;
  let lists = 0;
  const made = (id: string) => meter.subscribe(ACCOUNT, sec(), id);
  const entry = (id: string, status: string) => ({ id, object: "subscription", status, customer: CUSTOMER });
  const statusOf = (id: string) => meter.plan(id)?.status ?? null;

  function answer(method: string, url: URL): { status: number; body: unknown } {
    const path = url.pathname;
    const sub = /^\/v1\/subscriptions\/([^/]+)$/.exec(path);
    if (sub !== null) {
      const id = decodeURIComponent(sub[1] as string);
      const status = statusOf(id);
      if (method === "GET") return status === null ? { status: 404, body: MISSING } : { status: 200, body: entry(id, status) };
      if (status === null) return { status: 404, body: MISSING };
      if (status !== "active" || refused.has(id)) return { status: 400, body: REFUSED };
      meter.endSubscription(ACCOUNT, sec(), id);
      return { status: 200, body: entry(id, "canceled") };
    }
    if (path === `/v1/customers/${CUSTOMER}`) return { status: 200, body: { id: CUSTOMER, object: "customer", livemode: false } };
    if (path === "/v1/subscriptions") {
      lists += 1;
      c.beforeList?.(attempt, lists, made);
      const all = meter
        .plans(ACCOUNT)
        .map((p) => ({ id: p.id, status: stale.has(p.id) ? "active" : p.status }))
        .reverse();
      const after = url.searchParams.get("starting_after");
      const from = after === null ? 0 : all.findIndex((e) => e.id === after) + 1;
      const shown = all.slice(from).filter((e) => e.status !== "canceled");
      const limit = Number(url.searchParams.get("limit") ?? "10");
      return { status: 200, body: { object: "list", has_more: shown.length > limit, data: shown.slice(0, limit).map((e) => entry(e.id, e.status)) } };
    }
    return { status: 500, body: "" };
  }

  const standIn = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = new URL(String(input));
    const method = init?.method ?? "GET";
    asked.push({ attempt, method, path: url.pathname, after: url.searchParams.get("starting_after") });
    const d = c.delayMs ?? 0;
    if (c.down?.(attempt) === true) {
      elapsed += d;
      throw new TypeError("fetch failed");
    }
    if (elapsed + d > DEADLINE) {
      elapsed = DEADLINE;
      throw new DOMException("The operation was aborted due to timeout", "TimeoutError");
    }
    const reply = answer(method, url);
    elapsed += d;
    return new Response(JSON.stringify(reply.body), { status: reply.status });
  };
  const original = globalThis.fetch;
  globalThis.fetch = standIn as typeof fetch;
  const adapter = makeStripePort({ baseUrl: "https://provider.invalid", apiKey: "sk_test_round", signingSecret: "whsec_round", priceId: "price_round" }, () => w.now() + elapsed);
  const answers: string[] = [];
  const at: Ms[] = [];
  w.host.port.cancelSubscription = async (id, customerId, settled) => {
    attempt += 1;
    elapsed = 0;
    lists = 0;
    c.beforeTry?.(attempt, made);
    const got = await adapter.cancelSubscription(id, customerId, settled);
    at.push(w.now());
    answers.push(got.kind === "failed" ? `failed ${got.cancelled}/${got.unended}` : got.kind);
    return got;
  };
  try {
    const storedId = c.stored === true ? "sub_1" : c.stored === false ? null : c.stored;
    if (storedId !== null) await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_stored", providerTime: START + 1000, subscriptionId: storedId }));
    const mark = START + 10 * MINUTE;
    w.at(mark);
    await quiet(w.lines, () => w.entries().destroy(HASH));
    const until = mark + (c.days ?? 7) * DAY;
    let goneAt: Ms | null = null;
    let settledFor: string | number | null = null;
    for (let runs = 0; runs < 20_000; runs += 1) {
      const tables = await w.tables();
      const row = tables.skeleton?.[0];
      if (row === undefined) {
        goneAt = w.now();
        break;
      }
      settledFor = row.cancel_stored_settled_for ?? null;
      const due = await w.host.getAlarm();
      if (due === null || due > until) break;
      w.at(Math.max(due, w.now()));
      await quiet(w.lines, () => w.entries().alarm());
    }
    const lines: Record<string, number> = {};
    for (const line of w.lines) {
      const event = String(JSON.parse(line).event);
      lines[event] = (lines[event] ?? 0) + 1;
    }
    const ending: Ending = {
      tries: answers.length,
      answers,
      goneAfterMs: goneAt === null ? null : goneAt - mark,
      lines,
      live: meter
        .plans(ACCOUNT)
        .filter((p) => p.status === "active")
        .map((p) => p.id),
      periodsBegunAfterMark: meter.periods(ACCOUNT, Math.floor(until / 1000)).filter((p) => p.start * 1000 > mark).length,
      asked,
      waits: at.slice(1).map((t, k) => t - (at[k] as Ms)),
      storedSettledFor: settledFor,
    };
    return { ending, w };
  } finally {
    globalThis.fetch = original;
  }
}

/** The runs of equal answers in order, as "answer xN". */
export function runs(answers: string[]): string[] {
  const out: [string, number][] = [];
  for (const each of answers) {
    const last = out.at(-1);
    if (last !== undefined && last[0] === each) last[1] += 1;
    else out.push([each, 1]);
  }
  return out.map(([answer, n]) => `${answer} x${n}`);
}
