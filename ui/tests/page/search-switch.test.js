"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const LABEL = "Look for repeated tasks";
const LOOKS_WHEN = "Halvo looks for repeated tasks while this computer is plugged in and not in use.";
const NOTHING_FOUND = "Halvo has not found a repeated task yet.";
const OFF = "Halvo is not looking for repeated tasks.";
const OFF_KEPT = "Halvo is not looking for repeated tasks. What it noticed before stays here until you delete it.";
const BY_HAND = "You can also set up an agent yourself, without waiting for Halvo to notice a task. It fills in cells of an Excel workbook (.xlsx) from a data file.";
const STATE_FILE = "Halvo could not save its settings.";

const PROPOSED = "0123456789abcdef0123456789abcdef";
const proposal = { id: PROPOSED, category: "email", app: "Mail", site: "example.com", count: 5 };
const running = (proposals) => () => ({ state: "running", proposals, stored: [] });
const withSearch = (search) => ({ ...fixture("capture_status-running"), search });

/** The texts of the section's paragraphs that show, in the order of the page. */
const textsShown = (page) =>
  page
    .byId("proposals-section")
    .querySelectorAll("p")
    .filter((p) => page.visible(p))
    .map((p) => p.textContent);

test("r2 search-switch: one switch turns the search for repeated tasks off and on, beside a service of after alone", async () => {
  const before = await openPage({ replies: { workflows: running([]) } });
  assert.equal(before.window.document.getElementById("look-for-tasks"), null, "a service of before: no switch");
  assert.deepEqual(textsShown(before), [NOTHING_FOUND, LOOKS_WHEN, BY_HAND], "a service of before: as today");

  let status = withSearch("on");
  let answer = () => withSearch("off");
  const page = await openPage({
    replies: { capture_status: () => status, workflows: running([proposal]), set_search: (args) => answer(args) },
  });
  assert.equal(page.visible("look-for-tasks"), true);
  assert.equal(page.text("look-for-tasks"), LABEL);
  assert.equal(page.byId("look-for-tasks").getAttribute("aria-checked"), "true", "on");
  assert.deepEqual(textsShown(page).slice(0, 1), [LOOKS_WHEN], "on: the sentence on when Halvo looks, beside rows too");

  await page.click("look-for-tasks", { detail: 1 });
  assert.deepEqual(page.calls("set_search").map((c) => c.args), [{ on: false }], "one request");
  assert.equal(page.byId("look-for-tasks").getAttribute("aria-checked"), "false", "the answer's status: off");
  assert.deepEqual(textsShown(page).slice(0, 1), [OFF_KEPT], "off beside a row: both sentences");
  assert.equal(textsShown(page).includes(LOOKS_WHEN), false);
  assert.equal(page.byId("proposals-list").children.length, 1, "what Halvo noticed stays");

  status = withSearch("off");
  answer = () => Promise.reject({ error: "state_file" });
  await page.click("look-for-tasks", { detail: 1 });
  assert.deepEqual(page.calls("set_search")[1].args, { on: true });
  assert.equal(page.text("error"), STATE_FILE, "a refused turn: the standing sentence on the settings");
  assert.equal(page.byId("look-for-tasks").getAttribute("aria-checked"), "false", "the switch did not turn");

  const empty = await openPage({ replies: { capture_status: () => withSearch("off"), workflows: running([]) } });
  assert.deepEqual(textsShown(empty), [OFF, BY_HAND], "off with an empty list: the first sentence alone, no NOTHING_FOUND");
});
