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
const ten = read("state-ten.json").snapshot;

/** The sentence of `key` of the core's words table with its placeholders filled from `pairs`. */
const fill = (key, pairs = {}) => Object.keys(pairs).reduce((text, name) => text.split("{" + name + "}").join(pairs[name]), words[key]);

const PAIRS = { host: "hosted-model.test", models: "test-model-small", amount: "20.00 USD", period: "month" };
const QUESTION = {
  kind: "use_model",
  title: fill("ask.use_model.title"),
  text: ["sends", "models", "never", "bills"].map((k) => fill("ask.use_model." + k, PAIRS)),
  effects: [],
  record: null,
  controls: [
    { act: "reject", text: fill("ask.use_model.keep") },
    { act: "allow", text: fill("ask.use_model.allow") },
  ],
};

/** The state of ONE row, the first of the core's fixture state-ten, offering use_model before delete, with `asked`. */
function oneRow(asked) {
  const row = ten.rows[0];
  const at = row.controls.findIndex((c) => c.control === "delete");
  const control = { control: "use_model", text: fill("control.use_model") };
  const offered = { ...row, controls: [...row.controls.slice(0, at), control, ...row.controls.slice(at)], ...asked };
  return { ...ten, rows: [offered], items: [] };
}

/** The page's <dialog> as a web view has it, which the suite's stub DOM lacks: showModal opens it, close closes it
 * and sends its event close. */
function withDialogs(page) {
  const make = page.document.createElement.bind(page.document);
  page.document.createElement = (tag) => {
    const node = make(tag);
    if (String(tag).toLowerCase() === "dialog") {
      node.open = false;
      node.showModal = () => {
        node.open = true;
      };
      node.close = () => {
        if (!node.open) return;
        node.open = false;
        node.dispatchEvent(makeEvent("close"));
      };
    }
    return node;
  };
}

async function open(snapshot) {
  const page = await openPage({
    replies: { agents_snapshot: snapshot, agents_next: { closed: "yes" }, agents_control: { ok: "yes" } },
  });
  withDialogs(page);
  return page;
}

/** A press as the keyboard makes it: a click of detail 0. */
const press = (page, el) => page.click(el, { detail: 0 });
const control = (page, agent) => page.document.querySelector(`button[data-act="use_model"][data-param="${agent}"]`);
const questions = (page) => page.document.querySelectorAll("dialog").filter((d) => d.open);

test("a press on Use an AI model opens its question and calls nothing, Not now calls nothing, Allow for this agent calls agents_control once with use_model, and a row with a sentence shows it and no question", async () => {
  const agent = ten.rows[0].agent;
  const page = await open(oneRow({ ask_use_model: QUESTION, note_use_model: null }));
  const button = control(page, agent);
  assert.ok(button, "the row offers Use an AI model");
  assert.equal(button.textContent, fill("control.use_model"));

  await press(page, button);
  assert.deepEqual(page.calls("agents_control"), [], "the press calls nothing");
  const [asked] = questions(page);
  assert.equal(questions(page).length, 1, "one question");
  assert.equal(asked.querySelector("h3").textContent, QUESTION.title);
  assert.deepEqual(asked.querySelectorAll("p.hc-text").map((p) => p.textContent), QUESTION.text);
  assert.deepEqual(
    asked.querySelectorAll("button").map((b) => [b.getAttribute("data-act"), b.getAttribute("data-param"), b.textContent]),
    QUESTION.controls.map((c) => [c.act, agent, c.text]),
  );

  asked.close();
  await page.settle();
  assert.deepEqual(page.calls("agents_control"), [], "a question closed with no answer calls nothing");
  assert.equal(questions(page).length, 1, "one question is open");
  assert.equal(questions(page)[0], asked, "the page shows the same question again: an answer alone ends it");

  await press(page, asked.querySelector('button[data-act="reject"]'));
  assert.equal(questions(page).length, 0, "Not now closes the question");
  assert.deepEqual(page.calls("agents_control"), [], "Not now calls nothing");

  await press(page, control(page, agent));
  const [again] = questions(page);
  assert.ok(again, "the question opens again");
  await press(page, again.querySelector('button[data-act="allow"]'));
  assert.deepEqual(
    page.calls("agents_control").map((c) => c.args),
    [{ agent, action: "use_model" }],
    "Allow for this agent calls agents_control once",
  );

  const first = fill("ask.use_model.first");
  const noted = await open(oneRow({ ask_use_model: null, note_use_model: first }));
  await press(noted, control(noted, agent));
  assert.equal(questions(noted).length, 0, "no question");
  assert.deepEqual(noted.document.querySelectorAll(".r4a-answer").map((p) => p.textContent), [first]);
  assert.deepEqual(noted.calls("agents_control"), [], "the sentence calls nothing");
});
