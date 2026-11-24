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
  unsupported: "Capture service not running",
  another_version: "Halvo's capture service is of another version. Update Halvo.",
  not_started: "Halvo is not recording yet.",
  data_dir_unusable: "Halvo cannot use its data folder: the folder, or a folder above it, is a shortcut that leads nowhere, is not a folder, or cannot be read. Halvo started nothing and changed nothing there. Put the folder back in its place, or ask the person who set up this computer, then open Halvo again.",
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

test("the page draws every state fixture: the heading follows service and capture, the toggle follows desired and reads Resume where capture awaits a resume, and Delete everything is offered by the rule of C10.2", async () => {
  assert.ok(STATES.length >= 30, `${STATES.length} state fixtures`);
  for (const name of STATES) {
    const status = fixture(name);
    const p = await page(status);
    const heading = status.service === "running" ? CAPTURE[status.capture] : HEADINGS[status.service];
    assert.equal(p.text("state"), heading, name);
    const consent = status.service === "not_started";
    const notRunning = status.service === "not_running";
    const toggle = consent || notRunning ? "Start" : status.desired === "paused" || status.capture === "awaiting_resume" ? "Resume" : "Pause";
    assert.equal(p.text("toggle"), toggle, name);
    assert.equal(p.byId("toggle").disabled, !consent && !notRunning && status.desired === null, name);
    const store = status.store;
    const offer = notRunning || (status.service === "running" && store !== null && (store.state !== "deleting" || store.delete_unfinished === true) && store.error !== "locked" && !(store.state === "opening" && store.id === null));
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

test("RR-app-ui-30: a failed source gets a row with its steps, a failing drain gets its line, Unsupported reads as not running, and a login item that launchd does not hold gets its line", async () => {
  const failed = await page(fixture("capture_status-failed-source"));
  const items = failed.byId("failed-sources").children.map((li) => li.textContent);
  assert.deepEqual(items, ["Typing and the pointer has stopped. The steps under Permissions turn it back on."]);
  const drain = await page(fixture("capture_status-drain-failing"));
  assert.equal(drain.text("drain-line"), "Not saving: the disk is full.");
  const expiry = await page(fixture("capture_status-expiry-failing"));
  assert.match(expiry.text("expiry-line"), /^Old capture is not being deleted: a read or a write failed\./);
  assert.equal(offered(expiry), true);
  const unsupported = await page(fixture("capture_status-unsupported"));
  assert.equal(unsupported.text("state"), "Capture service not running");
  const notAtLogin = await page({ ...fixture("capture_status-not-running"), login_item_loaded: false });
  assert.equal(notAtLogin.text("login-line"), "Halvo should start by itself when you log in. If it does not, open System Settings, then General, then Login Items, and turn Halvo on.");
  for (const loaded of [true, null]) assert.equal((await page({ ...fixture("capture_status-running"), login_item_loaded: loaded })).text("login-line"), "", String(loaded));
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

test("cap-not-set: behind a reset of the settings the cap line says that Halvo has no storage limit, and a cap that cannot be read keeps its own sentence", async () => {
  const notSet = await page(fixture("capture_status-cap-not-set"));
  assert.equal(notSet.text("cap-storage"), "Halvo has no storage limit and removes nothing to make room until you set one.");
  assert.equal(notSet.byId("cap-gb").value, "");
  const unknown = await page(fixture("capture_status-cap-unknown"));
  assert.equal(unknown.text("cap-storage"), "Halvo could not read its storage limit and removes nothing to make room until you set one.");
});

test("cap-not-set: the Stored view says with no storage limit behind the size on disk for a limit that is not set, and keeps its line for a known cap and a cap that cannot be read", async () => {
  const line = async (name) => (await page(fixture(name))).byId("total").parentNode.textContent;
  assert.equal(await line("capture_status-cap-not-set"), "On disk: 1.0 MB with no storage limit. Recorded: 512 KB.");
  assert.equal(await line("capture_status-cap-unknown"), "On disk: 1.0 MB of a cap of unknown. Recorded: 512 KB.");
  assert.equal(await line("capture_status-running"), "On disk: 1.0 MB of a cap of 25 GB. Recorded: 512 KB.");
});

test("RR-app-ui-24: the page keeps no report sentence and draws what is recorded from texts.recorded", async () => {
  const p = await page(fixture("capture_status-running"));
  const text = visibleText(p.document.body);
  assert.doesNotMatch(text, /report/i);
  const recorded = p.byId("recorded").children.filter((li) => li.id !== "typing-control").map((li) => li.textContent);
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

test("pointer-alone: behind a lost grant of Input Monitoring the banner says that the part has stopped while the input line reads impaired for the grant, and that it keeps running while the line reads running", async () => {
  const base = fixture("onboarding_state-lost");
  const lost = {
    ...base,
    lost_grants: ["input_monitoring"],
    permissions: base.permissions.map((p) =>
      p.permission === "input_monitoring" ? { ...p, lost: true, grant: "not_granted" } : { ...p, lost: false, grant: "granted" },
    ),
  };
  const pane = lost.permissions.find((p) => p.permission === "input_monitoring").pane_name;
  const banner = async (name) => {
    const p = await openPage({ replies: { capture_status: () => fixture(name), onboarding_state: () => lost } });
    const w = p.window.Halvo.words;
    assert.equal(p.visible("lost-banner"), true, name);
    return [p.text("lost-banner"), w.LOST_STOPPED.replace("{names}", pane), w.LOST_RUNNING.replace("{names}", pane)];
  };
  const [alone, stopped] = await banner("capture_status-lost-grant-source-pointer-alone");
  assert.equal(alone, stopped, "an input line impaired for the grant: the part of the keys has stopped");
  const [running, , keepsRunning] = await banner("capture_status-lost-grant-source-running");
  assert.equal(running, keepsRunning, "an input line that reads running: the part keeps running");
});

test("control-2b: a service of another version reads update Halvo, with none of the four claims and never not running", async () => {
  const p = await page(fixture("capture_status-another-version"));
  assert.equal(p.text("state"), "Halvo's capture service is of another version. Update Halvo.");
  assert.deepEqual(FOUR.filter((w) => visibleText(p.document.body).includes(w)), []);
  assert.doesNotMatch(visibleText(p.document.body), /Capture service not running/);
  assert.equal(p.byId("toggle").disabled, true);
  assert.equal(p.window.Halvo.service(), "another_version");
});

test("control-2b: the sentence typed_ahead stands under the password sentence when the status gives it, and not otherwise", async () => {
  const mac = await page(fixture("capture_status-running"));
  const limits = mac.byId("keeps-limits").children.map((el) => el.textContent);
  const passwords = fixture("capture_status-running").texts.passwords;
  const typed = fixture("capture_status-running").texts.typed_ahead;
  assert.ok(typed, "the macOS fixture holds typed_ahead");
  assert.equal(limits.indexOf(typed), limits.indexOf(passwords) + 1);
  const win = await page(fixture("capture_status-running-windows"));
  assert.equal(win.byId("keeps-limits").children.some((el) => el.textContent === typed), false);
  const off = await page(fixture("held-child"));
  const offLimits = off.byId("keeps-limits").children.map((el) => el.textContent);
  assert.deepEqual([passwords, typed].filter((line) => offLimits.includes(line)), [], "a Mac view with typing off");
  assert.equal(off.byId("recorded").children.some((li) => /each key/.test(li.textContent)), false);
});

test("typing-words-mac: What Halvo keeps shows the sentence on withheld keys under typed_ahead when the status gives it, and not otherwise", async () => {
  const withheld = "Halvo did not save part of what you typed, because it may have been a password. What Halvo leaves out this way is never saved. You do not need to do anything.";
  const running = fixture("capture_status-running");
  const mac = await page({ ...running, texts: { ...running.texts, keys_withheld: withheld } });
  const limits = mac.byId("keeps-limits").children.map((el) => el.textContent);
  assert.equal(limits.indexOf(withheld), limits.indexOf(running.texts.typed_ahead) + 1);
  const none = await page(running);
  assert.equal(none.byId("keeps-limits").children.some((el) => el.textContent === withheld), false);
});

test("status-honest: where Halvo left out every record by doubt, the line under the heading says the ruled sentence with the heading and the dot as they are; plain, paused or held it does not", async () => {
  const said = "Halvo could not tell whether to leave out the app that you were using, so it has recorded nothing since then. It records again as soon as it can tell.";
  for (const name of ["capture_status-running", "capture_status-running-windows"]) {
    const running = fixture(name);
    for (const capture of ["running", "partial"]) {
      const doubt = await page({ ...running, capture, left_out_in_doubt: true });
      assert.equal(doubt.text("capture-reason"), said, `${name} ${capture}`);
      assert.equal(doubt.text("state"), CAPTURE[capture], `${name} ${capture}`);
      assert.equal(doubt.byId("dot").classList.contains("on"), true, `${name} ${capture}`);
    }
    const plain = await page(running);
    assert.equal(plain.text("capture-reason"), "", name);
    const paused = await page({ ...running, desired: "paused", capture: "paused", left_out_in_doubt: true });
    assert.equal(paused.text("capture-reason"), "", name);
    const held = await page({ ...running, hold: { press: "resume" }, left_out_in_doubt: true });
    assert.notEqual(held.text("capture-reason"), said, name);
  }
});

test("typing-says: where the Mac's input row reads impaired by secure_input_elsewhere, the line under the heading says the ruled sentence with the heading and the dot as they are; plain, by another reason, as the pointer, on Windows, paused or held it does not, and beside the doubt the doubt's sentence alone shows", async () => {
  const said = "Halvo is not recording typing right now, because a password field or another app has asked your Mac to keep typing private. It records typing again by itself when that ends.";
  const doubt = "Halvo could not tell whether to leave out the app that you were using, so it has recorded nothing since then. It records again as soon as it can tell.";
  const turned = (status, name, row) => ({ ...status, sources: status.sources.map((src) => (src.name === name ? { ...src, ...row } : src)) });
  const elsewhere = { state: "impaired", reason: "secure_input_elsewhere" };
  const running = fixture("capture_status-running");
  const kept = turned(running, "input", elsewhere);
  for (const capture of ["running", "partial"]) {
    const shown = await page({ ...kept, capture });
    assert.equal(shown.text("capture-reason"), said, capture);
    assert.equal(shown.text("state"), CAPTURE[capture], capture);
    assert.equal(shown.byId("dot").classList.contains("on"), true, capture);
  }
  assert.equal((await page(running)).text("capture-reason"), "", "plain");
  assert.equal((await page(turned(running, "input", { state: "impaired", reason: "permission" }))).text("capture-reason"), "", "permission");
  assert.equal((await page(turned(kept, "input", { name: "pointer" }))).text("capture-reason"), "", "as the pointer");
  const windows = fixture("capture_status-running-windows");
  assert.equal((await page(turned(windows, "keyboard", elsewhere))).text("capture-reason"), "", "windows");
  assert.equal((await page({ ...kept, desired: "paused", capture: "paused" })).text("capture-reason"), "", "paused");
  assert.notEqual((await page({ ...kept, hold: { press: "resume" } })).text("capture-reason"), said, "held");
  assert.equal((await page({ ...kept, left_out_in_doubt: true })).text("capture-reason"), doubt, "beside the doubt");
});
