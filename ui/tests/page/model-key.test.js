"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete } = require("../lib/page.js");

const HOST = "api.host.invalid";
const STATE = {
  saved: "A key is saved.",
  none: "No key is saved.",
  not_known: "Halvo cannot tell just now whether a key is saved.",
};

/** The reply of model_key_state: no host, or the host with `state`. */
const stateOf = (state) => (state === null ? { host: null } : { host: HOST, state });

/** A page whose model_key_state answers `state` and whose other replies are `more`. */
const pageAt = (state, more = {}) => openPage({ replies: { model_key_state: () => stateOf(state), ...more } });

/** A reply that the test holds: each call waits in `waiting` with its args until the test settles it. */
function held() {
  const waiting = [];
  return { waiting, reply: (args) => new Promise((resolve, reject) => waiting.push({ args, resolve, reject })) };
}

/** Every 4-byte part of `text`. */
const partsOf = (text) => Array.from({ length: text.length - 3 }, (_, i) => text.slice(i, i + 4));

/** Every text that the walk from `root` reaches through own values, keys and DOM nodes, functions left closed. */
function textsFrom(root) {
  const seen = new Set();
  const texts = [];
  const walk = (value) => {
    if (typeof value === "string") return void texts.push(value);
    if (value === null || (typeof value !== "object" && typeof value !== "function") || seen.has(value)) return;
    seen.add(value);
    for (const key of Object.getOwnPropertyNames(value)) {
      texts.push(key);
      const described = Object.getOwnPropertyDescriptor(value, key);
      if (described && "value" in described) walk(described.value);
    }
  };
  walk(root);
  return texts;
}

/** The field is empty, and no part of `pasted` stands in the document's text, in any field's value or in what Halvo reaches. */
function nothingHolds(page, pasted) {
  assert.equal(page.byId("model-key-field").value, "", "the field is empty after the press");
  const texts = [
    page.document.documentElement.textContent,
    ...page.document.querySelectorAll("input").map((field) => field.value),
    ...page.document.querySelectorAll("textarea").map((field) => field.value),
    ...textsFrom(page.window.Halvo),
  ];
  for (const part of partsOf(pasted)) {
    assert.deepEqual(
      texts.filter((text) => text.includes(part)),
      [],
      `a part of the pasted text stands on the page: ${part}`,
    );
  }
}

test("the_section_shows_only_at_a_host_with_its_texts_behind_the_agents", async () => {
  const none = await pageAt(null);
  const ours = [...none.document.elements()].filter((el) => el.id.startsWith("model-key") || el.textContent === "AI model");
  assert.deepEqual(ours.filter((el) => none.visible(el)).map((el) => el.id), [], "nothing of the section at no host");

  const page = await pageAt("none");
  assert.equal(page.visible("model-key-section"), true);
  assert.equal(page.text("model-key-heading"), "AI model");
  assert.equal(page.text("model-key-label"), `Key for ${HOST}`);
  assert.equal(page.byId("model-key-label").htmlFor, "model-key-field");
  assert.equal(
    page.text("model-key-text"),
    `Paste the key from your account at ${HOST}. Halvo keeps it on this computer, protected in the same way as your recordings, and sends it nowhere but ${HOST}.`,
  );
  assert.equal(page.visible("model-key-field"), true);
  assert.equal(page.text("model-key-save"), "Save the key");
  assert.equal(page.visible("model-key-state"), true);
  const sections = page.document.querySelectorAll("section").map((s) => s.id);
  assert.equal(sections[sections.indexOf("agents-section") + 1], "model-key-section");
});

/** r4 U3.34, S1 as the coordinator ruled it (log line 23311): while the reply of model_key_state names a host and says
 * hands_back true, the first line of the part says that no agent of this version asks an AI model, in each state of the
 * key; where the field is false or missing it shows nothing, and at no host the section with it is hidden. */
test("u334_the_part_says_first_that_no_agent_asks_a_model_while_every_row_hands_back", async () => {
  const said = `No agent of this version of Halvo asks an AI model, so a key that you save here is used by nothing, Halvo sends nothing to ${HOST}, and nothing is spent.`;
  for (const state of ["saved", "none", "not_known"]) {
    const page = await openPage({ replies: { model_key_state: () => ({ host: HOST, state, hands_back: true }) } });
    assert.equal(page.visible("model-key-no-agent"), true, state);
    assert.equal(page.text("model-key-no-agent"), said, state);
    assert.equal(page.byId("model-key-no-agent").parentNode.children[0].id, "model-key-no-agent", `${state}: first in the part`);
    assert.equal(page.text("model-key-state"), STATE[state], state);
  }
  for (const reply of [{ hands_back: false }, {}]) {
    const page = await openPage({ replies: { model_key_state: () => ({ host: HOST, state: "saved", ...reply }) } });
    assert.equal(page.visible("model-key-section"), true, JSON.stringify(reply));
    assert.equal(page.visible("model-key-no-agent"), false, JSON.stringify(reply));
    assert.equal(page.text("model-key-no-agent"), "", JSON.stringify(reply));
  }
  const none = await openPage({ replies: { model_key_state: () => ({ host: null, hands_back: true }) } });
  assert.equal(none.visible("model-key-section"), false);
  assert.equal(none.visible("model-key-no-agent"), false);
});

test("the_key_field_is_a_password_field", async () => {
  const page = await pageAt("none");
  const field = page.byId("model-key-field");
  assert.equal(field.localName, "input");
  assert.equal(field.getAttribute("type"), "password");
  assert.equal(field.getAttribute("autocomplete"), "off");
  assert.equal(field.getAttribute("autocorrect"), "off");
  assert.equal(field.getAttribute("autocapitalize"), "off");
  assert.equal(field.getAttribute("spellcheck"), "false");
});

test("the_key_field_is_empty_after_the_press_and_no_state_holds_the_key", async () => {
  const answers = [{ state: "saved" }, { error: "no_key" }, { error: "not_finished" }];
  const page = await pageAt("none", { save_model_key: () => answers.shift() });
  const presses = [
    ["Zq7Wx9Kp2Vm4Jt8Rb5", STATE.saved, "model-key-state"],
    ["Hn3Lc6Yf1Qs0Dg7Ue2 ", "That does not look like a key. Paste the key alone, with nothing before or after it.", "model-key-said"],
    ["Pw8Tk5Ma9Xe4Bj6Nr3", "Halvo could not finish this. Paste the key and choose Save the key again.", "model-key-said"],
  ];
  for (const [index, [pasted, sentence, line]] of presses.entries()) {
    const reads = page.calls("model_key_state").length;
    await page.type("model-key-field", pasted);
    await page.click("model-key-save");
    const sent = page.calls("save_model_key");
    assert.equal(sent.length, index + 1, "one command a press");
    assert.deepEqual(sent[index].args, { key: pasted });
    nothingHolds(page, pasted);
    assert.equal(page.text(line), sentence);
    assert.equal(page.calls("model_key_state").length, reads + (index === 2 ? 1 : 0), "a failed save reads the state again");
  }
});

test("the_state_line_and_the_removal_with_its_question", async () => {
  for (const state of ["saved", "none", "not_known"]) {
    const page = await pageAt(state);
    assert.equal(page.text("model-key-state"), STATE[state], state);
    assert.equal(page.visible("model-key-remove"), state === "saved", `the control at ${state}`);
  }

  const page = await pageAt("saved", { remove_model_key: () => ({ state: "none" }) });
  assert.equal(page.text("model-key-remove"), "Remove the key");
  assert.equal(page.visible("model-key-remove-row"), false);
  await page.click("model-key-remove");
  assert.equal(page.visible("model-key-remove-row"), true);
  assert.equal(
    page.text("model-key-remove-question"),
    `Remove the key? Agents that use an AI model at ${HOST} then stop and hand their work back to you until you add a key.`,
  );
  assert.deepEqual(
    page.byId("model-key-remove-row").querySelectorAll("button").map((b) => b.textContent),
    ["Keep the key", "Remove the key"],
  );
  const before = page.sent.length;
  await page.click("model-key-keep");
  assert.equal(page.visible("model-key-remove-row"), false);
  assert.equal(page.sent.length, before, "Keep the key sends nothing");
  await page.click("model-key-remove");
  await page.click("model-key-confirm-remove");
  assert.equal(page.calls("remove_model_key").length, 1);
  assert.equal(page.text("model-key-state"), STATE.none);

  const failed = await pageAt("saved", { remove_model_key: () => ({ error: "not_finished" }) });
  const reads = failed.calls("model_key_state").length;
  await failed.click("model-key-remove");
  await failed.click("model-key-confirm-remove");
  assert.equal(failed.calls("remove_model_key").length, 1);
  assert.equal(
    failed.text("model-key-said"),
    "Halvo could not finish this. If this page still says that a key is saved, choose Remove the key again.",
  );
  assert.equal(failed.calls("model_key_state").length, reads + 1, "a failed removal reads the state again");
});

test("the_three_questions_name_the_key_only_while_one_is_saved", async () => {
  const w = (await pageAt(null)).window.Halvo.words;
  const goes = `Halvo also removes your key for ${HOST}.`;
  const stays = `Your key for ${HOST} and what each agent may send stay as they are.`;
  const unnamed = (words) => words.replace(", {folder}", "");
  const corrupt = { ...fixture("capture_status-paused"), state_file: "corrupt", store_cap_bytes: null, reset_recorded: [] };
  const shown = (page, id) => page.document.getElementById(id) !== null && page.visible(id);
  for (const state of ["saved", "none", null, "not_known"]) {
    const at = `at ${state}`;
    const named = state === "saved";

    const page = await pageAt(state);
    await page.click("delete");
    assert.equal(page.text("delete-question"), w.DELETE_QUESTION, at);
    assert.equal(shown(page, "delete-model-key-line"), named, `Delete everything ${at}`);
    if (named) assert.equal(page.text("delete-model-key-line"), goes);
    await page.click("cancel-delete");
    await page.click("remove-halvo");
    assert.equal(page.text("remove-question"), w.REMOVE_QUESTION, at);
    assert.equal(shown(page, "remove-model-key-line"), named, `Remove Halvo ${at}`);
    if (named) assert.equal(page.text("remove-model-key-line"), goes);

    const offline = await pageAt(state, { capture_status: () => fixture("capture_status-not-running") });
    await offline.click("delete");
    assert.equal(offline.text("delete-question"), unnamed(w.DELETE_OFFLINE_QUESTION), at);
    assert.equal(shown(offline, "delete-model-key-line"), named, `the offline form ${at}`);
    if (named) assert.equal(offline.text("delete-model-key-line"), goes);

    const refused = await pageAt(state, keptDelete({ delete_all: () => fixture("delete_all-refused") }));
    for (let i = 0; i < 2; i++) {
      await refused.click("delete");
      await refused.clock.advance(600);
      await refused.press("confirm-delete");
    }
    await refused.click("delete-offline");
    assert.equal(refused.text("delete-question"), unnamed(w.DELETE_STOP_QUESTION), at);
    assert.equal(shown(refused, "delete-model-key-line"), named, `the form that stops the service ${at}`);
    if (named) assert.equal(refused.text("delete-model-key-line"), goes);

    const reset = await pageAt(state, { capture_status: () => corrupt });
    await reset.click("settings-reset");
    assert.equal(reset.text("settings-reset-question"), w.SETTINGS_RESET_QUESTION, at);
    assert.equal(shown(reset, "settings-reset-model-key-line"), named, `Reset settings ${at}`);
    if (named) assert.equal(reset.text("settings-reset-model-key-line"), stays);
  }
});

test("while_a_save_is_in_flight_the_controls_of_the_key_send_nothing", async () => {
  const save = held();
  const page = await pageAt("saved", { save_model_key: save.reply, remove_model_key: () => ({ state: "none" }) });
  const pasted = "Zq7Wx9Kp2Vm4Jt8Rb5";
  await page.click("model-key-remove");
  await page.type("model-key-field", pasted);
  await page.click("model-key-save");
  await page.click("model-key-save");
  await page.click("model-key-remove");
  await page.click("model-key-confirm-remove");
  assert.deepEqual(page.calls("save_model_key").map((c) => c.args), [{ key: pasted }], "two fast presses send one command");
  assert.equal(page.calls("remove_model_key").length, 0, "the removal and its answer send nothing while the save is in flight");
  for (const id of ["model-key-save", "model-key-remove", "model-key-confirm-remove"]) assert.equal(page.byId(id).disabled, true, id);

  for (const call of save.waiting) call.resolve(call.args.key === "" ? { error: "no_key" } : { state: "saved" });
  await page.settle();
  assert.equal(page.text("model-key-state"), STATE.saved);
  assert.equal(page.text("model-key-said"), "", "no sentence for a text that is no key");
  for (const id of ["model-key-remove", "model-key-confirm-remove"]) assert.equal(page.byId(id).disabled, false, id);
  assert.equal(page.visible("model-key-remove"), true);
});

test("a_state_read_sent_before_a_save_changes_nothing_when_it_comes_behind_the_save", async () => {
  const late = held();
  let reads = 0;
  const page = await openPage({
    replies: {
      model_key_state: () => (reads++ === 0 ? stateOf("none") : late.reply()),
      save_model_key: () => ({ state: "saved" }),
    },
  });
  page.window.Halvo.views.window("shown");
  await page.settle();
  assert.equal(late.waiting.length, 1, "the read of the show is held");
  await page.type("model-key-field", "Zq7Wx9Kp2Vm4Jt8Rb5");
  await page.click("model-key-save");
  assert.equal(page.text("model-key-state"), STATE.saved);

  late.waiting[0].resolve(stateOf("none"));
  await page.settle();
  assert.equal(page.text("model-key-state"), STATE.saved, "the late reply of the read turns nothing");
  assert.equal(page.visible("model-key-remove"), true);
});

test("a_second_press_of_save_behind_the_reply_sends_nothing_until_a_text_is_typed", async () => {
  const page = await pageAt("none", { save_model_key: (args) => (args.key === "" ? { error: "no_key" } : { state: "saved" }) });
  await page.type("model-key-field", "Zq7Wx9Kp2Vm4Jt8Rb5");
  await page.click("model-key-save");
  assert.equal(page.text("model-key-state"), STATE.saved);
  await page.click("model-key-save");
  assert.deepEqual(page.calls("save_model_key").map((c) => c.args), [{ key: "Zq7Wx9Kp2Vm4Jt8Rb5" }], "a press on the empty field sends nothing");
  assert.equal(page.text("model-key-state"), STATE.saved);
  assert.equal(page.text("model-key-said"), "", "no sentence for a text that is no key");

  await page.type("model-key-field", "Hn3Lc6Yf1Qs0Dg7Ue2");
  assert.equal(page.byId("model-key-save").disabled, false, "a typed text makes Save the key live");
  await page.click("model-key-save");
  assert.deepEqual(page.calls("save_model_key").map((c) => c.args), [{ key: "Zq7Wx9Kp2Vm4Jt8Rb5" }, { key: "Hn3Lc6Yf1Qs0Dg7Ue2" }]);
});

test("a_key_line_that_cannot_tell_asks_again_by_itself_and_then_no_more", async () => {
  const asksAt = (page) => page.calls("model_key_state").map((c) => c.at);

  const never = await pageAt("not_known");
  await never.clock.advance(100000);
  assert.deepEqual(asksAt(never), [0, 1000, 3000, 7000, 15000, 31000], "the ask of the load, five further asks behind their waits, and none behind the row");
  assert.equal(never.text("model-key-state"), STATE.not_known);

  const states = ["not_known", "not_known", "saved"];
  const page = await openPage({ replies: { model_key_state: () => stateOf(states.length > 1 ? states.shift() : states[0]) } });
  await page.type("model-key-field", "Zq7Wx9Kp2Vm4Jt8Rb5");
  await page.clock.advance(999);
  assert.deepEqual(asksAt(page), [0], "no ask before the first wait ends");
  await page.clock.advance(2001);
  assert.deepEqual(asksAt(page), [0, 1000, 3000]);
  assert.equal(page.text("model-key-state"), STATE.saved, "the answer of the third ask is drawn");
  assert.equal(page.visible("model-key-remove"), true);
  assert.equal(page.byId("model-key-field").value, "Zq7Wx9Kp2Vm4Jt8Rb5", "the asks leave a typed text in the field");
  await page.clock.advance(100000);
  assert.deepEqual(asksAt(page), [0, 1000, 3000], "a line that can tell asks no more by itself");

  const hidden = await pageAt("not_known");
  await hidden.clock.advance(1000);
  await hidden.hide();
  await hidden.clock.advance(100000);
  assert.deepEqual(asksAt(hidden), [0, 1000], "a hidden window asks nothing");
  hidden.document.visibilityState = "visible";
  hidden.window.Halvo.views.window("shown");
  await hidden.settle();
  await hidden.clock.advance(100000);
  assert.deepEqual(asksAt(hidden), [0, 1000, 101000, 102000, 104000, 108000, 116000, 132000], "a showing asks and begins the row anew");

  const loadedHidden = await openPage({ visibility: "hidden", replies: { model_key_state: () => stateOf("not_known") } });
  await loadedHidden.clock.advance(100000);
  assert.deepEqual(asksAt(loadedHidden), [0], "a page that loaded while the window was hidden asks at its load alone");
});

test("the_parts_that_cannot_tell_ask_again_where_the_page_learns_that_the_service_runs", async () => {
  const limit = (state) => ({ host: HOST, state, cap: null, spent: null, held: null, period: "month", period_end_ms: null });
  /** A page whose status reads the fixture `first` and whose parts get an answer only once `service.back` holds; a press of Start or of Resume brings the service. */
  const open = async (first, back = false) => {
    const service = { status: first, back };
    const brings = (reply) => () => {
      service.status = "running";
      service.back = true;
      return fixture(reply);
    };
    const page = await openPage({
      replies: {
        capture_status: () => fixture(`capture_status-${service.status}`),
        model_key_state: () => stateOf(service.back ? "none" : "not_known"),
        model_limit_state: () => limit(service.back ? "none" : "not_known"),
        start_capture: brings("start_capture-running"),
        set_capture_state: brings("set_capture_state-running"),
      },
    });
    return { page, service };
  };
  const asksAt = (page) => ["model_key_state", "model_limit_state"].map((cmd) => page.calls(cmd).map((c) => c.at));
  const waits = [0, 1000, 3000, 7000, 15000, 31000];

  for (const [first, button, command] of [
    ["not-started", "Start", "start_capture"],
    ["not-running", "Start", "set_capture_state"],
    ["paused", "Resume", "set_capture_state"],
  ]) {
    const { page } = await open(first);
    await page.clock.advance(40000);
    assert.deepEqual(asksAt(page), [waits, waits], `the waits are over before the press of ${button} at ${first}`);
    assert.equal(page.text("toggle"), button);
    await page.click("toggle");
    assert.equal(page.calls(command).length, 1, `${button} at ${first} sends ${command}`);
    assert.deepEqual(asksAt(page), [[...waits, 40000], [...waits, 40000]], `each part asks behind the answer of ${button} at ${first}`);
    assert.equal(page.text("model-key-state"), STATE.none, "the answer of that ask is drawn");
    assert.equal(page.visible("model-limit-control"), true, "the limit's control is back");
    await page.clock.advance(100000);
    assert.deepEqual(asksAt(page), [[...waits, 40000], [...waits, 40000]], "a part that can tell asks no more, and the status view's read behind the press asks nothing");
  }

  const byItself = await open("not-running");
  await byItself.page.clock.advance(40000);
  byItself.service.status = "running";
  byItself.service.back = true;
  await byItself.page.clock.advance(3000);
  assert.deepEqual(asksAt(byItself.page), [[...waits, 42000], [...waits, 42000]], "each part asks where the status view's own read turns to a running service");
  assert.equal(byItself.page.text("model-key-state"), STATE.none);

  const hidden = await open("not-running");
  await hidden.page.clock.advance(40000);
  await hidden.page.hide();
  hidden.service.status = "running";
  hidden.service.back = true;
  await hidden.page.clock.advance(100000);
  assert.deepEqual(asksAt(hidden.page), [waits, waits], "a hidden window asks nothing where the status turns to a running service");

  const still = await open("not-running");
  await still.page.clock.advance(40000);
  still.service.status = "running";
  await still.page.clock.advance(100000);
  const anew = [42000, 43000, 45000, 49000, 57000, 73000];
  assert.deepEqual(asksAt(still.page), [[...waits, ...anew], [...waits, ...anew]], "a part that still cannot tell begins its waits anew, and then asks no more");

  const told = await open("paused", true);
  await told.page.click("toggle");
  assert.equal(told.page.calls("set_capture_state").length, 1);
  assert.deepEqual(asksAt(told.page), [[0], [0]], "a part that can tell asks nothing behind a press");
});
