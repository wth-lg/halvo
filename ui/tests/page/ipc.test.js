"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage } = require("../lib/page.js");

test("Halvo.ipc.call sets no deadline of its own: a reply 30 s after the call reaches the caller", async () => {
  let answer;
  const page = await openPage({
    replies: {
      slow_command: () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    },
  });
  assert.equal(page.text("error"), "");
  let got = "pending";
  page.window.Halvo.ipc.call("slow_command", {}).then(
    (reply) => {
      got = reply;
    },
    (error) => {
      got = `rejected: ${error}`;
    },
  );
  await page.clock.advance(30000);
  assert.equal(got, "pending");
  answer("late reply");
  await page.settle();
  assert.equal(got, "late reply");
  assert.ok(Array.from(page.window.Halvo.ipc.replied()).includes("slow_command"));
  assert.equal(page.text("error"), "");
});
