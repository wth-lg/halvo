import { env, runInDurableObject } from "cloudflare:test";
import { expect, test } from "vitest";
import { type AccountObject, stepOf } from "../../src/do/account-object.ts";
import type { Env } from "../../src/types.ts";
import { expiryCases } from "./expiry-cases.ts";

const bindings = env as unknown as Env;

for (const [index, each] of expiryCases.entries()) {
  test(`${each.name} (the object's storage through stepOf)`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(`expiry-${index}`));
    await runInDurableObject(stub, async (_instance: AccountObject, state) => {
      expect(each.run(stepOf(state.storage))).toEqual(each.expected);
    });
  });
}
