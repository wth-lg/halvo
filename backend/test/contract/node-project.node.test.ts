import { expect, test } from "vitest";

test("the node project runs in Node 26 or later, which loads node:sqlite without a flag", () => {
  expect(Number(String(process.versions.node).split(".")[0])).toBeGreaterThanOrEqual(26);
  const sqlite = process.getBuiltinModule("node:sqlite");
  const db = new sqlite.DatabaseSync(":memory:");
  expect(db.prepare("SELECT 7 AS seven").all()).toEqual([{ seven: 7 }]);
  db.close();
});
