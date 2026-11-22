"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const page = (status) => openPage({ replies: { capture_status: () => status } });

test("app-4a: on Windows the page says that Halvo is not recording with Start, that it cannot record where Windows refused to keep its data out of error reports, and that it does not record what you type; the Mac keeps its words", async () => {
  const notRunning = { ...fixture("capture_status-not-running"), platform: "windows" };
  const idle = await page(notRunning);
  assert.equal(idle.text("state"), "Halvo is not recording. Press Start to record.");
  assert.equal(idle.text("toggle"), "Start");
  assert.equal(idle.byId("toggle").disabled, false);
  const refused = await page({ ...notRunning, last_exit: 6 });
  assert.equal(refused.text("state"), "Halvo cannot record on this computer. Windows refused the setting that keeps Halvo's data out of error reports.");
  assert.equal(refused.text("toggle"), "Start");
  assert.equal(refused.byId("toggle").disabled, false);
  const running = await page(fixture("capture_status-running-windows"));
  const rows = running.byId("recorded").children.map((li) => li.textContent);
  assert.ok(rows.includes("Halvo does not record what you type."), JSON.stringify(rows));
  const mac = await page(fixture("capture_status-not-running"));
  assert.equal(mac.text("state"), "Capture service not running");
});
