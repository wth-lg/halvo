"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { Clock } = require("../lib/page.js");

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
