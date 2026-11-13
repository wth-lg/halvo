import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { type Entries, type Host, makeEntries } from "../../src/do/entries.ts";
import { type EndResult, makeLedger, type Price, type ReserveResult, type Usage } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Cap, Micro, Ms, Sql, Tier } from "../../src/types.ts";
import { makeTestHost } from "../../test-kit/sql/node.ts";

export const ACCOUNT = "3b0c6a5e-2f1d-4e8a-9c7b-5d4e3f2a1b0c";
export const HASH = "e".repeat(64);
export const LOW: Price = { input: 3_100_000, output: 15_700_000 };
export const HOLD_LIFE_MS = 3000;

export const failingPort: ProviderPort = {
  startCheckout: async () => ({ kind: "failed" }),
  verifyWebhook: async () => ({ kind: "rejected" }),
  readSession: async () => ({ kind: "failed" }),
  openManagePage: async () => ({ kind: "failed" }),
  cancelSubscription: async () => ({ kind: "nothing-to-cancel" }),
  sendUsage: async () => ({ kind: "failed" }),
  readUsageTotal: async () => ({ kind: "failed" }),
};

export type Call =
  | { kind: "reserve"; requestId: string; amount: Micro; tier?: Tier; price?: Price }
  | { kind: "dispatch"; requestId: string }
  | { kind: "settle"; requestId: string; usage: Usage }
  | { kind: "void"; requestId: string }
  | { kind: "alarm" }
  | { kind: "capSet"; cap: Cap }
  | { kind: "capGet" }
  | { kind: "restart" };

export interface Step {
  at: Ms;
  call: Call;
}

export type Answer = ReserveResult | EndResult | "go" | "stop" | object | null;

export interface Ledgered {
  periods: Record<string, string | number | null>[];
  holds: Record<string, string | number | null>[];
  cap: Record<string, string | number | null>[];
}

export interface Rig {
  entries(): Entries;
  setNow(at: Ms): void;
  run(step: Step): Promise<Answer>;
  read(): Ledgered;
  alarmAt(): Ms | null;
  sql<T>(run: (sql: Sql) => T): T;
  close(): void;
}

export interface AlarmFault {
  throws: boolean;
}

export function injectedAlarmError(): Error {
  const error = new Error("the host's alarm call failed");
  error.name = "InjectedAlarmError";
  return error;
}

export async function seededRig(o: {
  cap?: Cap;
  anchor?: Ms;
  at: Ms;
  subscription?: "active" | "payment-failed" | "cancelled";
  holdLifeMs?: number;
  alarmFault?: AlarmFault;
}): Promise<Rig> {
  let now = o.at;
  const made = makeTestHost({ port: failingPort, clock: () => now, config: { holdLifeMs: o.holdLifeMs ?? HOLD_LIFE_MS, testClock: true } });
  const fault = o.alarmFault;
  const host: Host =
    fault === undefined
      ? made.host
      : {
          ...made.host,
          setAlarm(at) {
            if (fault.throws) throw injectedAlarmError();
            made.host.setAlarm(at);
          },
        };
  const fresh = () =>
    makeEntries(host, {
      accounts: makeAccounts(),
      ledger: makeLedger({ holdLifeMs: made.host.config.holdLifeMs }),
      metering: makeMetering({ intervalSeconds: 3600, marginSeconds: 300 }),
    });
  let entries = fresh();
  const seeded = await entries.testSeed({
    accountId: ACCOUNT,
    hash: HASH,
    subscription: o.subscription ?? "active",
    ...(o.cap === undefined ? {} : { cap: o.cap }),
    ...(o.anchor === undefined ? {} : { anchor: o.anchor }),
  });
  if (seeded.result !== "seeded") throw new Error(`the seed answered ${seeded.result}`);
  const rows = (sql: Sql, table: string) => sql.exec(`SELECT * FROM ${table}`).map((row) => ({ ...row }));
  return {
    entries: () => entries,
    setNow(at) {
      now = at;
    },
    async run(step) {
      now = step.at;
      const call = step.call;
      switch (call.kind) {
        case "reserve":
          return entries.reserve(HASH, { requestId: call.requestId, tier: call.tier ?? "low_cost_hosted", amount: call.amount, price: call.price ?? LOW });
        case "dispatch":
          return entries.dispatch(call.requestId);
        case "settle":
          return entries.settle(call.requestId, call.usage);
        case "void":
          return entries.void(call.requestId);
        case "alarm":
          await entries.alarm();
          return null;
        case "capSet":
          return entries.capSet(HASH, call.cap);
        case "capGet":
          return entries.capGet(HASH);
        case "restart":
          entries = fresh();
          await entries.rearm();
          return null;
      }
    },
    read() {
      return made.host.step((sql) => ({ periods: rows(sql, "ledger_periods"), holds: rows(sql, "ledger_holds"), cap: rows(sql, "ledger_cap") }));
    },
    alarmAt: made.alarmAt,
    sql: (run) => made.host.step(run),
    close: made.close,
  };
}
