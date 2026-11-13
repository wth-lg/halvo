"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { openPage, APP } = require("../lib/page.js");
const { parse } = require("../lib/dom.js");

const VIEWS = path.join(APP, "views");
const SIX = ["status.js", "keeps.js", "permissions.js", "stored.js", "cap.js", "delete.js"];
const NOT_LOADED = ["agents.js", "needs.js"];

const filesUnder = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? filesUnder(path.join(dir, e.name)) : [path.join(dir, e.name)]));

test("index.html holds markup only: no inline script, no style block and no style attribute", () => {
  const html = fs.readFileSync(path.join(APP, "index.html"), "utf8");
  const { document, scripts } = parse(html);
  assert.deepEqual(scripts.filter((s) => !s.getAttribute("src")).length, 0);
  assert.equal(document.querySelectorAll("style").length, 0);
  assert.deepEqual([...document.elements()].filter((el) => el.hasAttribute("style")).map((el) => el.localName), []);
  assert.doesNotMatch(html, /\son[a-z]+\s*=/i);
});

test("views/ holds the six views of SIX and the two of NOT_LOADED, and the page loads ipc.js, the frame and each of the six once, and nothing else", async () => {
  const page = await openPage();
  const views = fs.readdirSync(VIEWS).filter((f) => f.endsWith(".js")).sort();
  assert.deepEqual(views, [...SIX, ...NOT_LOADED].sort());
  assert.deepEqual(page.loaded.slice(0, 2), ["ipc.js", "frame.js"]);
  assert.deepEqual([...page.loaded.slice(2)].sort(), SIX.map((f) => `views/${f}`).sort());
});

test("the frame renders one section per view in ascending order, each under a level-2 heading", async () => {
  const page = await openPage();
  const sections = page.document.querySelectorAll("section");
  assert.deepEqual(
    sections.map((s) => s.id),
    ["status-section", "keeps-section", "permissions-section", "stored-section", "storage-cap-section", "delete-everything-section"],
  );
  for (const section of sections) {
    const heading = section.firstElementChild;
    assert.equal(heading.localName, "h2");
    assert.equal(section.getAttribute("aria-labelledby"), heading.id);
  }
  assert.deepEqual(
    sections.map((s) => s.firstElementChild.textContent),
    ["Capture", "What Halvo keeps", "Permissions", "Stored on this computer", "Storage cap", "Delete everything"],
  );
});

test("ipc.js is the only file under ui/app that names the Tauri object", () => {
  const naming = filesUnder(APP)
    .filter((f) => /__TAURI/.test(fs.readFileSync(f, "utf8")))
    .map((f) => path.relative(APP, f));
  assert.deepEqual(naming, ["ipc.js"]);
});

test("a seventh view file renders with one line added to the view list and no edit to the six views", async (t) => {
  const copy = fs.mkdtempSync(path.join(os.tmpdir(), "r1-app-views-"));
  t.after(() => fs.rmSync(copy, { recursive: true, force: true }));
  fs.cpSync(APP, copy, { recursive: true });
  fs.writeFileSync(
    path.join(copy, "views", "extra.js"),
    'Halvo.views.add({ id: "extra", order: 9, heading: "An extra view.", mount: function (el) { el.textContent = "extra body"; }, show: function () {}, hide: function () {} });\n',
  );
  const index = path.join(copy, "index.html");
  const lines = fs.readFileSync(index, "utf8").split("\n");
  const last = lines.findLastIndex((l) => /<script src="views\//.test(l));
  lines.splice(last + 1, 0, lines[last].replace(/views\/[a-z-]+\.js/, "views/extra.js"));
  fs.writeFileSync(index, lines.join("\n"));
  const page = await openPage({ dir: copy });
  const sections = page.document.querySelectorAll("section");
  assert.equal(sections.at(-1).id, "extra-section");
  assert.equal(sections.at(-1).firstElementChild.textContent, "An extra view.");
  assert.equal(page.byId("extra-section").textContent.endsWith("extra body"), true);
  assert.equal(sections.length, 7);
  for (const file of SIX) {
    assert.equal(fs.readFileSync(path.join(copy, "views", file), "utf8"), fs.readFileSync(path.join(VIEWS, file), "utf8"), file);
  }
  const original = fs.readFileSync(path.join(APP, "index.html"), "utf8").split("\n");
  const changed = fs.readFileSync(index, "utf8").split("\n");
  assert.equal(changed.length, original.length + 1);
  assert.deepEqual(changed.filter((l, i) => i !== last + 1), original);
});

test("the page runs with the global API off and shows the version from the core app version command", async () => {
  const page = await openPage({ replies: { "plugin:app|version": () => "9.8.7" } });
  assert.equal(page.window.__TAURI__, undefined);
  assert.equal(page.calls("plugin:app|version").length, 1);
  assert.equal(page.text("version"), "9.8.7");
  assert.equal(page.text("state"), "capturing");
});

test("Halvo.ipc.replied() names each command that has answered, and no command that failed", async () => {
  const page = await openPage({
    replies: {
      onboarding_state: () => {
        throw "not now";
      },
    },
  });
  assert.deepEqual(Array.from(page.window.Halvo.ipc.replied()).sort(), ["capture_status", "inventory", "plugin:app|version"]);
});
