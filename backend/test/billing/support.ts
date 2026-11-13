import { vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderEvent, ProviderPort } from "../../src/billing/port.ts";
import { type Entries, type Host, makeEntries, type Snapshot } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import type { Metering } from "../../src/metering/api.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Cap, Ms } from "../../src/types.ts";
import { makeTestHost } from "../../test-kit/sql/node.ts";

export const ACCOUNT = "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";
export const HASH = "a".repeat(64);
export const OTHER_HASH = "b".repeat(64);
export const CAP: Cap = { amount: 500, period: "month" };
export const OTHER_CAP: Cap = { amount: 900, period: "week" };

export function scriptedPort(over: Partial<ProviderPort> = {}): ProviderPort {
  return {
    startCheckout: async () => ({ kind: "failed" }),
    verifyWebhook: async () => ({ kind: "rejected" }),
    readSession: async () => ({ kind: "failed" }),
    openManagePage: async () => ({ kind: "failed" }),
    cancelSubscription: async () => ({ kind: "failed" }),
    sendUsage: async () => ({ kind: "failed" }),
    readUsageTotal: async () => ({ kind: "failed" }),
    ...over,
  };
}

export function sessionPort(expiresAt: Ms, over: Partial<ProviderPort> = {}): ProviderPort & { made: number } {
  const port = scriptedPort(over) as ProviderPort & { made: number };
  port.made = 0;
  port.startCheckout =
    over.startCheckout ??
    (async () => {
      port.made += 1;
      return { kind: "session", sessionId: `cs_${port.made}`, url: `http://127.0.0.1:1/c/${port.made}`, expiresAt };
    });
  return port;
}

export interface World {
  host: Host;
  entries: Entries;
  metering: Metering;
  logs: Record<string, unknown>[];
  alarmAt(): Ms | null;
  close(): void;
}

export function world(port: ProviderPort, clock: () => Ms = () => 1000): World {
  const made = makeTestHost({ port, clock });
  const metering = makeMetering({ intervalSeconds: made.host.config.intervalSeconds, marginSeconds: made.host.config.marginSeconds });
  const entries = makeEntries(made.host, {
    accounts: makeAccounts(),
    ledger: makeLedger({ holdLifeMs: made.host.config.holdLifeMs }),
    metering,
  });
  const logs: Record<string, unknown>[] = [];
  const spy = vi.spyOn(console, "log").mockImplementation((line: unknown) => {
    logs.push(JSON.parse(String(line)) as Record<string, unknown>);
  });
  return {
    host: made.host,
    entries,
    metering,
    logs,
    alarmAt: made.alarmAt,
    close() {
      spy.mockRestore();
      made.close();
    },
  };
}

export async function tables(entries: Entries): Promise<Snapshot["tables"] | null> {
  const read = await entries.testInspect();
  return read.result === "snapshot" ? (read as Snapshot).tables : null;
}

export function completed(over: Partial<Extract<ProviderEvent, { kind: "checkout-completed" }>> = {}): ProviderEvent {
  return {
    kind: "checkout-completed",
    eventId: "evt_completed",
    providerTime: 2_000,
    accountId: ACCOUNT,
    sessionId: "cs_1",
    customerId: "cus_1",
    subscriptionId: "sub_1",
    payment: "paid",
    ...over,
  };
}

export function subscriptionEvent(over: Partial<Extract<ProviderEvent, { kind: "subscription" }>> = {}): ProviderEvent {
  return {
    kind: "subscription",
    eventId: "evt_subscription",
    providerTime: 3_000,
    accountId: ACCOUNT,
    subscriptionId: "sub_1",
    customerId: "cus_1",
    state: "active",
    period: { start: 1_000, end: 2_592_001_000 },
    endedAt: null,
    ...over,
  };
}
