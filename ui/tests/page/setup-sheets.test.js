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
const NAMES = ["January", "<b>February</b>", "Summary & notes"];

/** An entry of a typed question of the spreadsheet agent as the core writes it. */
function typedEntry(name) {
  const e = {
    name,
    label: words["param." + name],
    candidates: [],
    typed: "yes",
    full: words["param_typed.full"],
    too_long: words["param_typed.too_long"],
    hint: words["param_hint." + HINTS[name]],
    needs_answer: words["param_typed.needs_answer"],
  };
  return FILES.includes(name) ? { ...e, choose: words["param_typed.choose"] } : e;
}

/** Which sheet as the core writes it behind a workbook whose names were read (r4 U3.22c): a list from files, no
 * typed line, no hint, and the sentence of an empty line for a list with no choice. */
function listEntry(names) {
  return {
    name: "sheet",
    label: words["param.sheet"],
    candidates: names.map((value, i) => ({ n: i + 1, value: [value], from: "files" })),
    typed: "no",
    full: "",
    too_long: "",
    hint: "",
    needs_answer: words["param_typed.needs_answer"],
  };
}

const [base] = setup.items;
const typedItem = { ...base, params: Object.keys(HINTS).map(typedEntry) };
const listItem = { ...base, params: Object.keys(HINTS).map((n) => (n === "sheet" ? listEntry(NAMES) : typedEntry(n))) };

const lineOf = (page, name) => page.document.querySelector(`input.hn-typed[data-param="${name}"]`);
const radiosOf = (page, name) => page.document.querySelectorAll(`input.hn-radio[data-param="${name}"]`);
const chooseOf = (page, name) => page.document.querySelector(`button[data-act="choose_file"][data-param="${name}"]`);
const useAnswers = (page) => page.document.querySelector('button[data-act="choose"]');
const setOf = (page, name) => page.document.querySelectorAll("fieldset.hn-param")[Object.keys(HINTS).indexOf(name)];
const press = (page, el) => page.click(el, { detail: 0 });

async function typeLine(page, name, text) {
  const line = lineOf(page, name);
  line.focus();
  line.value = text;
  await line.dispatchEvent(makeEvent("input"));
  await page.settle();
}

/** The replies of a page whose first snapshot holds `item` and whose next snapshot answers the page's latest wait
 * when `next` is called. */
function replies(item, extra = {}) {
  let next = null;
  return {
    next: (snapshot) => next({ ...snapshot, seq: setup.seq + 1 }),
    replies: {
      agents_snapshot: { ...setup, items: [item] },
      agents_next: () =>
        new Promise((resolve) => {
          next = resolve;
        }),
      agents_setup_typed: { ok: "yes" },
      agents_answer: { ok: "yes" },
      ...extra,
    },
  };
}

test("Which sheet with a list of three names draws three choices, each name as text, no typed line and no hint; a choice and Use these answers make the call of a choice with its number; a press with no choice shows the sentence of an empty line under the list with the focus on its first choice; without the list the typed line stands with its hint", async () => {
  const listed = await openPage(replies(listItem));
  const radios = radiosOf(listed, "sheet");
  assert.deepEqual(
    radios.map((r) => r.getAttribute("data-n")),
    ["1", "2", "3"],
    "three choices",
  );
  assert.deepEqual(
    radios.map((r) => r.parentNode.textContent),
    NAMES,
    "each name as its text",
  );
  assert.equal(setOf(listed, "sheet").querySelectorAll("b").length, 0, "no name is drawn as markup");
  assert.equal(lineOf(listed, "sheet"), null, "no typed line");
  assert.equal(setOf(listed, "sheet").querySelectorAll("p.hn-hint").length, 0, "no hint");

  await typeLine(listed, "workbook", "/Volumes/work/Budget.xlsx");
  await typeLine(listed, "range", "A2:C11");
  await typeLine(listed, "source", "/Volumes/work/receipts.csv");
  await press(listed, useAnswers(listed));
  assert.deepEqual(
    setOf(listed, "sheet").querySelectorAll("p.hn-full").map((p) => p.textContent),
    [words["param_typed.needs_answer"]],
    "the sentence of an empty line under a list with no choice",
  );
  assert.equal(listed.document.activeElement, radiosOf(listed, "sheet")[0], "the focus on its first choice");
  assert.equal(listed.calls("agents_answer").length, 0, "no choice was sent");

  radios[1].checked = true;
  await radios[1].dispatchEvent(makeEvent("change"));
  await listed.settle();
  assert.equal(setOf(listed, "sheet").querySelectorAll("p.hn-full").length, 0, "a choice takes the sentence away");
  await press(listed, useAnswers(listed));
  assert.deepEqual(
    listed.calls("agents_answer").map((c) => c.args),
    [{ item: listItem.item, step: listItem.step, choice: "choose", param: "sheet", n: 2 }],
    "the call of a choice with its number",
  );

  const typed = await openPage(replies(typedItem));
  assert.equal(radiosOf(typed, "sheet").length, 0, "no list");
  assert.notEqual(lineOf(typed, "sheet"), null, "the typed line stands");
  assert.deepEqual(
    setOf(typed, "sheet").querySelectorAll("p.hn-hint").map((p) => p.textContent),
    [words["param_hint.sheet"]],
    "with its hint",
  );
});

test("a pick in the chooser of Which spreadsheet file sends its line at once and brings the list with no press, the other lines kept; where the pick is the last answer that the item lacks it makes no call before the press of Use these answers", async () => {
  const path = "/Volumes/work/Budget.xlsx";
  const one = replies(typedItem, { agents_choose_file: { path } });
  const page = await openPage(one);
  await typeLine(page, "range", "A2:C11");
  await press(page, chooseOf(page, "workbook"));
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => c.args),
    [{ item: typedItem.item, step: typedItem.step, param: "workbook", text: path }],
    "the picked line goes at once",
  );
  one.next({ ...setup, items: [listItem] });
  await page.settle();
  assert.equal(radiosOf(page, "sheet").length, NAMES.length, "the list stands");
  assert.equal(page.calls("agents_answer").length, 0, "with no press of Use these answers");
  assert.equal(lineOf(page, "workbook").value, path, "the picked line stays");
  assert.equal(lineOf(page, "range").value, "A2:C11", "a typed line stays");

  const last = await openPage(replies(typedItem, { agents_choose_file: { path } }));
  await typeLine(last, "sheet", "January");
  await typeLine(last, "range", "A2:C11");
  await typeLine(last, "source", "/Volumes/work/receipts.csv");
  await press(last, useAnswers(last));
  assert.equal(last.calls("agents_setup_typed").length, 3, "the three other answers are held by the core");
  await press(last, chooseOf(last, "workbook"));
  assert.equal(lineOf(last, "workbook").value, path, "the pick fills the line");
  assert.equal(last.calls("agents_setup_typed").length, 3, "the last answer lacking makes no call at the pick");
  await press(last, useAnswers(last));
  assert.deepEqual(
    last.calls("agents_setup_typed").map((c) => c.args.param),
    ["sheet", "range", "source", "sheet", "range", "source", "workbook"],
    "it goes at the press, behind the lines that the core holds",
  );
});
