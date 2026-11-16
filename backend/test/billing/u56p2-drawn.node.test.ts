import fc from "fast-check";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { CALL_REQUEST_LIMIT, makeStripePort } from "../../src/billing/stripe/adapter.ts";
import { startRecorder } from "./servers.ts";
import { config, CUSTOMER, json, type Model, provider, type Reply, type Sub } from "./u56p2-provider.ts";

const STATUSES = ["active", "past_due", "trialing", "incomplete", "unpaid", "paused", "a_status_of_tomorrow", "canceled", "incomplete_expired"];
const ENDED = ["canceled", "incomplete_expired"];
const CUSTOMER_READS: Record<string, Reply> = {
  known: json({ id: CUSTOMER, object: "customer", livemode: false }),
  deleted: json({ id: CUSTOMER, object: "customer", deleted: true }),
  unknown: { status: 404, body: "gone" },
  "other mode": json({ id: CUSTOMER, object: "customer", livemode: true }),
  malformed: json({ id: CUSTOMER, object: "customer" }),
};
const PAGE_FAULTS: Reply[] = [{ status: 500, body: "" }, "drop", json({ object: "list", has_more: true, data: [] }), json({ object: "list", data: [] })];

const drawn = fc.record({
  statuses: fc.array(fc.constantFrom(...STATUSES), { maxLength: 60 }),
  stored: fc.option(fc.nat(59), { nil: null }),
  refused: fc.array(fc.nat(59), { maxLength: 2 }),
  customer: fc.constantFrom("known", "known", "known", "known", "known", "deleted", "unknown", "other mode", "malformed"),
  pageFault: fc.option(fc.record({ page: fc.nat(3), reply: fc.nat(PAGE_FAULTS.length - 1) }), { nil: null }),
});

beforeEach(() => {
  vi.spyOn(console, "log").mockImplementation(() => {});
});
afterEach(() => {
  vi.restoreAllMocks();
});

test("drawn customers (seed 562): no call answers cancelled or nothing-to-cancel while a subscription that has not ended stays on a customer that is not deleted, a fault is never an end, no call makes more than 20 requests, the count of a failed call is what its list ended, and a customer with no refusal and no fault ends", async () => {
  await fc.assert(
    fc.asyncProperty(drawn, async (c) => {
      const subs: Sub[] = c.statuses.map((status, index) => ({ id: `sub_${String(index).padStart(2, "0")}`, status, customer: CUSTOMER }));
      const stored = c.stored !== null && c.stored < subs.length ? (subs[c.stored] as Sub).id : null;
      const refused = c.refused.filter((index) => index < subs.length).map((index) => (subs[index] as Sub).id);
      const fault = c.pageFault;
      const model: Model = {
        subs,
        refused,
        customer: CUSTOMER_READS[c.customer],
        page: (index, reply) => (fault !== null && index === fault.page ? (PAGE_FAULTS[fault.reply] as Reply) : reply),
      };
      const server = await startRecorder(provider(model));
      try {
        const port = makeStripePort(config(server.url));
        let ended = false;
        for (let call = 0; call < 12 && !ended; call += 1) {
          const before = subs.map((each) => each.status);
          const requestsBefore = server.requests.length;
          const answer = await port.cancelSubscription(stored, CUSTOMER);
          const made = server.requests.slice(requestsBefore);
          const changed = subs.filter((each, index) => !ENDED.includes(before[index] as string) && each.status === "canceled");
          const listFaulted = fault !== null && made.some((request, index) => request.path === "/v1/subscriptions" && server.requests.slice(0, requestsBefore + index).filter((each) => each.path === "/v1/subscriptions").length === fault.page);
          expect(made.length).toBeLessThanOrEqual(CALL_REQUEST_LIMIT);
          if (answer.kind === "failed") {
            expect(answer.cancelled).toBe(changed.filter((each) => each.id !== stored).length);
            continue;
          }
          ended = true;
          expect(c.customer === "deleted" || subs.every((each) => ENDED.includes(each.status))).toBe(true);
          expect(["unknown", "other mode", "malformed"]).not.toContain(c.customer);
          expect(listFaulted).toBe(false);
        }
        const liveRefused = refused.some((id) => !ENDED.includes(subs.find((each) => each.id === id)?.status ?? "canceled"));
        if (c.customer === "known" && !liveRefused) expect(ended).toBe(true);
        if (liveRefused && c.customer !== "deleted") expect(ended).toBe(false);
      } finally {
        await server.close();
      }
    }),
    { seed: 562, numRuns: 60, endOnFailure: true },
  );
});
