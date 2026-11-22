"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete, loadedAgain } = require("../lib/page.js");

const REFUSED =
  "Halvo did not delete anything this time. What is stored now does not match what this page showed you when you pressed, or Halvo could not read it for a moment. An earlier delete may already have removed your data, and what is stored now may be new. Look at the page, then press Delete everything again if you still want to.";
const TWICE = "This is the second time in a row that Halvo could not delete.";

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
}

function pageAnswering(queue) {
  return openPage({ replies: keptDelete({ delete_all: () => queue.shift()() }) });
}

const refused = () => fixture("delete_all-refused");

test("contracts.md C10.5 (ruling PL-8 (4)(d), (e)): a delete_refused shows the standing words, reads the status and makes one call; the next press refused again adds the one sentence of the second refusal in a row, with no control; any other answer between ends the row", async () => {
  const queue = [refused, refused, refused];
  const page = await pageAnswering(queue);
  const buttons = page.document.body.querySelectorAll("button").length;
  const reads = page.calls("capture_status").length;
  await confirmDelete(page);
  assert.equal(page.text("delete-result"), REFUSED, "the first refusal: the standing words alone");
  assert.ok(page.calls("capture_status").length > reads, "the status is read again");
  for (let i = 0; i < 5; i++) await page.clock.fire(3000);
  await page.clock.advance(20000);
  assert.equal(page.calls("delete_all").length, 1, "the page sends no delete by itself");

  await confirmDelete(page);
  assert.equal(page.calls("delete_all").length, 2);
  assert.equal(page.text("delete-result"), REFUSED + " " + TWICE, "the second refusal in a row");
  assert.equal(page.document.body.querySelectorAll("button").length, buttons, "no control comes with the sentence");
  assert.equal(page.byId("delete").disabled, false, "a new press can arm again");
  await confirmDelete(page);
  assert.equal(page.text("delete-result"), REFUSED + " " + TWICE, "a third refusal in a row keeps the sentence");

  const between = [refused, () => Promise.reject({ error: "busy", outcome_not_known: false }), refused];
  const other = await pageAnswering(between);
  await confirmDelete(other);
  await confirmDelete(other);
  await confirmDelete(other);
  assert.equal(other.calls("delete_all").length, 3);
  assert.equal(other.text("delete-result"), REFUSED, "a busy answer between ends the row");

  const afterDeleted = [refused, () => fixture("delete_all-deleted"), refused];
  const first = await pageAnswering(afterDeleted);
  await confirmDelete(first);
  await confirmDelete(first);
  const next = await loadedAgain(first);
  await confirmDelete(next);
  assert.equal(next.calls("delete_all").length, 1);
  assert.equal(next.text("delete-result"), REFUSED, "a deleted answer between ends the row");
});
