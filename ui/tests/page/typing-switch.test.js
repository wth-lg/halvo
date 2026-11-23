"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const LABEL = "Record typing";
const IF_ON = "If you turn this on:";
const NOT_RECORDED = "Halvo does not record what you type.";
const MAC_KEYS =
  "Halvo records each key you press, when and in which app, and what kind of key it is: a letter, digit or sign, a delete key, an arrow or page key, Return or Enter, Tab, a shortcut, or another key. It never records which letter, digit or sign.";
const WINDOWS_KEYS =
  "Halvo records a key only when, just after you press it, the text changes in a field that Windows does not report as a password field. It records when you pressed the key, in which app, and what kind of key it was: a letter, digit or sign, or a delete key. It never records which letter, digit or sign. Halvo can tell this only in Chrome and Edge and in the plain text boxes of classic Windows programs: in every other program it records no key. It records no Return or Enter, no Tab, no arrow key, no Escape and no shortcut.";
const UPDATE = "Halvo's capture service is of another version. Update Halvo.";
const STATE_FILE = "Halvo could not save its settings.";

function keyRow(view, text) {
  return {
    ...view,
    texts: {
      ...view.texts,
      recorded: view.texts.recorded.map((row) => (row.switch === "key_class" ? { ...row, text } : row)),
    },
  };
}

function turnedOff(view, row) {
  return { ...keyRow(view, NOT_RECORDED), typing_switch: { label: LABEL, on: false, if_on: IF_ON, row } };
}

function turnedOn(view) {
  return { ...view, typing_switch: { label: LABEL, on: true } };
}

const keyText = (page) =>
  page
    .byId("recorded")
    .children.filter((li) => li.getAttribute("data-switch") === "key_class")
    .map((li) => li.textContent);

test("the control of typing reads each state, never says off beside a row of keys, and turns the one switch by one set_typing", async () => {
  const on = turnedOn(fixture("capture_status-running"));
  const off = turnedOff(fixture("capture_status-running"), MAC_KEYS);
  let status = on;
  let answer = () => off;
  const page = await openPage({
    replies: {
      capture_status: () => status,
      set_typing: (args) => answer(args),
    },
  });
  assert.equal(page.visible("record-typing"), true, "the control beside the row of keys");
  assert.equal(page.text("record-typing"), LABEL);
  assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "true", "on");
  assert.equal(page.visible("typing-if-on"), false, "nothing before a turn on while it is on");
  assert.deepEqual(keyText(page), [MAC_KEYS]);

  await page.click("record-typing", { detail: 1 });
  assert.deepEqual(
    page.calls("set_typing").map((c) => c.args),
    [{ on: false }],
    "one request that turns the switch of keys alone",
  );
  assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "false", "the answer's view: off");
  assert.deepEqual(keyText(page), [NOT_RECORDED], "the standing row in its new place");
  assert.equal(page.visible("typing-if-on"), true);
  assert.equal(page.text("typing-if-on-lead"), IF_ON);
  assert.equal(page.text("typing-if-on-row"), MAC_KEYS, "a Mac's row of keys before a turn on");

  status = off;
  answer = () => Promise.reject({ error: "update_the_app" });
  await page.click("record-typing", { detail: 1 });
  assert.equal(page.calls("set_typing").length, 2);
  assert.deepEqual(page.calls("set_typing")[1].args, { on: true });
  assert.equal(page.text("error"), UPDATE, "a service of before: the standing sentence");
  assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "false", "the switch did not turn");
  assert.equal(page.byId("record-typing").disabled, false);

  answer = () => Promise.reject({ error: "state_file" });
  await page.click("record-typing", { detail: 1 });
  assert.equal(page.text("error"), STATE_FILE);

  const pointerAlone = await openPage({
    replies: { capture_status: () => keyRow(turnedOn(fixture("capture_status-running")), NOT_RECORDED) },
  });
  assert.equal(pointerAlone.byId("record-typing").getAttribute("aria-checked"), "true", "on, the grant missing");
  assert.equal(pointerAlone.visible("typing-if-on"), false, "no new sentence there");
  assert.deepEqual(keyText(pointerAlone), [NOT_RECORDED]);

  const windows = await openPage({
    replies: { capture_status: () => turnedOff(fixture("capture_status-running-windows"), WINDOWS_KEYS) },
  });
  assert.equal(windows.byId("record-typing").getAttribute("aria-checked"), "false");
  assert.equal(windows.text("typing-if-on-row"), WINDOWS_KEYS, "the Windows row W1 before a turn on");

  const none = await openPage({ replies: { capture_status: () => fixture("capture_status-not-running") } });
  assert.equal(none.visible("record-typing"), false, "no control without a service");

  const { typing_switch: _carried, ...bare } = fixture("capture_status-running");
  const without = await openPage({ replies: { capture_status: () => bare } });
  assert.equal(without.visible("record-typing"), false, "no control where the view carries none");
});
