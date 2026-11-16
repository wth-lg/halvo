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

async function f1b() {
  let now = START;
  const meter = new FakeMeter({ uniquenessWindowSeconds: 86_400, duplicateAnswer: "error", graceSeconds: 3600, invoicePeriodSeconds: 30 * 86_400, readLagSeconds: 0, readGranularitySeconds: 1 });
  meter.subscribe(ACCOUNT, Math.floor(START / 1000));
  const made = meterPort({ meter, now: () => now, faults: [], always: { kind: "request-lost" }, checkout: { sessionId: "cs_1", url: "http://pay.test/cs_1" } });
  const late: { t: Ms; answer: string }[] = [];
  let fails = 1;
  let answeredAt = null as Ms | null;
  const port: ProviderPort = {
    ...made.port,
    async cancelSubscription(id) {
      if (id === null) return { kind: "nothing-to-cancel" };
      if (fails > 0) {
        fails -= 1;
        late.push({ t: now - START, answer: "failed" });
        return { kind: "failed", cancelled: 0, unended: 0, storedSettled: false };
      }
      late.push({ t: now - START, answer: "cancelled" });
      answeredAt = answeredAt ?? now;
      return { kind: "cancelled" };
    },
  };
  const test = makeTestHost({ port, clock: () => now });
  const host = test.host;
  let faultOn = false;
  let idHits = 0;
  const plain: Modules = {
    accounts: makeAccounts(),
    ledger: makeLedger({ holdLifeMs: host.config.holdLifeMs }),
    metering: makeMetering({ intervalSeconds: host.config.intervalSeconds, marginSeconds: host.config.marginSeconds }),
  };
  const forget = throwing(plain, () => faultOn, "accounts.forget");
  const subscription = forget.accounts.subscription;
  const modules: Modules = {
    ...forget,
    accounts: {
      ...forget.accounts,
      subscription: (...a: Parameters<typeof subscription>) => {
        if (faultOn && String(new Error().stack).includes("countFailure")) {
          idHits += 1;
          throw new RangeError("accounts.subscription throws in the count step");
        }
        return subscription(...a);
      },
    },
  };
  const entries = makeEntries(host, modules);
  expect((await entries.testSeed({ accountId: ACCOUNT, hash: HASH, subscription: "active", customerId: "cus_1", cap: { amount: 100_000_000, period: "month" } })).result).toBe("seeded");
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
  await entries.webhookEvent({ kind: "subscription", eventId: "evt_late", providerTime: now, accountId: ACCOUNT, subscriptionId: "sub_late", customerId: "cus_1", state: "active", period: null, endedAt: null });
  let goneAt: Ms | null = null;
  for (let n = 0; n < 200_000; n += 1) {
    const due = test.alarmAt();
    if (due === null || due - START > T0 + 3 * D) break;
    now = Math.max(now, due);
    await entries.alarm();
    if (test.host.step((sql) => sql.exec("SELECT name FROM sqlite_master WHERE name = ?", "skeleton").length) === 0) {
      goneAt = now;
      break;
    }
  }
  test.close();
  return { answer, late, answeredAt, goneAt, idHits };
}

describe("verifier-37's second reading, run F1b: the late id of finding 1, accounts.forget throwing in every run and accounts.subscription throwing inside the count step (P7-113 T4)", () => {
  test("the cancel for the id is answered at its stored next try, and the storage goes by the answer plus 24 hours plus SYNC_RETRY_MS", async () => {
    const run = await f1b();
    expect(run.answer).toBe("not-finished");
    expect(run.late.map((each) => each.answer)).toEqual(["failed", "cancelled"]);
    expect(run.idHits).toBeGreaterThan(0);
    expect(run.answeredAt).not.toBeNull();
    expect(run.goneAt).not.toBeNull();
    expect((run.goneAt as Ms) - (run.answeredAt as Ms)).toBeLessThanOrEqual(86_460_000);
  });
});
