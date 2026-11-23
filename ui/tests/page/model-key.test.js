"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

const HOST = "api.host.invalid";

/** The reply of model_key_state: no host, or the host with `state`. */
const stateOf = (state) => (state === null ? { host: null } : { host: HOST, state });

/** A page whose model_key_state answers `state` and whose other replies are `more`. */
const pageAt = (state, more = {}) => openPage({ replies: { model_key_state: () => stateOf(state), ...more } });

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
