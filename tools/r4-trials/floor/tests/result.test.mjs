import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LIST, parseList } from '../list.mjs';
import { checkResult, parseCell, verdictOf } from '../result.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const features = parseList(fs.readFileSync(LIST, 'utf8')).features;
const n = features.filter((f) => f.kind === 'allow').length;
const readings = execFileSync('sh', [path.join(HERE, 'read.sh')], { encoding: 'utf8' }).split('\n').filter((l) => l !== '');
const cell = (machine, system, missing = 0, names = '-', count = n) =>
  parseCell(`cell trial=floor machine=${machine} system=${system} webkit=1.2 features=${count} missing=${missing} missing_names=${names} aria_notify=undefined`);
const file = (cellLines, verdict) => [...readings, ...cellLines, verdict].join('\n') + '\n';
const macLine = `cell trial=floor machine=hosted-mac system=15.7.1 webkit=605.1.15 features=${n} missing=0 missing_names=- aria_notify=undefined`;
const winLine = `cell trial=floor machine=hosted-win system=10.0.26100.4652 webkit=142.0.3595.94 features=${n} missing=0 missing_names=- aria_notify=function`;

/** The cases of the result file and of the verdict rule. */
export const cases = [
  ['the_rule_gives_rule_only_and_the_lowest_macos_version', () => {
    assert.equal(verdictOf([cell('hosted-mac', '15.10'), cell('hosted-mac', '15.7.1'), cell('hosted-win', '10.0.1')], features),
      'verdict floor=rule-only oldest_run=15.7.1 missing=0');
  }],
  ['the_rule_gives_guest_only_with_a_cell_of_guest_mac14', () => {
    assert.equal(verdictOf([cell('hosted-mac', '15.7.1'), cell('guest-mac14', '14.0')], features), 'verdict floor=guest oldest_run=14.0 missing=0');
  }],
  ['the_rule_gives_no_verdict_where_a_feature_is_missing', () => {
    assert.equal(verdictOf([cell('hosted-mac', '15.7.1'), cell('hosted-win', '10.0.1', 1, 'click_event')], features), null);
  }],
  ['the_rule_gives_no_verdict_without_a_macos_cell_or_any_cell', () => {
    assert.equal(verdictOf([cell('hosted-win', '10.0.1')], features), null);
    assert.equal(verdictOf([], features), null);
  }],
  ['the_rule_gives_no_verdict_where_a_cell_counts_other_allow_lines', () => {
    assert.equal(verdictOf([cell('hosted-mac', '15.7.1', 0, '-', n - 1)], features), null);
  }],
  ['the_rule_gives_no_verdict_where_an_allow_line_is_above_the_floor', () => {
    const above = features.map((f, i) => (i === features.findIndex((g) => g.kind === 'allow') ? { ...f, version: '17.4' } : f));
    assert.equal(verdictOf([cell('hosted-mac', '15.7.1')], above), null);
  }],
  ['a_result_out_of_order_or_with_another_verdict_is_a_miss', () => {
    const verdict = 'verdict floor=rule-only oldest_run=15.7.1 missing=0';
    assert.deepEqual(checkResult(file([macLine, winLine], verdict), readings, features), []);
    const bad = [
      [file([macLine, winLine], verdict.replace('rule-only', 'guest')), "the verdict line is not the rule's"],
      [file([macLine, winLine], verdict.replace('15.7.1', '10.0.26100.4652')), "the verdict line is not the rule's"],
      [[macLine, ...readings, winLine, verdict].join('\n') + '\n', 'a reading line after a later kind'],
      [file([macLine, winLine], verdict) + 'cell x\n', 'the file does not end in one verdict line'],
      [file([macLine, 'figure trial=floor x=1', winLine], verdict), 'the floor trial has no figure line'],
      [file([macLine], verdict).replace(readings[0], readings[0].replace('17.0', '16.6')), 'the reading lines are not the lines that read.sh prints'],
      [file([macLine], verdict).replace(macLine, macLine.replace(' aria_notify=undefined', '')), 'a cell line out of form'],
      [file([macLine], verdict).replace('\n', '\r\n'), 'holds a carriage return'],
      ['# Floor\n' + file([macLine], verdict), 'line 1 is no reading, cell, figure or verdict line'],
      ['Floor\n' + file([macLine], verdict), 'line 1 is no reading, cell, figure or verdict line'],
    ];
    for (const [text, expected] of bad) {
      const problems = checkResult(text, readings, features).join('\n');
      assert.ok(problems.includes(expected), `${expected}: ${problems}`);
    }
  }],
];
