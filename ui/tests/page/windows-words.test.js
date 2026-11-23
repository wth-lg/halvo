"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const page = (status) => openPage({ replies: { capture_status: () => status } });

test("app-4a: on Windows the page says that Halvo is not recording with Start, that it cannot record where Windows refused to keep its data out of error reports, and that it says what it records of what you type; the Mac keeps its words", async () => {
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
  assert.ok(rows.includes("Halvo records a key only when, just after you press it, the text changes in a field that Windows does not report as a password field. It records when you pressed the key, in which app, and what kind of key it was: a letter, digit or sign, or a delete key. It never records which letter, digit or sign. Halvo can tell this only in Chrome and Edge and in the plain text boxes of classic Windows programs: in every other program it records no key. It records no Return or Enter, no Tab, no arrow key, no Escape and no shortcut."), JSON.stringify(rows));
  const mac = await page(fixture("capture_status-not-running"));
  assert.equal(mac.text("state"), "Capture service not running");
});

test("app-4b: where the app's one renewal still read the screen spent, the page says to quit Halvo and open it again; a renewed screen says nothing of it", async () => {
  const running = fixture("capture_status-running-windows");
  const stopped = running.sources.map((source) => (source.name === "screen" ? { ...source, state: "stopped" } : source));
  const spent = await page({ ...running, capture: "partial", sources: stopped, screen_not_renewed: true });
  assert.equal(spent.text("screen-line"), "Halvo is not recording the screen. Quit Halvo and open it again to record it.");
  const renewed = await page(running);
  assert.equal(renewed.text("screen-line"), "");
});

test("win-crash-reports: where a crash report of the window stayed at the start, the page names its folder in the three sentences, whatever the service reads; with none it says nothing of it", async () => {
  const folder = "D:\\halvo web\\EBWebView\\Crashpad\\reports";
  const said = `Halvo could not remove a crash report of its own window in ${folder}. The report can hold what the window showed. Remove it yourself.`;
  const running = fixture("capture_status-running-windows");
  const left = await page({ ...running, crash_reports_left: folder });
  assert.equal(left.text("crash-report-line"), said);
  const notRunning = await page({ ...fixture("capture_status-not-running"), platform: "windows", crash_reports_left: folder });
  assert.equal(notRunning.text("crash-report-line"), said);
  const none = await page(running);
  assert.equal(none.text("crash-report-line"), "");
});
