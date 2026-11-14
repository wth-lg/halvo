"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { openPage, fixture, UI } = require("../lib/page.js");

const APP_RS = fs.readFileSync(path.join(UI, "..", "src-tauri", "src", "app.rs"), "utf8");

const scriptOf = (name) => {
  const m = APP_RS.match(new RegExp(`pub const ${name}: &str = "([^"\\\\]*)";`));
  assert.ok(m, `src-tauri/src/app.rs holds ${name} as one plain string`);
  return m[1];
};

test("the two strings that the app sends at hide and show run against the page, and hide disarms Delete everything", async () => {
  const hidden = scriptOf("WINDOW_HIDDEN");
  const shown = scriptOf("WINDOW_SHOWN");
  const page = await openPage({ replies: { delete_all: () => fixture("delete_all-deleted") } });
  await page.click("delete", { detail: 0 });
  assert.equal(page.visible("confirm-delete"), true);
  vm.runInContext(hidden, page.window);
  await page.settle();
  assert.equal(page.visible("confirm-delete"), false);
  assert.equal(page.byId("delete").disabled, false);
  vm.runInContext(shown, page.window);
  await page.settle();
  await page.clock.advance(600);
  await page.click("confirm-delete", { detail: 0 });
  assert.equal(page.calls("delete_all").length, 0);
});
