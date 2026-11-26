"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { parse, makeEvent } = require("../lib/dom.js");

test("the stand-in document throws at a read of a member that it lacks, and answers undefined where a real document lacks the member too", () => {
  const { document } = parse("<ul><li>one</li></ul>");
  const list = document.querySelector("ul");
  assert.throws(() => list.firstChlid, /has no member firstChlid/);
  assert.throws(() => makeEvent("keydown").clientX, /has no member clientX/);
  assert.equal(list.firstChild.firstChild.localName, undefined);
});
