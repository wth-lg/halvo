"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete, loadedAgain } = require("../lib/page.js");

const OLD = fixture("capture_status-running").store.id;
const NEW = "00000000000000000000000000000010";
const rows = (page) => page.byId("inventory").querySelector("tbody").children.length;
const ofStore = (name, id) => ({ ...fixture(name), store_id: id });

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
}

test("L5: an inventory reply that was sent before the answer of a mutation is not drawn (adversary 2, ADV2-L2 by a pause)", async () => {
  let late = null;
  let n = 0;
  const page = await openPage({
    replies: {
      inventory: () => {
        n += 1;
        if (n === 2) return new Promise((resolve) => (late = () => resolve(fixture("inventory-sample"))));
        return fixture("inventory-two-rows-one-day");
      },
      set_capture_state: () => fixture("set_capture_state-paused"),
    },
  });
  assert.equal(rows(page), 2);
  const timer = page.clock.timers.find((t) => t.repeat && t.period === 15000);
  timer.fn();
  await page.settle();
  assert.ok(late, "the 15 s inventory read is out");
  await page.clock.advance(1100);
  await page.click("toggle");
  late();
  await page.settle();
  assert.equal(rows(page), 2, "the reply sent before the pause's answer is dropped");
});

test("L5: an inventory reply that was sent before a delete's answer is not drawn by the page that sent it", async () => {
  let late = null;
  let n = 0;
  const page = await openPage({
    replies: keptDelete({
      inventory: () => {
        n += 1;
        if (n === 2) return new Promise((resolve) => (late = () => resolve(fixture("inventory-sample"))));
        return fixture("inventory-two-rows-one-day");
      },
      delete_all: () => fixture("delete_all-deleted"),
    }),
  });
  const timer = page.clock.timers.find((t) => t.repeat && t.period === 15000);
  timer.fn();
  await page.settle();
  await confirmDelete(page);
  late();
  await page.settle();
  assert.equal(rows(page), 2, "the late reply of six rows is not drawn");
});

test("L5: after a delete the page that loads again draws no list of the deleted store beside the status of the new one", async () => {
  let status = fixture("capture_status-running");
  const page = await openPage({
    replies: keptDelete({
      capture_status: () => status,
      inventory: () => ofStore("inventory-sample", OLD),
      delete_all: () => {
        status = fixture("capture_status-after-delete");
        return fixture("delete_all-deleted");
      },
    }),
  });
  assert.equal(rows(page), 6);
  await confirmDelete(page);
  const next = await loadedAgain(page);
  assert.equal(next.visible("inventory"), false);
  assert.equal(rows(next), 0);
});

test("L5: after not running and back with another store, the list of the earlier store is not drawn (adversary 2, ADV2-FD)", async () => {
  let status = fixture("capture_status-running");
  const page = await openPage({ replies: { capture_status: () => status, inventory: () => ofStore("inventory-sample", OLD) } });
  assert.equal(rows(page), 6);
  status = fixture("capture_status-not-running");
  await page.clock.fire(3000);
  status = { ...fixture("capture_status-running"), store: { ...fixture("capture_status-running").store, id: NEW } };
  await page.clock.fire(3000);
  assert.equal(page.visible("inventory"), false);
  assert.equal(rows(page), 0);
});

test("L5: a list kept while the service is not running is drawn only for its own store", async () => {
  let status = fixture("capture_status-not-running");
  const page = await openPage({ replies: { capture_status: () => status, inventory: () => ofStore("inventory-sample", OLD) } });
  assert.equal(page.visible("inventory"), false);
  status = { ...fixture("capture_status-running"), store: { ...fixture("capture_status-running").store, id: NEW } };
  await page.clock.fire(3000);
  assert.equal(rows(page), 0, "the kept list of another store");
  status = fixture("capture_status-running");
  await page.clock.fire(3000);
  await page.clock.fire(15000);
  assert.equal(rows(page), 6, "a list of the store that stands is drawn");
});

test("G4 (N2, the review's case page-06): a list kept undrawn and older than the newest answer is dropped when a status of its store comes", async () => {
  const runningOf = (id) => ({ ...fixture("capture_status-running"), store: { ...fixture("capture_status-running").store, id } });
  let status = runningOf(NEW);
  const page = await openPage({
    replies: {
      capture_status: () => status,
      inventory: () => ofStore("inventory-sample", OLD),
      set_capture_state: () => ({ ...fixture("set_capture_state-paused"), store: runningOf(NEW).store }),
    },
  });
  assert.equal(rows(page), 0, "the list of another store is kept and not drawn");
  await page.clock.advance(1100);
  await page.click("toggle");
  assert.equal(page.calls("set_capture_state").length, 1);
  status = runningOf(OLD);
  await page.clock.fire(3000);
  assert.equal(rows(page), 0, "the kept list was sent before the pause's answer");
});

test("G5 (N3, the review's case page-03): a list kept during a hold and sent before a mutation's answer is not drawn at the release", async () => {
  let late = null;
  let n = 0;
  const page = await openPage({
    replies: {
      inventory: () => {
        n += 1;
        if (n === 2) return new Promise((resolve) => (late = () => resolve(ofStore("inventory-sample", OLD))));
        return fixture("inventory-two-rows-one-day");
      },
      set_capture_state: () => fixture("set_capture_state-paused"),
    },
  });
  assert.equal(rows(page), 2);
  const timer = page.clock.timers.find((t) => t.repeat && t.period === 15000);
  timer.fn();
  await page.settle();
  assert.ok(late, "the 15 s inventory read is out");
  await page.click("delete");
  assert.equal(page.visible("delete-confirm-row"), true, "Delete everything is armed, so the feeds are held");
  late();
  await page.settle();
  await page.clock.advance(1100);
  await page.click("toggle");
  assert.equal(page.calls("set_capture_state").length, 1, "the pause was answered while the hold stood");
  await page.click("cancel-delete");
  assert.equal(rows(page), 2, "the reply sent before the pause's answer is not drawn at the release");
});
