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
  | { kind: "cancel"; at: Ms; subscriptionId: string | null; answer: string };

export interface MeterPort {
  port: ProviderPort;
  calls: MeterCall[];
  faults: Fault[];
}

const NONE: Fault = { kind: "none" };

export function meterPort(o: {
  meter: FakeMeter;
  now: () => Ms;
  faults?: Fault[];
  always?: Fault;
  cancel?: () => Promise<{ kind: "cancelled" } | { kind: "nothing-to-cancel" } | { kind: "failed" }>;
  checkout?: { sessionId: string; url: string };
  session?: CompletedSession;
}): MeterPort {
  const calls: MeterCall[] = [];
  const faults = o.faults ?? [];
  const seconds = () => Math.floor(o.now() / 1000);
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
    async cancelSubscription(subscriptionId) {
      const answer = o.cancel === undefined ? { kind: "nothing-to-cancel" as const } : await o.cancel();
      calls.push({ kind: "cancel", at: o.now(), subscriptionId, answer: answer.kind });
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
  return { port, calls, faults };
}
