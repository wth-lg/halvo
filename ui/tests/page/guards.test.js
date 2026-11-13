"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const deletes = (page) => page.calls("delete_all").length;
const armed = (page) => page.visible("confirm-delete");

async function deletePage() {
  return openPage({ replies: { delete_all: () => fixture("delete_all-deleted") } });
}

test("arming keeps Delete everything in place and disabled, with the question and the confirmation on a second row", async () => {
  const page = await deletePage();
  const start = page.byId("delete");
  const row = start.parentNode;
  await page.click("delete");
  assert.equal(start.parentNode, row);
  assert.equal(start.disabled, true);
  assert.equal(page.visible("delete"), true);
  const confirmRow = page.byId("confirm-delete").parentNode;
  assert.notEqual(confirmRow, row);
  assert.equal(confirmRow.contains(page.byId("cancel-delete")), true);
  assert.equal(page.visible("delete-question"), true);
  assert.match(page.text("delete-question"), /Copies that Halvo made next to your own files stay/);
});

test("a confirmation 100 ms after arming sends no delete_all", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(100);
  await page.press("confirm-delete");
  assert.equal(deletes(page), 0);
});

test("a confirmation with a click count of 2 sends no delete_all, also after 600 ms", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(600);
  await page.click("confirm-delete", { detail: 2 });
  assert.equal(deletes(page), 0);
});

test("one click 600 ms after arming sends one delete_all", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  assert.equal(deletes(page), 1);
  assert.equal(armed(page), false);
  assert.equal(page.byId("delete").disabled, false);
});

test("a second click on Delete everything while armed sends nothing and does not re-arm", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(300);
  await page.click("delete", { detail: 2 });
  await page.clock.advance(300);
  await page.press("confirm-delete");
  assert.equal(deletes(page), 1);
});

test("the control disarms 10 s after arming", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(9999);
  assert.equal(armed(page), true);
  await page.clock.advance(1);
  assert.equal(armed(page), false);
  assert.equal(page.byId("delete").disabled, false);
  await page.press("confirm-delete");
  assert.equal(deletes(page), 0);
});

test("the control disarms when the window hides", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(600);
  await page.hide();
  assert.equal(armed(page), false);
  page.byId("confirm-delete").classList.remove("hidden");
  await page.press("confirm-delete");
  assert.equal(deletes(page), 0);
});

test("Cancel disarms and a later confirmation sends nothing", async () => {
  const page = await deletePage();
  await page.click("delete");
  await page.clock.advance(600);
  await page.click("cancel-delete");
  assert.equal(armed(page), false);
  page.byId("confirm-delete").classList.remove("hidden");
  await page.press("confirm-delete");
  assert.equal(deletes(page), 0);
});

function togglePage() {
  let state = "running";
  return openPage({
    replies: {
      capture_status: () => ({ ...fixture("capture_status-running"), state }),
      set_capture_state: ({ capture }) => {
        state = capture;
        return { ...fixture("capture_status-running"), state };
      },
    },
  });
}

const sentStates = (page) => page.calls("set_capture_state").map((c) => c.args.capture);

test("two clicks on Pause 100 ms apart send one request and capture stays paused", async () => {
  const page = await togglePage();
  await page.click("toggle");
  await page.clock.advance(100);
  await page.click("toggle");
  assert.deepEqual(sentStates(page), ["paused"]);
  assert.equal(page.text("state"), "paused");
});

test("a click on the toggle with a click count of 2 sends nothing", async () => {
  const page = await togglePage();
  await page.click("toggle", { detail: 2 });
  assert.deepEqual(sentStates(page), []);
});

test("the toggle sends what its label names", async () => {
  const page = await togglePage();
  assert.equal(page.text("toggle"), "Pause");
  await page.click("toggle");
  await page.clock.advance(1100);
  assert.equal(page.text("toggle"), "Resume");
  await page.click("toggle");
  assert.deepEqual(sentStates(page), ["paused", "running"]);
});

test("the toggle is held for about 1 s after the state changes", async () => {
  const page = await togglePage();
  await page.click("toggle");
  await page.clock.advance(900);
  await page.click("toggle");
  assert.deepEqual(sentStates(page), ["paused"]);
  await page.clock.advance(200);
  await page.click("toggle");
  assert.deepEqual(sentStates(page), ["paused", "running"]);
});

test("a confirmation 10 s or more after arming sends nothing, also when the disarm timer runs late", async () => {
  const page = await deletePage();
  await page.click("delete");
  page.clock.now = 12000;
  await page.press("confirm-delete");
  assert.equal(deletes(page), 0);
  assert.equal(armed(page), false);
  assert.equal(page.byId("delete").disabled, false);
});
