import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LIST, parseList } from '../list.mjs';
import { checkResult, parseCell } from '../result.mjs';
import { tempDir } from './temp.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const features = parseList(fs.readFileSync(LIST, 'utf8')).features;
const n = features.filter((f) => f.kind === 'allow').length;
const readings = execFileSync('sh', [path.join(HERE, 'read.sh')], { encoding: 'utf8' }).split('\n').filter((l) => l !== '');
const line = (machine, system, missing, names) =>
  `cell trial=floor machine=${machine} system=${system} webkit=1.2 features=${n} missing=${missing} missing_names=${names} aria_notify=undefined`;
const mac = line('hosted-mac', '15.8.2', 0, '-');
const win = line('hosted-win', '10.0.26100.1', 0, '-');
const verdict = 'verdict floor=rule-only oldest_run=15.8.2 missing=0';
const file = (cells, last = [verdict]) => [...readings, ...cells, ...last].join('\n') + '\n';
const problems = (text) => checkResult(text, readings, features).join('\n');

function result(argv) {
  return spawnSync(process.execPath, [path.join(HERE, 'result.mjs'), ...argv], { encoding: 'utf8' });
}

function refused(r) {
  assert.equal(r.status, 2);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, /^REFUSED usage: [^\n]*\n$/);
}

const missingPath = () => path.join(tempDir('missing'), 'none.md');

/** The cases of ruling RD-41 D1, D2 and D5 on the result file. */
export const cases = [
  ['d1_a_cell_with_missing_0_and_a_name_is_out_of_form', () => {
    const bad = line('hosted-win', '10.0.26100.1', 0, 'click_event');
    assert.equal(parseCell(bad), null);
    assert.ok(problems(file([mac, bad])).includes('a cell line out of form'));
  }],
  ['d1_a_cell_with_missing_1_and_no_name_is_out_of_form', () => {
    const bad = line('hosted-win', '10.0.26100.1', 1, '-');
    assert.equal(parseCell(bad), null);
    assert.ok(problems(file([mac, bad])).includes('a cell line out of form'));
  }],
  ['d1_a_cell_with_two_names_counts_two', () => {
    assert.equal(parseCell(line('hosted-win', '10.0.26100.1', 2, 'click_event,input_event')).missing, 2);
    assert.equal(parseCell(line('hosted-win', '10.0.26100.1', 1, 'click_event,input_event')), null);
  }],
  ['d1_a_cell_with_a_missing_feature_gives_no_verdict', () => {
    const text = file([mac, line('hosted-win', '10.0.26100.1', 1, 'click_event')]);
    assert.ok(problems(text).includes('the rule gives no verdict'));
    assert.ok(!problems(text).includes('out of form'));
  }],
  ['d1_a_file_with_two_verdict_lines_is_refused', () => {
    assert.equal(problems(file([mac, win])), '');
    assert.ok(problems(file([mac, win], [verdict, verdict])).includes('the file does not end in one verdict line'));
  }],
  ['d1_an_unknown_flag_is_refused_with_exit_2', () => {
    refused(result(['check', '--fiel', 'docs/r4/trials/floor.md']));
    refused(result(['check', '--file']));
  }],
  ['d2_a_file_without_the_hosted_win_cell_is_refused', () => {
    assert.ok(problems(file([mac])).includes('the file holds no cell of hosted-win'));
  }],
  ['d2_a_file_with_a_guest_cell_and_no_hosted_mac_cell_is_refused', () => {
    const guest = line('guest-mac14', '14.0', 0, '-');
    const text = file([guest, win], ['verdict floor=guest oldest_run=14.0 missing=0']);
    assert.ok(problems(text).includes('the file holds no cell of hosted-mac'));
    assert.equal(problems(file([guest, mac, win], ['verdict floor=guest oldest_run=14.0 missing=0'])), '');
  }],
  ['f5_a_comment_line_in_the_result_file_is_refused', () => {
    const note = '# The cell of hosted-mac is from run 1, the cell of hosted-win from run 2.';
    assert.ok(problems([...readings, mac, win, note, verdict].join('\n') + '\n').includes('line 8 is no reading, cell, figure or verdict line'));
  }],
  ['d5_a_result_file_that_cannot_be_read_is_refused', () => {
    refused(result(['check', '--file', missingPath()]));
  }],
  ['d5_a_list_that_cannot_be_read_is_refused', () => {
    refused(result(['check', '--list', missingPath()]));
  }],
];
