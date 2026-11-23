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

const OFF_NOT_SAVED = "Typing is off for now, but Halvo could not save that. After Halvo starts again, turn it off once more.";

test("a turn off whose save failed says so in place of the settings sentence, on a Mac's page and on Windows', while the page shows typing off", async () => {
  for (const [name, row] of [
    ["capture_status-running", MAC_KEYS],
    ["capture_status-running-windows", WINDOWS_KEYS],
  ]) {
    const on = turnedOn(fixture(name));
    const off = turnedOff(fixture(name), row);
    let status = on;
    const page = await openPage({
      replies: {
        capture_status: () => status,
        set_typing: () => {
          status = off;
          return Promise.reject({ error: "state_file" });
        },
      },
    });
    assert.equal(page.visible("typing-not-saved"), false, `${name}: nothing before a turn`);
    await page.click("record-typing", { detail: 1 });
    assert.equal(page.text("error"), "", `${name}: not the settings sentence`);
    await page.clock.advance(60000);
    await page.settle();
    assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "false", name);
    assert.equal(page.visible("typing-not-saved"), true, `${name}: a minute later`);
    assert.equal(page.text("typing-not-saved"), OFF_NOT_SAVED, name);
    status = on;
    await page.clock.advance(3000);
    await page.settle();
    assert.equal(page.visible("typing-not-saved"), false, `${name}: gone once typing reads on`);
  }
});

const KEPT_OFF = "Typing is off. Halvo could not save that in its settings, but typing stays off until you turn it on again.";

test("a page that loads while a turn off is not saved says so from the status alone: the kept turn's sentence, and the tenth where this life alone holds it", async () => {
  for (const [name, row] of [
    ["capture_status-running", MAC_KEYS],
    ["capture_status-running-windows", WINDOWS_KEYS],
  ]) {
    for (const [word, sentence] of [
      ["kept", KEPT_OFF],
      ["this_life", OFF_NOT_SAVED],
    ]) {
      const off = turnedOff(fixture(name), row);
      const view = { ...off, typing_switch: { ...off.typing_switch, not_saved: word } };
      const page = await openPage({ replies: { capture_status: () => view } });
      assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "false", `${name} ${word}`);
      assert.equal(page.visible("typing-not-saved"), true, `${name} ${word}: at a fresh load`);
      assert.equal(page.text("typing-not-saved"), sentence, `${name} ${word}`);
      assert.equal(page.text("error"), "", `${name} ${word}`);
    }
  }
  const saved = await openPage({
    replies: { capture_status: () => turnedOff(fixture("capture_status-running"), MAC_KEYS) },
  });
  assert.equal(saved.visible("typing-not-saved"), false, "a saved turn off carries no word and shows no sentence");
  const printed = await openPage({ replies: { capture_status: () => fixture("capture_status-typing-off-kept") } });
  assert.equal(printed.text("typing-not-saved"), KEPT_OFF, "the view that the app prints for a kept turn");
});

test("beside a service that carries the word, a page that stays loaded across a new life says nothing of an unsaved turn once the status reads off with no word", async () => {
  const on = turnedOn(fixture("capture_status-running"));
  const savedOff = turnedOff(fixture("capture_status-running"), MAC_KEYS);
  const keptOff = { ...savedOff, typing_switch: { ...savedOff.typing_switch, not_saved: "kept" } };
  let status = on;
  const page = await openPage({
    replies: {
      capture_status: () => status,
      set_typing: () => {
        status = keptOff;
        return Promise.reject({ error: "state_file" });
      },
    },
  });
  await page.click("record-typing", { detail: 1 });
  await page.clock.advance(60000);
  await page.settle();
  assert.equal(page.text("typing-not-saved"), KEPT_OFF, "the life of the turn: the kept sentence");
  status = fixture("capture_status-not-running");
  await page.clock.advance(3000);
  await page.settle();
  status = savedOff;
  await page.clock.advance(3000);
  await page.settle();
  assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "false");
  assert.equal(page.visible("typing-not-saved"), false, "the next life saved the turn: no sentence of an unsaved turn");
});

test("the control stands in a row of its own where no row of keys stands: a paused Windows view and a Mac view whose input line failed", async () => {
  const windows = fixture("capture_status-running-windows");
  const pausedWindows = {
    ...windows,
    capture: "paused",
    desired: "paused",
    sources: windows.sources.map((s) => (s.name === "keyboard" ? { ...s, state: "stopped" } : s)),
    texts: { ...windows.texts, recorded: windows.texts.recorded.filter((r) => r.switch !== "key_class") },
  };
  for (const [name, view, row] of [
    ["a paused Windows view", pausedWindows, WINDOWS_KEYS],
    ["capture_status-failed-source", fixture("capture_status-failed-source"), MAC_KEYS],
  ]) {
    assert.equal(view.typing_switch.on, true, name);
    assert.deepEqual(keyText(await openPage({ replies: { capture_status: () => view } })), [], `${name}: no row of keys`);
    const page = await openPage({
      replies: {
        capture_status: () => view,
        set_typing: () => ({ ...view, typing_switch: { label: LABEL, on: false, if_on: IF_ON, row } }),
      },
    });
    assert.equal(page.visible("record-typing"), true, `${name}: the control within reach before Resume`);
    assert.equal(page.text("record-typing"), LABEL, name);
    await page.click("record-typing", { detail: 1 });
    assert.deepEqual(page.calls("set_typing").map((c) => c.args), [{ on: false }], name);
    assert.equal(page.byId("record-typing").getAttribute("aria-checked"), "false", name);
    assert.equal(page.text("typing-if-on-row"), row, `${name}: the row of keys of that system before a turn on`);
  }
});
