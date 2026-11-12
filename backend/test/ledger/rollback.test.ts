import { env, runInDurableObject } from "cloudflare:test";
import { expect, test } from "vitest";
import { type AccountObject, stepOf } from "../../src/do/account-object.ts";
import type { Env } from "../../src/types.ts";
import { cases, FAULT, rangeCase, rangeEdges, rangeExpected, reads, runCase, runRead } from "./rollback-cases.ts";

const bindings = env as unknown as Env;

for (const each of cases) {
  test(`${each.step}: a throw in the middle of the step leaves storage as it was (the object's storage through stepOf)`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(`rollback-${each.step}`));
    await runInDurableObject(stub, async (_instance: AccountObject, state) => {
      const seen = runCase(stepOf(state.storage), each);
      expect(seen.fault).toBe(FAULT);
      expect(seen.writesBeforeFault).toBe(each.failAt - 1);
      expect(seen.afterFault).toBe(seen.before);
      expect(seen.afterClean).not.toBe(seen.before);
    });
  });
}

test("a settle whose cost would take its period's counted spend above 2^53 - 1 throws and rolls back; the hold expires at its amount and the period still answers (the object's storage through stepOf)", async () => {
  const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName("rollback-range"));
  await runInDurableObject(stub, async (_instance: AccountObject, state) => {
    expect(rangeCase(stepOf(state.storage))).toEqual(rangeExpected);
  });
});

for (const [index, each] of rangeEdges.entries()) {
  test(`${each.name} (the object's storage through stepOf)`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(`rollback-range-edge-${index}`));
    await runInDurableObject(stub, async (_instance: AccountObject, state) => {
      expect(each.run(stepOf(state.storage))).toEqual(each.expected);
    });
  });
}

for (const [index, each] of reads.entries()) {
  test(`${each.name}: the statement gives the case's result (the object's storage through stepOf)`, async () => {
    const stub = bindings.ACCOUNT.get(bindings.ACCOUNT.idFromName(`rollback-read-${index}`));
    await runInDurableObject(stub, async (_instance: AccountObject, state) => {
      expect(runRead(stepOf(state.storage), each)).toEqual(each.expected);
    });
  });
}
