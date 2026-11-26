"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, keptDelete } = require("../lib/page.js");

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

test("r3 U7.11: the card of a proposal that this version cannot do says so and offers Not now alone", async () => {
  const cannot = { id: PROPOSED, category: "utility_bill_payment", app: null, site: "example.com", count: 3, cannot_do: true };
  const can = { id: STORED, category: "utility_bill_payment", app: null, site: "example.org", count: 2, cannot_do: false };
  const page = await openPage({ replies: { workflows: running([cannot, can], []), dismiss_workflow: () => null } });
  const rows = page.byId("proposals-list").children;
  assert.equal(rows.length, 2);
  const texts = (row) => Array.from(row.querySelectorAll("p")).map((p) => p.textContent);
  const buttons = (row) => Array.from(row.querySelectorAll("button"));
  assert.deepEqual(texts(rows[0]), [
    "Paying a utility bill on example.com",
    "Seen 3 times.",
    "This version of Halvo cannot do this task.",
  ]);
  assert.deepEqual(buttons(rows[0]).map((b) => b.textContent), ["Not now"]);
  assert.deepEqual(texts(rows[1]), ["Paying a utility bill on example.org", "Seen 2 times."]);
  assert.deepEqual(buttons(rows[1]).map((b) => b.textContent), ["Approve", "Not now"]);
  await page.click(buttons(rows[0])[0]);
  assert.deepEqual(page.calls("dismiss_workflow").map((c) => c.args), [{ workflow: PROPOSED }]);
  assert.equal(page.calls("approve_workflow").length, 0);
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

/** The texts of the view's paragraphs that show, in the order of the page. */
const textsShown = (page) =>
  page
    .byId("proposals-section")
    .querySelectorAll("p")
    .filter((p) => page.visible(p))
    .map((p) => p.textContent);

test("the two empty states say what is true", async () => {
  const off = await openPage();
  assert.deepEqual(textsShown(off), ["Halvo is not looking for repeated tasks yet."]);
  const on = await openPage({ replies: { workflows: running([], []) } });
  assert.deepEqual(textsShown(on), [
    "Halvo has not found a repeated task yet.",
    "Halvo looks for repeated tasks while this computer is plugged in and not in use.",
    "You can also set up an agent yourself, without waiting for Halvo to notice a task. It fills in cells of an Excel workbook (.xlsx) from a data file.",
  ]);
  for (const [page, controls] of [
    [off, []],
    [on, ["Set up a spreadsheet agent"]],
  ]) {
    assert.equal(page.byId("proposals-list").children.length, 0);
    assert.equal(page.byId("stored-workflows").children.length, 0);
    assert.deepEqual(
      controlsOf(page).map((b) => b.textContent),
      controls,
    );
    assert.equal(page.text("error"), "");
  }
  const stored = { id: STORED, category: "ride_order", app: null, site: "example.org", count: 0 };
  const some = await openPage({ replies: { workflows: running([], [stored]) } });
  assert.deepEqual(textsShown(some), ["Workflows stored on this computer", "You can also set up an agent yourself, without waiting for Halvo to notice a task. It fills in cells of an Excel workbook (.xlsx) from a data file."]);
});

test("r4 U3.27: below the stored workflows a person can set up the spreadsheet agent by hand while no workflow set up by hand stands, and reads why not while one stands", async () => {
  const C1 = "You can also set up an agent yourself, without waiting for Halvo to notice a task. It fills in cells of an Excel workbook (.xlsx) from a data file.";
  const C2 = "Set up a spreadsheet agent";
  const C3 = "You have set up a spreadsheet agent yourself. Halvo keeps one such agent at a time: to set up another, delete this one first.";
  const MAIN = ["Halvo has not found a repeated task yet.", "Halvo looks for repeated tasks while this computer is plugged in and not in use."];
  const byHand = { id: STORED, category: "spreadsheet_fill", app: null, site: null, count: 0, by_hand: true };
  const replies = [running([], [])(), running([], [byHand])()];
  const page = await openPage({ replies: { workflows: () => replies.shift(), set_up_sheet_agent: () => null } });
  assert.deepEqual(textsShown(page), [...MAIN, C1]);
  const [setUp] = controlsOf(page);
  assert.equal(setUp.textContent, C2);
  assert.equal(page.visible(setUp), true);
  await page.click(setUp);
  assert.deepEqual(page.calls("set_up_sheet_agent").map((c) => c.args), [undefined]);
  assert.equal(page.calls("workflows").length, 2);
  assert.deepEqual(textsShown(page), [...MAIN, "Workflows stored on this computer", C3]);
  assert.deepEqual(
    controlsOf(page).filter((b) => page.visible(b)).map((b) => b.textContent),
    ["Delete"],
  );
  const fromProposal = { ...byHand, by_hand: false };
  const beside = await openPage({ replies: { workflows: running([], [fromProposal]) } });
  assert.deepEqual(textsShown(beside), ["Workflows stored on this computer", C1]);
  assert.deepEqual(
    controlsOf(beside).filter((b) => beside.visible(b)).map((b) => b.textContent),
    ["Delete", C2],
  );
  assert.equal(page.text("error"), "");
});

test("each state without a list shows its one text and no row", async () => {
  const texts = {
    not_running: "Halvo is not looking for repeated tasks yet.",
    store_unavailable: "Halvo's store cannot be opened right now.",
    deleting: "Halvo is deleting everything.",
    delete_unfinished: "The last delete did not finish. Press Delete everything to finish it.",
  };
  for (const [state, text] of Object.entries(texts)) {
    const page = await openPage({ replies: { workflows: () => ({ state }) } });
    assert.deepEqual(textsShown(page), [text], state);
    assert.equal(page.byId("proposals-list").children.length, 0, state);
    assert.equal(page.byId("stored-workflows").children.length, 0, state);
    assert.equal(controlsOf(page).length, 0, state);
    assert.equal(page.text("error"), "", state);
  }
  const stored = { id: STORED, category: "ride_order", app: null, site: "example.org", count: 0 };
  const held = await openPage({
    replies: { workflows: running([], [stored]), delete_workflow: () => Promise.reject({ error: "in_use" }) },
  });
  const [start, yes] = controlsOf(held);
  await held.click(start);
  await held.click(yes);
  assert.deepEqual(held.calls("delete_workflow").map((c) => c.args), [{ workflow: STORED }]);
  assert.equal(held.text("error"), "Halvo did not delete this workflow, because an agent is still using it. Try again later.");
  assert.equal(held.byId("stored-workflows").children.length, 1);
  assert.deepEqual(
    controlsOf(held).map((b) => [b.textContent, Boolean(b.disabled)]),
    [
      ["Delete", false],
      ["Yes, delete it", false],
      ["Cancel", false],
      ["Set up a spreadsheet agent", false],
    ],
  );
});

test("behind an answer of a delete that did not finish the view reads again and shows what that reply holds", async () => {
  const stored = { id: STORED, category: "ride_order", app: null, site: "example.org", count: 0 };
  const replies = [{ state: "running", proposals: [], stored: [stored] }, { state: "delete_unfinished" }];
  const page = await openPage({
    replies: keptDelete({
      workflows: () => replies.shift(),
      delete_all: () => Promise.reject({ error: "internal", outcome_not_known: true }),
    }),
  });
  assert.equal(page.byId("stored-workflows").children.length, 1);
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  await page.settle();
  assert.equal(page.calls("delete_all").length, 1);
  assert.equal(page.loadsAgain(), 0);
  assert.equal(page.calls("workflows").length, 2);
  assert.deepEqual(textsShown(page), ["The last delete did not finish. Press Delete everything to finish it."]);
  assert.equal(page.byId("stored-workflows").children.length, 0);
});

test("while the newest reply reads deleting the view reads again every 3 s, and after another reply no timer reads", async () => {
  const replies = [{ state: "deleting" }, { state: "delete_unfinished" }];
  const page = await openPage({ replies: { workflows: () => replies.shift() } });
  assert.deepEqual(textsShown(page), ["Halvo is deleting everything."]);
  await page.clock.advance(2999);
  assert.equal(page.calls("workflows").length, 1);
  await page.clock.advance(1);
  assert.equal(page.calls("workflows").length, 2);
  assert.deepEqual(textsShown(page), ["The last delete did not finish. Press Delete everything to finish it."]);
  await page.clock.advance(60000);
  assert.equal(page.calls("workflows").length, 2);
  assert.equal(page.text("error"), "");
});

test("the view reads again at a show and after a decision, and not on a status reply", async () => {
  const proposal = { id: PROPOSED, category: "email", app: null, site: null, count: 2 };
  const page = await openPage({
    replies: {
      workflows: running([proposal], []),
      approve_workflow: () => null,
      dismiss_workflow: () => Promise.reject({ error: "not_running" }),
    },
  });
  const reads = () => page.calls("workflows").length;
  assert.equal(reads(), 1, "the first show reads");
  page.window.Halvo.views.window("shown");
  await page.settle();
  assert.equal(reads(), 2, "a second show reads again");
  await page.click(controlsOf(page)[0]);
  assert.equal(page.calls("approve_workflow").length, 1);
  assert.equal(reads(), 3, "an Approve that succeeded reads again");
  await page.click(controlsOf(page)[1]);
  assert.equal(page.calls("dismiss_workflow").length, 1);
  assert.equal(page.text("error"), "The request failed.");
  assert.equal(reads(), 3, "a decision that found no service reads nothing");
  const statusBefore = page.calls("capture_status").length;
  await page.clock.advance(60000);
  assert.ok(page.calls("capture_status").length >= statusBefore + 2, "two status replies came in the 60 s");
  assert.equal(reads(), 3, "a status reply reads nothing");
});
