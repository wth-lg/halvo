import type { CompletedSession, ProviderPort, UsageRefusal } from "../../src/billing/port.ts";
import type { Ms } from "../../src/types.ts";
import type { FakeMeter } from "./fake-meter.ts";

export type Fault =
  | { kind: "none" }
  | { kind: "request-lost" }
  | { kind: "response-lost" }
  | { kind: "refuse"; reason: UsageRefusal }
  | { kind: "during"; run: () => Promise<void>; after?: Fault };

export type MeterCall =
  | { kind: "send"; at: Ms; identifier: string; amountMinor: number; timestamp: number; answer: string }
  | { kind: "read"; at: Ms; from: number; to: number; answer: string }
  | {
      kind: "cancel";
      at: Ms;
      subscriptionId: string | null;
      customerId: string | null;
      storedSettled: boolean;
      answer: string;
      cancelled?: number;
      unended?: number;
      settledAfter?: boolean;
    };

/**
 * A fault of one call of the customer model's cancel: `no-answer` reaches nothing; `list-no-answer` makes the stored id's
 * cancel and then gets no answer to the read of the customer or the list.
 */
export type CancelFault = { kind: "none" } | { kind: "no-answer" } | { kind: "list-no-answer" } | { kind: "during"; run: () => Promise<void>; after?: CancelFault };

/**
 * The provider's customer records behind `cancelSubscription`, by the rule of contract 2: the stored id's cancel, the
 * customer, the list of the customer's subscriptions that have not ended, each cancelled, and the answer of a whole list
 * after the last cancel. `customers` maps a customer id to the meter's account; a customer id it does not map is one that
 * the provider does not know. `refused` names subscriptions whose every cancel is refused while they stay live.
 * `perCall` is the most listed cancels that one call makes, as the adapter's bound of requests does. A call whose stored
 * id is settled makes no request for it; the stored id is settled by its own cancel or by finding it ended or unknown. A
 * call without the second argument, as the port of unit 5.6 part 1 made it, is answered by part 1's rule for the stored id alone, so that
 * a run of these tests over that head's code reads what that head did.
 */
export interface CustomerModel {
  customers: Record<string, string>;
  deleted?: string[];
  refused?: string[];
  perCall?: number;
  faults?: CancelFault[];
}

type CancelAnswer = Awaited<ReturnType<ProviderPort["cancelSubscription"]>>;

export interface MeterPort {
  port: ProviderPort;
  calls: MeterCall[];
  faults: Fault[];
  cancelFaults: CancelFault[];
}

const NONE: Fault = { kind: "none" };

export function meterPort(o: {
  meter: FakeMeter;
  now: () => Ms;
  faults?: Fault[];
  always?: Fault;
  cancel?: () => Promise<{ kind: "cancelled" } | { kind: "nothing-to-cancel" } | { kind: "failed"; cancelled?: number; unended?: number; storedSettled?: boolean }>;
  customer?: CustomerModel;
  checkout?: { sessionId: string; url: string };
  session?: CompletedSession;
}): MeterPort {
  const calls: MeterCall[] = [];
  const faults = o.faults ?? [];
  const cancelFaults = o.customer?.faults ?? [];
  const seconds = () => Math.floor(o.now() / 1000);
  async function nextCancelFault(): Promise<CancelFault> {
    let fault = cancelFaults.shift() ?? { kind: "none" };
    while (fault.kind === "during") {
      await fault.run();
      fault = fault.after ?? { kind: "none" };
    }
    return fault;
  }
  function storedIdAlone(model: CustomerModel, subscriptionId: string | null): CancelAnswer {
    const stored = subscriptionId === null ? null : o.meter.plan(subscriptionId);
    if (subscriptionId === null || stored === null || stored.status !== "active") return { kind: "nothing-to-cancel" };
    if ((model.refused ?? []).includes(subscriptionId)) return { kind: "failed", cancelled: 0, unended: 1, storedSettled: false };
    o.meter.endSubscription(stored.account, seconds(), subscriptionId);
    return { kind: "cancelled" };
  }
  async function customerCancel(
    model: CustomerModel,
    subscriptionId: string | null,
    customerId: string | null | undefined,
    storedSettled: boolean,
  ): Promise<CancelAnswer> {
    if (customerId === undefined) return storedIdAlone(model, subscriptionId);
    if (subscriptionId === null && customerId === null) return { kind: "nothing-to-cancel" };
    let settled = subscriptionId !== null && storedSettled;
    const fault = await nextCancelFault();
    if (fault.kind === "no-answer") return { kind: "failed", cancelled: 0, unended: 0, storedSettled: settled };
    const refused = new Set(model.refused ?? []);
    let counted = 0;
    let unended = 0;
    let endedOne = false;
    let storedUnanswered = false;
    const live = (status: string) => status !== "canceled" && status !== "incomplete_expired";
    if (subscriptionId !== null && !settled) {
      const stored = o.meter.plan(subscriptionId);
      if (stored !== null && live(stored.status) && refused.has(subscriptionId)) {
        storedUnanswered = true;
        unended += 1;
      } else {
        if (stored !== null && live(stored.status)) {
          o.meter.endSubscription(stored.account, seconds(), subscriptionId);
          endedOne = true;
        }
        settled = true;
      }
    }
    const failed = (): CancelAnswer => ({ kind: "failed", cancelled: counted, unended, storedSettled: settled });
    const end = (): CancelAnswer => {
      if (storedUnanswered) return failed();
      return endedOne ? { kind: "cancelled" } : { kind: "nothing-to-cancel" };
    };
    if (fault.kind === "list-no-answer" || customerId === null) return failed();
    if ((model.deleted ?? []).includes(customerId)) return end();
    const account = model.customers[customerId];
    if (account === undefined) return failed();
    let listed = 0;
    for (const plan of o.meter.plans(account)) {
      if (!live(plan.status)) continue;
      if (listed === (model.perCall ?? Number.POSITIVE_INFINITY)) return failed();
      listed += 1;
      if (refused.has(plan.id)) {
        unended += 1;
        continue;
      }
      o.meter.endSubscription(account, seconds(), plan.id);
      counted += 1;
      endedOne = true;
    }
    return o.meter.plans(account).some((plan) => live(plan.status)) ? failed() : end();
  }
  async function next(): Promise<Fault> {
    let fault = faults.shift() ?? o.always ?? NONE;
    while (fault.kind === "during") {
      await fault.run();
      fault = fault.after ?? NONE;
    }
    return fault;
  }
  const port: ProviderPort = {
    async startCheckout(a) {
      if (o.checkout === undefined) return { kind: "failed" };
      return { kind: "session", sessionId: o.checkout.sessionId, url: o.checkout.url, expiresAt: a.expiresAt };
    },
    async verifyWebhook() {
      return { kind: "rejected" };
    },
    async readSession() {
      if (o.session === undefined) return { kind: "failed" };
      return { kind: "complete", session: o.session };
    },
    async openManagePage() {
      return { kind: "failed" };
    },
    async cancelSubscription(subscriptionId, customerId, storedSettled) {
      let answer: CancelAnswer;
      if (o.cancel !== undefined) {
        const given = await o.cancel();
        answer =
          given.kind === "failed"
            ? { kind: "failed", cancelled: given.cancelled ?? 0, unended: given.unended ?? 0, storedSettled: given.storedSettled ?? false }
            : given;
      } else if (o.customer !== undefined) answer = await customerCancel(o.customer, subscriptionId, customerId, storedSettled === true);
      else answer = { kind: "nothing-to-cancel" };
      calls.push({
        kind: "cancel",
        at: o.now(),
        subscriptionId,
        customerId: customerId ?? null,
        storedSettled: storedSettled === true,
        answer: answer.kind,
        ...(answer.kind === "failed" ? { cancelled: answer.cancelled, unended: answer.unended, settledAfter: answer.storedSettled } : {}),
      });
      return answer;
    },
    async sendUsage(e) {
      const fault = await next();
      const record = (answer: string) =>
        calls.push({ kind: "send", at: o.now(), identifier: e.identifier, amountMinor: e.amountMinor, timestamp: e.timestamp, answer });
      if (fault.kind === "request-lost") {
        record("failed");
        return { kind: "failed" };
      }
      if (fault.kind === "refuse") {
        record(`refused ${fault.reason}`);
        return { kind: "refused", reason: fault.reason };
      }
      const sent = o.meter.send({ account: e.accountId, identifier: e.identifier, amountMinor: e.amountMinor, timestamp: e.timestamp }, seconds());
      if (fault.kind === "response-lost") {
        record(`failed after ${sent.status}`);
        return { kind: "failed" };
      }
      record(sent.status === "refused" ? `refused ${sent.reason}` : sent.status);
      return sent.status === "refused" ? { kind: "refused", reason: sent.reason } : { kind: sent.status };
    },
    async readUsageTotal(a) {
      const fault = await next();
      const record = (answer: string) => calls.push({ kind: "read", at: o.now(), from: a.from, to: a.to, answer });
      if (fault.kind === "request-lost" || fault.kind === "refuse") {
        record("failed");
        return { kind: "failed" };
      }
      const read = o.meter.read(a.accountId, a.from, a.to, seconds());
      if (fault.kind === "response-lost") {
        record("failed");
        return { kind: "failed" };
      }
      record(`total ${read.totalMinor}`);
      return { kind: "total", from: read.widenedFrom, to: read.widenedTo, totalMinor: read.totalMinor };
    },
  };
  return { port, calls, faults, cancelFaults };
}
