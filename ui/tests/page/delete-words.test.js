"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete, loadedAgain } = require("../lib/page.js");

const W = {
  done: "Deleted. Nothing that Halvo recorded in its data folder can be read again, and nothing of it is left there.",
  copies:
    "Copies that Halvo made next to your own files stay. A copy or a backup of Halvo's whole data folder that was made before can still be opened, and Halvo cannot reach it. Remove those yourself if you want them gone.",
  keys: "Halvo destroyed its keys, so nothing that was recorded can be read again.",
  notFollowed:
    "Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there. They cannot be read, and you can remove them.",
  rootLeft: "Halvo could not remove one key file. Remove it with this command:",
  rootUnnamed: "Halvo could not remove one key file and cannot name it. What was recorded may still be readable on this computer.",
  rootUnknown: "Halvo cannot tell whether a key file is left.",
  files: (n) => `Some files could not be removed: ${n}. They cannot be read without the keys.`,
  database: "A database file could not be removed. It cannot be read without the keys.",
  entries: (n) => `Halvo could not remove ${n} of its own folders or files in its data folder. Something that Halvo recorded may still be in them.`,
  unknown: (n) => `Halvo's data folder holds ${n} things that Halvo does not know. Halvo did not open or remove them.`,
};

const COMMAND = "rm '/Users/<name>/Library/Application Support/Halvo/roots/00000000000000000000000000000009.root'";
const clean = () => fixture("delete_all-deleted");
const deleted = (fields) => ({ ...clean(), nothing_left: false, ...fields });

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
}

const AT = "At 09:30: ";

async function resultAfterDelete(reply) {
  const page = await openPage({ replies: keptDelete({ delete_all: () => reply }), timezoneOffset: 0 });
  await confirmDelete(page);
  const next = await loadedAgain(page);
  const shown = next.text("delete-result");
  assert.ok(shown.startsWith(AT), shown);
  return { text: shown.slice(AT.length), cleanup: next.visible("delete-cleanup") ? next.text("delete-cleanup") : null };
}

const lines = (...parts) => parts.join(" ");

test("I-98 (4) and I-98 (5): a delete with each of its six fields clean reads done, with the clause on copies after it", async () => {
  const shown = await resultAfterDelete(clean());
  assert.equal(shown.text, lines(W.done, W.copies));
  assert.equal(shown.cleanup, null);
});

test("L1: nothing is left only with each of the six fields clean, and each field that is not clean has its own line", async () => {
  const rows = [
    ["root_destroyed no with a command", { root_destroyed: "no", cleanup_command: COMMAND }, lines(W.rootLeft), COMMAND],
    ["root_destroyed no and no command", { root_destroyed: "no" }, lines(W.rootUnnamed), null],
    ["root_destroyed unknown", { root_destroyed: "unknown" }, lines(W.rootUnknown), null],
    ["not_followed 2", { not_followed: 2 }, lines(W.keys, W.copies, W.notFollowed), null],
    ["files_left 3", { files_left: 3 }, lines(W.files(3)), null],
    ["database_left", { database_left: true }, lines(W.database), null],
    ["entries_left 1", { entries_left: 1 }, lines(W.entries(1)), null],
    ["unknown_entries 1", { unknown_entries: 1 }, lines(W.unknown(1)), null],
    ["files_left 3 and database_left", { files_left: 3, database_left: true }, lines(W.files(3), W.database), null],
  ];
  for (const [name, fields, words, cleanup] of rows) {
    const shown = await resultAfterDelete(deleted(fields));
    assert.equal(shown.text, words, name);
    assert.equal(shown.cleanup, cleanup, name);
    assert.doesNotMatch(shown.text, /nothing of it is left|nothing is left/, name);
  }
});

test("L1: a reply that says nothing_left beside a field that is not clean reads as that field, never as done (adversaries 2 and 3)", async () => {
  const shown = await resultAfterDelete({ ...clean(), nothing_left: true, files_left: 3, database_left: true });
  assert.equal(shown.text, lines(W.files(3), W.database));
});

test("I-98 (1): entries_left 1 alone has its own line, and no line about a key file", async () => {
  const shown = await resultAfterDelete(deleted({ entries_left: 1 }));
  assert.equal(shown.text, W.entries(1));
  assert.doesNotMatch(shown.text, /key file/);
});

test("I-98 (2): unknown_entries 1 alone has its own line, never the files line", async () => {
  const shown = await resultAfterDelete(deleted({ unknown_entries: 1 }));
  assert.equal(shown.text, W.unknown(1));
  assert.doesNotMatch(shown.text, /could not be removed|key file/);
});

test("L1: the first sentence of the not-followed text shows only beside a destroyed root", async () => {
  for (const [root, extra, tail] of [
    ["no", { cleanup_command: COMMAND }, W.rootLeft],
    ["no", {}, W.rootUnnamed],
    ["unknown", {}, W.rootUnknown],
  ]) {
    const shown = await resultAfterDelete(deleted({ not_followed: 1, root_destroyed: root, ...extra }));
    assert.equal(shown.text, lines(W.notFollowed, tail), root);
    assert.doesNotMatch(shown.text, /destroyed its keys/, root);
  }
});

test("I-98 (3): a root that was not destroyed with no command never asks to remove it with a command", async () => {
  const shown = await resultAfterDelete(deleted({ root_destroyed: "no", cleanup_command: null }));
  assert.equal(shown.text, W.rootUnnamed);
  assert.doesNotMatch(shown.text, /Remove it with this command/);
  assert.equal(shown.cleanup, null);
});

test("L1: no reply with a destroyed root says that the page cannot tell whether a key file is left", async () => {
  for (const fields of [{}, { entries_left: 1 }, { unknown_entries: 2 }, { files_left: 1 }, { database_left: true }, { not_followed: 1 }]) {
    const shown = await resultAfterDelete(deleted({ root_destroyed: "yes", ...fields }));
    assert.doesNotMatch(shown.text, /cannot tell whether a key file is left/, JSON.stringify(fields));
  }
});

test("I-98 (5): the clause on copies stands after the done sentence and after the first sentence of the not-followed text", async () => {
  const done = await resultAfterDelete(clean());
  assert.ok(done.text.startsWith(lines(W.done, W.copies)));
  const notFollowed = await resultAfterDelete(deleted({ not_followed: 1 }));
  assert.ok(notFollowed.text.startsWith(lines(W.keys, W.copies, W.notFollowed)));
});

test("I-98 (6): a delete whose answer never arrived says that Halvo cannot tell whether the data was deleted, beside the error", async () => {
  for (const error of [{ error: "deadline", seconds: 120 }, { error: "channel_failed" }, { error: "broken_reply" }]) {
    const page = await openPage({
      replies: {
        delete_all: () => {
          throw error;
        },
      },
    });
    await confirmDelete(page);
    assert.equal(page.text("delete-result"), "Halvo cannot tell whether your data was deleted. Look at what this page shows as stored.", error.error);
    assert.notEqual(page.text("error"), "", error.error);
  }
  for (const error of [{ error: "busy" }, { error: "bad_store_id" }, { error: "internal" }]) {
    const page = await openPage({
      replies: {
        delete_all: () => {
          throw error;
        },
      },
    });
    await confirmDelete(page);
    assert.doesNotMatch(page.text("delete-result"), /cannot tell whether your data was deleted/, error.error);
  }
});

test("I-98 (13): a delete answered busy says that this press deleted nothing, offers Delete everything again, and makes one call (adversary 3, P8)", async () => {
  const page = await openPage({
    replies: {
      set_capture_state: () => fixture("set_capture_state-paused"),
      delete_all: () => {
        throw { error: "busy" };
      },
    },
  });
  await page.clock.advance(1100);
  await page.click("toggle");
  await confirmDelete(page);
  assert.equal(page.text("delete-result"), "This press deleted nothing. Halvo's capture service is busy with another change. Press Delete everything again in a moment.");
  assert.equal(page.text("error"), "", "no second sentence about the busy service");
  assert.equal(page.byId("delete").disabled, false, "Delete everything is offered again");
  for (let i = 0; i < 10; i++) await page.clock.fire(3000);
  await page.clock.advance(60000);
  assert.equal(page.calls("delete_all").length, 1, "no second ask");
  assert.equal(page.loadsAgain(), 0);
});
