"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const WRITES = [
  "set_capture_state",
  "delete_all",
  "set_storage_cap",
  "request_permission",
  "open_permission_pane",
  "acknowledge_lost_permission",
  "start_capture",
  "approve_workflow",
  "dismiss_workflow",
  "delete_workflow",
  "agents_answer",
  "agents_setup_typed",
  "agents_present",
  "agents_control",
  "agents_delete",
];

test("the not-started page sends start_capture at the click alone", async () => {
  const notStarted = fixture("capture_status-not-started");
  let answer = null;
  const page = await openPage({
    replies: {
      capture_status: () => notStarted,
      start_capture: () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    },
  });
  const text = page.text("first-run");
  assert.ok(
    text.includes("Halvo looks through what it recorded for tasks that repeat, on this computer alone."),
    "the sentence on repeated tasks",
  );
  for (const row of notStarted.texts.recorded) assert.ok(text.includes(row.text), row.text);
  for (const line of [notStarted.texts.repeated_tasks, notStarted.texts.storage, notStarted.texts.limits, notStarted.texts.custody]) {
    assert.ok(text.includes(line), line);
  }
  const starts = page.document.querySelectorAll("button").filter((b) => b.textContent === "Start" && page.visible(b));
  assert.equal(starts.length, 1, "one Start control");
  assert.equal(starts[0].disabled, false);
  await page.clock.advance(3000);
  assert.deepEqual(
    page.sent.map((c) => c.cmd).filter((cmd) => WRITES.includes(cmd)),
    [],
    "before the click the page calls its reads alone",
  );
  const click = page.click(starts[0]);
  await page.settle();
  await page.click(starts[0]);
  assert.equal(page.calls("start_capture").length, 1, "one start_capture, and a press in flight sends none");
  answer(fixture("start_capture-running"));
  await click;
  assert.equal(page.calls("start_capture").length, 1);
  assert.equal(page.visible("first-run"), false, "the reply's view follows the click");
});
