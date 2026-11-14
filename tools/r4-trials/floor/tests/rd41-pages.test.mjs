import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseList } from '../list.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const scripts = (file) => [...fs.readFileSync(path.join(HERE, file), 'utf8').matchAll(/<script src="([^"]+)"><\/script>/g)].map((m) => m[1]);

/** The cases of the two pages that the trial app loads, and of the source date (the verifier's v48 and v03). */
export const cases = [
  ['the_probe_page_loads_call_js_before_probe_js', () => {
    assert.deepEqual(scripts('probe.html'), ['/__app/call.js', 'probe.js']);
  }],
  ['the_index_page_loads_call_js_alone', () => {
    assert.deepEqual(scripts('index.html'), ['/__app/call.js']);
  }],
  ['a_source_date_must_be_a_day_of_the_calendar', () => {
    const head = '# https://developer.apple.com/documentation/safari-release-notes/safari-17-release-notes\n';
    const list = (date) => `${head}# https://developer.mozilla.org/docs/Web/API/X, Safari 9 of ${date}\nallow\tx_line\t9\ttypeof X === 'function'\n`;
    assert.deepEqual(parseList(list('2015-09-30')).problems, []);
    for (const date of ['2015-9-30', '2015-09-31', 'never', '2015-09-30x']) {
      assert.ok(parseList(list(date)).problems.join('\n').includes('the line above names no page and date of its version'), date);
    }
  }],
];
