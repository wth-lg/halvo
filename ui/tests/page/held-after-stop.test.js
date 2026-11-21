"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const CAUSE =
  "Capture is off. One part of capture did not stop in time, so Halvo shut its capture service down to make sure that nothing is being read. Nothing is recorded until you turn capture back on.";
const RESUME_LINE = "Press Resume to turn capture back on.";
const START_LINE = "This happened three times in a row, so Halvo did not start its capture service again. Press Start to turn capture back on.";
const NOT_PAUSED = "Capture was not paused. Try again, or quit Halvo to stop capture.";

const visibleText = (node) =>
  node.localName ? (node.classList.contains("hidden") || node.hidden ? "" : node.childNodes.map(visibleText).join(" ")) : node.textContent;

test("after capture ended itself the page names the cause and the press that brings it back, a Pause that did not happen is said, and the page never says that Halvo is paused", async () => {
  const shown = [];
  const rows = [
    ["a held child that answers", "held-child", RESUME_LINE, "Resume"],
    ["no child after the third end with 5", "held-no-child", START_LINE, "Start"],
  ];
  for (const [name, file, line, word] of rows) {
    const page = await openPage({ replies: { capture_status: () => fixture(file), set_capture_state: () => ({ ...fixture("capture_status-no-indicator"), reached: true }) } });
    assert.equal(page.text("state"), CAUSE, name);
    assert.equal(page.text("capture-reason"), line, name);
    assert.equal(page.text("toggle"), word, name);
    assert.equal(page.byId("toggle").disabled, false, name);
    shown.push(visibleText(page.document.body));
    await page.clock.advance(1100);
    await page.click("toggle");
    assert.deepEqual(page.calls("set_capture_state").map((c) => c.args), [{ capture: "running" }], name);
  }

  const pauses = [
    ["a Pause answered with a view of no status", () => ({ ...fixture("capture_status-not-running"), reached: false })],
    [
      "a Pause rejected",
      () => {
        throw { error: "busy" };
      },
    ],
  ];
  for (const [name, answer] of pauses) {
    const page = await openPage({ replies: { capture_status: () => fixture("capture_status-running"), set_capture_state: answer } });
    await page.click("toggle");
    assert.deepEqual(page.calls("set_capture_state").map((c) => c.args), [{ capture: "paused" }], name);
    assert.equal(page.text("error"), NOT_PAUSED, name);
    shown.push(visibleText(page.document.body));
  }
  for (const text of shown) assert.doesNotMatch(text, /Halvo is paused/);
});
