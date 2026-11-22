"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

const FOLDER = "/scratch/halvo-data";

const step = (name, result, detail = {}) => ({ step: name, result, detail });

const WHOLE = {
  steps: [
    step("account", "skipped", { cause: "no_account" }),
    step("delete_all", "done", { folder: FOLDER }),
    step("keychain", "done", { count: 0 }),
    step("login_item", "done", { count: 1 }),
    step("app_files", "done", { count: 3 }),
    step("data_dir", "done", { folder: FOLDER, count: 1 }),
  ],
};

const AT_THE_DELETE = {
  steps: [step("account", "skipped", { cause: "no_account" }), step("delete_all", "failed", { folder: FOLDER, cause: "delete_held" })],
};

const AT_THE_DATA_FOLDER = {
  steps: [
    step("account", "skipped", { cause: "no_account" }),
    step("delete_all", "done", { folder: FOLDER }),
    step("keychain", "done", { count: 0 }),
    step("login_item", "skipped", { cause: "own_data_folder" }),
    step("app_files", "skipped", { cause: "own_data_folder" }),
    step("data_dir", "failed", { folder: FOLDER, count: 0, cause: "own_files_left" }),
  ],
};

function heldUntilReleased(reply) {
  const app = { release: null };
  app.replies = {
    remove_halvo: () =>
      new Promise((resolve) => {
        app.release = () => resolve(reply);
      }),
  };
  return app;
}

function lines(page) {
  return page.byId("remove-result").children.map((line) => line.textContent);
}

async function armRemove(page) {
  await page.click("remove-halvo");
  await page.clock.advance(600);
}

test("Remove Halvo asks its own question and one click sends nothing", async () => {
  const app = heldUntilReleased(WHOLE);
  const page = await openPage({ replies: app.replies });
  const w = page.window.Halvo.words;
  assert.equal(page.text("remove-halvo"), w.REMOVE_HALVO, "the delete view shows Remove Halvo");
  assert.equal(page.byId("remove-halvo").disabled, false);
  assert.equal(page.text("delete"), w.DELETE_EVERYTHING, "beside Delete everything");

  await page.click("remove-halvo");
  assert.equal(page.calls("remove_halvo").length, 0, "one click sends nothing");
  assert.ok(page.visible("remove-question"), "it shows its own question");
  assert.equal(page.text("remove-question"), w.REMOVE_QUESTION);
  assert.equal(page.text("confirm-remove"), w.REMOVE_YES);
  assert.ok(!page.visible("delete-question"), "and not the question of Delete everything");
  await page.click("cancel-remove");
  assert.ok(!page.visible("remove-question"), "Cancel takes the question away");
  assert.equal(page.calls("remove_halvo").length, 0);

  await armRemove(page);
  const pressed = page.press("confirm-remove");
  await page.settle();
  assert.equal(page.calls("remove_halvo").length, 1, "the confirmed press sends remove_halvo once");
  assert.equal(page.calls("remove_halvo")[0].args, undefined, "with no id");
  assert.equal(page.text("remove-halvo"), w.REMOVING);
  assert.equal(page.byId("delete").disabled, true, "Delete everything waits for the removal");
  await armRemove(page);
  await page.press("confirm-remove");
  assert.equal(page.calls("remove_halvo").length, 1, "a second press sends none while it runs");
  app.release();
  await pressed;
  await page.settle();
  assert.deepEqual(
    lines(page),
    [
      w.REMOVE_DELETE_DONE,
      w.REMOVE_KEYCHAIN_DONE,
      w.REMOVE_LOGIN_ITEM_DONE,
      w.REMOVE_APP_FILES_DONE,
      w.REMOVE_DATA_DIR_UNKNOWN.replace("{folder}", FOLDER).replace("{count}", "1"),
      w.REMOVE_BY_HAND,
      w.REMOVE_DONE,
      w.REMOVE_CANNOT_REMOVE,
    ],
    "each step of a whole removal shows its line, and the last words follow",
  );
  assert.equal(page.byId("remove-halvo").disabled, true, "the app is ending");

  for (const [reply, expected] of [
    [AT_THE_DELETE, [w.REMOVE_DELETE_FAILED, w.REMOVE_STOPPED]],
    [
      AT_THE_DATA_FOLDER,
      [w.REMOVE_DELETE_DONE, w.REMOVE_KEYCHAIN_DONE, w.REMOVE_OWN_FOLDER_SKIPPED, w.REMOVE_DATA_DIR_FAILED.replace("{folder}", FOLDER), w.REMOVE_BY_HAND, w.REMOVE_STOPPED],
    ],
    [{ steps: [] }, [w.REMOVE_NOT_STARTED]],
  ]) {
    const stopped = heldUntilReleased(reply);
    const again = await openPage({ replies: stopped.replies });
    await armRemove(again);
    const sent = again.press("confirm-remove");
    await again.settle();
    stopped.release();
    await sent;
    await again.settle();
    assert.deepEqual(lines(again), expected, "a list that stopped shows its lines and the stop");
    assert.equal(again.byId("remove-halvo").disabled, false, "and the control again");
    assert.equal(again.text("remove-halvo"), w.REMOVE_HALVO);
  }
});
