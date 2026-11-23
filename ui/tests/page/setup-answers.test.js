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

test("each question draws its hint under its words and names it to its line, a line that the core did not take shows its sentence under it until the next answer for it, and Change its answers calls agents_control once", async () => {
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
  const refused = await open(sheetSetup(sentence));
  const set = refused.document.querySelectorAll("fieldset.hn-param")[0];
  const line = set.querySelector("input.hn-typed");
  const note = set.childNodes[set.childNodes.indexOf(line) + 1];
  assert.ok(note && note.textContent === sentence, "the sentence stands under the line");
  assert.ok(describedBy(line).includes(note.id), "the sentence describes the line");
  assert.ok(describedBy(line).includes(set.querySelector("p.hn-hint").id), "the hint still describes the line");
  line.focus();
  line.value = "/Users/ana/Documents/Budget.xlsx";
  await line.dispatchEvent(makeEvent("input"));
  await refused.settle();
  assert.ok(set.textContent.includes(sentence), "typing alone keeps the sentence");
  const [item] = sheetSetup(sentence).items;
  await press(refused, refused.document.querySelector(`button[data-act="choose"][data-param="${item.item}"]`));
  assert.deepEqual(
    refused.calls("agents_setup_typed").map((c) => c.args),
    [{ item: item.item, step: item.step, param: "workbook", text: "/Users/ana/Documents/Budget.xlsx" }],
  );
  assert.ok(!set.textContent.includes(sentence), "the next answer takes the sentence away");
  assert.ok(!describedBy(line).includes(note.id), "and its name on the line");

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
