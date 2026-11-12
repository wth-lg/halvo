import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { ownTables } from "../../src/do/entries.ts";
import { openSql } from "../../test-kit/sql/node.ts";

type Statement = { query: string; bindings: (string | number | null)[]; rows: Record<string, string | number | null>[] };

const statements: Statement[] = JSON.parse(readFileSync("test-kit/sql/statements.json", "utf8"));

test("every statement of the list gives its committed rows through openSql, in one step", () => {
  const db = openSql();
  const got = db.step((sql) => statements.map((each) => sql.exec(each.query, ...each.bindings)));
  expect(got).toEqual(statements.map((each) => each.rows));
  db.close();
});

test("a query with a semicolon is refused with SqlSemicolonError, and the step rolls back", () => {
  const db = openSql();
  let name: string | null = null;
  try {
    db.step((sql) => {
      sql.exec("CREATE TABLE kept (a INTEGER)");
      sql.exec("SELECT 1; SELECT 2");
    });
  } catch (error) {
    name = (error as Error).name;
  }
  expect(name).toBe("SqlSemicolonError");
  expect(db.step((sql) => sql.exec("SELECT name FROM sqlite_master WHERE type = ?", "table"))).toEqual([]);
  db.close();
});

test("the one list of an object's own tables leaves out sqlite_ and _cf_ names and __miniflare_do_name, and dropAll uses it", () => {
  const db = openSql();
  db.step((sql) => {
    for (const name of ["own", "_cf_platform", "__miniflare_do_name", "__miniflare_other"]) sql.exec(`CREATE TABLE "${name}" (a INTEGER)`);
    sql.exec("CREATE TABLE counted (id INTEGER PRIMARY KEY AUTOINCREMENT, a INTEGER)");
    sql.exec("INSERT INTO counted (a) VALUES (?)", 1);
    expect(sql.exec("SELECT name FROM sqlite_master WHERE type = ? AND name = ?", "table", "sqlite_sequence")).toHaveLength(1);
    expect(ownTables(sql)).toEqual(["own", "__miniflare_other", "counted"]);
  });
  db.dropAll();
  expect(db.step((sql) => sql.exec("SELECT name FROM sqlite_master WHERE type = ?", "table").map((row) => row.name))).toEqual([
    "_cf_platform",
    "__miniflare_do_name",
    "sqlite_sequence",
  ]);
  db.close();
});
