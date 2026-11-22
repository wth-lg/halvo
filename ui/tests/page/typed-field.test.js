"use strict";

const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, UI } = require("../lib/page.js");

const setup = JSON.parse(fs.readFileSync(path.join(UI, "tests", "fixtures", "coord", "state-setup_three.json"), "utf8"));

test("the typed field of a setup item asks the system not to complete, correct, capitalise or check what is typed", async () => {
  const page = await openPage({ replies: { agents_snapshot: setup.snapshot, agents_next: { closed: "yes" } } });
  const fields = page.document.querySelectorAll("input.hn-typed");
  assert.equal(fields.length, 1);
  const field = fields[0];
  assert.equal(field.getAttribute("data-param"), "instrument");
  assert.equal(field.getAttribute("autocomplete"), "off");
  assert.equal(field.getAttribute("autocorrect"), "off");
  assert.equal(field.getAttribute("autocapitalize"), "off");
  assert.equal(field.getAttribute("spellcheck"), "false");
});
