"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const UNREADABLE = "Halvo cannot read its settings file. Reset settings keeps everything that Halvo has recorded and puts the settings back to how a new start has them.";
const RESET = "Reset settings";
const QUESTION = "Reset Halvo's settings? Halvo keeps everything that it has recorded. It stays paused until you press Resume, and from then on it records what a new start records:";
const YES = "Yes, reset settings";
const NEWER = "Halvo's settings file was saved by a newer version of Halvo. Update Halvo to read it, or use Delete everything, which resets it and also deletes everything that Halvo has recorded.";
const DELETE_QUESTION = "Delete everything that Halvo has recorded on this computer? This cannot be undone. Copies that Halvo made next to your own files stay.";
const BOX = { left: 31, top: 600, width: 140, height: 34 };
const INSIDE = { clientX: 60, clientY: 615 };
const BELOW = { clientX: 60, clientY: 700 };

function pageWith(status) {
  return openPage({ replies: { capture_status: () => status } });
}

test("a settings file that cannot be read offers Reset settings, which asks first with the first run's rows and sends one reset at its yes; a newer one keeps Delete everything", async () => {
  const rows = fixture("capture_status-not-started").texts.recorded;
  let status = { ...fixture("capture_status-paused"), state_file: "corrupt", store_cap_bytes: null, reset_recorded: rows };
  const page = await openPage({
    replies: {
      capture_status: () => status,
      reset_settings: () => {
        status = { ...fixture("capture_status-paused"), store_cap_bytes: null };
        return status;
      },
    },
  });
  assert.equal(page.text("settings-line"), UNREADABLE, "the one sentence");
  assert.equal(page.visible("settings-reset"), true, "the control");
  assert.equal(page.text("settings-reset"), RESET);
  assert.equal(page.visible("settings-delete"), false, "no delete at a file that cannot be read");

  page.byId("settings-reset").rect = BOX;
  await page.press("settings-reset", INSIDE);
  assert.equal(page.visible("settings-reset-row"), true, "the press shows the question");
  assert.equal(page.text("settings-reset-question"), QUESTION);
  assert.deepEqual(
    page.byId("settings-reset-recorded").children.map((li) => li.textContent),
    rows.map((row) => row.text),
    "the first run's rows stand under the question",
  );
  assert.equal(page.text("confirm-settings-reset"), YES);
  assert.equal(page.text("cancel-settings-reset"), "Cancel");
  assert.equal(page.visible("delete-confirm-row"), false, "the press arms no delete");
  assert.equal(page.calls("reset_settings").length, 0, "the press sends nothing");

  await page.clock.advance(600);
  await page.press("cancel-settings-reset", BELOW);
  assert.equal(page.visible("settings-reset-row"), false, "the safe answer closes the question");
  await page.press("confirm-settings-reset", BELOW);
  assert.equal(page.calls("reset_settings").length, 0, "the safe answer sends nothing");

  await page.press("settings-reset", INSIDE);
  await page.press("confirm-settings-reset", BELOW);
  assert.equal(page.calls("reset_settings").length, 0, "a yes before 500 ms sends nothing");
  await page.clock.advance(600);
  await page.press("confirm-settings-reset", INSIDE);
  assert.equal(page.calls("reset_settings").length, 0, "a yes at the place of the control sends nothing");
  await page.press("confirm-settings-reset", BELOW);
  assert.equal(page.calls("reset_settings").length, 1, "the yes sends one reset");
  assert.equal(page.calls("delete_all").length, 0);
  assert.equal(page.visible("settings-reset-row"), false);
  assert.equal(page.text("settings-line"), "", "the answer's view has no settings line");
  assert.equal(page.visible("settings-reset"), false);
  assert.equal(page.text("state"), "Halvo is paused");
  await page.press("confirm-settings-reset", BELOW);
  assert.equal(page.calls("reset_settings").length, 1, "a second yes sends nothing");

  const newer = await pageWith({ ...fixture("capture_status-paused"), state_file: "newer_version" });
  assert.equal(newer.text("settings-line"), NEWER, "the standing sentence");
  assert.equal(newer.visible("settings-reset"), false, "no reset is offered");
  assert.equal(newer.visible("settings-delete"), true);
  assert.equal(newer.text("settings-delete"), "Delete everything");
  await newer.click("settings-delete", { detail: 1, clientX: 5, clientY: 5 });
  assert.equal(newer.visible("delete-confirm-row"), true, "the control arms the standing delete");
  assert.equal(newer.text("delete-question"), DELETE_QUESTION);
  assert.equal(newer.calls("delete_all").length, 0, "nothing is sent before the confirming press");
  assert.equal(newer.calls("reset_settings").length, 0);

  const read = await pageWith(fixture("capture_status-running"));
  assert.equal(read.text("settings-line"), "", "no sentence where the reply has no key");
  assert.equal(read.visible("settings-reset"), false, "no reset where the reply has no key");
  assert.equal(read.visible("settings-delete"), false, "no delete there either");
});

test("no reset is offered while the store reads deleting, with the field of an unfinished delete and without it; at any other store state a settings file that cannot be read offers it", async () => {
  const rows = fixture("capture_status-not-started").texts.recorded;
  const unread = (name) => ({ ...fixture(name), state_file: "corrupt", reset_recorded: rows });
  for (const name of ["capture_status-store-deleting", "capture_status-store-deleting-unfinished"]) {
    const page = await pageWith(unread(name));
    assert.equal(page.text("settings-line"), UNREADABLE, `${name}: the sentence stays`);
    assert.equal(page.visible("settings-reset"), false, `${name}: no reset is offered`);
    await page.click("settings-reset", { detail: 1, clientX: 5, clientY: 5 });
    assert.equal(page.visible("settings-reset-row"), false, `${name}: the question does not open`);
    await page.clock.advance(600);
    await page.press("confirm-settings-reset", BELOW);
    assert.equal(page.calls("reset_settings").length, 0, `${name}: nothing is sent`);
  }
  for (const name of [
    "capture_status-paused",
    "capture_status-store-opening",
    "capture_status-store-sealed",
    "capture_status-store-unavailable-io",
    "capture_status-store-newer",
    "capture_status-store-no-release-custody",
  ]) {
    const page = await pageWith(unread(name));
    assert.equal(page.visible("settings-reset"), true, `${name}: the reset is offered`);
    await page.click("settings-reset", { detail: 1, clientX: 5, clientY: 5 });
    assert.equal(page.visible("settings-reset-row"), true, `${name}: the question opens`);
  }
});
