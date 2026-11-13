"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const { openPage, fixture, FIXTURES } = require("../lib/page.js");

const FOUR = ["capturing", "paused", "Nothing stored yet.", "0 B"];
const visibleText = (node) =>
  node.localName ? (node.classList.contains("hidden") || node.hidden ? "" : node.childNodes.map(visibleText).join(" ")) : node.textContent;

const STATES = fs
  .readdirSync(FIXTURES)
  .filter((f) => f.startsWith("capture_status-") && f.endsWith(".json"))
  .map((f) => f.slice(0, -5));

const page = (status, more = {}) => openPage({ replies: { capture_status: () => status, ...more } });
const offered = (p) => p.byId("delete").disabled === false;

const HEADINGS = {
  connecting: "Connecting to Halvo's capture service",
  not_running: "Capture service not running",
  held_unreachable: "Halvo's capture service is not answering. Capture may still be running. Halvo keeps trying.",
  status_refused: "Halvo's capture service reported a state that Halvo cannot rely on. Capture may be running.",
  unsupported: "Capture is not available on Windows in this version.",
};
const CAPTURE = {
  running: "Halvo is capturing",
  partial: "Halvo is capturing, but not every part of capture runs",
  starting: "Halvo's capture is starting",
  pausing: "Capture is stopping",
  paused: "Halvo is paused",
  no_indicator: "Halvo is not capturing while it cannot show that it runs",
  awaiting_resume: "Capture is off until you press Resume",
  faulted: "Halvo is not capturing",
};

test("the page draws every state fixture: the heading follows service and capture, the toggle follows desired, and Delete everything is offered by the rule of C10.2", async () => {
  assert.ok(STATES.length >= 30, `${STATES.length} state fixtures`);
  for (const name of STATES) {
    const status = fixture(name);
    const p = await page(status);
    const heading = status.service === "running" ? CAPTURE[status.capture] : HEADINGS[status.service];
    assert.equal(p.text("state"), heading, name);
    const toggle = status.desired === "paused" ? "Resume" : "Pause";
    assert.equal(p.text("toggle"), toggle, name);
    assert.equal(p.byId("toggle").disabled, status.desired === null, name);
    const store = status.store;
    const offer = status.service === "running" && store !== null && store.state !== "deleting" && store.error !== "locked" && !(store.state === "opening" && store.id === null);
    assert.equal(offered(p), offer, name);
    if (status.service !== "running") {
      assert.deepEqual(FOUR.filter((w) => visibleText(p.document.body).includes(w)), [], name);
    }
    if (status.service === "held_unreachable" || status.service === "status_refused") {
      assert.doesNotMatch(visibleText(p.document.body), /Capture service not running/, name);
    }
    if (status.capture === "pausing" || status.capture === "awaiting_resume") {
      assert.doesNotMatch(visibleText(p.document.body), /paused/, name);
    }
  }
});

test("a store value that this page does not know is drawn as unavailable with the value, and Delete everything stays offered", async () => {
  const status = { ...fixture("capture_status-store-unavailable-io"), store: { state: "frozen_by_a_later_contract", id: "0123456789abcdef0123456789abcdef", error: null, retry_in_s: null } };
  const p = await page(status);
  assert.equal(p.text("store-state"), "Halvo's store is in a state that this version does not know: frozen_by_a_later_contract");
  assert.equal(offered(p), true);
});

test("a sealed store and one made under the other custody offer Delete everything, and so does no_release_custody", async () => {
  for (const name of ["capture_status-store-sealed", "capture_status-store-sealed-custody-kind", "capture_status-store-no-release-custody", "capture_status-store-older", "capture_status-store-newer"]) {
    const p = await page(fixture(name));
    assert.equal(offered(p), true, name);
    assert.notEqual(p.text("store-state"), "", name);
  }
});

test("RR-app-ui-16: an unavailable store shows its cause and countdown and offers Delete everything, except while the cause is locked", async () => {
  const io = await page(fixture("capture_status-store-unavailable-io"));
  assert.equal(io.text("store-state"), "Halvo's store cannot be opened right now. Halvo tries again in 5 s.");
  assert.equal(offered(io), true);
  const locked = await page(fixture("capture_status-store-unavailable-locked"));
  assert.equal(locked.text("store-state"), "Another copy of Halvo's recorder is already using your data. Quit the other copy or restart this computer. Nothing was lost or changed. Halvo tries again in 5 s.");
  assert.equal(offered(locked), false);
});

test("RR-app-ui-16: a refused root has no countdown and offers Delete everything", async () => {
  const p = await page(fixture("capture_status-store-unavailable-root-denied"));
  assert.equal(p.text("store-state"), "This account cannot read the key of Halvo's store. Delete everything removes it.");
  assert.doesNotMatch(p.text("store-state"), /tries again/);
  assert.equal(offered(p), true);
});

test("ruling ST-30: the cause obstructed shows the owner's words, which do not lead to Delete everything, and the delete stays offered", async () => {
  const p = await page(fixture("capture_status-store-unavailable-obstructed"));
  assert.equal(p.text("store-state"), "Halvo is not recording. Something is in the way of its data folder.");
  assert.match(p.text("store-text"), /^Something that is not Halvo's stands where Halvo keeps what it records/);
  assert.doesNotMatch(p.text("store-state") + p.text("store-text"), /Delete everything|delete/);
  assert.equal(offered(p), true);
});

test("an opening store with no id offers no delete, and one with an id does", async () => {
  const noId = await page(fixture("capture_status-store-opening"));
  assert.equal(offered(noId), false);
  const withId = await page({ ...fixture("capture_status-store-opening"), store: { ...fixture("capture_status-store-opening").store, id: "0123456789abcdef0123456789abcdef" } });
  assert.equal(offered(withId), true);
});

test("a faulted status reaches the page with its reason", async () => {
  const p = await page(fixture("capture_status-faulted"));
  assert.equal(p.text("state"), "Halvo is not capturing");
  assert.equal(p.text("capture-reason"), "A permission that capture needs is off.");
});

test("RR-app-ui-30: a failed source gets a row with its steps, a failing drain gets its line, and Unsupported says that capture is not available on Windows", async () => {
  const failed = await page(fixture("capture_status-failed-source"));
  const items = failed.byId("failed-sources").children.map((li) => li.textContent);
  assert.deepEqual(items, ["Typing and the pointer has stopped. The steps under Permissions turn it back on."]);
  const drain = await page(fixture("capture_status-drain-failing"));
  assert.equal(drain.text("drain-line"), "Not saving: the disk is full.");
  const expiry = await page(fixture("capture_status-expiry-failing"));
  assert.match(expiry.text("expiry-line"), /^Old capture is not being deleted: a read or a write failed\./);
  assert.equal(offered(expiry), true);
  const unsupported = await page(fixture("capture_status-unsupported"));
  assert.equal(unsupported.text("state"), "Capture is not available on Windows in this version.");
});

test("X-app-ui-01: an unsigned macOS build shows the custody sentence of texts.custody word for word, and another custody shows none of it", async () => {
  const running = await page(fixture("capture_status-running"));
  assert.equal(
    running.text("custody"),
    "This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.",
  );
  const enclave = await page(fixture("capture_status-custody-enclave"));
  assert.doesNotMatch(enclave.text("custody"), /unsigned/);
  const none = await page(fixture("capture_status-not-running"));
  assert.equal(none.text("custody"), "");
});

test("X-app-ui-05: the totals on disk and of records are labelled apart, GB is defined, and an unknown cap formats no number", async () => {
  const p = await page(fixture("capture_status-running"));
  const line = p.byId("total").parentNode.textContent;
  assert.equal(line, "On disk: 1.0 MB of a cap of 25 GB. Recorded: 512 KB.");
  assert.match(visibleText(p.document.body), /GB here means 1,073,741,824 bytes/);
  assert.equal(p.text("cap-storage"), "Halvo keeps at most 25 GB and removes the oldest days first when it reaches that.");
  const unknown = await page(fixture("capture_status-cap-unknown"));
  assert.equal(unknown.text("cap"), "unknown");
  assert.equal(unknown.byId("cap-gb").value, "");
  assert.equal(unknown.text("cap-storage"), "Halvo could not read its storage limit and removes nothing to make room until you set one.");
});

test("RR-app-ui-24: the page keeps no report sentence and draws what is recorded from texts.recorded", async () => {
  const p = await page(fixture("capture_status-running"));
  const text = visibleText(p.document.body);
  assert.doesNotMatch(text, /report/i);
  const recorded = p.byId("recorded").children.map((li) => li.textContent);
  assert.deepEqual(recorded, fixture("capture_status-running").texts.recorded.map((r) => r.text));
  assert.ok(recorded.includes("Screen activity is not recorded."));
});

test("RR-app-ui-29: the column reads Day (UTC) with the local hour at which a day starts, and a row after 16:00 local on a UTC-8 clock keeps its UTC day", async () => {
  const inventory = { ...fixture("inventory-empty"), rows: [{ day: "2026-11-09", modality: "keystroke", events: 1, bytes: 10 }] };
  const p = await openPage({ timezoneOffset: 480, replies: { inventory: () => inventory } });
  const head = p.byId("inventory").querySelector("thead").textContent;
  assert.match(head, /^Day \(UTC\)/);
  assert.equal(p.text("day-start"), "A day here is a UTC day: it starts at 16:00 on this computer's clock.");
  assert.equal(p.byId("inventory").querySelector("tbody").children[0].children[0].textContent, "2026-11-09");
  const east = await openPage({ timezoneOffset: -330 });
  assert.equal(east.text("day-start"), "A day here is a UTC day: it starts at 05:30 on this computer's clock.");
});

test("RR-app-ui-24: the kind screen_frame is named Screen activity in the inventory", async () => {
  const p = await openPage({ replies: { inventory: () => fixture("inventory-two-rows-one-day") } });
  const kinds = p.byId("inventory").querySelector("tbody").children.map((tr) => tr.children[1].textContent);
  assert.deepEqual(kinds, ["Screen activity", "Typing"]);
});

test("RR-app-ui-11: with a lost grant whose source still runs the banner does not say that capture has stopped, and with a stopped source it does", async () => {
  const base = fixture("onboarding_state-lost");
  const lost = {
    ...base,
    lost_grants: ["input_monitoring"],
    permissions: base.permissions.map((p) =>
      p.permission === "input_monitoring" ? { ...p, lost: true, grant: "not_granted" } : { ...p, lost: false, grant: "granted" },
    ),
  };
  const runningSource = await openPage({ replies: { capture_status: () => fixture("capture_status-lost-grant-source-running"), onboarding_state: () => lost } });
  assert.equal(runningSource.visible("lost-banner"), true);
  assert.doesNotMatch(runningSource.text("lost-banner"), /has stopped/);
  assert.doesNotMatch(runningSource.text("lost-banner"), /macOS/);
  const stoppedSource = await openPage({ replies: { capture_status: () => fixture("capture_status-lost-grant-source-stopped"), onboarding_state: () => lost } });
  assert.match(stoppedSource.text("lost-banner"), /has stopped/);
});
