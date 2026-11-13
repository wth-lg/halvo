import { env, runInDurableObject } from "cloudflare:test";
import { expect, test } from "vitest";
import expectedText from "./engines-cases.json?raw";
import { type AccountObject, stepOf } from "../../src/do/account-object.ts";
import type { Env } from "../../src/types.ts";
import { engineCases } from "./engines-cases.ts";

const bindings = env as unknown as Env;
const expected: Record<string, unknown> = JSON.parse(expectedText);

for (const [index, each] of engineCases.entries()) {
  test(`${each.name}: the metering module's statements give the committed result (the object's storage through stepOf)`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(`metering-engines-${index}`));
    await runInDurableObject(stub, async (_instance: AccountObject, state) => {
      expect(each.run(stepOf(state.storage))).toEqual(expected[each.name]);
    });
  });
}
