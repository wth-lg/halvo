import { expect, test } from "vitest";
import { openSql } from "../../test-kit/sql/node.ts";
import { cases, FAULT, rangeCase, rangeEdges, rangeExpected, reads, runCase, runRead } from "./rollback-cases.ts";

for (const each of cases) {
  test(`${each.step}: a throw in the middle of the step leaves storage as it was (node:sqlite)`, () => {
    const db = openSql();
    const seen = runCase(db.step, each);
    db.close();
    expect(seen.fault).toBe(FAULT);
    expect(seen.writesBeforeFault).toBe(each.failAt - 1);
    expect(seen.afterFault).toBe(seen.before);
    expect(seen.afterClean).not.toBe(seen.before);
  });
}

test("a settle whose cost would take its period's counted spend above 2^53 - 1 throws and rolls back; the hold expires at its amount and the period still answers (node:sqlite)", () => {
  const db = openSql();
  let seen: unknown;
  try {
    seen = rangeCase(db.step);
  } finally {
    db.close();
  }
  expect(seen).toEqual(rangeExpected);
});

for (const each of rangeEdges) {
  test(`${each.name} (node:sqlite)`, () => {
    const db = openSql();
    let seen: unknown;
    try {
      seen = each.run(db.step);
    } finally {
      db.close();
    }
    expect(seen).toEqual(each.expected);
  });
}

for (const each of reads) {
  test(`${each.name}: the statement gives the case's result (node:sqlite)`, () => {
    const db = openSql();
    let seen: unknown;
    try {
      seen = runRead(db.step, each);
    } finally {
      db.close();
    }
    expect(seen).toEqual(each.expected);
  });
}
