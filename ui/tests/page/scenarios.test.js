"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture } = require("../lib/page.js");

const GB = 1024 * 1024 * 1024;
const running = (capGb = 25) => ({ ...fixture("capture_status-running"), store_bytes: 3 * GB, store_cap_bytes: capGb * GB });
const paused = () => fixture("capture_status-paused");
const noOnboarding = { permissions: [], lost_grants: [], all_granted: true, walkback: [] };
const keystrokes = { ...fixture("inventory-empty"), rows: [{ day: "2026-11-08", modality: "keystroke", events: 1200, bytes: 2 * GB }] };
const FOUR = ["capturing", "paused", "Nothing stored yet.", "0 B"];

const rows = (page) => page.byId("inventory").querySelector("tbody").children.length;
const visibleText = (node) =>
  node.localName ? (node.classList.contains("hidden") || node.hidden ? "" : node.childNodes.map(visibleText).join(" ")) : node.textContent;
const shown = (page) => visibleText(page.document.body);
const claims = (page) => FOUR.filter((word) => shown(page).includes(word));

const everyPoll = async (page) => {
  for (const ms of [...new Set(page.clock.periods())]) await page.clock.fire(ms);
};

test("the page starts two polls of 3 s and one of 15 s", async () => {
  const page = await openPage({ replies: { capture_status: running, inventory: () => fixture("inventory-empty"), onboarding_state: () => noOnboarding } });
  assert.deepEqual(page.clock.periods(), [3000, 3000, 15000]);
});

test("scenario 1: Save with an empty field sends nothing and the message outlives a status poll", async () => {
  const page = await openPage({ replies: { capture_status: running, onboarding_state: () => noOnboarding } });
  page.byId("cap-gb").value = "";
  await page.click("save-cap");
  assert.deepEqual(page.calls("set_storage_cap"), []);
  assert.equal(page.text("cap-message"), "Enter the cap in GB.");
  await page.clock.fire(3000);
  assert.equal(page.text("cap-message"), "Enter the cap in GB.");
});

test("RR-app-ui-05: while a pause is in flight the toggle stays locked and reads Pausing across a poll, and a second click sends nothing", async () => {
  let finishPause;
  const page = await openPage({
    replies: {
      capture_status: running,
      onboarding_state: () => noOnboarding,
      set_capture_state: () => new Promise((resolve) => (finishPause = () => resolve({ ...paused(), reached: true }))),
    },
  });
  page.byId("toggle").click();
  await page.settle();
  assert.equal(page.byId("toggle").disabled, true);
  await page.clock.fire(3000);
  assert.equal(page.byId("toggle").disabled, true);
  assert.equal(page.text("toggle"), "Pausing");
  page.byId("toggle").click();
  await page.settle();
  assert.deepEqual(page.calls("set_capture_state").map((c) => c.args.capture), ["paused"]);
  finishPause();
  await page.settle();
  assert.equal(page.text("state"), "Halvo is paused");
  assert.equal(page.text("toggle"), "Resume");
  assert.equal(page.byId("toggle").disabled, false);
});

test("RR-app-ui-05 and A1-5: a status read sent before a resume was answered is dropped, so the page does not show paused after the resume", async () => {
  let answerPoll;
  let pollPending = false;
  const page = await openPage({
    replies: {
      capture_status: () => {
        if (!pollPending) return paused();
        return new Promise((resolve) => (answerPoll = () => resolve(paused())));
      },
      onboarding_state: () => noOnboarding,
      set_capture_state: () => ({ ...fixture("set_capture_state-running"), reached: true }),
    },
  });
  assert.equal(page.text("state"), "Halvo is paused");
  await page.clock.advance(1100);
  pollPending = true;
  const polling = page.clock.fire(3000);
  await page.settle();
  pollPending = false;
  await page.click("toggle");
  assert.equal(page.text("state"), "Halvo is capturing");
  answerPoll();
  await polling;
  assert.equal(page.text("state"), "Halvo is capturing");
  await page.clock.fire(3000);
  assert.equal(page.text("state"), "Halvo is paused", "a read sent after the answer is drawn");
});

test("ruling PL-8 (5)(e): a pause whose reply does not reach its end shows the reply and an action error", async () => {
  const page = await openPage({
    replies: {
      capture_status: running,
      onboarding_state: () => noOnboarding,
      set_capture_state: () => ({ ...fixture("capture_status-pausing"), reached: false }),
    },
  });
  await page.click("toggle");
  assert.equal(page.text("state"), "Capture is stopping");
  assert.equal(page.text("error"), "Capture was not paused. Try again, or quit Halvo to stop capture.");
  assert.doesNotMatch(shown(page), /Halvo is paused/);
});

test("a resume whose reply does not reach its end says that capture did not start", async () => {
  const page = await openPage({
    replies: {
      capture_status: paused,
      onboarding_state: () => noOnboarding,
      set_capture_state: () => ({ ...fixture("capture_status-awaiting-resume"), reached: false }),
    },
  });
  await page.click("toggle");
  assert.equal(page.text("error"), "Capture did not start. Try again.");
});

test("scenario 3: an unsaved cap and the status polls", async () => {
  const page = await openPage({
    replies: {
      capture_status: () => running(25),
      onboarding_state: () => noOnboarding,
      set_storage_cap: ({ capBytes }) => ({ ...running(capBytes / GB), evicting: false }),
    },
  });
  const field = page.byId("cap-gb");
  field.focus();
  field.value = "10";
  await page.clock.fire(3000);
  assert.equal(field.value, "10");
  field.blur();
  await page.clock.fire(3000);
  assert.equal(field.value, "25");
  await page.click("save-cap");
  assert.deepEqual(page.calls("set_storage_cap").map((c) => c.args.capBytes), [25 * GB]);
});

test("A2-7 and A1-2: a save of the cap that fails reads as not done, keeps the field and says why", async () => {
  const page = await openPage({
    replies: {
      capture_status: () => running(25),
      onboarding_state: () => noOnboarding,
      set_storage_cap: () => {
        throw { error: "busy" };
      },
    },
  });
  await page.type("cap-gb", "30");
  await page.click("save-cap");
  assert.equal(page.text("error"), "Halvo's capture service is busy with another change. Try again in a moment.");
  assert.equal(page.byId("cap-gb").value, "30");
  assert.equal(page.text("cap"), "25 GB");
  assert.equal(page.byId("save-cap").disabled, false);
});

test("the cap's reply with evicting says that Halvo removes the oldest capture", async () => {
  const page = await openPage({
    replies: { capture_status: () => running(25), set_storage_cap: () => ({ ...running(2), store_bytes: 2 * GB, evicting: true }) },
  });
  await page.type("cap-gb", "2");
  await page.click("save-cap");
  await page.click("confirm-cap");
  assert.equal(page.text("cap-evicting"), "Halvo is removing the oldest capture to fit the new cap.");
});

test("scenario 4: a failed inventory read keeps the rows and the next good read clears nothing of the action channel", async () => {
  let fail = true;
  const page = await openPage({
    replies: {
      capture_status: running,
      inventory: () => {
        if (fail) throw { error: "deadline", seconds: 2 };
        return keystrokes;
      },
      onboarding_state: () => noOnboarding,
    },
  });
  assert.equal(page.text("total"), "3.0 GB");
  fail = false;
  await page.clock.fire(15000);
  assert.equal(rows(page), 1);
  fail = true;
  await page.clock.fire(15000);
  assert.equal(rows(page), 1);
  assert.equal(page.text("error"), "Halvo's capture service did not answer within 2 s.");
});

test("X-app-ui-02: a failed delete outlives two good status polls, and Dismiss clears it", async () => {
  const page = await openPage({
    replies: {
      capture_status: running,
      onboarding_state: () => noOnboarding,
      delete_all: () => {
        throw { error: "busy" };
      },
    },
  });
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  const failure = "Halvo's capture service is busy with another change. Try again in a moment.";
  assert.equal(page.text("error"), failure);
  await page.clock.fire(3000);
  await page.clock.fire(3000);
  assert.equal(page.text("error"), failure);
  assert.equal(page.visible("dismiss-error"), true);
  await page.click("dismiss-error");
  assert.equal(page.text("error"), "");
  assert.equal(page.visible("dismiss-error"), false);
});

test("an error of a reply is shown in the page's own words and never as it came", async () => {
  const page = await openPage({
    replies: {
      capture_status: running,
      set_capture_state: () => {
        throw "capture service not running at /Users/<name>/Library/Application Support/Halvo/run/control.sock";
      },
    },
  });
  await page.click("toggle");
  assert.equal(page.text("error"), "The request failed.");
  assert.doesNotMatch(shown(page), /<name>|control\.sock/);
});

test("scenario 5: an onboarding poll and the permission buttons", async () => {
  const onboarding = {
    permissions: [{ permission: "accessibility", pane_name: "Device Control and Data Access", settings_url: "", why: "", grant: "not_granted", lost: false, asked: false }],
    lost_grants: [],
    all_granted: false,
    walkback: [
      { permission: "accessibility", action: "request_from_system", settings_url: "", text: "step 1" },
      { permission: "accessibility", action: "open_settings_pane", settings_url: "", text: "step 2" },
    ],
  };
  const page = await openPage({ replies: { capture_status: running, onboarding_state: () => onboarding } });
  const open = () => page.byId("permissions").children[0].querySelector("button");
  const ask = () => page.byId("walkback-steps").children[0].querySelector("button");
  const before = open();
  const askBefore = ask();
  before.focus();
  await page.clock.fire(3000);
  assert.equal(open() === before, false);
  assert.equal(ask() === askBefore, false);
  assert.equal(page.byId("permissions").contains(before), false);
});

test("RR-app-ui-04: when every command fails from launch the page shows none of the four claims, through 20 rounds of polls", async () => {
  const down = () => {
    throw { error: "internal" };
  };
  const page = await openPage({ replies: { capture_status: down, inventory: down, onboarding_state: down } });
  for (let i = 0; i < 20; i++) {
    assert.deepEqual(claims(page), [], `round ${i}`);
    assert.equal(page.text("state"), "Connecting to Halvo's capture service");
    assert.equal(page.byId("toggle").disabled, true);
    assert.equal(page.byId("delete").disabled, true);
    await everyPoll(page);
  }
  assert.equal(page.text("connection"), "Halvo could not read the state of its capture service.");
});

test("RR-app-ui-04: with no daemon the page says that the capture service is not running and claims nothing else", async () => {
  const page = await openPage({
    replies: {
      capture_status: () => fixture("capture_status-not-running"),
      inventory: () => ({ ...fixture("inventory-empty"), available: false, store_id: null, as_of: null, totals: null }),
      onboarding_state: () => fixture("onboarding_state-none"),
    },
  });
  assert.equal(page.text("state"), "Capture service not running");
  assert.deepEqual(claims(page), []);
  assert.equal(page.byId("toggle").disabled, true);
  assert.equal(page.visible("inventory-unavailable"), true);
  assert.equal(page.visible("empty"), false);
  assert.equal(page.byId("delete").disabled, true);
  assert.equal(page.window.Halvo.service(), "not_running");
});

test("scenario 7: reads work once, then the service stops answering, and the page drops every claim of the first reads", async () => {
  let up = true;
  const page = await openPage({
    replies: {
      capture_status: () => (up ? running() : fixture("capture_status-held-unreachable")),
      inventory: () => (up ? keystrokes : { ...fixture("inventory-empty"), available: false }),
      onboarding_state: () => noOnboarding,
    },
  });
  assert.equal(page.text("state"), "Halvo is capturing");
  assert.equal(rows(page), 1);
  up = false;
  for (let i = 0; i < 3; i++) await everyPoll(page);
  assert.equal(page.text("state"), "Halvo's capture service is not answering. Capture may still be running. Halvo keeps trying.");
  assert.deepEqual(claims(page), []);
  assert.equal(rows(page), 0);
  assert.equal(page.byId("dot").classList.contains("on"), false);
  assert.doesNotMatch(shown(page), /Capture service not running/);
});
