import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { CASES, checkCases, compareVersions, denied, LIST, main, parseCases, parseList, PROBE, probeSource } from '../list.mjs';
import { tempDir } from './temp.mjs';

const text = fs.readFileSync(LIST, 'utf8');
const casesText = fs.readFileSync(CASES, 'utf8');
const lines = text.split('\n');
const firstOf = (kind) => lines.findIndex((l) => l.startsWith(`${kind}\t`));

function edit(index, change) {
  const copy = lines.slice();
  copy[index] = change(copy[index]);
  return copy.join('\n');
}

function problemsOf(listText) {
  return parseList(listText).problems.join('\n');
}

function run(argv) {
  let out = '';
  let err = '';
  const code = main(argv, (s) => { out += s; }, (s) => { err += s; });
  return { code, out, err };
}

const PLANTED = [
  ['no_floor_page_on_line_1', () => text.split('\n').slice(1).join('\n'), "line 1 names no page of the floor's Safari release notes"],
  ['allow_above_the_floor', () => edit(firstOf('allow') - 1, (l) => l.replace(/Safari \S+ of/, 'Safari 17.4 of')).split('\n').map((l, i) => (i === firstOf('allow') ? l.replace(/^(allow\t[^\t]+\t)[^\t]+/, '$117.4') : l)).join('\n'), 'above the floor 17.0'],
  ['deny_at_the_floor', () => edit(firstOf('deny') - 1, (l) => l.replace(/Safari \S+ of/, 'Safari 17.0 of')).split('\n').map((l, i) => (i === firstOf('deny') ? l.replace(/^(deny\t[^\t]+\t)[^\t]+/, '$117.0') : l)).join('\n'), 'at or under the floor 17.0'],
  ['three_cells', () => edit(firstOf('deny'), (l) => l.split('\t').slice(0, 3).join('\t')), '3 cells, not 4'],
  ['unknown_kind', () => edit(firstOf('deny'), (l) => l.replace(/^deny/, 'permit')), 'is neither allow nor deny'],
  ['name_out_of_form', () => edit(firstOf('deny'), (l) => l.replace(/^(deny\t)([^\t]+)/, '$1Bad-Name')), "the name is not of I0.6's form"],
  ['name_twice', () => edit(firstOf('deny'), (l) => l.replace(/^(deny\t)[^\t]+/, `$1${lines[firstOf('allow')].split('\t')[1]}`)), 'is used twice'],
  ['no_comment_above', () => edit(firstOf('deny') - 1, () => '# no page here'), 'the line above names no page and date of its version'],
  ['comment_of_another_version', () => edit(firstOf('deny') - 1, (l) => l.replace(/Safari \S+ of/, 'Safari 99 of')), 'the comment above gives the version 99'],
  ['expression_that_calls_function', () => edit(firstOf('allow'), (l) => l.replace(/\t[^\t]+$/, "\tFunction('return 1')() === 1")), 'the expression calls eval or Function'],
  ['expression_that_calls_eval', () => edit(firstOf('allow'), (l) => l.replace(/\t[^\t]+$/, "\teval ('1') === 1")), 'the expression calls eval or Function'],
  ['expression_that_does_not_parse',() => edit(firstOf('allow'), (l) => l.replace(/\t[^\t]+$/, '\ttypeof (')), 'the expression does not parse'],
  ['pattern_that_does_not_compile', () => edit(firstOf('deny'), (l) => l.replace(/\t[^\t]+$/, '\t\\-x')), 'the pattern does not compile with the flags g and u'],
  ['empty_fourth_cell', () => edit(firstOf('deny'), (l) => l.replace(/\t[^\t]+$/, '\t')), 'the fourth cell is empty'],
  ['carriage_return', () => text.replace('\n', '\r\n'), 'the list holds a carriage return'],
  ['no_last_line_feed', () => text.slice(0, -1), 'the list does not end with a line feed'],
];

/** The cases of the list, its deny patterns and its probe. */
export const cases = [
  ['list_holds_its_form', () => {
    const { features, problems } = parseList(text);
    assert.deepEqual(problems, []);
    assert.ok(features.some((f) => f.kind === 'deny'));
    for (const f of features) assert.equal(compareVersions(f.version, '17.0') > 0, f.kind === 'deny', f.name);
  }],
  ...PLANTED.map(([name, make, expected]) => [`list_refuses_${name}`, () => {
    const changed = make();
    assert.notEqual(changed, text);
    assert.ok(problemsOf(changed).includes(expected), problemsOf(changed));
  }]),
  ['list_refuses_the_floor_page_on_a_later_line', () => {
    const moved = [lines[1], lines[0], ...lines.slice(2)].join('\n');
    assert.ok(problemsOf(moved).includes("line 1 names no page of the floor's Safari release notes"));
  }],
  ['an_allow_line_at_the_floor_stands', () => {
    const at = firstOf('allow');
    const copy = lines.slice();
    copy[at - 1] = copy[at - 1].replace(/Safari \S+ of/, 'Safari 17.0 of');
    copy[at] = copy[at].replace(/^(allow\t[^\t]+\t)[^\t]+/, '$117.0');
    assert.deepEqual(parseList(copy.join('\n')).problems, []);
  }],
  ['a_case_line_out_of_form_is_a_problem', () => {
    for (const line of ['use\tobject_group_by', 'use\tobject_group_by\t', 'free', 'free\ta\tb', 'take\tx\ty', 'use\tBad\tx']) {
      assert.deepEqual(parseCases(`${line}\n`).problems, ['cases line 1 is neither "use, name, text" nor "free, text"'], line);
    }
  }],
  ['check_finds_a_probe_that_is_not_the_lists', () => {
    const dir = tempDir('probe');
    const stale = path.join(dir, 'probe.js');
    fs.writeFileSync(stale, fs.readFileSync(PROBE, 'utf8').replace(/\n {2}\["[a-z0-9_]+", [^\n]*\n/, '\n'));
    const r = run(['check', '--probe', stale]);
    assert.equal(r.code, 1);
    assert.match(r.out, /^miss probe\.js is not the probe of the list: /m);
    assert.equal(run(['check', '--probe', PROBE]).code, 0);
  }],
  ['versions_compare_field_by_field', () => {
    assert.equal(compareVersions('17.10', '17.4'), 1);
    assert.equal(compareVersions('17', '17.0'), 0);
    assert.equal(compareVersions('16.6', '17.0'), -1);
    assert.equal(compareVersions('18', '17.6'), 1);
  }],
  ['every_deny_pattern_finds_its_uses_and_no_free_text', () => {
    const parsed = parseCases(casesText);
    assert.deepEqual(parsed.problems, []);
    assert.deepEqual(checkCases(parseList(text).features, parsed.cases), []);
  }],
  ['a_deny_line_without_a_use_case_is_a_miss', () => {
    const { features } = parseList(text);
    const name = features.find((f) => f.kind === 'deny').name;
    const kept = parseCases(casesText).cases.filter((c) => c.name !== name);
    assert.ok(checkCases(features, kept).includes(`${name} has no use case`));
  }],
  ['a_pattern_that_misses_its_use_is_a_miss', () => {
    const { features } = parseList(text);
    const changed = features.map((f) => (f.kind === 'deny' && f.name === 'object_group_by' ? { ...f, cell: 'nothing_matches_this' } : f));
    assert.ok(checkCases(changed, parseCases(casesText).cases).some((p) => p.endsWith('the pattern of object_group_by misses its use')));
  }],
  ['a_free_text_that_a_pattern_finds_is_a_miss', () => {
    const { features } = parseList(text);
    const extra = [...parseCases(casesText).cases, { kind: 'free', text: 'Promise.withResolvers()', line: 0 }];
    assert.ok(checkCases(features, extra).includes('cases line 0: a free text is found by promise_with_resolvers'));
  }],
  ['a_use_case_of_no_deny_line_is_a_miss', () => {
    const extra = [...parseCases(casesText).cases, { kind: 'use', name: 'no_such_line', text: 'x', line: 0 }];
    assert.ok(checkCases(parseList(text).features, extra).includes('cases line 0: no_such_line is no deny line'));
  }],
  ['a_deny_pattern_finds_a_use_at_any_place_of_the_text', () => {
    const { features } = parseList(text);
    assert.deepEqual(denied(features, 'a\nb\nconst x = Object.groupBy(rows, f);\n'), ['object_group_by']);
    assert.deepEqual(denied(features, 'Object.groupBy(a); Object.groupBy(b);'), ['object_group_by']);
  }],
  ['probe_js_is_the_probe_of_the_list', () => {
    assert.equal(fs.readFileSync(PROBE, 'utf8'), probeSource(parseList(text).features));
  }],
  ['check_finds_a_denied_use_in_a_view_and_passes_a_clean_one', () => {
    const dir = tempDir('list');
    const bad = path.join(dir, 'bad.js');
    const good = path.join(dir, 'good.js');
    fs.writeFileSync(bad, "(function () { const g = Object.groupBy([], String); document.ariaNotify('x'); })();\n");
    fs.writeFileSync(good, "(function () { const k = Object.keys({}).map(String); })();\n");
    const r1 = run(['check', bad]);
    assert.equal(r1.code, 1);
    assert.match(r1.out, /^miss the view bad\.js uses object_group_by, which the list denies$/m);
    assert.match(r1.out, /^miss the view bad\.js uses aria_notify, which the list denies$/m);
    assert.match(r1.out, / views=1 found=2 problems=2\n$/);
    const r2 = run(['check', good]);
    assert.equal(r2.code, 0, r2.out);
    assert.match(r2.out, / views=1 found=0 problems=0\n$/);
  }],
  ['check_refuses_an_unknown_mode_or_flag', () => {
    for (const argv of [[], ['list'], ['check', '--nope'], ['probe', 'view.js'], ['check', '--list']]) {
      const r = run(argv);
      assert.equal(r.code, 2, argv.join(' '));
      assert.equal(r.out, '');
      assert.match(r.err, /^REFUSED usage: [^\n]*\n$/);
    }
  }],
];
