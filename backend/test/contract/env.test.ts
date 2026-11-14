import { env, runInDurableObject, SELF } from "cloudflare:test";
import { afterEach, expect, test, vi } from "vitest";
import statementsText from "../../test-kit/sql/statements.json?raw";
import { AccountObject, stepOf } from "../../src/do/account-object.ts";
import { AFTER_CLIENT_GONE_MS } from "../../src/gateway/stream.ts";
import type { Price } from "../../src/ledger/index.ts";
import type { Env } from "../../src/types.ts";
import prices from "../../test-kit/prices/v1.json";

type Statement = { query: string; bindings: (string | number | null)[]; rows: Record<string, string | number | null>[] };

const bindings = env as unknown as Env & Record<string, unknown>;
const statements: Statement[] = JSON.parse(statementsText);

test("a test under workerd reads the seven variables that vitest.config.ts passes to the plugin", () => {
  expect(bindings.PROVIDER_API_KEY).toBe("sk_test_halvolocal");
  expect(bindings.WEBHOOK_SIGNING_SECRET).toBe("whsec_test_halvo_local");
  expect(bindings.PROVIDER_PRICE_ID).toBe("price_test_halvo_local");
  expect(bindings.PROVIDER_METER_ID).toBe("mtr_test_halvo_local");
  expect(bindings.PROVIDER_METER_EVENT).toBe("halvo_usage");
  expect(bindings.PUBLIC_ORIGIN).toBe("http://halvo.test");
  expect(bindings.PROVIDER_BASE_URL).toMatch(/^(http:\/\/127\.0\.0\.1:[0-9]+)?$/);
  expect(bindings.HOLD_LIFE_MS).toBe("3000");
  expect(bindings.TEST_CLOCK).toBeUndefined();
});

test("an object that holds nothing has no table at all; the statements give their committed rows through stepOf; the platform keeps only _cf_ tables", async () => {
  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName("statements"));
  await runInDurableObject(stub, async (_instance: AccountObject, state) => {
    const before = state.storage.sql.exec("SELECT name FROM sqlite_master WHERE type = 'table'").toArray();
    expect(before).toEqual([]);
    const step = stepOf(state.storage);
    expect(step((sql) => statements.map((each) => sql.exec(each.query, ...each.bindings)))).toEqual(statements.map((each) => each.rows));
    const names = state.storage.sql
      .exec("SELECT name FROM sqlite_master WHERE type = 'table'")
      .toArray()
      .map((row) => String(row.name));
    console.log(`statements test, workerd leg: the tables of the object are ${names.join(" ")}`);
    const own = new Set(["skeleton", "test_clock", "account", "ledger_cap"]);
    expect(names.filter((name) => !own.has(name) && !name.startsWith("sqlite_") && !name.startsWith("_cf_"))).toEqual([]);
    let refused: string | null = null;
    try {
      step((sql) => sql.exec("SELECT 1; SELECT 2"));
    } catch (error) {
      refused = (error as Error).name;
    }
    expect(refused).toBe("SqlSemicolonError");
  });
});

test("the deployable entry answers the gateway path and the three test paths with 404", async () => {
  for (const path of ["/v1/chat", "/test/clock", "/test/seed", "/test/inspect"]) {
    const response = await SELF.fetch(`http://backend.test${path}`, { method: "POST", body: "{}" });
    expect({ path, status: response.status, body: await response.text() }).toEqual({ path, status: 404, body: '{"result":"rejected"}' });
  }
});

test("the object turns its test methods on only when TEST_CLOCK is exactly 1", async () => {
  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName("clock-switch"));
  await runInDurableObject(stub, async (_instance: AccountObject, state) => {
    for (const [value, result] of [
      ["1", "no-such-account"],
      ["0", "refused"],
      ["", "refused"],
      ["true", "refused"],
      [" 1", "refused"],
      [undefined, "refused"],
    ] as const) {
      const object = new AccountObject(state, { ...bindings, TEST_CLOCK: value } as Env);
      expect({ value, result: (await object.testInspect()).result }).toEqual({ value, result });
    }
  });
});

const BOUND_CLOCK = Date.UTC(2030, 0, 1) + 1_237;
const LOW: Price = { input: prices.low_cost_hosted.input, output: prices.low_cost_hosted.output };
const realNow = Date.now;
afterEach(() => {
  Date.now = realNow;
  vi.restoreAllMocks();
});

function consoleLines(): string[] {
  const lines: string[] = [];
  for (const method of ["log", "error", "warn", "info", "debug", "trace", "dir", "table"] as const) {
    vi.spyOn(console, method).mockImplementation((...args: unknown[]) => {
      lines.push(`${method} ${args.map(String).join(" ")}`);
    });
  }
  return lines;
}

test("contract 6, unit 4.9: the object refuses HOLD_LIFE_MS above 30,000 ms, which is AFTER_CLIENT_GONE_MS, and SYNC_INTERVAL_SECONDS above 86,400 s where it reads its settings, as numberVariable refuses a value that does not parse: one line bad-variable with the variable's name and an Error of that name whose message is 'a number variable is above its bound', one above the bound, at 100000 and at the largest value that the parser takes, with TEST_CLOCK absent and with it 1; below the bound (5000 and 9000, whose first digits are larger than the bound's) and at each bound the object starts and writes no line, and its alarm stands at the open hold's expiry, 30,000 ms after a reserve at a clock of no whole second, and, after the hold settles, at the end of the 86,400 s interval that starts at the whole second of the seed", async () => {
  expect(AFTER_CLIENT_GONE_MS).toBe(30_000);
  const hash = "5d".repeat(32);
  Date.now = () => BOUND_CLOCK;
  for (const [name, bound] of [
    ["HOLD_LIFE_MS", 30_000],
    ["SYNC_INTERVAL_SECONDS", 86_400],
  ] as const) {
    for (const value of [String(bound + 1), "100000", "999999999999999"]) {
      for (const testClock of [undefined, "1"]) {
        const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.newUniqueId());
        await runInDurableObject(stub, async (_instance: AccountObject, state) => {
          const lines = consoleLines();
          let refused: unknown = null;
          try {
            new AccountObject(state, { ...bindings, TEST_CLOCK: testClock, [name]: value } as Env);
          } catch (error) {
            refused = error;
          }
          vi.restoreAllMocks();
          expect({ name, value, testClock, refused: refused instanceof Error ? refused.name : refused, message: refused instanceof Error ? refused.message : null, lines }).toEqual({
            name,
            value,
            testClock,
            refused: name,
            message: "a number variable is above its bound",
            lines: [`log {"event":"bad-variable","variable":"${name}"}`],
          });
          expect(await state.storage.getAlarm()).toBeNull();
        });
      }
    }
  }

  for (const [hold, interval] of [
    ["5000", "9000"],
    ["30000", "86400"],
  ] as const) {
    await runInDurableObject(bindings.ACCOUNT.get(bindings.ACCOUNT.newUniqueId()), async (_instance: AccountObject, state) => {
      const lines = consoleLines();
      const object = new AccountObject(state, { ...bindings, HOLD_LIFE_MS: hold, SYNC_INTERVAL_SECONDS: interval } as Env);
      vi.restoreAllMocks();
      expect({ hold, interval, made: object instanceof AccountObject, lines }).toEqual({ hold, interval, made: true, lines: [] });
    });
  }

  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.newUniqueId());
  await runInDurableObject(stub, async (_instance: AccountObject, state) => {
    const lines = consoleLines();
    const object = new AccountObject(state, { ...bindings, TEST_CLOCK: "1", HOLD_LIFE_MS: "30000", SYNC_INTERVAL_SECONDS: "86400" } as Env);
    expect(await object.testSeed({ accountId: "4d2c1b0a-9f8e-4d7c-8b6a-5f4e3d2c1b0a", hash, subscription: "active", customerId: "cus_test_bound", cap: { amount: 10, period: "week" } })).toEqual({ result: "seeded" });
    Date.now = () => BOUND_CLOCK + 389;
    expect(await object.reserve(hash, { requestId: "bound", tier: "low_cost_hosted", amount: 10_000, price: LOW })).toMatchObject({ outcome: "admitted" });
    expect(await state.storage.getAlarm()).toBe(BOUND_CLOCK + 389 + 30_000);
    expect(await object.dispatch("bound")).toBe("go");
    expect(await object.settle("bound", { inputTokens: 7, outputTokens: 9 })).toMatchObject({ outcome: "settled" });
    expect(await state.storage.getAlarm()).toBe(Date.UTC(2030, 0, 1) + 1_000 + 86_400_000);
    vi.restoreAllMocks();
    expect(lines).toEqual([]);
  });
});

function storedTables(state: DurableObjectState): Record<string, Record<string, unknown>[]> {
  const dump: Record<string, Record<string, unknown>[]> = {};
  const names = state.storage.sql.exec("SELECT type, name, sql FROM sqlite_master ORDER BY type, name").toArray() as Record<string, unknown>[];
  dump.schema = names.filter((entry) => !String(entry.name).startsWith("_cf_"));
  for (const entry of dump.schema) {
    if (entry.type !== "table") continue;
    dump[String(entry.name)] = state.storage.sql.exec(`SELECT * FROM "${String(entry.name)}"`).toArray() as Record<string, unknown>[];
  }
  return dump;
}

test("contract 6, unit 4.9 (ii), ruling P7-99 G2: over an account with an open hold, a construction with HOLD_LIFE_MS or SYNC_INTERVAL_SECONDS one above its bound at a later clock is refused with its one line and, one timer turn later, has changed no row of any table and not the alarm, once with the alarm as the reserve left it and once after the stored alarm was deleted (null stays null); then a construction at the bound, at a clock past the hold's expiry, arms the alarm at the hold's stored expiry, and its alarm ends the hold and leaves the period's open amount at 0", async () => {
  const hash = "8e".repeat(32);
  const reserveAt = BOUND_CLOCK + 41;
  const refusedAt = reserveAt + 777;
  const pastExpiry = reserveAt + 3_000 + 509;
  for (const [name, bound] of [
    ["HOLD_LIFE_MS", 30_000],
    ["SYNC_INTERVAL_SECONDS", 86_400],
  ] as const) {
    await runInDurableObject(bindings.ACCOUNT.get(bindings.ACCOUNT.newUniqueId()), async (_instance: AccountObject, state) => {
      Date.now = () => BOUND_CLOCK;
      const seeding = new AccountObject(state, { ...bindings, TEST_CLOCK: "1" } as Env);
      expect(await seeding.testSeed({ accountId: "5e3d2c1b-0a9f-4e8d-9c7b-6a5f4e3d2c1b", hash, subscription: "active", cap: { amount: 10, period: "week" } })).toEqual({ result: "seeded" });
      Date.now = () => reserveAt;
      expect(await seeding.reserve(hash, { requestId: "over-stored", tier: "low_cost_hosted", amount: 10_000, price: LOW })).toMatchObject({ outcome: "admitted" });
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(await state.storage.getAlarm()).toBe(reserveAt + 3_000);

      for (const alarm of ["as the reserve left it", "deleted"] as const) {
        if (alarm === "deleted") await state.storage.deleteAlarm();
        const before = { tables: storedTables(state), alarm: await state.storage.getAlarm() };
        expect({ name, alarm, before: before.alarm }).toEqual({ name, alarm, before: alarm === "deleted" ? null : reserveAt + 3_000 });
        Date.now = () => refusedAt;
        const lines = consoleLines();
        let refused: unknown = null;
        try {
          new AccountObject(state, { ...bindings, [name]: String(bound + 1) } as Env);
        } catch (error) {
          refused = error instanceof Error ? error.name : error;
        }
        await new Promise((resolve) => setTimeout(resolve, 50));
        vi.restoreAllMocks();
        const after = { tables: storedTables(state), alarm: await state.storage.getAlarm() };
        expect({ name, alarm, refused, lines, tables: after.tables, alarmAfter: after.alarm }).toEqual({
          name,
          alarm,
          refused: name,
          lines: [`log {"event":"bad-variable","variable":"${name}"}`],
          tables: before.tables,
          alarmAfter: before.alarm,
        });
      }

      Date.now = () => pastExpiry;
      const lines = consoleLines();
      const object = new AccountObject(state, { ...bindings, [name]: String(bound) } as Env);
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect({ name, alarm: await state.storage.getAlarm() }).toEqual({ name, alarm: reserveAt + 3_000 });
      await object.alarm();
      vi.restoreAllMocks();
      const holds = state.storage.sql.exec("SELECT request_id, state, expires_at, ended_at FROM ledger_holds").toArray();
      const periods = state.storage.sql.exec("SELECT open FROM ledger_periods").toArray();
      expect({ name, lines, holds, periods }).toEqual({
        name,
        lines: [],
        holds: [{ request_id: "over-stored", state: "voided", expires_at: reserveAt + 3_000, ended_at: pastExpiry }],
        periods: [{ open: 0 }],
      });
    });
  }
});
