import { makeAccounts } from "../../src/accounts/index.ts";
import type { CompletedSession, ProviderEvent } from "../../src/billing/port.ts";
import { type Entries, type Host, makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms, Step } from "../../src/types.ts";
import { FakeMeter, type FakeMeterConfig } from "../../test-kit/fake-meter/fake-meter.ts";
import { type CancelFault, type CustomerModel, type Fault, type MeterCall, meterPort } from "../../test-kit/fake-meter/port.ts";
import { makeTestHost } from "../../test-kit/sql/node.ts";

export const ACCOUNT = "00000000-0000-4000-8000-0000000000a1";
export const HASH = "b".repeat(64);
export const START: Ms = 1_800_000_017_000;
export const HOUR: Ms = 3_600_000;
export const DAY: Ms = 86_400_000;
export const UNIT = { input: 1_000_000, output: 1_000_000 };
export const CUSTOMER = "cus_1";

type Row = Record<string, string | number | null>;

/**
 * The customer at the provider, for a world whose port lists the customer's subscriptions (contract 2): `plans` are
 * subscriptions beside the account's own `sub_1`, each started at `at` (START when not given); `known: false` is a
 * customer that the provider does not know; the other fields are the customer model's.
 */
export interface ListedCustomer {
  plans?: { id: string; status?: "active" | "incomplete_expired"; at?: Ms }[];
  known?: boolean;
  deleted?: boolean;
  refused?: string[];
  perCall?: number;
  faults?: CancelFault[];
}

export interface World {
  meter: FakeMeter;
  calls: MeterCall[];
  faults: Fault[];
  cancelFaults: CancelFault[];
  host: Host;
  entries(): Entries;
  restart(): void;
  now(): Ms;
  at(time: Ms): void;
  clock(at: Ms): Promise<string>;
  use(requestId: string, micro: number): Promise<unknown>;
  reserve(requestId: string): Promise<unknown>;
  settle(requestId: string, micro: number): Promise<unknown>;
  tables(): Promise<Record<string, Row[]>>;
  intervals(): Promise<Row[]>;
  stopNextStep(when: "before" | "after"): void;
  /** Ends every pause that the host's timer holds, as the passing of the paused time does. */
  endPauses(): void;
  lines: string[];
  close(): void;
}

export async function world(o: {
  meter?: Partial<FakeMeterConfig>;
  subscribed?: boolean;
  faults?: Fault[];
  always?: Fault;
  config?: Partial<Host["config"]>;
  testClock?: boolean;
  customer?: boolean;
  seed?: boolean;
  checkout?: { sessionId: string; url: string };
  session?: CompletedSession;
  cancel?: () => Promise<{ kind: "cancelled" } | { kind: "nothing-to-cancel" } | { kind: "failed"; cancelled?: number }>;
  listed?: ListedCustomer;
} = {}): Promise<World> {
  let now = START;
  const meter = new FakeMeter({
    uniquenessWindowSeconds: 86_400,
    duplicateAnswer: "error",
    graceSeconds: 3600,
    invoicePeriodSeconds: 30 * 86_400,
    readLagSeconds: 0,
    readGranularitySeconds: 1,
    ...o.meter,
  });
  if (o.subscribed !== false) meter.subscribe(ACCOUNT, Math.floor(START / 1000));
  for (const plan of o.listed?.plans ?? []) meter.subscribe(ACCOUNT, Math.floor((plan.at ?? START) / 1000), plan.id, plan.status ?? "active");
  const customer: CustomerModel | undefined =
    o.listed === undefined
      ? undefined
      : {
          customers: o.listed.known === false ? {} : { [CUSTOMER]: ACCOUNT },
          deleted: o.listed.deleted === true ? [CUSTOMER] : [],
          refused: o.listed.refused,
          perCall: o.listed.perCall,
          faults: o.listed.faults ?? [],
        };
  const made = meterPort({ meter, now: () => now, faults: o.faults ?? [], always: o.always, cancel: o.cancel, customer, checkout: o.checkout, session: o.session });
  const test = makeTestHost({ port: made.port, clock: () => now, config: o.config });
  const host = test.host;
  let stop: "before" | "after" | null = null;
  const commit = host.step;
  const stopped = () => {
    const error = new Error("the object stopped");
    error.name = "ObjectStoppedError";
    return error;
  };
  host.step = ((run) => {
    const when = stop;
    stop = null;
    if (when === "before") throw stopped();
    if (when === "after") {
      return commit((sql) => {
        run(sql);
        throw stopped();
      });
    }
    return commit(run);
  }) as Step;
  const modules = () => ({
    accounts: makeAccounts(),
    ledger: makeLedger({ holdLifeMs: host.config.holdLifeMs }),
    metering: makeMetering({ intervalSeconds: host.config.intervalSeconds, marginSeconds: host.config.marginSeconds }),
  });
  let entries = makeEntries(host, modules());
  const lines: string[] = [];
  const seeded = o.seed === false ? { result: "seeded" } : await entries.testSeed({
    accountId: ACCOUNT,
    hash: HASH,
    subscription: "active",
    customerId: o.customer === false ? undefined : CUSTOMER,
    cap: { amount: 100_000_000, period: "month" },
    ...(o.testClock === false ? {} : { time: START }),
  });
  if (seeded.result !== "seeded") throw new Error(`the seed answered ${seeded.result}`);
  const w: World = {
    meter,
    calls: made.calls,
    faults: made.faults,
    cancelFaults: made.cancelFaults,
    host,
    entries: () => entries,
    restart() {
      entries = makeEntries(host, modules());
    },
    now: () => now,
    at(time) {
      now = time;
    },
    async clock(at) {
      now = at;
      const original = console.log;
      console.log = (line: string) => lines.push(line);
      try {
        return (await entries.testSetClock(at)).result;
      } finally {
        console.log = original;
      }
    },
    async reserve(requestId) {
      return entries.reserve(HASH, { requestId, tier: "low_cost_hosted", amount: 100, price: UNIT });
    },
    async settle(requestId, micro) {
      return entries.settle(requestId, { inputTokens: micro, outputTokens: 0 });
    },
    async use(requestId, micro) {
      const reserved = await w.reserve(requestId);
      const settled = await w.settle(requestId, micro);
      return { reserved, settled };
    },
    async tables() {
      const snapshot = await entries.testInspect();
      if (snapshot.result !== "snapshot") return {};
      const intervals = snapshot.tables.metering_interval;
      if (intervals === undefined) return snapshot.tables;
      return { ...snapshot.tables, metering_interval: [...intervals].sort((a, b) => Number(a.start) - Number(b.start)) };
    },
    async intervals() {
      return (await w.tables()).metering_interval ?? [];
    },
    stopNextStep(when) {
      stop = when;
    },
    endPauses: test.endPauses,
    lines,
    close: test.close,
  };
  return w;
}

export function quiet<T>(lines: string[], run: () => Promise<T>): Promise<T> {
  const original = console.log;
  console.log = (line: string) => lines.push(line);
  return run().finally(() => {
    console.log = original;
  });
}

export function subscriptionEvent(over: Partial<Extract<ProviderEvent, { kind: "subscription" }>>): ProviderEvent {
  return {
    kind: "subscription",
    eventId: "evt_sub",
    providerTime: START,
    accountId: ACCOUNT,
    subscriptionId: "sub_1",
    customerId: "cus_1",
    state: "active",
    period: null,
    endedAt: null,
    ...over,
  };
}
