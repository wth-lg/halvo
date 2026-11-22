"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

test("where the service awaits a resume and the app holds nothing, the toggle reads Resume, a click sends the Resume alone, and the view follows the answer", async () => {
  const status = fixture("capture_status-awaiting-resume");
  assert.equal(status.desired, "running");
  assert.equal(status.hold, null);
  const page = await openPage({
    replies: { capture_status: () => status, set_capture_state: () => ({ ...fixture("capture_status-starting"), reached: true }) },
  });
  assert.equal(page.text("state"), "Capture is off until you press Resume");
  assert.equal(page.text("toggle"), "Resume");
  assert.equal(page.byId("toggle").disabled, false);
  await page.clock.advance(1100);
  await page.click("toggle");
  assert.deepEqual(
    page.calls("set_capture_state").map((c) => c.args),
    [{ capture: "running" }],
  );
  assert.equal(page.text("state"), "Halvo's capture is starting");
  assert.equal(page.text("toggle"), "Pause");
});
