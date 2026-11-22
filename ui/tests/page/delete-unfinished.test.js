"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, KEPT_AT_MS } = require("../lib/page.js");

const FOLDER = "/scratch/halvo-data";
const NOT_FINISHED = "The delete did not finish. Press Delete everything again to finish it.";
const AGAIN = `The delete did not finish a second time in a row. Something is still left in Halvo's data folder, ${FOLDER}.`;

const notRunning = () => ({ ...fixture("capture_status-not-running"), data_folder: FOLDER });

function leftSomething(inARow) {
  return {
    ...fixture("delete_all-deleted"),
    segments_shredded: null,
    keys_destroyed: null,
    root_destroyed: "no",
    files_left: 1,
    entries_left: 1,
    new_store_id: null,
    cleanup_command: null,
    looked_in: FOLDER,
    nothing_left: false,
    unfinished_in_a_row: inARow,
  };
}

function appKeeping(answers) {
  const app = { kept: { state: "none" }, sent: 0, release: null };
  app.replies = {
    capture_status: notRunning,
    last_delete: () => app.kept,
    delete_offline: () => {
      app.sent += 1;
      app.kept = { state: "in_flight" };
      const answer = answers.shift();
      return new Promise((resolve) => {
        app.release = () => {
          app.kept = { state: "answered", at_ms: KEPT_AT_MS, answer };
          resolve(answer);
        };
      });
    },
  };
  return app;
}

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  const pressed = page.press("confirm-delete");
  await page.settle();
  return { pressed };
}

test("a second unfinished delete says that something is left", async () => {
  const app = appKeeping([leftSomething(1), leftSomething(2)]);
  const page = await openPage({ replies: app.replies });
  assert.equal(page.text("toggle"), "Start", "a not_running reply shows Start");
  assert.equal(page.byId("toggle").disabled, false);
  assert.equal(page.byId("delete").disabled, false, "and Delete everything");
  await page.click("delete");
  assert.match(page.text("delete-question"), new RegExp(`its data folder, ${FOLDER}\\?`), "the question names the folder");
  await page.clock.advance(600);
  const pressed = page.press("confirm-delete");
  await page.settle();
  assert.equal(page.calls("delete_offline").length, 1, "the confirmed press sends the offline delete once");
  assert.equal(page.calls("delete_offline")[0].args, undefined, "with no id");
  assert.equal(page.calls("delete_all").length, 0);
  await confirmDelete(page);
  assert.equal(app.sent, 1, "a second press sends none while the first runs");
  app.release();
  await pressed;
  await page.settle();
  assert.equal(page.loadsAgain(), 1);

  const first = await page.reopen();
  assert.ok(first.text("delete-result").includes(NOT_FINISHED), "the first unfinished answer");
  assert.equal(first.byId("delete").disabled, false, "beside the offered control at the offline door");
  assert.ok(!first.text("delete-result").includes(AGAIN));
  const again = await confirmDelete(first);
  assert.equal(app.sent, 2);
  app.release();
  await again.pressed;
  await first.settle();
  const second = await first.reopen();
  assert.ok(second.text("delete-result").includes(AGAIN), "the next in a row");
  assert.ok(second.text("delete-result").includes(second.window.Halvo.words.REMOVE_BY_HAND), "and what a person does by hand with that folder");
  assert.ok(!second.text("delete-result").includes(NOT_FINISHED));
});

test("behind the running service a delete that did not finish offers Delete everything and its state line says to press it", async () => {
  const kept = () => ({ state: "answered", at_ms: KEPT_AT_MS, answer: leftSomething(1) });
  const unfinished = await openPage({
    replies: { capture_status: () => fixture("capture_status-store-deleting-unfinished"), last_delete: kept },
  });
  assert.equal(unfinished.byId("delete").disabled, false, "the last delete did not finish, so Delete everything can be pressed again");
  assert.equal(unfinished.text("store-state"), "The last delete did not finish. Press Delete everything to finish it.");
  assert.ok(unfinished.text("delete-result").includes(NOT_FINISHED), "the kept answer says to press again");
  assert.ok(!unfinished.text("delete-result").includes(AGAIN));
  const deleting = await openPage({
    replies: { capture_status: () => fixture("capture_status-store-deleting"), last_delete: kept },
  });
  assert.equal(deleting.byId("delete").disabled, true, "a delete runs, so Delete everything cannot be pressed");
  assert.equal(deleting.text("store-state"), "Halvo is deleting everything.");
});

test("an offline delete whose two counts read unknown says that it deleted, names the folder and gives no count", async () => {
  const unknownCounts = {
    ...fixture("delete_all-deleted"),
    segments_shredded: null,
    keys_destroyed: null,
    new_store_id: null,
    looked_in: FOLDER,
  };
  const app = appKeeping([unknownCounts]);
  const page = await openPage({ replies: app.replies });
  const { pressed } = await confirmDelete(page);
  assert.equal(page.calls("delete_offline").length, 1, "the confirmed press sends the offline delete");
  app.release();
  await pressed;
  await page.settle();
  const next = await page.reopen();
  const shown = next.text("delete-result");
  const w = next.window.Halvo.words;
  assert.ok(shown.endsWith([w.DELETE_DONE, w.DELETE_COPIES, `The delete ran in Halvo's data folder, ${FOLDER}.`].join(" ")), shown);
  assert.doesNotMatch(shown.replace(/^At \d\d:\d\d: /, "").replace(FOLDER, ""), /\d/, "no count");
});

test("on Windows a second unfinished delete says what a person does by hand in File Explorer, and a Mac keeps the Finder", async () => {
  const BY_HAND_WINDOWS =
    "To remove what is left yourself, quit Halvo, open that folder in File Explorer, delete what you do not want to keep, and empty the Recycle Bin.";
  const app = appKeeping([leftSomething(2)]);
  app.replies.capture_status = () => ({ ...notRunning(), platform: "windows" });
  const page = await openPage({ replies: app.replies });
  const { pressed } = await confirmDelete(page);
  app.release();
  await pressed;
  await page.settle();
  const next = await page.reopen();
  await next.settle();
  const shown = next.text("delete-result");
  assert.ok(shown.includes(AGAIN), shown);
  assert.ok(shown.includes(BY_HAND_WINDOWS), shown);
  assert.ok(!shown.includes(next.window.Halvo.words.REMOVE_BY_HAND), shown);
  const mac = appKeeping([leftSomething(2)]);
  const onMac = await openPage({ replies: mac.replies });
  const macPress = await confirmDelete(onMac);
  mac.release();
  await macPress.pressed;
  await onMac.settle();
  const macShown = (await onMac.reopen()).text("delete-result");
  assert.ok(macShown.includes(onMac.window.Halvo.words.REMOVE_BY_HAND), macShown);
  assert.ok(!macShown.includes(BY_HAND_WINDOWS), macShown);
});

test("a crash report of Halvo's own window that stayed at the end of a whole delete is said with its folder, beside the delete's own words", async () => {
  const REPORTS = "D:\\halvo-webview\\Crashpad\\reports";
  const whole = { ...fixture("delete_all-deleted"), new_store_id: null, looked_in: FOLDER };
  const app = appKeeping([{ ...whole, crash_reports_left: REPORTS }]);
  const page = await openPage({ replies: app.replies });
  const { pressed } = await confirmDelete(page);
  app.release();
  await pressed;
  await page.settle();
  const next = await page.reopen();
  const shown = next.text("delete-result");
  const w = next.window.Halvo.words;
  const own = [w.DELETE_DONE, w.DELETE_COPIES, `The delete ran in Halvo's data folder, ${FOLDER}.`].join(" ");
  const said = `Halvo could not remove a crash report of its own window in ${REPORTS}. The report can hold what the window showed. Remove it yourself.`;
  assert.ok(shown.endsWith(`${own} ${said}`), shown);
});
