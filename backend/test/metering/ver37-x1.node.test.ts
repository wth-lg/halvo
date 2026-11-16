import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderPort } from "../../src/billing/port.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import { FakeMeter } from "../../test-kit/fake-meter/fake-meter.ts";
import { meterPort } from "../../test-kit/fake-meter/port.ts";
import { makeTestHost } from "../../test-kit/sql/node.ts";

const START = 1_800_000_017_000;
const ACCOUNT = "00000000-0000-4000-8000-0000000000a1";
const HASH = "b".repeat(64);
const S = 1000;
const H = 3600 * S;
const D = 24 * H;
const T0 = 100 * S;

type Modules = { accounts: ReturnType<typeof makeAccounts>; ledger: ReturnType<typeof makeLedger>; metering: ReturnType<typeof makeMetering> };
type Fault = null | "accounts.forget" | "ledger.expire" | "due:ledger.nextDue";

beforeEach(() => {
  vi.spyOn(console, "log").mockImplementation(() => {});
});
afterEach(() => {
  vi.restoreAllMocks();
});

function throwing(modules: Modules, on: () => boolean, spec: string, only?: string): Modules {
  const [module, fn] = spec.split(".") as [keyof Modules, string];
  const inner = modules[module] as unknown as Record<string, (...a: unknown[]) => unknown>;
  const real = inner[fn] as (...a: unknown[]) => unknown;
  return {
    ...modules,
    [module]: {
      ...inner,
      [fn]: (...a: unknown[]) => {
        if (on() && (only === undefined || String(new Error().stack).includes(only))) throw new RangeError(`${spec} throws`);
        return real(...a);
      },
    },
  };
}

async function x1(fault: Fault) {
  let now = START;
  const meter = new FakeMeter({ uniquenessWindowSeconds: 86_400, duplicateAnswer: "error", graceSeconds: 3600, invoicePeriodSeconds: 30 * 86_400, readLagSeconds: 0, readGranularitySeconds: 1 });
  meter.subscribe(ACCOUNT, Math.floor(START / 1000));
  const made = meterPort({ meter, now: () => now, faults: [], always: { kind: "request-lost" }, checkout: { sessionId: "cs_1", url: "http://pay.test/cs_1" } });
  const cancelCalls: { t: Ms; id: string | null; answer: string }[] = [];
  let fails = 1;
  const port: ProviderPort = {
    ...made.port,
    async cancelSubscription(id) {
      const call = { t: now - START, id, answer: "" };
      cancelCalls.push(call);
      if (id === null) {
        call.answer = "nothing-to-cancel";
        return { kind: "nothing-to-cancel" };
      }
      if (fails > 0) {
        fails -= 1;
        call.answer = "failed";
        return { kind: "failed" };
      }
      call.answer = "cancelled";
      return { kind: "cancelled" };
    },
  };
  const test = makeTestHost({ port, clock: () => now });
  const host = test.host;
  let faultOn = false;
  const plain: Modules = {
    accounts: makeAccounts(),
    ledger: makeLedger({ holdLifeMs: host.config.holdLifeMs }),
    metering: makeMetering({ intervalSeconds: host.config.intervalSeconds, marginSeconds: host.config.marginSeconds }),
  };
  const modules = fault === null ? plain : fault.startsWith("due:") ? throwing(plain, () => faultOn, fault.slice(4), "dueOf") : throwing(plain, () => faultOn, fault);
  const entries = makeEntries(host, modules);
  expect((await entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", customerId: "cus_1", cap: { amount: 100_000_000, period: "month" } })).result).toBe("seeded");
  const giveId = (id: string, eventId: string) =>
    entries.webhookEvent({ kind: "subscription", eventId, providerTime: now, accountId: ACCOUNT, subscriptionId: id, customerId: "cus_1", state: "active", period: null, endedAt: null });
  now = START + 10_000;
  await entries.reserve(HASH, { requestId: "r_usage", tier: "low_cost_hosted", amount: 25_000, price: { input: 1_000_000, output: 1_000_000 } });
  await entries.settle("r_usage", { inputTokens: 25_000, outputTokens: 0 });
  now = START + T0;
  faultOn = true;
  const answer = await entries.destroy(HASH).then(
    (a) => a.result,
    (e: Error) => `threw ${e.name}`,
  );
  now = START + T0 + 10 * S;
  await giveId("sub_late", "evt_late");
  let goneAt: Ms | null = null;
  for (let n = 0; n < 200_000; n += 1) {
    const due = test.alarmAt();
    if (due === null || due - START > T0 + 3 * D) break;
    now = Math.max(now, due);
    await entries.alarm();
    if (goneAt === null && test.host.step((sql) => sql.exec("SELECT name FROM sqlite_master WHERE name = ?", "skeleton").length) === 0) goneAt = now - START;
  }
  test.close();
  const late = cancelCalls.filter((c) => c.id === "sub_late").map((c) => (c.t - T0) / S);
  return { answer, late, answers: cancelCalls.filter((c) => c.id === "sub_late").map((c) => c.answer), goneAt };
}

describe("verifier-37 run X1: an id arrives after an answer stored for null; the cancel for the id answers failed once; with and without a recurring caught error (P7-111 M1)", () => {
  for (const fault of [null, "accounts.forget", "ledger.expire", "due:ledger.nextDue"] as const) {
    test(`${fault ?? "no fault"}: the cancel for the id is tried again within its stored delay, and a period that begins 3 h after the delete is not charged`, async () => {
      const run = await x1(fault);
      const firstPeriod = 3 * 3600;
      expect(run.answer).toBe("not-finished");
      expect(run.answers.slice(0, 2)).toEqual(["failed", "cancelled"]);
      expect(run.late.length).toBeGreaterThanOrEqual(2);
      expect((run.late[1] as number) - (run.late[0] as number)).toBeLessThanOrEqual(60);
      expect((run.late[1] as number) <= firstPeriod).toBe(true);
    });
  }
});
