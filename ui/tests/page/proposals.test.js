"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

const PROPOSED = "0123456789abcdef0123456789abcdef";
const STORED = "fedcba9876543210fedcba9876543210";

const running = (proposals, stored) => () => ({ state: "running", proposals, stored });

const controlsOf = (page) => page.byId("proposals-section").querySelectorAll("button");

test("a proposal row says what was seen and how often, and its two controls send their decision", async () => {
  const proposal = { id: PROPOSED, category: "email", app: "Mail", site: "example.com", count: 5 };
  const page = await openPage({
    replies: { workflows: running([proposal], []), approve_workflow: () => null, dismiss_workflow: () => null },
  });
  const rows = page.byId("proposals-list").children;
  assert.equal(rows.length, 1);
  const [seen, count, controls] = rows[0].children;
  assert.equal(seen.textContent, "Writing or answering email in Mail on example.com");
  assert.equal(count.textContent, "Seen 5 times.");
  const [approve, notNow] = controls.children;
  assert.deepEqual([approve.textContent, notNow.textContent], ["Approve", "Not now"]);
  assert.equal(approve.getAttribute("aria-describedby"), seen.id);
  await page.click(approve);
  assert.deepEqual(page.calls("approve_workflow").map((c) => c.args), [{ workflow: PROPOSED }]);
  await page.click(notNow);
  assert.deepEqual(page.calls("dismiss_workflow").map((c) => c.args), [{ workflow: PROPOSED }]);
  assert.equal(page.text("error"), "");
});

test("a stored workflow is deleted only after the person confirms", async () => {
  const stored = { id: STORED, category: "ride_order", app: null, site: "example.org", count: 0 };
  const page = await openPage({ replies: { workflows: running([], [stored]), delete_workflow: () => null } });
  assert.equal(page.text("stored-workflows-line"), "Workflows stored on this computer");
  assert.equal(page.visible("stored-workflows-line"), true);
  const rows = page.byId("stored-workflows").children;
  assert.equal(rows.length, 1);
  const [line, confirmRow] = rows[0].children;
  const [seen, start] = line.children;
  assert.equal(seen.textContent, "Ordering a ride on example.org");
  assert.equal(start.textContent, "Delete");
  assert.equal(page.visible(confirmRow), false);
  await page.click(start);
  assert.equal(page.calls("delete_workflow").length, 0);
  assert.equal(page.visible(confirmRow), true);
  const [question, buttons] = confirmRow.children;
  assert.equal(question.textContent, "Delete this workflow? This cannot be undone.");
  const [yes, cancel] = buttons.children;
  assert.deepEqual([yes.textContent, cancel.textContent], ["Yes, delete it", "Cancel"]);
  await page.click(yes);
  assert.deepEqual(page.calls("delete_workflow").map((c) => c.args), [{ workflow: STORED }]);
});

test("the two empty states say what is true", async () => {
  const off = await openPage();
  assert.equal(off.visible("workflows-not-looking"), true);
  assert.equal(off.text("workflows-not-looking"), "Halvo is not looking for repeated tasks yet.");
  assert.equal(off.visible("workflows-nothing-found"), false);
  assert.equal(off.text("error"), "");
  const on = await openPage({ replies: { workflows: running([], []) } });
  assert.equal(on.visible("workflows-nothing-found"), true);
  assert.equal(on.text("workflows-nothing-found"), "Halvo has not found a repeated task yet.");
  assert.equal(on.visible("workflows-not-looking"), false);
  for (const page of [off, on]) {
    assert.equal(page.byId("proposals-list").children.length, 0);
    assert.equal(page.byId("stored-workflows").children.length, 0);
    assert.equal(page.visible("stored-workflows-line"), false);
    assert.equal(controlsOf(page).length, 0);
  }
});

test("the view reads workflows once and not again on a status reply", async () => {
  const page = await openPage();
  const statusBefore = page.calls("capture_status").length;
  await page.clock.advance(60000);
  assert.ok(page.calls("capture_status").length >= statusBefore + 2, "two status replies came in the 60 s");
  assert.equal(page.calls("workflows").length, 1);
});
