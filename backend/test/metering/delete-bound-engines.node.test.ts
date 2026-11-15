import { expect, test } from "vitest";
import { openSql } from "../../test-kit/sql/node.ts";
import { DELETE_BOUND_CASE, DELETE_BOUND_EXPECTED, HOLDS_AT_CASE, HOLDS_AT_EXPECTED, runDeleteBoundCase, runHoldsAtCase } from "./delete-bound-cases.ts";

test(`${DELETE_BOUND_CASE}: the statements give the committed result (node:sqlite)`, () => {
  const db = openSql();
  try {
    expect(runDeleteBoundCase(db.step)).toEqual(DELETE_BOUND_EXPECTED);
  } finally {
    db.close();
  }
});

test(`${HOLDS_AT_CASE}: the statement gives the committed result (node:sqlite)`, () => {
  const db = openSql();
  try {
    expect(runHoldsAtCase(db.step)).toEqual(HOLDS_AT_EXPECTED);
  } finally {
    db.close();
  }
});
