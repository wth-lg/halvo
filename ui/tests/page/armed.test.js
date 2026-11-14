"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const BOX = { left: 31, top: 600, width: 140, height: 34 };
const INSIDE = { clientX: 60, clientY: 615 };
const BELOW = { clientX: 60, clientY: 700 };

function armedPage(overrides = {}) {
  return openPage({ replies: { delete_all: () => fixture("delete_all-deleted"), ...overrides } });
}

async function armAtPoint(page) {
  page.byId("delete").rect = BOX;
  await page.press("delete", INSIDE);
}

const deletes = (page) => page.calls("delete_all").length;
const rows = (page) => page.byId("inventory").querySelector("tbody").children.length;

test("a reply for each view that arrives while Delete everything is armed draws nothing until the disarm, and is drawn after it", async () => {
  let status = fixture("capture_status-running");
  let onboarding = fixture("onboarding_state-none");
  let inventory = fixture("inventory-empty");
  const page = await armedPage({ capture_status: () => status, onboarding_state: () => onboarding, inventory: () => inventory });
  await armAtPoint(page);
  status = { ...fixture("capture_status-paused"), store_bytes: 1024 };
  onboarding = fixture("onboarding_state-missing");
  inventory = fixture("inventory-two-rows-one-day");
  await page.clock.fire(3000);
  await page.clock.fire(15000);
  assert.equal(page.text("state"), "capturing");
  assert.equal(page.visible("permissions-section"), false);
  assert.equal(rows(page), 0);
  assert.equal(page.text("total"), "370 B");
  assert.equal(page.byId("cap-gb").value, "25");
  await page.click("cancel-delete");
  assert.equal(page.text("state"), "paused");
  assert.equal(page.visible("permissions-section"), true);
  assert.equal(rows(page), 2);
  assert.equal(page.text("total"), "1.0 KB");
});

test("a confirmation at a point inside the box that the armed button had sends none, and one at another point 600 ms after arming sends one", async () => {
  const page = await armedPage();
  await armAtPoint(page);
  await page.clock.advance(600);
  await page.press("confirm-delete", INSIDE);
  assert.equal(deletes(page), 0);
  await page.press("confirm-delete", BELOW);
  assert.equal(deletes(page), 1);
});

test("a click on the confirmation whose press began on another element, or before its row was shown, sends none", async () => {
  const page = await armedPage();
  await page.pressDown("confirm-delete", BELOW);
  await armAtPoint(page);
  await page.clock.advance(600);
  await page.click("confirm-delete", { detail: 1, ...BELOW });
  assert.equal(deletes(page), 0);
  await page.pressDown("cancel-delete", BELOW);
  await page.click("confirm-delete", { detail: 1, ...BELOW });
  assert.equal(deletes(page), 0);
});

test("a click with no press of its own, from the keyboard or a screen reader, sends one", async () => {
  const page = await armedPage();
  await page.click("delete", { detail: 0 });
  await page.clock.advance(600);
  await page.click("confirm-delete", { detail: 0 });
  assert.equal(deletes(page), 1);
});

test("P5: a pressed click on the confirmation with a click count above 1 sends none", async () => {
  const page = await armedPage();
  await armAtPoint(page);
  await page.clock.advance(600);
  await page.press("confirm-delete", { ...BELOW, detail: 2 });
  assert.equal(deletes(page), 0);
});

test("P10: a pressed click inside the kept box away from the arming point sends none", async () => {
  const page = await armedPage();
  await armAtPoint(page);
  await page.clock.advance(600);
  await page.press("confirm-delete", { clientX: 150, clientY: 630 });
  assert.equal(deletes(page), 0);
  await page.press("confirm-delete", BELOW);
  assert.equal(deletes(page), 1);
});

test("P24: of two replies for one view that arrive while armed, the newest is drawn at the disarm", async () => {
  let status = fixture("capture_status-running");
  const page = await armedPage({ capture_status: () => status });
  await armAtPoint(page);
  status = { ...fixture("capture_status-paused"), store_bytes: 2048 };
  await page.clock.fire(3000);
  status = { ...fixture("capture_status-running"), store_bytes: 1024 };
  await page.clock.fire(3000);
  await page.click("cancel-delete");
  assert.equal(page.text("state"), "capturing");
  assert.equal(page.text("total"), "1.0 KB");
});

test("P30: the release draws the kept replies in the order of the views", async () => {
  let onboarding = fixture("onboarding_state-none");
  let inventory = fixture("inventory-empty");
  const page = await armedPage({ onboarding_state: () => onboarding, inventory: () => inventory });
  const order = [];
  for (const name of ["inventory", "onboarding", "status"]) page.window.Halvo.feed(name).on(() => order.push(name));
  await armAtPoint(page);
  inventory = fixture("inventory-two-rows-one-day");
  await page.clock.fire(15000);
  onboarding = fixture("onboarding_state-missing");
  await page.clock.fire(3000);
  assert.deepEqual(order, []);
  await page.click("cancel-delete");
  assert.deepEqual(order, ["status", "onboarding", "inventory"]);
});
