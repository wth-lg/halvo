"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const FIXTURES = ["capture_status-running", "inventory-two-rows-one-day", "onboarding_state-lost", "onboarding_state-missing", "delete_all-deleted"];
const OTHER_STATE = "read only in a state that these fixtures do not show; states.test.js or delete.test.js holds it";
const NOT_DRAWN = "a field of the view that the page does not draw in R1";
const NOT_READ = {
  all_granted: "the page shows the permission rows and reads no summary of them",
  asked: "the page offers the step that the walk-back lists, and does not read whether a permission was asked",
  settings_url: "the page opens a settings pane through the command open_permission_pane and does not read its address",
  capture_reason: OTHER_STATE,
  drain: OTHER_STATE,
  expiry: OTHER_STATE,
  error: OTHER_STATE,
  reason: OTHER_STATE,
  retry_in_s: OTHER_STATE,
  last_at: OTHER_STATE,
  since: OTHER_STATE,
  cleanup_command: OTHER_STATE,
  database_left: OTHER_STATE,
  files_left: OTHER_STATE,
  not_followed: OTHER_STATE,
  root_destroyed: OTHER_STATE,
  backup_limit: "the Windows sentence, null in the macOS fixtures; keeps.js draws it when it is a string",
  as_of: NOT_DRAWN,
  day_is: "every day of R1 is a UTC day, and the page labels the column so without reading the key",
  disk_bytes: "the page draws store_bytes of the status, which is the same figure",
  totals: "the page draws the totals of the status",
  store_id: "the page arms a delete with the id of the status, not of the inventory",
  entries_left: NOT_DRAWN,
  unknown_entries: NOT_DRAWN,
  keys_destroyed: "the delete's words do not count keys",
  segments_shredded: "the delete's words do not count segments",
  new_store_id: "the page reads the new id from the next status",
  looked_in: "the offline delete that fills it is app-3's",
  events_lost: NOT_DRAWN,
  events_pending: NOT_DRAWN,
  loss: NOT_DRAWN,
  final: NOT_DRAWN,
  first: NOT_DRAWN,
  last: NOT_DRAWN,
  last_stop: NOT_DRAWN,
  last_stop_kind: NOT_DRAWN,
  last_stop_lost: NOT_DRAWN,
  last_stop_newest_capture: NOT_DRAWN,
  last_stop_sources: NOT_DRAWN,
  last_exit: "the status of the child's last exit, which app-3 fills",
  ext: "the sections of a later run, which app-3 draws",
  first_report_day: "R1 ships no report",
  lease: NOT_DRAWN,
  password_guard: "the page draws the sentence texts.passwords",
  platform: "the page draws the sentences of texts, which carry the system",
  screen_filter_excludes_app: "the rule of the window, which app-3 applies",
  switch: "the page draws the text of each recorded sentence",
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
    const sent = page.sent.filter((c) => c.cmd === "request_permission" || c.cmd === "open_permission_pane" || c.cmd === "delete_all");
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
