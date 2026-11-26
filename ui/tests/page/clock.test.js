"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const { Clock, openPage, fixture, KEPT_AT_MS } = require("../lib/page.js");

/** What `code` gives in the page's own context, through JSON. */
const inPage = (page, code) => JSON.parse(vm.runInContext(`JSON.stringify(${code})`, page.window));

test("the fake clock runs due timers by time and then by creation, and fire runs each timer of a period once without moving now", async () => {
  const clock = new Clock();
  const order = [];
  clock.add(() => order.push("once-200"), 200, false);
  clock.add(() => order.push("once-100-first"), 100, false);
  clock.add(() => order.push("once-100-second"), 100, false);
  clock.add(() => order.push("every-150"), 150, true);
  clock.add(() => order.push("every-400"), 400, true);
  await clock.advance(300);
  assert.deepEqual(order, ["once-100-first", "once-100-second", "every-150", "once-200", "every-150"]);
  assert.equal(clock.now, 300);
  order.length = 0;
  await clock.fire(150);
  assert.deepEqual(order, ["every-150"]);
  assert.equal(clock.now, 300);
  order.length = 0;
  await clock.fire(400);
  assert.deepEqual(order, ["every-400"]);
  assert.equal(clock.now, 300);
  order.length = 0;
  await clock.advance(99);
  assert.deepEqual(order, []);
  await clock.advance(1);
  assert.deepEqual(order, ["every-400"]);
});

test("a date that each of the three views writes reads day number, month name in English and year, where the system's region orders them otherwise or names the month in its own language", async () => {
  const second = (ms) => new Date(ms).toISOString().replace(/\.\d+Z$/, "Z");
  const until = KEPT_AT_MS + 31 * 60000;
  const status = { ...fixture("capture_status-running"), left_out_before: [{ cause: "doubt", what: ["everything"], from: second(KEPT_AT_MS), until: second(until) }] };
  const host = "api.host.invalid";
  const limit = { host, state: "reached", cap: "20.00 USD", spent: "20.00 USD", held: "0.00 USD", period: "month", period_end_ms: Date.UTC(2026, 11, 12, 0, 30) };
  const form = { timeZone: "UTC", day: "numeric", month: "long", year: "numeric" };
  for (const region of ["en-US", "de-DE"]) {
    const theirs = new Date(KEPT_AT_MS).toLocaleDateString(region, form);
    assert.notEqual(theirs, "14 November 2026", `${region} writes the date another way: ${theirs}`);
    const deleted = await openPage({ replies: { last_delete: () => ({ state: "answered", at_ms: KEPT_AT_MS, answer: fixture("delete_all-deleted") }) }, nowMs: KEPT_AT_MS + 2 * 86400000, timezoneOffset: 0, region });
    const at = `new Date(${KEPT_AT_MS})`;
    const f = JSON.stringify(form);
    assert.deepEqual(
      inPage(deleted, `[${at}.toLocaleDateString(undefined, ${f}), ${at}.toLocaleString(undefined, ${f}), new Intl.DateTimeFormat(undefined, ${f}).format(${at}), ${at}.toLocaleDateString("en-GB", ${f})]`),
      [theirs, theirs, theirs, "14 November 2026"],
      `the page's system is set to ${region}, and a call that names a region keeps it`,
    );
    assert.ok(deleted.text("delete-result").startsWith("On 14 November 2026 at 09:30: "), `${region}: ${deleted.text("delete-result")}`);
    const limited = await openPage({ replies: { model_key_state: () => ({ host, state: "none" }), model_limit_state: () => limit }, timezoneOffset: 60, region });
    assert.equal(limited.text("model-limit-period"), "Halvo counts a month at a time, from the day this count began. It starts again on 11 December 2026.", region);
    assert.equal(
      limited.text("model-limit-reached"),
      `The limit is reached. Agents that use an AI model at ${host} wait until 11 December 2026 or until you raise the limit. Work that stays on this computer goes on.`,
      region,
    );
    const told = await openPage({ replies: { capture_status: () => status }, nowMs: until + 2 * 86400000, timezoneOffset: 0, region });
    assert.equal(
      told.text("left-out-before-line"),
      "From 14 November 2026 at 09:30 to 14 November 2026 at 10:01, Halvo recorded nothing, because it could not tell whether to leave out the app that you were using.",
      region,
    );
  }
});

test("the count of events of a row of the stored data groups its digits with a comma, where the system's region groups them otherwise", async () => {
  const inventory = { ...fixture("inventory-empty"), rows: [{ day: "2026-11-08", modality: "keystroke", events: 1234567, bytes: 10 }] };
  const page = await openPage({ replies: { inventory: () => inventory }, region: "de-DE" });
  assert.deepEqual(
    inPage(page, `[(1234567).toLocaleString(), new Intl.NumberFormat().format(1234567), (1234567).toLocaleString("en-GB")]`),
    ["1.234.567", "1.234.567", "1,234,567"],
    "the page's system is set to de-DE, and a call that names a region keeps it",
  );
  const cells = page.byId("inventory").querySelector("tbody").children[0].children;
  assert.equal(cells[2].textContent, "1,234,567");
});
