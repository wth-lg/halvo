import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { openSql } from "../../test-kit/sql/node.ts";
import { engineCases } from "./engines-cases.ts";

const expected: Record<string, unknown> = JSON.parse(readFileSync("test/metering/engines-cases.json", "utf8"));

for (const each of engineCases) {
  test(`${each.name}: the metering module's statements give the committed result (node:sqlite)`, () => {
    const db = openSql();
    let seen: unknown;
    try {
      seen = each.run(db.step);
    } finally {
      db.close();
    }
    expect(seen).toEqual(expected[each.name]);
  });
}
