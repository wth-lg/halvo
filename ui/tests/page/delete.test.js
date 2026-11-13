"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const ID = fixture("capture_status-running").store.id;
const OTHER = "00000000000000000000000000000010";

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
}

const deleted = (fields) => ({ ...fixture("delete_all-deleted"), ...fields });

test("RR-app-ui-18: the page sends the id that the status showed when the person armed the delete, not a later one", async () => {
  let status = fixture("capture_status-running");
  const page = await openPage({ replies: { capture_status: () => status, delete_all: () => fixture("delete_all-deleted") } });
  await page.click("delete");
  status = { ...status, store: { ...status.store, id: OTHER } };
  await page.clock.fire(3000);
  await page.clock.advance(600);
  await page.press("confirm-delete");
  assert.deepEqual(page.calls("delete_all").map((c) => c.args), [{ storeId: ID }]);
});

test("a status with no readable id arms a delete with null", async () => {
  const page = await openPage({ replies: { capture_status: () => fixture("capture_status-store-unavailable-no-id"), delete_all: () => fixture("delete_all-deleted") } });
  await confirmDelete(page);
  assert.deepEqual(page.calls("delete_all").map((c) => c.args), [{ storeId: null }]);
});

test("already_deleted says that the store was already deleted and that anything captured since is still stored, and is no report of zero counts", async () => {
  const page = await openPage({ replies: { delete_all: () => fixture("delete_all-already-deleted") } });
  await confirmDelete(page);
  assert.equal(page.text("delete-result"), "That store was already deleted. Anything captured since then is still stored.");
  assert.doesNotMatch(page.text("delete-result"), /0 /);
});

test("delete_refused shows the owner's words, reads the status, and the page sends no second delete by itself", async () => {
  const page = await openPage({ replies: { delete_all: () => fixture("delete_all-refused") } });
  const reads = page.calls("capture_status").length;
  await confirmDelete(page);
  assert.match(page.text("delete-result"), /^Halvo did not delete anything this time\./);
  assert.ok(page.calls("capture_status").length > reads, "the status is read again");
  for (let i = 0; i < 5; i++) await page.clock.fire(3000);
  await page.clock.advance(20000);
  assert.equal(page.calls("delete_all").length, 1);
  assert.equal(page.byId("delete").disabled, false, "a new press can arm again");
});

test("ruling ST-30: a delete is shown as done with nothing left only when the root is destroyed and nothing was not followed", async () => {
  const cases = [
    [deleted({ nothing_left: true }), "Deleted. Nothing that Halvo recorded can be read again, and nothing is left."],
    [
      deleted({ nothing_left: false, not_followed: 2 }),
      "Halvo destroyed its keys, so nothing that was recorded can be read again. Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there. They cannot be read, and you can remove them.",
    ],
    [deleted({ nothing_left: false, root_destroyed: "unknown" }), "Halvo cannot tell whether a key file is left."],
  ];
  for (const [reply, words] of cases) {
    const page = await openPage({ replies: { delete_all: () => reply } });
    await confirmDelete(page);
    assert.equal(page.text("delete-result"), words);
    assert.equal(page.visible("delete-cleanup"), false);
  }
});

test("a root that survived shows the cleanup command with the owner's note on the account name", async () => {
  const command = "rm '/Users/<name>/Library/Application Support/Halvo/roots/00000000000000000000000000000009.root'";
  const page = await openPage({ replies: { delete_all: () => deleted({ nothing_left: false, root_destroyed: "no", cleanup_command: command }) } });
  await confirmDelete(page);
  assert.equal(page.text("delete-result"), "Halvo could not remove one key file. Remove it with this command:");
  assert.equal(page.text("delete-cleanup"), command);
  assert.equal(page.visible("delete-cleanup-note"), true);
  assert.equal(page.text("delete-cleanup-note"), "This line names a folder on this computer and your account name. Take it out before you send it to someone, if you prefer.");
});

test("the page clears the rows after a delete even when the refresh fails", async () => {
  let failing = false;
  const page = await openPage({
    replies: {
      inventory: () => {
        if (failing) throw { error: "internal" };
        return fixture("inventory-two-rows-one-day");
      },
      delete_all: () => {
        failing = true;
        return fixture("delete_all-deleted");
      },
    },
  });
  assert.equal(page.byId("inventory").querySelector("tbody").children.length, 2);
  await confirmDelete(page);
  assert.equal(page.byId("inventory").querySelector("tbody").children.length, 0);
});

test("C10.5: a change of service between the arming and the confirming press sends no delete and reads as delete_refused", async () => {
  let status = fixture("capture_status-running");
  const page = await openPage({ replies: { capture_status: () => status, delete_all: () => fixture("delete_all-deleted") } });
  await page.click("delete");
  status = fixture("capture_status-held-unreachable");
  await page.clock.fire(3000);
  await page.clock.advance(600);
  await page.press("confirm-delete");
  assert.equal(page.calls("delete_all").length, 0);
  assert.match(page.text("delete-result"), /^Halvo did not delete anything this time\./);
});

test("Delete everything is locked while its request is in flight", async () => {
  let finish;
  const page = await openPage({ replies: { delete_all: () => new Promise((resolve) => (finish = () => resolve(fixture("delete_all-deleted")))) } });
  await page.click("delete");
  await page.clock.advance(600);
  const pressing = page.press("confirm-delete");
  await page.settle();
  assert.equal(page.byId("delete").disabled, true);
  assert.equal(page.text("delete"), "Deleting");
  const again = page.click("delete");
  await page.settle();
  assert.equal(page.calls("delete_all").length, 1);
  finish();
  await pressing;
  await again;
  assert.equal(page.text("delete"), "Delete everything");
});

test("ruling AP-4 (3): a confirming press within 2 px of the arming point, outside the kept box, sends none, and one at 3 px sends one", async () => {
  const BOX = { left: 31, top: 600, width: 140, height: 34 };
  const EDGE = { clientX: 171, clientY: 634 };
  for (const [dx, sends] of [
    [2, 0],
    [3, 1],
  ]) {
    const page = await openPage({ replies: { delete_all: () => fixture("delete_all-deleted") } });
    page.byId("delete").rect = BOX;
    await page.press("delete", EDGE);
    await page.clock.advance(600);
    const at = { clientX: EDGE.clientX + dx, clientY: EDGE.clientY };
    await page.pressDown("confirm-delete", at);
    await page.click("confirm-delete", { detail: 1, ...at });
    assert.equal(page.calls("delete_all").length, sends, `${dx} px to the right of the arming point`);
  }
  for (const [dy, sends] of [
    [2, 0],
    [3, 1],
  ]) {
    const page = await openPage({ replies: { delete_all: () => fixture("delete_all-deleted") } });
    page.byId("delete").rect = BOX;
    await page.press("delete", EDGE);
    await page.clock.advance(600);
    const at = { clientX: EDGE.clientX, clientY: EDGE.clientY + dy };
    await page.pressDown("confirm-delete", at);
    await page.click("confirm-delete", { detail: 1, ...at });
    assert.equal(page.calls("delete_all").length, sends, `${dy} px below the arming point`);
  }
});
