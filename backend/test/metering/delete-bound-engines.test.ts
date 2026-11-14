import { env, runInDurableObject } from "cloudflare:test";
import { expect, test } from "vitest";
import { type AccountObject, stepOf } from "../../src/do/account-object.ts";
import type { Env } from "../../src/types.ts";
import { DELETE_BOUND_CASE, DELETE_BOUND_EXPECTED, runDeleteBoundCase } from "./delete-bound-cases.ts";

const bindings = env as unknown as Env;

test(`${DELETE_BOUND_CASE}: the statements give the committed result (the object's storage through stepOf)`, async () => {
  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName("metering-delete-bound"));
  await runInDurableObject(stub, async (_instance: AccountObject, state) => {
    expect(runDeleteBoundCase(stepOf(state.storage))).toEqual(DELETE_BOUND_EXPECTED);
  });
});
