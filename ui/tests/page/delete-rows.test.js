"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, fixture, keptDelete, loadedAgain } = require("../lib/page.js");

const W = {
  done: "Deleted. Nothing that Halvo recorded in its data folder can be read again, and nothing of it is left there.",
  copies:
    "Copies that Halvo made next to your own files stay. A copy or a backup of Halvo's whole data folder that was made before can still be opened, and Halvo cannot reach it. Remove those yourself if you want them gone.",
  keys: "Halvo destroyed its keys, so nothing that was recorded can be read again.",
  notFollowed:
    "Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there. They cannot be read, and you can remove them.",
  notFollowedKeyLeft:
    "Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there, and you can remove them.",
  rootLeft: "Halvo could not remove one key file. Remove it with this command:",
  rootUnnamed: "Halvo could not remove one key file and cannot name it. What was recorded may still be readable on this computer.",
  rootUnknown: "Halvo cannot tell whether a key file is left.",
  files: (n) => `Some files could not be removed: ${n}. They cannot be read without the keys.`,
  filesCount: (n) => `Some files could not be removed: ${n}.`,
  database: "A database file could not be removed. It cannot be read without the keys.",
  databaseAlone: "A database file could not be removed.",
  entries: (n) => `Halvo could not remove ${n} of its own folders or files in its data folder. Something that Halvo recorded may still be in them.`,
  unknown: (n) => `Halvo's data folder holds ${n} things that Halvo does not know. Halvo did not open or remove them.`,
};
const COMMAND = "rm '/Users/<name>/Library/Application Support/Halvo/roots/00000000000000000000000000000009.root'";
const AT = "At 09:30: ";

function rows() {
  const out = [];
  for (const R of ["yes", "no", "unknown"])
    for (const NF of [0, 2])
      for (const F of [0, 3])
        for (const D of [false, true])
          for (const E of [0, 1])
            for (const U of [0, 4])
              for (const C of [false, true]) out.push({ R, NF, F, D, E, U, C });
  return out;
}

const isClean = (r) => r.R === "yes" && r.NF === 0 && r.F === 0 && !r.D && r.E === 0 && r.U === 0;

function expected(r) {
  if (isClean(r)) return { text: [W.done, W.copies].join(" "), cleanup: null };
  const keys = r.R === "yes";
  const lines = [];
  if (r.NF > 0) {
    if (keys) lines.push(W.keys, W.copies, W.notFollowed);
    else lines.push(W.notFollowedKeyLeft);
  }
  if (r.R === "no") lines.push(r.C ? W.rootLeft : W.rootUnnamed);
  if (r.R === "unknown") lines.push(W.rootUnknown);
  if (r.F > 0) lines.push(keys ? W.files(r.F) : W.filesCount(r.F));
  if (r.D) lines.push(keys ? W.database : W.databaseAlone);
  if (r.E > 0) lines.push(W.entries(r.E));
  if (r.U > 0) lines.push(W.unknown(r.U));
  return { text: lines.join(" "), cleanup: r.R === "no" && r.C ? COMMAND : null };
}

function reply(r, forged) {
  return {
    ...fixture("delete_all-deleted"),
    root_destroyed: r.R,
    not_followed: r.NF,
    files_left: r.F,
    database_left: r.D,
    entries_left: r.E,
    unknown_entries: r.U,
    cleanup_command: r.C ? COMMAND : null,
    nothing_left: forged ? true : isClean(r),
  };
}

async function shown(rep) {
  const page = await openPage({ replies: keptDelete({ delete_all: () => rep }), timezoneOffset: 0 });
  await page.click("delete");
  await page.clock.advance(600);
  await page.press("confirm-delete");
  const next = await loadedAgain(page);
  const text = next.text("delete-result");
  return { text: text.startsWith(AT) ? text.slice(AT.length) : `NO TIME PREFIX: ${text}`, cleanup: next.visible("delete-cleanup") ? next.text("delete-cleanup") : null };
}

test("G1 (the review's case page-01): the not-followed text beside a root that was not destroyed never says that anything cannot be read (I-114 (16))", async () => {
  for (const [root, command] of [
    ["no", true],
    ["no", false],
    ["unknown", false],
  ]) {
    const r = { R: root, NF: 1, F: 0, D: false, E: 0, U: 0, C: command };
    const s = await shown(reply(r, false));
    assert.doesNotMatch(s.text, /cannot be read/, `${root} command=${command}: ${s.text}`);
    assert.equal(s.text, expected(r).text, `${root} command=${command}`);
  }
});

for (const forged of [false, true]) {
  test(`G1 (the verifier's 192 rows, nothing_left ${forged ? "forged to true" : "by the six fields"}): no sentence with the words "cannot be read" beside a root that is not destroyed, and each row shows exactly its lines (I-114 (16) and (17))`, async () => {
    const bad = [];
    let n = 0;
    for (const r of rows()) {
      n += 1;
      const s = await shown(reply(r, forged));
      const want = expected(r);
      if (r.R !== "yes" && /cannot be read/.test(s.text)) bad.push({ r, rule: "cannot be read beside a root that is not destroyed", text: s.text });
      else if (s.text !== want.text || s.cleanup !== want.cleanup) bad.push({ r, want, got: s });
    }
    assert.equal(n, 192);
    assert.deepEqual(bad.slice(0, 3), [], `${bad.length} of 192 rows differ`);
  });
}

test("G4 (N2, the review's case page-02): a deleted reply with each of its six fields clean and nothing_left false never says that nothing is left", async () => {
  const s = await shown({ ...fixture("delete_all-deleted"), nothing_left: false });
  assert.doesNotMatch(s.text, /nothing (of it )?is left/);
  assert.equal(s.text, "Halvo could not show the result of your delete. Look at what this page shows as stored.");
});
