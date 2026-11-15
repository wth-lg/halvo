import { expect, test } from "vitest";
import { openSql } from "../../test-kit/sql/node.ts";
import { DELETE_BOUND_CASE, DELETE_BOUND_EXPECTED, runDeleteBoundCase } from "./delete-bound-cases.ts";

test(`${DELETE_BOUND_CASE}: the statements give the committed result (node:sqlite)`, () => {
  const db = openSql();
  try {
    expect(runDeleteBoundCase(db.step)).toEqual(DELETE_BOUND_EXPECTED);
  } finally {
    db.close();
  }
});
