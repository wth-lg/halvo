"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, calls } = require("../lib/page.js");

const rowsOf = (page) =>
  page.byId("inventory").querySelector("tbody").children.map((tr) => tr.children.map((td) => td.textContent));

test("the running status fixture renders the heading, the toggle, the totals and the window", async () => {
  const page = await openPage({ replies: { capture_status: () => fixture("capture_status-running") } });
  assert.equal(page.text("state"), "capturing");
  assert.equal(page.byId("dot").classList.contains("on"), true);
  assert.equal(page.text("toggle"), "Pause");
  assert.equal(page.byId("toggle").disabled, false);
  assert.equal(page.text("total"), "370 B");
  assert.equal(page.text("cap"), "25 GB");
  assert.match(page.text("window-text"), /the last 14 days/);
  assert.match(page.text("window-text"), /after day 7\./);
  assert.equal(page.text("backend"), `(${fixture("capture_status-running").store_backend})`);
});

test("the paused status fixture renders paused and Resume", async () => {
  const page = await openPage({ replies: { capture_status: () => fixture("capture_status-paused") } });
  assert.equal(page.text("state"), "paused");
  assert.equal(page.byId("dot").classList.contains("on"), false);
  assert.equal(page.text("toggle"), "Resume");
});

test("two rows on one day render in the order of the reply", async () => {
  const page = await openPage({ replies: { inventory: () => fixture("inventory-two-rows-one-day") } });
  assert.equal(page.visible("empty"), false);
  assert.equal(page.visible("inventory"), true);
  assert.deepEqual(rowsOf(page), [
    ["2026-11-08", "Screen", "3", "300 B"],
    ["2026-11-08", "Typing", "7", "70 B"],
  ]);
});

test("an empty inventory renders the empty note", async () => {
  const page = await openPage({ replies: { inventory: () => fixture("inventory-empty") } });
  assert.equal(page.visible("empty"), true);
  assert.equal(page.visible("inventory"), false);
  assert.equal(page.text("error"), "");
});

for (const [name, reply] of [
  ["rows renamed", () => ({ row: fixture("inventory-two-rows-one-day").rows })],
  ["rows not an array", () => ({ rows: { day: "2026-11-08" } })],
  ["a bare array", () => fixture("inventory-two-rows-one-day").rows],
]) {
  test(`an inventory reply with ${name} is an error and not the empty note`, async () => {
    const page = await openPage({ replies: { inventory: reply } });
    assert.notEqual(page.text("error"), "");
    assert.equal(page.visible("empty"), false);
  });
}

test("a missing permission shows its row and the Ask now step", async () => {
  const page = await openPage({ replies: { onboarding_state: () => fixture("onboarding_state-missing") } });
  assert.equal(page.visible("permissions-section"), true);
  const badges = page.byId("permissions").querySelectorAll("span.badge").map((b) => b.textContent);
  assert.deepEqual(badges, ["On", "On", "Not asked yet"]);
  const steps = page.byId("walkback-steps").children;
  assert.equal(steps.length, 3);
  assert.equal(steps[0].querySelector("button").textContent, "Ask now");
  assert.equal(page.visible("lost-banner"), false);
  const missing = fixture("onboarding_state-missing");
  const reasons = page.byId("permissions").querySelectorAll("span.why").map((s) => s.textContent);
  assert.deepEqual(reasons, missing.permissions.map((p) => p.why));
  assert.deepEqual(
    steps.map((li) => li.childNodes[0].textContent),
    missing.walkback.map((s) => s.text + " "),
  );
  for (const shown of [...reasons, ...steps.map((li) => li.childNodes[0].textContent)]) {
    assert.match(shown, /^[A-Z][a-z]|^lets |^Screen /, `a reason or a step that the page draws: ${JSON.stringify(shown)}`);
    assert.doesNotMatch(shown, /undefined/);
  }
});

test("every permission granted shows every row as On and no steps", async () => {
  const page = await openPage({ replies: { onboarding_state: () => fixture("onboarding_state-granted") } });
  const badges = page.byId("permissions").querySelectorAll("span.badge").map((b) => b.textContent);
  assert.deepEqual(badges, ["On", "On", "On"]);
  assert.equal(page.visible("walkback"), false);
});

test("a lost permission shows the banner that names its pane", async () => {
  const page = await openPage({ replies: { onboarding_state: () => fixture("onboarding_state-lost") } });
  assert.equal(page.visible("lost-banner"), true);
  assert.match(page.text("lost-banner"), /Accessibility/);
  const badges = page.byId("permissions").querySelectorAll("span.badge").map((b) => b.textContent);
  assert.deepEqual(badges, ["On", "Lost", "On"]);
});

test("no required permission hides the permissions section", async () => {
  const page = await openPage({ replies: { onboarding_state: () => fixture("onboarding_state-none") } });
  assert.equal(page.visible("permissions-section"), false);
});

const argsOf = (command) => {
  const listed = calls().filter((c) => c.command === command);
  assert.ok(listed.length > 0, `calls.json lists ${command}`);
  return listed.map((c) => Object.keys(c.args).sort());
};

test("the toggle sends the argument object of calls.json and renders the reply", async () => {
  const page = await openPage({
    replies: { capture_status: () => fixture("capture_status-running"), set_capture_state: () => fixture("set_capture_state-paused") },
  });
  await page.click("toggle");
  const sent = page.calls("set_capture_state");
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].args, { capture: "paused" });
  assert.ok(argsOf("set_capture_state").every((keys) => keys.join() === Object.keys(sent[0].args).sort().join()));
  assert.equal(page.text("state"), "paused");
});

test("Save sends the cap in bytes under the key of calls.json and renders the reply", async () => {
  const page = await openPage({ replies: { set_storage_cap: () => fixture("set_storage_cap-10-gib") } });
  page.byId("cap-gb").value = "10";
  await page.click("save-cap");
  const sent = page.calls("set_storage_cap");
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].args, { capBytes: 10737418240 });
  assert.ok(argsOf("set_storage_cap").every((keys) => keys.join() === "capBytes"));
  assert.equal(page.text("cap"), "10 GB");
});

test("a cap that the app refuses shows the error text of its fixture", async () => {
  const page = await openPage({
    replies: {
      set_storage_cap: () => {
        throw fixture("set_storage_cap-zero-error");
      },
    },
  });
  page.byId("cap-gb").value = "5";
  await page.click("save-cap");
  assert.deepEqual(page.calls("set_storage_cap").map((c) => c.args), [{ capBytes: 5 * 1024 * 1024 * 1024 }]);
  assert.equal(page.text("error"), fixture("set_storage_cap-zero-error"));
});

test("the permission steps send the argument object of calls.json", async () => {
  const page = await openPage({
    replies: {
      onboarding_state: () => fixture("onboarding_state-missing"),
      request_permission: () => fixture("request_permission-requested"),
      open_permission_pane: () => fixture("open_permission_pane-opened"),
    },
  });
  const steps = page.byId("walkback-steps").children;
  await page.click(steps[0].querySelector("button"));
  await page.click(steps[1].querySelector("button"));
  assert.deepEqual(page.calls("request_permission").map((c) => c.args), [{ permission: "input_monitoring" }]);
  assert.deepEqual(page.calls("open_permission_pane").map((c) => c.args), [{ permission: "input_monitoring" }]);
  for (const command of ["request_permission", "open_permission_pane"]) {
    assert.ok(argsOf(command).every((keys) => keys.join() === "permission"));
  }
});

test("calls.json names a fixture that exists for every reply and error", () => {
  for (const call of calls()) {
    for (const name of [call.reply, call.error, call.reply_off_macos].filter(Boolean)) {
      assert.doesNotThrow(() => fixture(name), `${name}.json`);
    }
  }
});

test("Delete everything, once confirmed, sends delete_all and renders the report of its fixture", async () => {
  const page = await openPage({ replies: { delete_all: () => fixture("delete_all-deleted") } });
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  assert.equal(page.calls("delete_all").length, 1);
  assert.equal(page.text("delete-result"), "Deleted. 2 segments shredded, 2 keys destroyed.");
});
