"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

const GB = 1024 * 1024 * 1024;
const status = (state, capGb = 25) => ({
  state,
  store_bytes: 3 * GB,
  store_cap_bytes: capGb * GB,
  window: { rolling_days: 14, first_report_day: 7 },
  store_backend: "",
});
const noOnboarding = { permissions: [], lost_grants: [], all_granted: true, walkback: [] };
const keystrokes = { rows: [{ day: "2026-11-08", modality: "keystroke", events: 1200, bytes: 2 * GB }] };

const rows = (page) => page.byId("inventory").querySelector("tbody").children.length;
const describe = (page) =>
  `heading="Halvo is ${page.text("state")}", dot on=${page.byId("dot").classList.contains("on")}, ` +
  `toggle="${page.text("toggle")}" disabled=${page.byId("toggle").disabled}, ` +
  `total="${page.text("total")} of a ${page.text("cap")} cap", "Nothing stored yet." visible=${page.visible("empty")}, ` +
  `rows=${rows(page)}, permissions section visible=${page.visible("permissions-section")}, error="${page.text("error")}"`;

const everyPoll = async (page) => {
  for (const ms of [...new Set(page.clock.periods())]) await page.clock.fire(ms);
};

test("the page starts two polls of 3 s and one of 15 s", async () => {
  const page = await openPage({ replies: { capture_status: () => status("running"), inventory: () => ({ rows: [] }), onboarding_state: () => noOnboarding } });
  assert.deepEqual(page.clock.periods(), [3000, 3000, 15000]);
});

test("scenario 1: Save with an empty field sends nothing and the message outlives a status poll", async () => {
  const page = await openPage({
    replies: {
      capture_status: () => status("running"),
      inventory: () => ({ rows: [] }),
      onboarding_state: () => noOnboarding,
      set_storage_cap: ({ capBytes }) => {
        throw `the consent action failed: the storage cap must be at least 1073741824 bytes (one day of capture); ${capBytes} was given`;
      },
    },
  });
  page.byId("cap-gb").value = "";
  await page.click("save-cap");
  assert.deepEqual(page.calls("set_storage_cap"), []);
  assert.equal(page.text("cap-message"), "Enter the cap in GB.");
  assert.equal(page.text("error"), "");
  await page.clock.fire(3000);
  assert.equal(page.text("cap-message"), "Enter the cap in GB.");
});

test("scenario 2: a status poll while a pause request is in flight", async () => {
  let finishPause;
  const page = await openPage({
    replies: {
      capture_status: () => status("running"),
      inventory: () => ({ rows: [] }),
      onboarding_state: () => noOnboarding,
      set_capture_state: () => new Promise((resolve) => (finishPause = () => resolve(status("paused")))),
    },
  });
  page.byId("toggle").click();
  await page.settle();
  assert.equal(page.byId("toggle").disabled, true);
  await page.clock.fire(3000);
  assert.equal(page.byId("toggle").disabled, false);
  assert.equal(page.text("toggle"), "Pause");
  assert.equal(page.text("state"), "capturing");
  page.byId("toggle").click();
  await page.settle();
  assert.deepEqual(page.calls("set_capture_state").map((c) => c.args.capture), ["paused", "paused"]);
  finishPause();
  await page.settle();
});

test("scenario 3: an unsaved cap and the status polls", async () => {
  const page = await openPage({
    replies: {
      capture_status: () => status("running", 25),
      inventory: () => ({ rows: [] }),
      onboarding_state: () => noOnboarding,
      set_storage_cap: ({ capBytes }) => status("running", capBytes / GB),
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

test("scenario 4: a failed inventory read", async () => {
  let fail = true;
  const page = await openPage({
    replies: {
      capture_status: () => status("running"),
      inventory: () => {
        if (fail) throw "the consent action failed: the store is sealed";
        return keystrokes;
      },
      onboarding_state: () => noOnboarding,
    },
  });
  assert.equal(page.text("total"), "3.0 GB");
  assert.equal(page.visible("empty"), true);
  assert.equal(page.visible("inventory"), false);
  fail = false;
  await page.clock.fire(15000);
  assert.equal(rows(page), 1);
  fail = true;
  await page.clock.fire(15000);
  assert.equal(rows(page), 1);
  assert.equal(page.text("error"), "the consent action failed: the store is sealed");
  await page.clock.fire(3000);
  assert.equal(rows(page), 1);
  assert.equal(page.text("error"), "");
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
  const page = await openPage({ replies: { capture_status: () => status("running"), inventory: () => ({ rows: [] }), onboarding_state: () => onboarding } });
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

test("scenario 6: every read fails from launch", async () => {
  const down = () => {
    throw "capture service not running";
  };
  const page = await openPage({ replies: { capture_status: down, inventory: down, onboarding_state: down } });
  const expected =
    'heading="Halvo is starting", dot on=false, toggle="Pause" disabled=true, total="0 B of a — cap", ' +
    '"Nothing stored yet." visible=true, rows=0, permissions section visible=false, error="capture service not running"';
  assert.equal(describe(page), expected);
  for (let i = 0; i < 20; i++) await everyPoll(page);
  assert.equal(describe(page), expected);
});

test("scenario 7: reads work once, then every read fails", async () => {
  let up = true;
  const or = (value) => () => {
    if (!up) throw "capture service not running";
    return value;
  };
  const page = await openPage({ replies: { capture_status: or(status("running")), inventory: or(keystrokes), onboarding_state: or(noOnboarding) } });
  assert.equal(
    describe(page),
    'heading="Halvo is capturing", dot on=true, toggle="Pause" disabled=false, total="3.0 GB of a 25 GB cap", ' +
      '"Nothing stored yet." visible=false, rows=1, permissions section visible=false, error=""',
  );
  up = false;
  for (let i = 0; i < 20; i++) await everyPoll(page);
  assert.equal(
    describe(page),
    'heading="Halvo is capturing", dot on=true, toggle="Pause" disabled=false, total="3.0 GB of a 25 GB cap", ' +
      '"Nothing stored yet." visible=false, rows=1, permissions section visible=false, error="capture service not running"',
  );
});
