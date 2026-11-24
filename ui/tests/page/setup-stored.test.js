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
const STORED = {
  workbook: "/Volumes/work/<b>Budget</b>.xlsx",
  sheet: "Sheet1",
  range: "A2:C11",
  source: "/Volumes/work/receipts.csv",
};

/** An entry of a typed question of the spreadsheet agent as the core writes it, with the key `stored` where
 * `stored` holds the question's stored line (r4 U3.22g). */
function entry(name, stored) {
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
  const chooser = FILES.includes(name) ? { ...e, choose: words["param_typed.choose"] } : e;
  return stored ? { ...chooser, stored: STORED[name] } : chooser;
}

const [base] = setup.items;
const storedItem = { ...base, params: Object.keys(HINTS).map((n) => entry(n, true)) };
const emptyItem = { ...base, params: Object.keys(HINTS).map((n) => entry(n, false)) };

const lineOf = (page, name) => page.document.querySelector(`input.hn-typed[data-param="${name}"]`);
const setOf = (page, name) => page.document.querySelectorAll("fieldset.hn-param")[Object.keys(HINTS).indexOf(name)];
const useAnswers = (page) => page.document.querySelector('button[data-act="choose"]');

/** Which sheet as the core writes it behind a workbook whose names were read (r4 U3.22c). */
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

const radiosOf = (page, name) => page.document.querySelectorAll(`input.hn-radio[data-param="${name}"]`);

/** The replies of a page whose first snapshot holds `item` and whose next snapshot answers the page's latest wait
 * when `next` is called. */
function replies(item) {
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
    },
  };
}

test("a set-up item whose four questions carry stored lines draws each line filled with its stored line, as text, under its hint; one line turned and Use these answers make the calls with the three stored lines and the new one; where the workbook lacks the sheet's name the list stands with its sentence under it at that press; an item whose questions carry no stored line draws empty lines", async () => {
  const one = replies(storedItem);
  const page = await openPage(one);
  for (const name of Object.keys(HINTS)) {
    assert.equal(lineOf(page, name).value, STORED[name], `${name}: the stored line fills it`);
    assert.deepEqual(
      setOf(page, name).querySelectorAll("p.hn-hint").map((p) => p.textContent),
      [words["param_hint." + HINTS[name]]],
      `${name}: the hint stays under the question's words`,
    );
  }
  assert.equal(setOf(page, "workbook").querySelectorAll("b").length, 0, "the stored line is text, never markup");

  const turned = "/Volumes/work/receipts-december.csv";
  const line = lineOf(page, "source");
  line.focus();
  line.value = turned;
  await line.dispatchEvent(makeEvent("input"));
  await page.settle();
  await page.click(useAnswers(page), { detail: 0 });
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => [c.args.param, c.args.text]),
    [
      ["workbook", STORED.workbook],
      ["sheet", STORED.sheet],
      ["range", STORED.range],
      ["source", turned],
    ],
    "the three stored lines and the new one",
  );
  assert.equal(page.calls("agents_answer").length, 0, "no other call");
  const lacking = ["Plan", "Actual"];
  one.next({ ...setup, items: [{ ...storedItem, params: storedItem.params.map((p) => (p.name === "sheet" ? listEntry(lacking) : p)) }] });
  await page.settle();
  assert.deepEqual(
    radiosOf(page, "sheet").map((r) => r.parentNode.textContent),
    lacking,
    "the list of a workbook that lacks the sheet's name stands",
  );
  assert.deepEqual(
    setOf(page, "sheet").querySelectorAll("p.hn-full").map((p) => p.textContent),
    [words["param_typed.needs_answer"]],
    "with its sentence under it at that press",
  );

  const empty = await openPage(replies(emptyItem));
  assert.deepEqual(
    Object.keys(HINTS).map((name) => lineOf(empty, name).value),
    ["", "", "", ""],
    "no stored line: every line empty",
  );
});
