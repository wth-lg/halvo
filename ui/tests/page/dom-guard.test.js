"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { parse, makeEvent } = require("../lib/dom.js");

test("the stand-in document throws at a read of a member that it lacks, and answers undefined where a real document lacks the member too", () => {
  const { document } = parse("<ul><li>one</li></ul>");
  const list = document.querySelector("ul");
  assert.throws(() => list.firstChlid, /has no member firstChlid/);
  assert.throws(() => makeEvent("keydown").clientX, /has no member clientX/);
  assert.throws(() => list.children.lenght, /has no member lenght/);
  assert.throws(() => list.style.colour, /has no member colour/);
  assert.throws(() => list.classList.contians, /has no member contians/);
  assert.equal(list.firstChild.firstChild.localName, undefined);
});
