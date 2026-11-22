"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const W = {
  ending: "This press deleted nothing, because Halvo's capture service was shutting down.",
  nothingRemoved: "Everything was already deleted, so this press deleted nothing.",
  notBack: "Halvo's capture service shut down during your delete and did not start again in time, so Halvo could not ask it again.",
  unknown: "Halvo cannot tell whether your data was deleted. Look at what this page shows as stored.",
  notAnswering: "Halvo's capture service did not take the request.",
  busy: "Halvo's capture service is busy with another change. Try again in a moment.",
  deleteBusy: "This press deleted nothing. Halvo's capture service is busy with another change. Press Delete everything again in a moment.",
  pauseNotReached: "Capture was not paused. Try again, or quit Halvo to stop capture.",
};

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
}

function rejecting(error) {
  return () => Promise.reject(error);
}

test("I-82 (5), rulings CT-13 (1) and AP-20: the error of a life that is ending and the nothing-removed error each draw their own sentence for a delete, never ERROR_BUSY's, and the ending life's kind reaches a Pause and a Resume", async () => {
  const rows = [
    ["the typed busy of a life that is ending", { error: "service_ending", outcome_not_known: false }, W.ending],
    ["an already_deleted that answers no request of this press", { error: "nothing_removed", outcome_not_known: false }, W.nothingRemoved],
    ["no life answered the second ask, after a busy", { error: "service_not_back", outcome_not_known: false }, W.notBack],
    ["no life answered the second ask, after no reply", { error: "service_not_back", outcome_not_known: true }, W.notBack + " " + W.unknown],
  ];
  for (const [name, error, words] of rows) {
    const page = await openPage({ replies: { delete_all: rejecting(error) } });
    await confirmDelete(page);
    const shown = page.text("delete-result");
    assert.equal(shown, words, name);
    assert.equal(page.text("error"), "", `${name}: no second sentence beside it`);
    assert.ok(!shown.includes(W.busy) && !shown.includes(W.deleteBusy), `${name}: never the busy sentence`);
    assert.equal(page.calls("delete_all").length, 1, `${name}: one call`);
    assert.equal(page.byId("delete").disabled, false, `${name}: offered again`);
  }

  const words = (await openPage()).window.Halvo;
  for (const kind of ["service_ending", "nothing_removed", "service_not_back"]) {
    const sentence = words.errorWords({ error: kind });
    assert.notEqual(sentence, words.words.ERROR_BUSY, kind);
    assert.notEqual(sentence, words.words.ERROR_OTHER, `${kind}: a sentence of its own kind`);
  }
  assert.equal(words.errorWords({ error: "service_ending" }), W.notAnswering);

  const resume = await openPage({
    replies: {
      capture_status: () => fixture("capture_status-paused"),
      set_capture_state: rejecting({ error: "service_ending" }),
    },
  });
  await resume.clock.advance(1100);
  await resume.click("toggle");
  assert.equal(resume.calls("set_capture_state").length, 1);
  assert.equal(resume.text("error"), W.notAnswering, "a Resume that met an ending life");

  const pause = await openPage({ replies: { set_capture_state: rejecting({ error: "service_ending" }) } });
  await pause.clock.advance(1100);
  await pause.click("toggle");
  assert.equal(pause.calls("set_capture_state").length, 1);
  assert.equal(pause.text("error"), W.pauseNotReached, "a Pause that met an ending life did not happen");
});
