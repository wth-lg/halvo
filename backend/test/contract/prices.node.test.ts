import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import type { PriceTable } from "../../src/ledger/prices.ts";

test("test-kit/prices/v1.json is a PriceTable with S1's rates and a frontier tier at five times them", () => {
  const text = readFileSync("test-kit/prices/v1.json", "utf8");
  const table: PriceTable = JSON.parse(text);
  expect(Object.keys(table)).toEqual(["low_cost_hosted", "frontier"]);
  expect(table.low_cost_hosted).toEqual({ input: 3_100_000, output: 15_700_000, notBilled: [] });
  expect(table.frontier).toEqual({ input: 5 * 3_100_000, output: 5 * 15_700_000, notBilled: [] });
  expect(text).toBe(`${JSON.stringify(table, null, 2)}\n`);
});
