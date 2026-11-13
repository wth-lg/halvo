import { env, runInDurableObject, SELF } from "cloudflare:test";
import { expect, test } from "vitest";
import statementsText from "../../test-kit/sql/statements.json?raw";
import { AccountObject, stepOf } from "../../src/do/account-object.ts";
import type { Env } from "../../src/types.ts";

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
