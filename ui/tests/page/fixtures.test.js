"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, calls, keptDelete, loadedAgain } = require("../lib/page.js");
const { makeEvent } = require("../lib/dom.js");

const rowsOf = (page) =>
  page.byId("inventory").querySelector("tbody").children.map((tr) => tr.children.map((td) => td.textContent));

test("the running status fixture renders the heading, the toggle, the totals and the window", async () => {
  const page = await openPage({ replies: { capture_status: () => fixture("capture_status-running") } });
  assert.equal(page.text("state"), "Halvo is capturing");
  assert.equal(page.byId("dot").classList.contains("on"), true);
  assert.equal(page.text("toggle"), "Pause");
  assert.equal(page.byId("toggle").disabled, false);
  assert.equal(page.text("total"), "1.0 MB");
  assert.equal(page.text("logical"), "512 KB");
  assert.equal(page.text("cap"), "25 GB");
  assert.match(page.text("window-text"), /the last 14 days/);
  assert.doesNotMatch(page.document.body.textContent, /report/);
  assert.equal(page.document.body.textContent.includes("store_backend"), false);
});

test("the paused status fixture renders paused and Resume", async () => {
  const page = await openPage({ replies: { capture_status: () => fixture("capture_status-paused") } });
  assert.equal(page.text("state"), "Halvo is paused");
  assert.equal(page.byId("dot").classList.contains("on"), false);
  assert.equal(page.text("toggle"), "Resume");
});

test("two rows on one day render in the order of the reply", async () => {
  const page = await openPage({ replies: { inventory: () => fixture("inventory-two-rows-one-day") } });
  assert.equal(page.visible("empty"), false);
  assert.equal(page.visible("inventory"), true);
  assert.deepEqual(rowsOf(page), [
    ["2026-11-08", "Screen activity", "3", "300 B"],
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

test("every permission granted shows every row as On and no steps, and the view asks its rows only while it shows", async () => {
  const page = await openPage({ replies: { onboarding_state: () => fixture("onboarding_state-granted") } });
  const badges = page.byId("permissions").querySelectorAll("span.badge").map((b) => b.textContent);
  assert.deepEqual(badges, ["On", "On"]);
  assert.equal(page.visible("walkback"), false);
  const reads = () => page.calls("onboarding_state").length;
  const atLoad = reads();
  await page.clock.advance(3000);
  assert.equal(reads(), atLoad + 1, "the shown view asks its rows every 3 s");
  await page.hide();
  const hidden = reads();
  await page.clock.advance(9000);
  assert.equal(reads(), hidden, "a view hidden with its window asks nothing");
  page.document.visibilityState = "visible";
  page.document.dispatchEvent(makeEvent("visibilitychange"));
  await page.settle();
  assert.equal(reads(), hidden + 1, "a show asks at once");
  await page.clock.advance(3000);
  assert.equal(reads(), hidden + 2, "and then every 3 s");
  page.window.Halvo.views.window("hidden");
  await page.clock.advance(9000);
  assert.equal(reads(), hidden + 2, "the close handler's hide stops it too");
  for (const visibility of ["hidden", "visible"]) {
    const never = await openPage({ visibility, replies: { onboarding_state: () => fixture("onboarding_state-granted") } });
    never.window.Halvo.views.window("hidden");
    await never.settle();
    assert.equal(never.calls("onboarding_state").length, 1, `the mount's one read, the document reading ${visibility}`);
    await never.clock.advance(9000);
    assert.equal(never.calls("onboarding_state").length, 1, `a page never shown asks nothing after it, the document reading ${visibility}`);
  }
});

test("a lost permission shows the banner that names its pane", async () => {
  const lost = fixture("onboarding_state-lost");
  const putAway = { ...lost, lost_grants: [], permissions: lost.permissions.map((p) => ({ ...p, lost: false })) };
  let answer = lost;
  const page = await openPage({
    replies: {
      onboarding_state: () => answer,
      acknowledge_lost_permission: () => {
        answer = putAway;
        return putAway;
      },
    },
  });
  assert.equal(page.visible("lost-banner"), true);
  assert.match(page.text("lost-banner"), /Accessibility/);
  const badges = () => page.byId("permissions").querySelectorAll("span.badge").map((b) => b.textContent);
  assert.deepEqual(badges(), ["On", "Lost", "On"]);
  const controls = page.byId("permissions").querySelectorAll("button");
  assert.deepEqual(controls.map((b) => b.textContent), ["Leave it off"], "one control puts the lost row away");
  await page.click(controls[0]);
  assert.deepEqual(page.calls("acknowledge_lost_permission").map((c) => c.args), [{ permission: "accessibility" }]);
  assert.ok(argsOf("acknowledge_lost_permission").every((keys) => keys.join() === "permission"));
  assert.deepEqual(badges(), ["On", "Not asked yet", "On"]);
  assert.equal(page.visible("lost-banner"), false);
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
  assert.equal(page.text("state"), "Halvo is paused");
  assert.equal(page.text("error"), "");
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

test("a cap that the app refuses shows the page's words for the error of its fixture, in GB", async () => {
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
  assert.equal(page.text("error"), "The cap must be from 1 GB to 1024 GB.");
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

test("the walk asks by the asked record and again while a row reads not_determined", async () => {
  const accessibility = fixture("onboarding_state-missing").permissions.find((p) => p.permission === "accessibility");
  const step = (action, text) => ({ action, permission: "accessibility", settings_url: accessibility.settings_url, text });
  const ask = step("request_from_system", "Click Ask now, then click Open System Settings in the message from macOS, and turn on Halvo under Accessibility.");
  const settings = step(
    "open_settings_pane",
    "Open System Settings > Privacy & Security > Accessibility and turn on Halvo. If Halvo is already listed and still not working, remove it with the minus button and add it again.",
  );
  const rows = (asked, grant, walkback) => ({ permissions: [{ ...accessibility, asked, grant, lost: false }], lost_grants: [], all_granted: false, walkback });
  const controls = (page) => page.byId("permissions-section").querySelectorAll("button").map((b) => b.textContent);
  for (const [pair, answer, badge, offered] of [
    ["no record, not determined", rows(false, "not_determined", [ask, settings]), "Not asked yet", ["Ask now", "Open System Settings"]],
    ["no record, not granted", rows(false, "not_granted", [ask, settings]), "Not asked yet", ["Ask now", "Open System Settings"]],
    ["a record, not determined", rows(true, "not_determined", [ask]), "Off", ["Ask now"]],
    ["a record, not granted", rows(true, "not_granted", [settings]), "Off", ["Open System Settings"]],
  ]) {
    const page = await openPage({ replies: { onboarding_state: () => answer } });
    assert.equal(page.byId("permissions").querySelector("span.badge").textContent, badge, pair);
    assert.deepEqual(controls(page), offered, pair);
  }

  let answer = rows(true, "not_determined", [ask]);
  let reply = () => "requested";
  const page = await openPage({ replies: { onboarding_state: () => answer, request_permission: (args) => reply(args) } });
  const w = page.window.Halvo.words;
  const press = () => page.click(page.byId("walkback-steps").querySelector("button"));
  const reads = page.calls("onboarding_state").length;
  await press();
  assert.deepEqual(page.calls("request_permission").map((c) => c.args), [{ permission: "accessibility" }], "one press, one request");
  assert.equal(page.text("ask-outcome"), String(w.ASK_REQUESTED).replace("{pane}", "Accessibility"));
  assert.ok(page.calls("onboarding_state").length > reads, "the page reads the choice from the next rows");
  reply = () => "already_granted";
  await press();
  assert.equal(page.calls("request_permission").length, 2);
  assert.equal(page.text("ask-outcome"), String(w.ASK_ALREADY_ON).replace("{pane}", "Accessibility"));
  for (const [kind, words] of [
    ["busy", w.ERROR_BUSY],
    ["unsupported_op", w.ASK_UNSUPPORTED],
    ["state_file", w.ERROR_STATE_FILE],
    ["internal", w.ERROR_INTERNAL],
  ]) {
    reply = () => {
      throw { error: kind, message: "the message field of the service's reply" };
    };
    await press();
    assert.equal(typeof words, "string", kind);
    assert.equal(page.text("ask-outcome"), words, kind);
    assert.doesNotMatch(page.document.body.textContent, /message field/, kind);
  }
  answer = rows(true, "not_granted", [settings]);
  await page.clock.advance(3000);
  assert.deepEqual(controls(page), ["Ask now", "Open System Settings"], "the Ask stays after an error reply");
  reply = () => "requested";
  await press();
  assert.deepEqual(controls(page), ["Open System Settings"], "an answered ask leaves the walk to the rows");
  assert.equal(page.calls("request_permission").length, 7);
});

test("calls.json names a fixture that exists for every reply and error", () => {
  for (const call of calls()) {
    for (const name of [call.reply, call.error].filter(Boolean)) {
      assert.doesNotThrow(() => fixture(name), `${name}.json`);
    }
  }
});

test("Delete everything, once confirmed, sends the id of the status under the key of calls.json and says that nothing is left", async () => {
  const page = await openPage({ replies: keptDelete({ delete_all: () => fixture("delete_all-deleted") }), timezoneOffset: 0 });
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  assert.deepEqual(page.calls("delete_all").map((c) => c.args), [{ storeId: fixture("capture_status-running").store.id }]);
  assert.ok(argsOf("delete_all").every((keys) => keys.join() === "storeId"));
  assert.equal((await loadedAgain(page)).text("delete-result"), "At 09:30: Deleted. Nothing that Halvo recorded in its data folder can be read again, and nothing of it is left there. Copies that Halvo made next to your own files stay. A copy or a backup of Halvo's whole data folder that was made before can still be opened, and Halvo cannot reach it. Remove those yourself if you want them gone.");
});
