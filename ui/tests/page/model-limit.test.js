"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

const HOST = "api.host.invalid";
/** The end of the period, 2026-12-12T00:30:00Z: on a clock one hour behind UTC it is still 11 December. */
const END = Date.UTC(2026, 11, 12, 0, 30);
const OFFSET = 60;
const DATE = new Date(END - OFFSET * 60000).toLocaleDateString(undefined, { timeZone: "UTC", day: "numeric", month: "long", year: "numeric" });

/** The reply of model_limit_state at `state`, with the texts of a count that reads unless `more` says otherwise. */
const stateOf = (state, more = {}) => ({
  host: HOST,
  state,
  cap: "20.00 USD",
  spent: "4.39 USD",
  held: "0.02 USD",
  period: "month",
  period_end_ms: END,
  ...more,
});

/** A page at a host whose model_limit_state answers `limit` (a reply or a function) and whose other replies are `more`. */
const pageAt = (limit, more = {}) =>
  openPage({
    replies: { model_key_state: () => ({ host: HOST, state: "none" }), model_limit_state: typeof limit === "function" ? limit : () => limit, ...more },
    timezoneOffset: OFFSET,
  });

/** A reply that the test holds: each call waits in `waiting` with its args until the test settles it. */
function held() {
  const waiting = [];
  return { waiting, reply: (args) => new Promise((resolve, reject) => waiting.push({ args, resolve, reject })) };
}

const fill = (text, values) => Object.entries(values).reduce((t, [key, value]) => t.split(`{${key}}`).join(value), text);

/** The sentences of `keys` as words.js holds them, filled with the texts of `reply`. */
const sentences = (page, keys, reply) =>
  keys.map((key) =>
    fill(page.window.Halvo.words[`MODEL_LIMIT_${key.toUpperCase()}`], {
      host: HOST,
      cap: reply.cap,
      amount: reply.cap,
      spent: reply.spent,
      held: reply.held,
      period: reply.period,
      date: DATE,
    }),
  );

const shownLines = (page) => page.byId("model-limit-lines").querySelectorAll("p").map((p) => p.textContent);

/** The part at `reply` shows its heading, the sentences of `keys` and no other, and the control where `control` says. */
async function shows(reply, keys, control) {
  const page = await pageAt(reply);
  assert.equal(page.text("model-limit-heading"), "Spending limit");
  assert.deepEqual(shownLines(page), sentences(page, keys, reply), reply.state);
  assert.equal(page.visible("model-limit-control"), control, `the control at ${reply.state}`);
  if (control) {
    assert.equal(page.text("model-limit-field-label"), "Limit for a month");
    assert.equal(page.text("model-limit-set"), "Set the limit");
  }
  return page;
}

test("the_cap_view_shows_the_counts_figures_unchanged", async () => {
  const reply = stateOf("ok", { cap: "1234567.89 USD", spent: "0.01 USD", held: "18446744073709.56 USD" });
  const page = await pageAt(reply);
  assert.equal(page.text("model-limit-limit"), "Limit: 1234567.89 USD a month.");
  assert.equal(page.text("model-limit-spent"), "Counted so far: 0.01 USD.");
  assert.equal(page.text("model-limit-held"), "Held for requests in progress: 18446744073709.56 USD.");
  assert.equal(
    page.text("model-limit-bill"),
    `${HOST} bills the key that you gave Halvo. Halvo counts what its own requests cost and stops at 1234567.89 USD a month.`,
  );
  assert.equal(page.text("model-limit-period"), `Halvo counts a month at a time, from the day this count began. It starts again on ${DATE}.`);
});

test("the_state_ok_shows_its_sentences_and_no_other", async () => {
  await shows(stateOf("ok"), ["limit", "spent", "held", "period", "bill", "pay", "elsewhere", "delete"], true);
});

test("the_state_near_shows_its_sentences_and_no_other", async () => {
  await shows(stateOf("near"), ["limit", "spent", "held", "near", "period", "bill", "pay", "elsewhere", "delete"], true);
});

test("the_state_reached_shows_its_sentences_and_no_other", async () => {
  const page = await shows(stateOf("reached"), ["limit", "spent", "held", "reached", "period", "bill", "pay", "elsewhere", "delete"], true);
  assert.equal(
    page.text("model-limit-reached"),
    `The limit is reached. Agents that use an AI model at ${HOST} wait until ${DATE} or until you raise the limit. Work that stays on this computer goes on.`,
  );
});

test("the_state_none_shows_its_sentences_and_no_other", async () => {
  const reply = stateOf("none", { spent: null, held: null, period_end_ms: null });
  const page = await shows(reply, ["none", "bill", "pay", "elsewhere", "delete"], true);
  assert.equal(page.text("model-limit-none"), "Halvo has counted nothing yet. It starts with a limit of 20.00 USD a month, which you can change.");
});

test("the_state_no_limit_shows_its_sentences_and_no_other", async () => {
  await shows(stateOf("no_limit", { cap: null, spent: "0.01 USD", held: "0.00 USD" }), ["no_limit", "spent", "held"], true);
  await shows(stateOf("no_limit", { cap: null, spent: null, held: null, period_end_ms: null }), ["no_limit"], true);
});

test("the_state_unread_shows_its_sentences_and_no_other", async () => {
  const reply = stateOf("unread", { cap: null, spent: null, held: null, period_end_ms: null });
  const page = await shows(reply, ["unread"], true);
  assert.equal(
    page.text("model-limit-unread"),
    `Halvo cannot read its count for this month, so it sends nothing to ${HOST}. If you set a limit again, Halvo counts from nothing.`,
  );
  assert.equal(page.document.getElementById("model-limit-delete"), null, "no sentence of Delete everything");
});

test("the_state_not_known_shows_its_sentences_and_no_other", async () => {
  const reply = stateOf("not_known", { cap: null, spent: null, held: null, period_end_ms: null });
  await shows(reply, ["not_known"], false);
  const failed = await pageAt(() => Promise.reject(new Error("no answer")));
  assert.deepEqual(shownLines(failed), ["Halvo cannot tell just now what it has counted."]);
  assert.equal(failed.visible("model-limit-control"), false);

  const states = [stateOf("ok"), reply];
  const behind = await pageAt(() => states.shift(), { set_model_limit: () => ({ error: "failed" }) });
  await behind.type("model-limit-field", "25");
  await behind.click("model-limit-set");
  assert.deepEqual(shownLines(behind), ["Halvo cannot tell just now what it has counted."], "behind a press that failed");
  assert.equal(behind.visible("model-limit-control"), false, "no control behind a press that failed");
  assert.equal(behind.text("model-limit-said"), "", "no sentence that names the hidden control");
});

test("a_text_that_is_no_amount_shows_its_one_sentence", async () => {
  const page = await pageAt(stateOf("ok"), { set_model_limit: () => ({ error: "no_amount" }) });
  const reads = page.calls("model_limit_state").length;
  await page.type("model-limit-field", "20,50");
  await page.click("model-limit-set");
  assert.deepEqual(page.calls("set_model_limit").map((c) => c.args), [{ amount: "20,50" }]);
  assert.equal(page.text("model-limit-said"), "Enter an amount such as 20 or 20.50.");
  assert.equal(page.calls("model_limit_state").length, reads, "a text that is no amount reads nothing again");
});

test("the_part_shows_only_where_the_table_names_a_host", async () => {
  const none = await openPage({ replies: { model_key_state: () => ({ host: null }), model_limit_state: () => stateOf("ok") } });
  assert.equal(none.document.getElementById("model-limit"), null, "no part at no host");
  assert.equal(none.calls("model_limit_state").length, 0, "nothing asked at no host");

  const page = await pageAt(stateOf("ok"));
  assert.equal(page.visible("model-limit"), true);
  assert.equal(page.calls("model_limit_state").length, 1);
  assert.equal(page.byId("model-limit").parentNode.id, "model-key-parts", "the part stands in the section's parts");
});

test("the_control_sends_nothing_while_the_field_is_empty_or_a_command_is_in_flight", async () => {
  const state = held();
  const set = held();
  const page = await pageAt(state.reply, { set_model_limit: set.reply });
  state.waiting.shift().resolve(stateOf("ok"));
  await page.settle();

  assert.equal(page.byId("model-limit-set").disabled, true, "the empty field");
  await page.click("model-limit-set");
  assert.equal(page.calls("set_model_limit").length, 0, "a press on the empty field sends nothing");

  await page.type("model-limit-field", "25");
  assert.equal(page.byId("model-limit-set").disabled, false);
  await page.click("model-limit-set");
  await page.click("model-limit-set");
  assert.deepEqual(page.calls("set_model_limit").map((c) => c.args), [{ amount: "25" }], "two fast presses send one command");
  assert.equal(page.byId("model-limit-set").disabled, true, "while the set is in flight");

  set.waiting.shift().resolve({ answer: "set", amount: "25.00 USD", period: "month" });
  await page.settle();
  assert.equal(page.text("model-limit-said"), "Halvo now stops at 25.00 USD a month.");
  assert.equal(state.waiting.length, 1, "the part reads its state again behind the answer");
  await page.type("model-limit-field", "30");
  await page.click("model-limit-set");
  assert.equal(page.calls("set_model_limit").length, 1, "nothing is sent while the state read is in flight");

  state.waiting.shift().resolve(stateOf("ok", { cap: "25.00 USD" }));
  await page.settle();
  assert.equal(page.text("model-limit-limit"), "Limit: 25.00 USD a month.");
  await page.click("model-limit-set");
  assert.deepEqual(page.calls("set_model_limit").map((c) => c.args), [{ amount: "25" }, { amount: "30" }]);
  set.waiting.shift().resolve({ answer: "set_new", amount: "30.00 USD", period: "month" });
  await page.settle();
  assert.equal(page.text("model-limit-said"), "Halvo now stops at 30.00 USD a month and counts from nothing.");
  state.waiting.shift().resolve(stateOf("ok"));
  await page.settle();

  await page.type("model-limit-field", "35");
  await page.click("model-limit-set");
  set.waiting.shift().resolve({ error: "failed" });
  await page.settle();
  assert.equal(page.text("model-limit-said"), "Halvo could not finish this. Choose Set the limit again.");
  assert.equal(state.waiting.length, 1, "a failed set reads the state again");
});

test("a_limit_part_that_cannot_tell_asks_again_by_itself_and_then_no_more", async () => {
  const asksAt = (page) => page.calls("model_limit_state").map((c) => c.at);
  const away = stateOf("not_known", { cap: null, spent: null, held: null, period_end_ms: null });
  const sentence = "Halvo cannot tell just now what it has counted.";

  const never = await pageAt(away);
  const line = never.byId("model-limit-not-known");
  await never.clock.advance(100000);
  assert.deepEqual(asksAt(never), [0, 1000, 3000, 7000, 15000, 31000], "the ask of the mount, five further asks behind their waits, and none behind the row");
  assert.deepEqual(shownLines(never), [sentence]);
  assert.equal(never.byId("model-limit-not-known"), line, "an answer that finds the part as it stood builds no line anew");

  const states = [stateOf("ok"), away, away, stateOf("ok", { cap: "25.00 USD" })];
  const page = await pageAt(() => (states.length > 1 ? states.shift() : states[0]), { set_model_limit: () => ({ error: "failed" }) });
  await page.type("model-limit-field", "25");
  await page.click("model-limit-set");
  assert.deepEqual(shownLines(page), [sentence], "behind a press that failed");
  await page.clock.advance(999);
  assert.deepEqual(asksAt(page), [0, 0], "no ask before the first wait ends");
  await page.clock.advance(2001);
  assert.deepEqual(asksAt(page), [0, 0, 1000, 3000]);
  assert.equal(page.text("model-limit-limit"), "Limit: 25.00 USD a month.", "the answer of the second further ask is drawn");
  assert.equal(page.visible("model-limit-control"), true, "the control is back");
  assert.equal(page.byId("model-limit-field").value, "25", "the asks leave a typed text in the field");
  assert.equal(page.byId("model-limit-set").disabled, false);
  await page.clock.advance(100000);
  assert.deepEqual(asksAt(page), [0, 0, 1000, 3000], "a part that can tell asks no more by itself");
  page.window.Halvo.views.window("shown");
  await page.settle();
  assert.deepEqual(asksAt(page), [0, 0, 1000, 3000], "a part that can tell asks nothing at a showing");

  const hidden = await pageAt(away);
  await hidden.clock.advance(1000);
  await hidden.hide();
  await hidden.clock.advance(100000);
  assert.deepEqual(asksAt(hidden), [0, 1000], "a hidden window asks nothing");
  hidden.document.visibilityState = "visible";
  hidden.window.Halvo.views.window("shown");
  await hidden.settle();
  await hidden.clock.advance(100000);
  assert.deepEqual(asksAt(hidden), [0, 1000, 101000, 102000, 104000, 108000, 116000, 132000], "a showing asks and begins the row anew");

  const loadedHidden = await openPage({
    visibility: "hidden",
    replies: { model_key_state: () => ({ host: HOST, state: "none" }), model_limit_state: () => away },
    timezoneOffset: OFFSET,
  });
  await loadedHidden.clock.advance(100000);
  assert.deepEqual(asksAt(loadedHidden), [0], "a page that loaded while the window was hidden asks at its mount alone");
});
