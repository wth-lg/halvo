"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete, loadedAgain, KEPT_AT_MS } = require("../lib/page.js");

async function confirmDelete(page) {
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
}

const DONE =
  "Deleted. Nothing that Halvo recorded in its data folder can be read again, and nothing of it is left there. Copies that Halvo made next to your own files stay. A copy or a backup of Halvo's whole data folder that was made before can still be opened, and Halvo cannot reach it. Remove those yourself if you want them gone.";
const visibleText = (node) => {
  if (node.classList && (node.classList.contains("hidden") || node.hidden)) return "";
  if (!node.localName) return node.textContent;
  return node.childNodes.map(visibleText).join(" ");
};

const UNREADABLE = "Halvo could not show the result of your delete. Look at what this page shows as stored.";

test("L4: a delete answered deleted or already_deleted loads the page again, and the result is drawn from last_delete", async () => {
  for (const name of ["delete_all-deleted", "delete_all-already-deleted"]) {
    const page = await openPage({ replies: keptDelete({ delete_all: () => fixture(name) }), timezoneOffset: 0 });
    await confirmDelete(page);
    assert.equal(page.loadsAgain(), 1, name);
    assert.equal(page.text("delete-result"), "", `${name}: the page that loads again draws the result, not this one`);
    const next = await loadedAgain(page);
    assert.equal(next.calls("last_delete").length, 1, name);
    assert.match(next.text("delete-result"), /^At 09:30: /, name);
  }
});

test("L4: delete_refused, an error and no answer do not load the page again", async () => {
  const cases = [
    ["delete_refused", () => fixture("delete_all-refused")],
    ["busy", () => Promise.reject({ error: "busy" })],
    ["deadline", () => Promise.reject({ error: "deadline", seconds: 120 })],
  ];
  for (const [name, answer] of cases) {
    const page = await openPage({ replies: keptDelete({ delete_all: answer }) });
    await confirmDelete(page);
    assert.equal(page.loadsAgain(), 0, name);
  }
  const page = await openPage({ replies: keptDelete({ delete_all: () => new Promise(() => {}) }) });
  await page.click("delete");
  await page.clock.advance(600);
  page.press("confirm-delete");
  await page.settle();
  await page.clock.advance(30000);
  assert.equal(page.loadsAgain(), 0, "no answer");
});

test("I-114 (7): the time of the answer stands before the result, in the clock of this computer", async () => {
  for (const [offset, words] of [
    [0, "At 09:30: "],
    [-60, "At 10:30: "],
    [300, "At 04:30: "],
  ]) {
    const page = await openPage({ replies: keptDelete({ delete_all: () => fixture("delete_all-deleted") }), timezoneOffset: offset });
    await confirmDelete(page);
    const next = await loadedAgain(page);
    assert.equal(next.text("delete-result"), words + DONE, String(offset));
  }
});

test("L4: after the load the result has the keyboard focus", async () => {
  const page = await openPage({ replies: keptDelete({ delete_all: () => fixture("delete_all-deleted") }) });
  await confirmDelete(page);
  const next = await loadedAgain(page);
  assert.equal(next.document.activeElement, next.byId("delete-result"));
  assert.equal(next.byId("delete-result").getAttribute("tabindex"), "-1");
});

test("I-114 (8): a read of the kept answer that fails after a delete shows the sentence of point 8 and never Deleted", async () => {
  const replies = keptDelete({ delete_all: () => fixture("delete_all-deleted") });
  const page = await openPage({ replies });
  await confirmDelete(page);
  replies.last_delete = () => {
    throw { error: "internal" };
  };
  const next = await loadedAgain(page);
  assert.equal(next.text("delete-result"), UNREADABLE);
  assert.doesNotMatch(next.text("delete-result"), /Deleted/);
});

test("I-114 (8): a load that no delete started shows nothing when the read of the kept answer fails", async () => {
  const page = await openPage({
    replies: {
      last_delete: () => {
        throw { error: "internal" };
      },
    },
  });
  assert.equal(page.text("delete-result"), "");
});

test("L4: a kept answer of delete_refused draws nothing at a load", async () => {
  const page = await openPage({
    replies: { last_delete: () => ({ state: "answered", at_ms: KEPT_AT_MS, answer: fixture("delete_all-refused") }) },
  });
  assert.equal(page.text("delete-result"), "");
});

test("L4: a load while a delete is in flight keeps Delete everything locked and draws the answer once it is kept", async () => {
  let kept = { state: "in_flight" };
  const page = await openPage({ replies: { last_delete: () => kept }, timezoneOffset: 0 });
  assert.equal(page.byId("delete").disabled, true);
  kept = { state: "answered", at_ms: KEPT_AT_MS, answer: fixture("delete_all-deleted") };
  await page.clock.fire(3000);
  assert.equal(page.text("delete-result"), "At 09:30: " + DONE);
});

test("L4: during the load no size, no capturing, no paused and no Nothing stored yet stands before a status made after the answer", async () => {
  let status = fixture("capture_status-running");
  const replies = keptDelete({
    capture_status: () => status,
    inventory: () => fixture("inventory-empty"),
    delete_all: () => {
      status = fixture("capture_status-connecting");
      return fixture("delete_all-deleted");
    },
  });
  const page = await openPage({ replies });
  await confirmDelete(page);
  const next = await loadedAgain(page);
  const body = visibleText(next.document.body);
  assert.match(body, /Connecting to Halvo's capture service/);
  assert.doesNotMatch(body, /capturing/i);
  assert.doesNotMatch(body, /\bpaused\b/i);
  assert.doesNotMatch(body, /Nothing stored yet\./);
  for (const id of ["total", "logical", "cap"]) assert.equal(next.text(id), "—", id);
  status = fixture("capture_status-after-delete");
  await next.clock.fire(3000);
  assert.notEqual(next.text("total"), "—", "a status made after the answer draws its size");
});

test("G4 (N2, the review's case page-07): a load that a delete started and whose kept answer reads none shows the sentence of point 8", async () => {
  const page = await openPage({ replies: { ...keptDelete({ delete_all: () => fixture("delete_all-deleted") }), last_delete: () => ({ state: "none" }) } });
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  const next = await loadedAgain(page);
  assert.equal(next.text("delete-result"), "Halvo could not show the result of your delete. Look at what this page shows as stored.");
});

test("G5 (N4, the review's case page-04): the page that loads again after a delete offers no second delete during the load", async () => {
  const page = await openPage({ replies: keptDelete({ delete_all: () => fixture("delete_all-deleted") }) });
  await confirmDelete(page);
  assert.equal(page.loadsAgain(), 1);
  assert.equal(page.byId("delete").disabled, true, "Delete everything is not offered against the deleted store while the page loads again");
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  assert.equal(page.calls("delete_all").length, 1, "a second press during the load makes no second delete call");
});

const answered = (atMs) => ({ state: "answered", at_ms: atMs, answer: fixture("delete_all-deleted") });

test("G5 (N11): at a load that no delete started the kept answer stands in its place without the focus and without a scroll", async () => {
  const page = await openPage({ replies: { last_delete: () => answered(KEPT_AT_MS) }, timezoneOffset: 0 });
  assert.ok(page.text("delete-result").startsWith("At 09:30: "), page.text("delete-result"));
  assert.notEqual(page.document.activeElement, page.byId("delete-result"));
  assert.equal(page.byId("delete-result").getAttribute("tabindex"), null);
});

test("G5 (I-114 (19)): the kept answer of an earlier day carries its date before its time, in the clock of this computer", async () => {
  const page = await openPage({ replies: { last_delete: () => answered(KEPT_AT_MS) }, nowMs: KEPT_AT_MS + 2 * 86400000, timezoneOffset: 0 });
  const date = new Date(KEPT_AT_MS).toLocaleDateString("en-GB", { timeZone: "UTC", day: "numeric", month: "long", year: "numeric" });
  assert.ok(page.text("delete-result").startsWith(`On ${date} at 09:30: `), page.text("delete-result"));
  const today = await openPage({ replies: { last_delete: () => answered(KEPT_AT_MS) }, nowMs: KEPT_AT_MS + 3600000, timezoneOffset: 0 });
  assert.ok(today.text("delete-result").startsWith("At 09:30: "), today.text("delete-result"));
});

test("G10: a kept answer with no time prints no NaN and no time", async () => {
  const page = await openPage({ replies: { last_delete: () => ({ state: "answered", answer: fixture("delete_all-deleted") }) } });
  assert.doesNotMatch(page.text("delete-result"), /NaN/);
  assert.ok(page.text("delete-result").startsWith("Deleted."), page.text("delete-result"));
});
