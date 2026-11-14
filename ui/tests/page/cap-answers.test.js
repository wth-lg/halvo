"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete, loadedAgain } = require("../lib/page.js");

const GB = 1024 * 1024 * 1024;
const status = (bytesGb, capGb, id) => {
  const s = fixture("capture_status-running");
  return { ...s, store: { ...s.store, id: id || s.store.id }, store_bytes: bytesGb * GB, store_cap_bytes: capGb === null ? null : capGb * GB };
};
const EVICTING = "Halvo removes the oldest capture to fit the new cap. The size above shows what is stored now.";

async function lowerTo(page, gb) {
  await page.type("cap-gb", String(gb));
  await page.click("save-cap");
  await page.click("confirm-cap");
}

test("L7: a cap Save answered with no status keeps the field as typed and says one sentence by the reply's service", async () => {
  const rows = [
    ["capture_status-not-running", "I-98 (9)", "Halvo could not reach its capture service, so the cap was not changed."],
    ["capture_status-held-unreachable", "I-98 (9)", "Halvo could not reach its capture service, so the cap was not changed."],
    ["capture_status-unsupported", "I-98 (10)", "The cap was not changed."],
    [
      "capture_status-status-refused",
      "I-98 (11)",
      "Halvo cannot tell whether the cap was changed. Look at the cap shown here once the capture service answers. If it was lowered, older capture may have been removed.",
    ],
  ];
  for (const [name, entry, words] of rows) {
    let current = status(25, 25);
    const page = await openPage({
      replies: {
        capture_status: () => current,
        set_storage_cap: () => {
          current = { ...fixture(name), evicting: null };
          return current;
        },
      },
    });
    await lowerTo(page, 2);
    assert.equal(page.text("cap-message"), words, `${name} ${entry}`);
    assert.equal(page.byId("cap-gb").value, "2", `${name}: the field after the reply`);
    await page.clock.fire(3000);
    assert.equal(page.byId("cap-gb").value, "2", `${name}: the field after one more status`);
    current = status(3, 25);
    await page.clock.fire(3000);
    assert.equal(page.byId("cap-gb").value, "2", `${name}: a status of the service does not overwrite the typed value`);
  }
});

test("L7: a cap Save answered with a status takes the field from it as before", async () => {
  const page = await openPage({ replies: { capture_status: () => status(25, 25), set_storage_cap: () => ({ ...status(3, 2), evicting: false }) } });
  await lowerTo(page, 2);
  assert.equal(page.text("cap-message"), "");
  assert.equal(page.byId("cap-gb").value, "2");
});

async function evictingPage(after) {
  let current = status(25, 25);
  const replies = keptDelete({
    capture_status: () => current,
    set_storage_cap: () => ({ ...status(25, 2), evicting: true }),
    delete_all: () => fixture("delete_all-deleted"),
  });
  const page = await openPage({ replies });
  await lowerTo(page, 2);
  assert.equal(page.text("cap-evicting"), EVICTING);
  current = after;
  return { page, set: (s) => (current = s) };
}

test("I-98 (12): the eviction line stands while a status after the cap's answer reads the size above the cap", async () => {
  const { page } = await evictingPage(status(10, 2));
  await page.clock.fire(3000);
  assert.equal(page.text("cap-evicting"), EVICTING);
});

test("L8: the eviction line is cleared by a status after the cap's answer whose size is at or under the cap", async () => {
  const { page } = await evictingPage(status(2, 2));
  await page.clock.fire(3000);
  assert.equal(page.text("cap-evicting"), "");
});

test("L8: the eviction line is cleared by a status whose size or cap is not a number", async () => {
  for (const after of [{ ...status(10, 2), store_bytes: null }, status(10, null)]) {
    const { page } = await evictingPage(after);
    await page.clock.fire(3000);
    assert.equal(page.text("cap-evicting"), "");
  }
});

test("L8: the eviction line is cleared by a status that names another store", async () => {
  const { page } = await evictingPage(status(10, 2, "00000000000000000000000000000010"));
  await page.clock.fire(3000);
  assert.equal(page.text("cap-evicting"), "");
});

test("L8: the eviction line is cleared at a no-status view", async () => {
  const { page } = await evictingPage(fixture("capture_status-not-running"));
  await page.clock.fire(3000);
  assert.equal(page.text("cap-evicting"), "");
});

test("L8: the eviction line is cleared at the next cap reply", async () => {
  let reply = { ...status(25, 2), evicting: true };
  const page = await openPage({ replies: { capture_status: () => status(25, 25), set_storage_cap: () => reply } });
  await lowerTo(page, 2);
  assert.equal(page.text("cap-evicting"), EVICTING);
  reply = { ...status(25, 30), evicting: false };
  await page.type("cap-gb", "30");
  await page.click("save-cap");
  assert.equal(page.text("cap-evicting"), "");
});

test("L8: the eviction line is gone after a delete answered deleted, with the load, and at any load", async () => {
  const { page } = await evictingPage(status(10, 2));
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  const next = await loadedAgain(page);
  assert.equal(next.text("cap-evicting"), "");
  for (let i = 0; i < 20; i++) await next.clock.fire(3000);
  assert.equal(next.text("cap-evicting"), "");
});
