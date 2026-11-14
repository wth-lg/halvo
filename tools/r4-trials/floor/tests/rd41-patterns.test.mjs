import assert from 'node:assert/strict';
import fs from 'node:fs';
import { denied, LIST, parseList } from '../list.mjs';

const text = fs.readFileSync(LIST, 'utf8');
const features = parseList(text).features;
const deny = features.filter((f) => f.kind === 'deny');

/** The cases of ruling RD-41 D9 and D10 on the deny patterns. */
export const cases = [
  ['d10_no_deny_pattern_runs_unbounded_between_braces', () => {
    for (const f of deny) assert.ok(!/\[\^\{\}\][*+]/.test(f.cell), f.name);
  }],
  ['d10_the_review_text_of_64007_characters_ends_under_one_second', () => {
    const slow = `{ /* ${'any '.repeat(16000)}*/`;
    assert.equal(slow.length, 64007);
    const started = process.hrtime.bigint();
    const found = denied(features, slow);
    const ms = Number(process.hrtime.bigint() - started) / 1e6;
    assert.deepEqual(found, []);
    assert.ok(ms < 1000, `${ms} ms`);
  }],
  ['d10_a_destructuring_far_from_its_braces_is_not_read', () => {
    assert.deepEqual(denied(features, 'const { groupBy } = Object;'), ['object_group_by']);
    assert.deepEqual(denied(features, `const { ${'a, '.repeat(100)}groupBy } = Object;`), []);
  }],
  ['d9_the_list_names_its_limits_above_the_deny_block', () => {
    const top = text.slice(0, text.indexOf('\ndeny\t'));
    for (const words of ['another name', 'kept in a variable', 'put together from parts', 'comment between an object and its member', '\\u escape',
      '.command =', '.transfer(', '.toArray(', '.detached']) {
      assert.ok(top.includes(words), words);
    }
  }],
];
