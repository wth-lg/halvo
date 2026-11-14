"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const GB = 1024 * 1024 * 1024;

function capPage(storeGb = 3) {
  let cap = 25 * GB;
  return openPage({
    replies: {
      capture_status: () => ({ ...fixture("capture_status-running"), store_bytes: storeGb * GB, store_cap_bytes: cap }),
      set_storage_cap: ({ capBytes }) => {
        cap = capBytes;
        return { ...fixture("capture_status-running"), store_bytes: Math.min(storeGb * GB, capBytes), store_cap_bytes: capBytes };
      },
    },
  });
}

const sentCaps = (page) => page.calls("set_storage_cap").map((c) => c.args.capBytes);

test("an edited cap field keeps its value across status polls until Save", async () => {
  const page = await capPage();
  assert.equal(page.byId("cap-gb").value, "25");
  await page.type("cap-gb", "10");
  page.byId("cap-gb").blur();
  await page.clock.advance(9000);
  assert.equal(page.byId("cap-gb").value, "10");
  await page.click("save-cap");
  assert.deepEqual(sentCaps(page), [10 * GB]);
  await page.clock.advance(3000);
  assert.equal(page.byId("cap-gb").value, "10");
});

for (const [value, message] of [
  ["", "Enter the cap in GB."],
  ["abc", "Enter the cap in GB."],
  ["0", "The cap must be at least 1 GB."],
  ["0.5", "The cap must be at least 1 GB."],
  ["1025", "The cap can be at most 1024 GB."],
]) {
  test(`Save with the field "${value}" sends nothing and says why next to the field`, async () => {
    const page = await capPage();
    await page.type("cap-gb", value);
    await page.click("save-cap");
    assert.deepEqual(sentCaps(page), []);
    assert.equal(page.text("cap-message"), message);
    assert.equal(page.text("error"), "");
  });
}

test("a cap that deletes nothing is saved at once, in bytes, and clears the message", async () => {
  const page = await capPage(3);
  await page.type("cap-gb", "");
  await page.click("save-cap");
  await page.type("cap-gb", "1024");
  await page.click("save-cap");
  assert.deepEqual(sentCaps(page), [1024 * GB]);
  assert.equal(page.text("cap-message"), "");
  assert.equal(page.visible("cap-confirm-row"), false);
});

test("a lower cap that deletes capture asks first and names the amount", async () => {
  const page = await capPage(3);
  await page.type("cap-gb", "2");
  await page.click("save-cap");
  assert.deepEqual(sentCaps(page), []);
  assert.equal(page.visible("cap-confirm-row"), true);
  assert.equal(page.text("cap-question"), "A cap of 2 GB deletes about 1.0 GB of the oldest capture now. This cannot be undone.");
  await page.click("confirm-cap");
  assert.deepEqual(sentCaps(page), [2 * GB]);
  assert.equal(page.visible("cap-confirm-row"), false);
});

test("Keep the current cap sends nothing", async () => {
  const page = await capPage(3);
  await page.type("cap-gb", "2");
  await page.click("save-cap");
  await page.click("cancel-cap");
  assert.deepEqual(sentCaps(page), []);
  assert.equal(page.visible("cap-confirm-row"), false);
});

test("before the first status reply a cap sends nothing and says that the stored size is not known", async () => {
  const page = await openPage({
    replies: {
      capture_status: () => new Promise(() => {}),
      set_storage_cap: ({ capBytes }) => ({ ...fixture("capture_status-running"), store_cap_bytes: capBytes }),
    },
  });
  await page.type("cap-gb", "1");
  await page.click("save-cap");
  assert.deepEqual(sentCaps(page), []);
  assert.equal(page.text("cap-message"), "The stored size is not known yet. Try again in a moment.");
});

test("an edit of the field closes an open confirmation, which then sends nothing", async () => {
  const page = await capPage(3);
  await page.type("cap-gb", "2");
  await page.click("save-cap");
  assert.equal(page.visible("cap-confirm-row"), true);
  await page.type("cap-gb", "20");
  assert.equal(page.visible("cap-confirm-row"), false);
  await page.click("confirm-cap");
  assert.deepEqual(sentCaps(page), []);
});
