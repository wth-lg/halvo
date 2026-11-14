import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { denied, LIST, main, parseCases, parseList } from '../list.mjs';
import { tempDir } from './temp.mjs';

const text = fs.readFileSync(LIST, 'utf8');
const lines = text.split('\n');
const firstOf = (kind) => lines.findIndex((l) => l.startsWith(`${kind}\t`));

function edit(index, change) {
  const copy = lines.slice();
  copy[index] = change(copy[index]);
  return copy.join('\n');
}

function allowAt(version) {
  const at = firstOf('allow');
  const copy = lines.slice();
  copy[at - 1] = copy[at - 1].replace(/Safari \S+ of/, `Safari ${version} of`);
  copy[at] = copy[at].replace(/^(allow\t[^\t]+\t)[^\t]+/, `$1${version}`);
  return copy.join('\n');
}

function expression(cell) {
  return edit(firstOf('allow'), (l) => l.replace(/\t[^\t]+$/, `\t${cell}`));
}

const problems = (listText) => parseList(listText).problems.join('\n');

function run(argv) {
  let out = '';
  let err = '';
  const code = main(argv, (s) => { out += s; }, (s) => { err += s; });
  return { code, out, err };
}

/** The cases of ruling RD-41 D3 and D7 on the list. */
export const cases = [
  ['d3_an_allow_line_at_17_0_1_is_refused', () => {
    assert.ok(problems(allowAt('17.0.1')).includes('above the floor 17.0'));
    assert.equal(problems(allowAt('17.0')), '');
  }],
  ['d3_a_view_that_cannot_be_read_is_a_miss', () => {
    const missing = path.join(tempDir('view'), 'gone.js');
    const r = run(['check', missing]);
    assert.equal(r.code, 1);
    assert.match(r.out, /^miss the view gone\.js cannot be read$/m);
  }],
  ['d3_line_1_naming_the_safari_18_notes_is_refused', () => {
    const changed = text.replace('safari-17-release-notes', 'safari-18-release-notes');
    assert.notEqual(changed, text);
    assert.ok(problems(changed).includes("line 1 names no page of the floor's Safari release notes"));
  }],
  ['d3_a_name_of_33_characters_is_refused', () => {
    const changed = edit(firstOf('deny'), (l) => l.replace(/^(deny\t)[^\t]+/, `$1${'a'.repeat(33)}`));
    assert.ok(problems(changed).includes("the name is not of I0.6's form"));
    const fits = edit(firstOf('deny'), (l) => l.replace(/^(deny\t)[^\t]+/, `$1${'a'.repeat(32)}`));
    assert.equal(problems(fits), '');
  }],
  ['d3_a_version_of_four_fields_is_refused', () => {
    assert.ok(problems(allowAt('16.1.0.1')).includes('the version is not a version'));
  }],
  ['d3_a_cell_with_a_trailing_space_is_refused', () => {
    const changed = edit(firstOf('allow'), (l) => `${l} `);
    assert.ok(problems(changed).includes('a cell starts or ends with a space'));
    const name = edit(firstOf('deny'), (l) => l.replace(/^deny\t/, 'deny\t '));
    assert.ok(problems(name).includes('a cell starts or ends with a space'));
  }],
  ['d3_a_line_of_five_cells_is_refused', () => {
    assert.ok(problems(edit(firstOf('deny'), (l) => `${l}\textra`)).includes('5 cells, not 4'));
  }],
  ['d3_a_source_comment_whose_date_is_no_date_is_refused', () => {
    const changed = edit(firstOf('deny') - 1, (l) => l.replace(/of \d{4}-\d{2}-\d{2}$/, 'of 2024-13-45'));
    assert.notEqual(changed, text);
    assert.ok(problems(changed).includes('the line above names no page and date of its version'));
  }],
  ['d3_a_use_line_of_more_than_three_cells_is_refused', () => {
    assert.deepEqual(parseCases('use\tobject_group_by\tObject.groupBy(a, f);\textra\n').problems,
      ['cases line 1 is neither "use, name, text" nor "free, text"']);
  }],
  ['d3_a_pattern_is_found_only_under_the_flag_u', () => {
    const f = [{ kind: 'deny', name: 'astral', cell: '\\u{61}\\u{1F600}', version: '18', line: 0 }];
    assert.deepEqual(denied(f, 'a\u{1F600}'), ['astral']);
    assert.deepEqual(denied(f, `${'u'.repeat(61)}`), []);
  }],
  ['d3_a_use_that_differs_in_the_case_of_a_letter_is_not_found', () => {
    const { features } = parseList(text);
    assert.deepEqual(denied(features, 'const g = Object.groupBy(rows, f);'), ['object_group_by']);
    assert.deepEqual(denied(features, 'const g = Object.groupby(rows, f); const h = object.GroupBy(rows, f);'), []);
  }],
  ['the_last_line_of_a_list_with_no_final_line_feed_is_read', () => {
    const cut = text.slice(0, -1);
    const bad = cut.replace(/\t[^\t\n]+$/, '');
    assert.ok(problems(cut).includes('the list does not end with a line feed'));
    assert.ok(problems(bad).includes('3 cells, not 4'), problems(bad));
  }],
  ['d7_an_allow_expression_true_is_refused', () => {
    assert.ok(problems(expression('true')).includes('the expression names nothing of the page'));
  }],
  ['d7_an_allow_expression_not_0_is_refused', () => {
    assert.ok(problems(expression('!0')).includes('the expression names nothing of the page'));
  }],
  ['d7_an_allow_expression_1_equals_1_is_refused', () => {
    assert.ok(problems(expression('1 === 1')).includes('the expression names nothing of the page'));
    assert.ok(problems(expression("typeof 'window' === 'string'")).includes('the expression names nothing of the page'));
  }],
  ['d7_every_allow_expression_of_the_head_names_the_page', () => {
    assert.ok(!problems(text).includes('names nothing of the page'));
  }],
];
