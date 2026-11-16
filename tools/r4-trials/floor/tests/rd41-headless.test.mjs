import assert from 'node:assert/strict';
import fs from 'node:fs';
import { standIn } from '../headless.mjs';
import { LIST, parseList } from '../list.mjs';

/** The cases of ruling RD-41 D6: the script that takes an interface away, and the two dialog lines. */
export const cases = [
  ['d6_the_stand_in_deletes_the_named_global_after_call_js', () => {
    const plain = standIn(null);
    assert.ok(!plain.includes('delete '));
    assert.equal(standIn('HTMLDialogElement'), `${plain}\ndelete window.HTMLDialogElement;\n`);
  }],
  ['d6_the_dialog_event_lines_test_the_dialog_interface', () => {
    const features = parseList(fs.readFileSync(LIST, 'utf8')).features;
    for (const name of ['dialog_cancel_event', 'dialog_close_event']) {
      const line = features.find((f) => f.name === name);
      assert.match(line.cell, /\bin HTMLDialogElement\.prototype$/, name);
      assert.ok(!/createElement/.test(line.cell), name);
    }
  }],
];
