"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const CORRUPT = "Halvo cannot read its settings file. Delete everything resets it, and it also deletes everything that Halvo has recorded.";
const QUESTION = "Delete everything that Halvo has recorded on this computer? This cannot be undone. Copies that Halvo made next to your own files stay.";

function pageWith(status) {
  return openPage({ replies: { capture_status: () => status } });
}

test("a settings file that does not load offers the reset", async () => {
  const unread = await pageWith({ ...fixture("capture_status-running"), state_file: "corrupt" });
  assert.equal(unread.text("settings-line"), CORRUPT, "the one sentence");
  assert.equal(unread.visible("settings-reset"), true, "the control");
  assert.equal(unread.text("settings-reset"), "Delete everything");
  await unread.click("settings-reset", { detail: 1, clientX: 5, clientY: 5 });
  assert.equal(unread.visible("delete-confirm-row"), true, "the control arms the standing delete");
  assert.equal(unread.text("delete-question"), QUESTION);
  assert.equal(unread.calls("delete_all").length, 0, "nothing is sent before the confirming press");

  const read = await pageWith(fixture("capture_status-running"));
  assert.equal(read.text("settings-line"), "", "no sentence where the reply has no key");
  assert.equal(read.visible("settings-reset"), false, "no control where the reply has no key");
});
