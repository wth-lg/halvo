"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { APP } = require("../lib/page.js");

const filesUnder = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? filesUnder(path.join(dir, e.name)) : [path.join(dir, e.name)]));

const lines = (pattern) =>
  filesUnder(APP).flatMap((file) =>
    fs
      .readFileSync(file, "utf8")
      .split("\n")
      .map((line, i) => ({ file: path.relative(APP, file), n: i + 1, line }))
      .filter(({ line }) => pattern.test(line)),
  );

test("no file under ui/app writes HTML through innerHTML, outerHTML or insertAdjacentHTML", () => {
  assert.deepEqual(lines(/innerHTML|outerHTML|insertAdjacentHTML|document\.write/), []);
});

test("no file under ui/app uses web storage, IndexedDB or cookies", () => {
  assert.deepEqual(lines(/localStorage|sessionStorage|indexedDB|document\.cookie/), []);
});

test("no file under ui/app runs text as code", () => {
  assert.deepEqual(lines(/\beval\s*\(|new Function\s*\(|setTimeout\s*\(\s*["'`]|setInterval\s*\(\s*["'`]/), []);
});
