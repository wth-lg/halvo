"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

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
    `Paste the key from your account at ${HOST}. Halvo keeps it on this computer, protected in the same way as your recordings, and sends it to ${HOST} alone.`,
  );
  assert.equal(page.visible("model-key-field"), true);
  assert.equal(page.text("model-key-save"), "Save the key");
  assert.equal(page.visible("model-key-state"), true);
  const sections = page.document.querySelectorAll("section").map((s) => s.id);
  assert.equal(sections[sections.indexOf("agents-section") + 1], "model-key-section");
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
