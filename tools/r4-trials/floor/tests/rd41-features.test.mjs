import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { lines, lookup, verify } from '../features.mjs';
import { tempDir } from './temp.mjs';

const FEATURES = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'features.mjs');
const compat = (safari, url = true) => ({ __compat: { ...(url ? { mdn_url: 'https://developer.mozilla.org/docs/Web/API/Fx' } : {}), support: { safari } } });

/** A small data fixture in the form of the compatibility data, with invented entries. */
function fixture() {
  return {
    __meta: { version: '0.0.1', timestamp: '2026-01-02T03:04:05.000Z' },
    browsers: { safari: { releases: {
      9: { release_date: '2015-09-30' }, 10: { release_date: '2016-09-20' }, 17: { release_date: '2023-09-18' },
      17.4: { release_date: '2024-03-05' }, 18: { release_date: '2024-09-16' },
    } } },
    api: { Fx: {
      plain: compat({ version_added: '10' }),
      numbers: compat([{ version_added: '10' }, { version_added: '9' }]),
      partial: compat({ version_added: '18', partial_implementation: true }),
      flagged: compat([{ version_added: '17.4', flags: [{ type: 'preference', name: 'x' }] }, { version_added: '18' }]),
      prefixed: compat([{ version_added: '18' }, { version_added: '9', prefix: '-webkit-' }]),
      named: compat([{ version_added: '18' }, { version_added: '9', alternative_name: 'webkitFx' }]),
      later: compat({ version_added: '18' }, false),
    } },
  };
}

const ROWS = { allow: [['fx_plain', 'api.Fx.plain', "typeof Fx === 'function'"]], deny: [['fx_later', 'api.Fx.later', '\\bfxLater\\b']] };
const HEAD = '# The floor is macOS 14.0, Safari 17.0: https://developer.apple.com/documentation/safari-release-notes/safari-17-release-notes\n';

function listOf(data, rows = ROWS) {
  const made = lines(data, rows);
  assert.deepEqual(made.problems, []);
  return HEAD + made.out.join('\n') + '\n';
}

function cli(argv) {
  return spawnSync(process.execPath, [FEATURES, ...argv], { encoding: 'utf8' });
}

function dataFile() {
  const dir = tempDir('data');
  const file = path.join(dir, 'data.json');
  fs.writeFileSync(file, JSON.stringify(fixture()));
  return { dir, file };
}

/** The cases of ruling RD-41 D4 and D5 on features.mjs. */
export const cases = [
  ['d4_a_partial_entry_counts_for_a_deny_line_alone', () => {
    assert.equal(lookup(fixture(), 'api.Fx.partial', 'deny').version, '18');
    assert.equal(lookup(fixture(), 'api.Fx.partial', 'allow').problem, 'api.Fx.partial has no plain Safari version');
  }],
  ['d4_a_flagged_entry_does_not_count', () => {
    assert.equal(lookup(fixture(), 'api.Fx.flagged', 'deny').version, '18');
  }],
  ['d4_a_prefixed_entry_and_another_name_do_not_count', () => {
    assert.equal(lookup(fixture(), 'api.Fx.prefixed', 'deny').version, '18');
    assert.equal(lookup(fixture(), 'api.Fx.named', 'deny').version, '18');
  }],
  ['d4_versions_9_and_10_sort_as_numbers', () => {
    assert.deepEqual(lookup(fixture(), 'api.Fx.numbers'), { version: '9', date: '2015-09-30', page: 'https://developer.mozilla.org/docs/Web/API/Fx' });
  }],
  ['d4_a_page_comes_from_the_path_where_the_data_has_none', () => {
    assert.equal(lookup(fixture(), 'api.Fx.later', 'deny').page, 'https://developer.mozilla.org/ (api.Fx.later)');
  }],
  ['d4_the_rows_make_their_lines_and_verify_reads_them', () => {
    const list = listOf(fixture());
    assert.match(list, /\n# https:\/\/developer\.mozilla\.org\/docs\/Web\/API\/Fx, Safari 10 of 2016-09-20\nallow\tfx_plain\t10\ttypeof Fx === 'function'\n/);
    assert.match(list, /\n# https:\/\/developer\.mozilla\.org\/ \(api\.Fx\.later\), Safari 18 of 2024-09-16\ndeny\tfx_later\t18\t\\bfxLater\\b\n$/);
    assert.deepEqual(verify(fixture(), list, ROWS), { problems: [], read: 2, unread: 0 });
  }],
  ['d4_a_wrong_date_on_a_line_is_a_problem', () => {
    const list = listOf(fixture()).replace('Safari 10 of 2016-09-20', 'Safari 10 of 2016-09-21');
    assert.ok(verify(fixture(), list, ROWS).problems.includes('fx_plain: the list gives 10 of 2016-09-21, the data 10 of 2016-09-20'));
  }],
  ['d4_a_wrong_version_on_a_line_is_a_problem', () => {
    const list = listOf(fixture()).replace('Safari 10 of 2016-09-20\nallow\tfx_plain\t10', 'Safari 9 of 2016-09-20\nallow\tfx_plain\t9');
    assert.ok(verify(fixture(), list, ROWS).problems.includes('fx_plain: the list gives 9 of 2016-09-20, the data 10 of 2016-09-20'));
  }],
  ['d4_a_list_that_does_not_end_with_the_block_is_a_problem', () => {
    const list = listOf(fixture()).replace('\\bfxLater\\b', '\\bfxLater');
    assert.ok(verify(fixture(), list, ROWS).problems.includes('the list does not end with the lines that the rows make from the data'));
    const after = `${listOf(fixture())}# a comment after the block\n`;
    assert.ok(verify(fixture(), after, ROWS).problems.includes('the list does not end with the lines that the rows make from the data'));
  }],
  ['d4_verify_counts_the_lines_it_did_not_read', () => {
    const list = listOf(fixture()).replace(HEAD, `${HEAD}# https://developer.mozilla.org/docs/Web/API/Other, Safari 9 of 2015-09-30\nallow\tother_line\t9\ttypeof Other === 'function'\n`);
    assert.deepEqual(verify(fixture(), list, ROWS), { problems: [], read: 2, unread: 1 });
  }],
  ['d4_a_deny_row_at_or_under_the_floor_is_refused', () => {
    const made = lines(fixture(), { allow: [], deny: [['fx_old', 'api.Fx.plain', '\\bfx\\b']] });
    assert.ok(made.problems.includes('fx_old: Safari 10 is not above the floor'));
    const above = lines(fixture(), { allow: [['fx_new', 'api.Fx.later', 'typeof Fx === "function"']], deny: [] });
    assert.ok(above.problems.includes('fx_new: Safari 18 is above the floor'));
  }],
  ['d4_verify_with_no_list_is_refused', () => {
    const { file } = dataFile();
    for (const argv of [[file, '--verify'], [file, '--check', 'x'], [], [file, '--verify', 'a', 'b']]) {
      const r = cli(argv);
      assert.equal(r.status, 2, argv.join(' '));
      assert.equal(r.stdout, '');
      assert.match(r.stderr, /^REFUSED usage: [^\n]*\n$/);
    }
  }],
  ['d5_verify_of_a_list_that_cannot_be_read_is_refused', () => {
    const { dir, file } = dataFile();
    const r = cli([file, '--verify', path.join(dir, 'none.txt')]);
    assert.equal(r.status, 2);
    assert.equal(r.stdout, '');
    assert.match(r.stderr, /^REFUSED usage: [^\n]*\n$/);
  }],
];
