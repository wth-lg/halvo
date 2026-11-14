"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const FIXTURES = ["capture_status-running", "inventory-two-rows-one-day", "onboarding_state-lost", "onboarding_state-missing", "delete_all-deleted"];
const NOT_READ = {
  all_granted: "the page shows the permission rows and reads no summary of them",
  asked: "the page offers the step that the walk-back lists, and does not read whether a permission was asked",
  settings_url: "the page opens a settings pane through the command open_permission_pane and does not read its address",
};

const shape = (node) =>
  node.localName
    ? `<${node.localName} ${node.getAttribute("class") || ""}${node.disabled ? " disabled" : ""}>` + node.childNodes.map(shape).join("") + "</>"
    : node.textContent;

async function render(files) {
  const reply = (name) => () => files[name];
  const shown = [];
  for (const onboarding of ["onboarding_state-lost", "onboarding_state-missing"]) {
    const page = await openPage({
      replies: {
        capture_status: reply("capture_status-running"),
        inventory: reply("inventory-two-rows-one-day"),
        onboarding_state: reply(onboarding),
        delete_all: reply("delete_all-deleted"),
        request_permission: () => "requested",
        open_permission_pane: () => null,
      },
    });
    for (const button of page.byId("permissions-section").querySelectorAll("button")) await page.click(button, { detail: 0 });
    await page.click("delete", { detail: 0 });
    await page.clock.advance(600);
    await page.click("confirm-delete", { detail: 0 });
    const sent = page.sent.filter((c) => c.cmd === "request_permission" || c.cmd === "open_permission_pane");
    shown.push(shape(page.document.documentElement) + JSON.stringify(sent));
  }
  return shown.join("\n");
}

const keysOf = (value, keys = new Set()) => {
  if (Array.isArray(value)) value.forEach((v) => keysOf(v, keys));
  else if (value && typeof value === "object") for (const [k, v] of Object.entries(value)) keysOf(v, keys.add(k));
  return keys;
};

const renamed = (value, key) => {
  if (Array.isArray(value)) return value.map((v) => renamed(v, key));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k === key ? `${k}_renamed` : k, renamed(v, key)]));
  return value;
};

test("a rename of any key of the reply fixtures that the page reads changes what the page shows", async () => {
  const files = Object.fromEntries(FIXTURES.map((name) => [name, fixture(name)]));
  const base = await render(files);
  const keys = new Set();
  const read = new Set();
  for (const name of FIXTURES) {
    for (const key of keysOf(files[name])) {
      keys.add(key);
      const changed = await render({ ...files, [name]: renamed(files[name], key) });
      if (changed !== base) read.add(key);
    }
  }
  const unread = new Set([...keys].filter((k) => !read.has(k)));
  const listed = new Set(Object.keys(NOT_READ));
  assert.deepEqual([...unread].filter((k) => !listed.has(k)).sort(), [], "keys whose rename the page does not show");
  assert.deepEqual([...listed].filter((k) => !unread.has(k)).sort(), [], "keys on the short list that the page reads after all");
});
