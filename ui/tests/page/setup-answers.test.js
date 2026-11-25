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
const ten = read("state-ten.json").snapshot;

const HINTS = { workbook: "workbook_mac", sheet: "sheet", range: "range", source: "source_mac" };

/** An entry of a typed question of the spreadsheet agent as the core writes it, with its hint, and with `notTaken` as
 * the sentence of a line that the core did not take, where given. */
function entry(name, notTaken) {
  const e = {
    name,
    label: words["param." + name],
    candidates: [],
    typed: "yes",
    full: words["param_typed.full"],
    too_long: words["param_typed.too_long"],
    hint: words["param_hint." + HINTS[name]],
  };
  return notTaken ? { ...e, not_taken: notTaken } : e;
}

/** The state of the core's fixture needs_setup with the spreadsheet agent's four questions in its item, the first
 * carrying `notTaken`. */
function sheetSetup(notTaken) {
  const [item] = setup.items;
  const params = Object.keys(HINTS).map((name, i) => entry(name, i === 0 ? notTaken : null));
  return { ...setup, items: [{ ...item, params }] };
}

async function open(snapshot) {
  return openPage({
    replies: { agents_snapshot: snapshot, agents_next: { closed: "yes" }, agents_setup_typed: { ok: "yes" }, agents_control: { ok: "yes" } },
  });
}

/** The ids that a line names as what describes it. */
const describedBy = (line) => (line.getAttribute("aria-describedby") || "").split(/\s+/).filter(Boolean);

/** A press as the keyboard makes it: a click of detail 0. */
const press = (page, el) => page.click(el, { detail: 0 });

/** A stand-in of the core for the spreadsheet agent's set-up item, as the page reads it: it answers each typed line,
 * taking a line of a path question only where it starts with a slash, as a full path of a Mac does, and every other
 * line as typed; a line that it does not take empties that question and carries its sentence. It publishes a new
 * state, with a new seq, only where the state changed, as the core does, and closes the item when every question holds
 * a value. `other(id)` puts up an item of another id. */
function sheetCore() {
  const [base] = setup.items;
  const fresh = () => Object.keys(HINTS).map((name) => entry(name, null));
  let seq = setup.seq;
  let item = { ...base, params: fresh() };
  const held = new Map();
  let waiting = [];
  const snapshot = () => ({ ...setup, seq, items: item ? [item] : [] });
  const publish = (next) => {
    if (JSON.stringify(next) === JSON.stringify(item)) return;
    item = next;
    seq += 1;
    const now = snapshot();
    waiting.forEach((resolve) => resolve(now));
    waiting = [];
  };
  const replies = {
    agents_snapshot: () => snapshot(),
    agents_next: (args) => (args.after !== seq ? snapshot() : new Promise((resolve) => waiting.push(resolve))),
    agents_setup_typed: (args) => {
      const path = args.param === "workbook" || args.param === "source";
      const taken = !path || args.text.startsWith("/");
      if (taken) held.set(args.param, args.text);
      else held.delete(args.param);
      const notTaken = taken ? null : words["param_typed.not_path_mac"];
      const params = item.params.map((p) => (p.name === args.param ? entry(p.name, notTaken) : p));
      publish(held.size === params.length ? null : { ...item, params });
      return { ok: "yes" };
    },
    agents_control: { ok: "yes" },
  };
  return { replies, other: (id) => publish({ ...base, item: id, params: fresh() }) };
}

const lineOf = (page, name) => page.document.querySelector(`input.hn-typed[data-param="${name}"]`);

/** Types `text` into the line of `name` as a person does, in place of what it held. */
async function typeLine(page, name, text) {
  const line = lineOf(page, name);
  line.focus();
  line.value = text;
  await line.dispatchEvent(makeEvent("input"));
  await page.settle();
}

/** The sentences that stand under the line of `name`. */
const sentencesOf = (page, name) => lineOf(page, name).parentNode.querySelectorAll("p.hn-full").map((p) => p.textContent);

const useAnswers = (page) => page.document.querySelector('button[data-act="choose"]');

test("each question draws its hint under its words and names it to its line, a line that the core did not take shows its sentence under it until the core takes an answer for it, and Change its answers calls agents_control once", async () => {
  const page = await open(sheetSetup(null));
  const sets = page.document.querySelectorAll("fieldset.hn-param");
  assert.equal(sets.length, 4);
  for (const set of sets) {
    const line = set.querySelector("input.hn-typed");
    const name = line.getAttribute("data-param");
    const hint = set.querySelector("p.hn-hint");
    assert.ok(hint, `${name}: a hint`);
    assert.equal(hint.textContent, words["param_hint." + HINTS[name]], name);
    const kids = Array.from(set.childNodes);
    assert.ok(kids.indexOf(set.querySelector("legend")) < kids.indexOf(hint), `${name}: under the question's words`);
    assert.ok(kids.indexOf(hint) < kids.indexOf(line), `${name}: above its line`);
    assert.ok(hint.id && describedBy(line).includes(hint.id), `${name}: the hint describes the line`);
  }

  const sentence = words["param_typed.not_path_mac"];
  const refused = await openPage({ replies: sheetCore().replies });
  await typeLine(refused, "workbook", "Budget.xlsx");
  await press(refused, useAnswers(refused));
  const line = lineOf(refused, "workbook");
  const set = line.parentNode;
  const note = set.childNodes[set.childNodes.indexOf(line) + 1];
  assert.ok(note && note.textContent === sentence, "the sentence stands under the line");
  assert.ok(describedBy(line).includes(note.id), "the sentence describes the line");
  assert.ok(describedBy(line).includes(set.querySelector("p.hn-hint").id), "the hint still describes the line");
  line.focus();
  line.value = "/Users/ana/Documents/Budget.xlsx";
  await line.dispatchEvent(makeEvent("input"));
  await refused.settle();
  assert.ok(set.textContent.includes(sentence), "typing alone keeps the sentence");
  await press(refused, useAnswers(refused));
  assert.deepEqual(
    refused.calls("agents_setup_typed").map((c) => [c.args.param, c.args.text]),
    [
      ["workbook", "Budget.xlsx"],
      ["workbook", "/Users/ana/Documents/Budget.xlsx"],
    ],
  );
  const now = lineOf(refused, "workbook");
  assert.deepEqual(sentencesOf(refused, "workbook"), [], "the next answer, which the core takes, takes the sentence away");
  assert.deepEqual(describedBy(now), [now.parentNode.querySelector("p.hn-hint").id], "and its name on the line: the hint alone");

  const row = ten.rows.find((r) => r.controls.some((c) => c.control === "change_answers"));
  assert.ok(row, "the core's fixture offers Change its answers");
  const rows = await open({ ...ten, items: [] });
  const control = rows.document.querySelector(`button[data-act="change_answers"][data-param="${row.agent}"]`);
  assert.ok(control, "the row shows the control");
  assert.equal(control.textContent, words["control.change_answers"]);
  await press(rows, control);
  assert.deepEqual(
    rows.calls("agents_control").map((c) => c.args),
    [{ agent: row.agent, action: "change_answers" }],
  );
});

test("a second line of one question that the core does not take shows its sentence again, and a line that it takes takes the sentence away", async () => {
  const page = await openPage({ replies: sheetCore().replies });
  const sentence = words["param_typed.not_path_mac"];
  await typeLine(page, "workbook", "Budget.xlsx");
  await press(page, useAnswers(page));
  assert.deepEqual(sentencesOf(page, "workbook"), [sentence], "a line that is not taken shows the sentence");
  await typeLine(page, "workbook", "Documents/Budget.xlsx");
  await press(page, useAnswers(page));
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => c.args.text),
    ["Budget.xlsx", "Documents/Budget.xlsx"],
  );
  assert.deepEqual(sentencesOf(page, "workbook"), [sentence], "a second line that is not taken shows it again");
  await typeLine(page, "workbook", "/Volumes/work/Budget.xlsx");
  await press(page, useAnswers(page));
  assert.deepEqual(sentencesOf(page, "workbook"), [], "a line that is taken takes it away");
});

test("behind a line that the core does not take the card keeps every line as typed, the next press sends the lines that the core holds first, and a card of another item starts with empty lines", async () => {
  const core = sheetCore();
  const page = await openPage({ replies: core.replies });
  const typed = { workbook: "Budget.xlsx", sheet: "Sheet1", range: "A2:C11", source: "/Volumes/work/receipts.csv" };
  for (const [name, text] of Object.entries(typed)) await typeLine(page, name, text);
  await press(page, useAnswers(page));
  for (const [name, text] of Object.entries(typed)) assert.equal(lineOf(page, name).value, text, `${name} keeps what was typed`);
  assert.deepEqual(sentencesOf(page, "workbook"), [words["param_typed.not_path_mac"]], "the refused line over its sentence");
  const before = page.calls("agents_setup_typed").length;
  await typeLine(page, "workbook", "/Volumes/work/Budget.xlsx");
  await press(page, useAnswers(page));
  assert.deepEqual(
    page.calls("agents_setup_typed").slice(before).map((c) => [c.args.param, c.args.text]),
    [
      ["sheet", "Sheet1"],
      ["range", "A2:C11"],
      ["source", "/Volumes/work/receipts.csv"],
      ["workbook", "/Volumes/work/Budget.xlsx"],
      ["sheet", "Sheet1"],
    ],
    "the lines that the core holds go first, then the mended line, then the sheet's line once more, so that the body holds what every line shows",
  );
  core.other("0000000000000000000000000000000a");
  await page.settle();
  for (const name of Object.keys(typed)) assert.equal(lineOf(page, name).value, "", `${name} of another item`);
});
