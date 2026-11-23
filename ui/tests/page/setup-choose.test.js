"use strict";

const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, UI } = require("../lib/page.js");
const { makeEvent } = require("../lib/dom.js");

const COORD = path.join(UI, "tests", "fixtures", "coord");
const read = (name) => JSON.parse(fs.readFileSync(path.join(COORD, name), "utf8"));
const words = read("words.json");
const setup = read("state-needs_setup.json").snapshot;

const HINTS = { workbook: "workbook_mac", sheet: "sheet", range: "range", source: "source_mac" };
const FILES = ["workbook", "source"];

/** An entry of a typed question of the spreadsheet agent as the core writes it: the key `choose`, the control's
 * label, on the two file questions alone. */
function entry(name) {
  const e = {
    name,
    label: words["param." + name],
    candidates: [],
    typed: "yes",
    full: words["param_typed.full"],
    too_long: words["param_typed.too_long"],
    hint: words["param_hint." + HINTS[name]],
  };
  return FILES.includes(name) ? { ...e, choose: words["param_typed.choose"] } : e;
}

const lineOf = (page, name) => page.document.querySelector(`input.hn-typed[data-param="${name}"]`);
const chooseOf = (page, name) => page.document.querySelector(`button[data-act="choose_file"][data-param="${name}"]`);
const useAnswers = (page) => page.document.querySelector('button[data-act="choose"]');

/** A press as the keyboard makes it: a click of detail 0. */
const press = (page, el) => page.click(el, { detail: 0 });

/** The sentences that stand under the line of `name`. */
const sentencesOf = (page, name) => lineOf(page, name).parentNode.querySelectorAll("p.hn-full").map((p) => p.textContent);

/** Types `text` into the line of `name` as a person does, in place of what it held. */
async function typeLine(page, name, text) {
  const line = lineOf(page, name);
  line.focus();
  line.value = text;
  await line.dispatchEvent(makeEvent("input"));
  await page.settle();
}

test("Choose a file stands beside the lines of the two file questions alone; a press makes one call and the picked path fills the line, which goes as a typed line; a closed chooser leaves the line as it was; a second press while a chooser is open makes no call", async () => {
  const [base] = setup.items;
  const item = { ...base, params: Object.keys(HINTS).map(entry) };
  let answer = null;
  const page = await openPage({
    replies: {
      agents_snapshot: { ...setup, items: [item] },
      agents_next: { closed: "yes" },
      agents_setup_typed: { ok: "yes" },
      agents_choose_file: () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    },
  });

  const controls = page.document.querySelectorAll('button[data-act="choose_file"]');
  assert.deepEqual(
    controls.map((b) => b.getAttribute("data-param")),
    FILES,
    "one control for each file question and none for Which sheet or Which cells",
  );
  for (const name of FILES) {
    const control = chooseOf(page, name);
    assert.equal(control.textContent, words["param_typed.choose"], name);
    const kids = Array.from(control.parentNode.childNodes);
    assert.equal(kids.indexOf(control), kids.indexOf(lineOf(page, name)) + 1, `${name}: beside its line`);
  }

  await press(page, chooseOf(page, "workbook"));
  await press(page, chooseOf(page, "workbook"));
  await press(page, chooseOf(page, "source"));
  assert.deepEqual(
    page.calls("agents_choose_file").map((c) => c.args),
    [{ item: item.item, step: item.step, param: "workbook" }],
    "one call, and none while that chooser is open",
  );
  answer({ path: "/Volumes/work/Budget.xlsx" });
  await page.settle();
  assert.equal(lineOf(page, "workbook").value, "/Volumes/work/Budget.xlsx", "the picked path fills the line");

  await typeLine(page, "source", "/Volumes/work/receipts.csv");
  await press(page, chooseOf(page, "source"));
  assert.equal(page.calls("agents_choose_file").length, 2, "a press once the first chooser closed");
  answer({ closed: "yes" });
  await page.settle();
  assert.equal(lineOf(page, "source").value, "/Volumes/work/receipts.csv", "a closed chooser leaves the line as it was");

  await press(page, useAnswers(page));
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => [c.args.param, c.args.text]),
    [
      ["workbook", "/Volumes/work/Budget.xlsx"],
      ["source", "/Volumes/work/receipts.csv"],
    ],
    "the filled line goes as a typed line does",
  );
});
