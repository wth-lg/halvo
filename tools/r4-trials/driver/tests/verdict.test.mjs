import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { checkFile } from '../verdict.mjs';

const OUTCOMES = ['failed_call', 'cancel_response', 'unknown_response', 'timeout'];

const here = path.dirname(fileURLToPath(import.meta.url));
const tool = path.join(here, '..', 'verdict.mjs');

const DRIVER = [
  'reading trial=driver machine=mac candidate=webdriver version=0.2.4 build=ok licences=ok input_calls=0 banned=1',
  'reading trial=driver machine=mac candidate=webdriver entry=target-lexicon version=0.13.5 licence=Apache-2.0_WITH_LLVM-exception built_targets=- rulings=P7-52,CP-102',
  'reading trial=driver machine=mac candidate=wdio version=1.5.1 build=ok licences=miss input_calls=0 banned=1',
  'reading trial=driver machine=mac candidate=wdio entry=target-lexicon version=0.13.5 licence=Apache-2.0_WITH_LLVM-exception built_targets=- rulings=P7-52,CP-102',
  'cell trial=driver machine=hosted-mac candidate=webdriver sessions=5 ok=5',
  'cell trial=driver machine=hosted-mac candidate=wdio sessions=5 ok=5',
  'cell trial=driver machine=hosted-win candidate=webdriver sessions=5 ok=5',
  'cell trial=driver machine=hosted-win candidate=wdio sessions=5 ok=5',
];
const DRIVER_PARAGRAPH = 'Read on every target and not on the four built targets alone, licences would be miss for both candidates.';

const HEAD_DIALOG = [
  'reading trial=dialog machine=mac system=macos default_attr=AXDefaultButton cancel_attr=AXCancelButton',
  'reading trial=dialog machine=mac system=windows default_attr=STATE_SYSTEM_DEFAULT cancel_attr=IDCANCEL',
  'reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none default_file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs default_line=71 cancel_file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs cancel_line=94 close_file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs close_line=94',
  'reading trial=dialog machine=mac form=plugin system=windows default=reject cancel=allow close=allow default_file=rfd-0.17.2/src/backend/win_cid/message_dialog.rs default_line=155 cancel_file=tauri-plugin-dialog-2.8.4/src/desktop.rs cancel_line=238 close_file=tauri-plugin-dialog-2.8.4/src/desktop.rs close_line=238',
  'reading trial=dialog machine=mac form=system system=macos default=reject cancel=none close=none default_file=tools/r4-trials/driver/src/native.rs default_line=72 cancel_file=tools/r4-trials/driver/src/native.rs cancel_line=71 close_file=tools/r4-trials/driver/src/native.rs close_line=71',
  'reading trial=dialog machine=mac form=system system=windows default=reject cancel=reject close=reject default_file=tools/r4-trials/driver/src/native.rs default_line=113 cancel_file=tools/r4-trials/driver/src/card.rs cancel_line=104 close_file=tools/r4-trials/driver/src/card.rs close_line=104',
];
const OLD_FORM = 'reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs line=71';
const HEAD_VERDICT = 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes';
const PLUGIN_SRC = 'tauri-plugin-dialog-2.8.4/src/desktop.rs';
const outcome = (form, system, o, gives, file = 'x.rs', line = 1) => `reading trial=dialog machine=mac form=${form} system=${system} outcome=${o} gives=${gives} file=${file} line=${line}`;
const SOURCE_OUTCOMES = [
  ...['failed_call', 'cancel_response', 'unknown_response'].map((o) => outcome('plugin', 'macos', o, 'allow', PLUGIN_SRC, 238)),
  outcome('plugin', 'macos', 'timeout', 'none', 'rfd-0.17.2/src/backend/macos/utils/user_alert.rs', 60),
  ...['failed_call', 'cancel_response', 'unknown_response'].map((o) => outcome('plugin', 'windows', o, 'allow', PLUGIN_SRC, 238)),
  outcome('plugin', 'windows', 'timeout', 'none', 'rfd-0.17.2/src/backend/win_cid/message_dialog.rs', 153),
  outcome('system', 'macos', 'failed_call', 'reject', 'tools/r4-trials/driver/src/native.rs', 80),
  outcome('system', 'macos', 'cancel_response', 'reject', 'tools/r4-trials/driver/src/card.rs', 95),
  outcome('system', 'macos', 'unknown_response', 'reject', 'tools/r4-trials/driver/src/card.rs', 95),
  outcome('system', 'macos', 'timeout', 'none', 'tools/r4-trials/driver/src/native.rs', 78),
  ...['failed_call', 'cancel_response', 'unknown_response'].map((o) => outcome('system', 'windows', o, 'reject', 'tools/r4-trials/driver/src/card.rs', 104)),
  outcome('system', 'windows', 'timeout', 'none', 'tools/r4-trials/driver/src/native.rs', 108),
];
const dcell = (m, form, keys = {}) => {
  const k = { text: 'same', default: 'reject', cancel: 'none', close: 'none', allow_control: 'allow', reject_control: 'reject', driven: 'yes', ...keys };
  return `cell trial=dialog machine=${m} form=${form} text=${k.text} default=${k.default} cancel=${k.cancel} close=${k.close} allow_control=${k.allow_control} reject_control=${k.reject_control} driven=${k.driven}`;
};
const undriven = (m, form) => `cell trial=dialog machine=${m} form=${form} text=- default=- cancel=- close=- allow_control=- reject_control=- driven=no`;
const HEAD_CELLS = [
  dcell('hosted-mac', 'plugin'),
  dcell('hosted-mac', 'system'),
  dcell('hosted-win', 'plugin', { cancel: 'allow', close: 'allow' }),
  dcell('hosted-win', 'system', { cancel: 'reject', close: 'reject' }),
];

const file = (lines, verdict, paragraph) => `${paragraph ? `${paragraph}\n\n` : ''}${[...lines, verdict].join('\n')}\n`;
const computed = (trial, lines, paragraph) => checkFile(trial, file(lines, `verdict ${trial} hosted_mac=none hosted_win=none${trial === 'dialog' ? ' driven_mac=no driven_win=no' : ''}`, paragraph)).computed;
const refused = (trial, text, cause) => assert.throws(() => checkFile(trial, text), (e) => e.cause === cause, `${cause}: ${text.slice(0, 200)}`);
const replaced = (lines, from, to) => {
  assert.ok(lines.includes(from), from);
  return lines.map((l) => (l === from ? to : l));
};
const dropped = (lines, gone) => {
  assert.ok(lines.includes(gone), gone);
  return lines.filter((l) => l !== gone);
};
const DIALOG = [...HEAD_DIALOG, ...SOURCE_OUTCOMES, ...HEAD_CELLS];
const SAFE_PLUGIN = [...HEAD_DIALOG, ...SOURCE_OUTCOMES.map((l) => l.replace(/gives=allow/, 'gives=reject')), ...HEAD_CELLS];

test('the driver file of the head gives its verdict, with its paragraph and its entry lines', () => {
  assert.equal(computed('driver', DRIVER, DRIVER_PARAGRAPH), 'verdict driver hosted_mac=webdriver hosted_win=webdriver');
  const text = fs.readFileSync(path.join(here, '..', '..', '..', '..', 'docs', 'r4', 'trials', 'driver.md'), 'utf8');
  const result = checkFile('driver', text);
  assert.equal(result.computed, result.verdict);
});

test('the first candidate that passes on a machine wins there, and a reading condition makes it lose', () => {
  const wdioOk = replaced(DRIVER, DRIVER[2], DRIVER[2].replace('licences=miss', 'licences=ok'));
  assert.equal(computed('driver', replaced(wdioOk, DRIVER[4], DRIVER[4].replace('ok=5', 'ok=4'))), 'verdict driver hosted_mac=wdio hosted_win=webdriver');
  for (const change of [['build=ok', 'build=fail'], ['licences=ok', 'licences=miss'], ['input_calls=0', 'input_calls=1']]) {
    assert.equal(computed('driver', replaced(DRIVER, DRIVER[0], DRIVER[0].replace(...change))), 'verdict driver hosted_mac=none hosted_win=none', change[1]);
    assert.equal(computed('driver', replaced(wdioOk, DRIVER[0], DRIVER[0].replace(...change))), 'verdict driver hosted_mac=wdio hosted_win=wdio', change[1]);
  }
  assert.equal(computed('driver', replaced(DRIVER, DRIVER[0], DRIVER[0].replace('banned=1', 'banned=3'))), 'verdict driver hosted_mac=webdriver hosted_win=webdriver');
});

test('D1: the head dialog file with the 16 outcome lines of the sources names system on macOS', () => {
  assert.equal(computed('dialog', [...HEAD_DIALOG, ...HEAD_CELLS]).length > 0, true);
  assert.equal(computed('dialog', DIALOG), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes');
  const text = fs.readFileSync(path.join(here, '..', '..', '..', '..', 'docs', 'r4', 'trials', 'dialog.md'), 'utf8');
  const result = checkFile('dialog', text);
  assert.equal(result.computed, 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes');
  assert.equal(result.verdict, result.computed);
});

test('D1: one outcome line that gives allow makes the form that would win lose', () => {
  assert.equal(computed('dialog', SAFE_PLUGIN), 'verdict dialog hosted_mac=plugin hosted_win=system driven_mac=yes driven_win=yes');
  for (const o of OUTCOMES) {
    const line = SAFE_PLUGIN.find((l) => l.includes('form=plugin system=macos') && l.includes(`outcome=${o} `));
    const lines = replaced(SAFE_PLUGIN, line, line.replace(/gives=\w+/, 'gives=allow'));
    assert.equal(computed('dialog', lines), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes', o);
  }
});

test('D1: an outcome line that gives unknown, or one that is missing, makes the form lose and is no refusal', () => {
  const line = SAFE_PLUGIN.find((l) => l.includes('form=plugin system=macos') && l.includes('outcome=timeout '));
  assert.equal(computed('dialog', replaced(SAFE_PLUGIN, line, line.replace(/gives=\w+/, 'gives=unknown'))),
    'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes');
  assert.equal(computed('dialog', dropped(SAFE_PLUGIN, line)), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes');
  const systemLine = DIALOG.find((l) => l.includes('form=system system=windows') && l.includes('outcome=failed_call '));
  assert.equal(computed('dialog', dropped(DIALOG, systemLine)), 'verdict dialog hosted_mac=system hosted_win=none driven_mac=yes driven_win=no');
});

test('a driven form passes with the same text, no allowing default, cancel or close, and both controls', () => {
  for (const keys of [{ text: 'differ' }, { default: 'allow' }, { cancel: 'allow' }, { close: 'allow' },
    { allow_control: 'miss' }, { reject_control: 'miss' }]) {
    const lines = replaced(SAFE_PLUGIN, HEAD_CELLS[0], dcell('hosted-mac', 'plugin', keys));
    assert.equal(computed('dialog', lines), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes', JSON.stringify(keys));
  }
  for (const keys of [{ default: 'none' }, { cancel: 'reject' }, { close: 'reject' }]) {
    const lines = replaced(SAFE_PLUGIN, HEAD_CELLS[0], dcell('hosted-mac', 'plugin', keys));
    assert.equal(computed('dialog', lines), 'verdict dialog hosted_mac=plugin hosted_win=system driven_mac=yes driven_win=yes', JSON.stringify(keys));
  }
});

test('an undriven form passes on each of default, cancel and close of its reading, and is driven=no', () => {
  const base = replaced(SAFE_PLUGIN, HEAD_CELLS[0], undriven('hosted-mac', 'plugin'));
  assert.equal(computed('dialog', base), 'verdict dialog hosted_mac=plugin hosted_win=system driven_mac=no driven_win=yes');
  for (const key of ['default', 'cancel', 'close']) {
    const lines = replaced(base, HEAD_DIALOG[2], HEAD_DIALOG[2].replace(`${key}=${key === 'default' ? 'reject' : 'none'}`, `${key}=allow`));
    assert.equal(computed('dialog', lines), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes', key);
  }
});

test('the earlier form wins even when the later one also passes', () => {
  const lines = replaced(SAFE_PLUGIN, HEAD_CELLS[2], dcell('hosted-win', 'plugin'));
  const fixed = lines.map((l) => (l.includes('form=plugin system=windows default=') ? l.replace('cancel=allow close=allow', 'cancel=none close=none') : l));
  assert.equal(computed('dialog', fixed), 'verdict dialog hosted_mac=plugin hosted_win=plugin driven_mac=yes driven_win=yes');
});

test('D3: a cell missing for a machine and a driver or a form is refused', () => {
  for (const gone of DRIVER.slice(4)) refused('driver', file(dropped(DRIVER, gone), 'verdict driver hosted_mac=none hosted_win=none'), 'missing_cell');
  for (const gone of HEAD_CELLS) refused('dialog', file(dropped(DIALOG, gone), 'verdict dialog hosted_mac=none hosted_win=none driven_mac=no driven_win=no'), 'missing_cell');
});

test('D3: two cells for one machine and one driver or form are refused, also for one that loses', () => {
  refused('driver', file([...DRIVER, DRIVER[5]], 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'duplicate');
  refused('driver', file([...DRIVER, DRIVER[5].replace('ok=5', 'ok=0')], 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'duplicate');
  refused('dialog', file([...DIALOG, HEAD_CELLS[2]], 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes'), 'duplicate');
  const outcomeLine = SOURCE_OUTCOMES[0];
  refused('dialog', file([...HEAD_DIALOG, ...SOURCE_OUTCOMES, outcomeLine, ...HEAD_CELLS], 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes'), 'duplicate');
  refused('driver', file([DRIVER[0], DRIVER[0], ...DRIVER.slice(1)], 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'duplicate');
});

test('D3: a driver cell holds sessions=5 and an ok of 0 to 5 in its plain form', () => {
  for (const bad of ['sessions=4 ok=5', 'sessions=6 ok=5', 'ok=5', 'sessions=5 ok=05', 'sessions=5 ok=+5', 'sessions=5 ok=6', 'sessions=5 ok=-1', 'sessions=5']) {
    const line = `cell trial=driver machine=hosted-mac candidate=webdriver ${bad}`;
    refused('driver', file(replaced(DRIVER, DRIVER[4], line), 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'line_form');
  }
});

test('D3: a dialog cell with driven=no holds - in every other key, and one with driven=yes holds none', () => {
  const bad = 'cell trial=dialog machine=hosted-mac form=plugin text=- default=allow cancel=allow close=allow allow_control=miss reject_control=- driven=no';
  refused('dialog', file(replaced(DIALOG, HEAD_CELLS[0], bad), 'verdict dialog hosted_mac=plugin hosted_win=plugin driven_mac=no driven_win=no'), 'line_form');
  refused('dialog', file(replaced(DIALOG, HEAD_CELLS[0], dcell('hosted-mac', 'plugin', { text: '-' })), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes'), 'line_form');
});

test('D3: a line whose keys, order or words are not those of its form is refused', () => {
  const D = 'verdict driver hosted_mac=webdriver hosted_win=webdriver';
  const G = 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes';
  for (const [at, bad] of [
    [0, `${DRIVER[0]} run=other`],
    [0, DRIVER[0].replace('machine=mac', 'machine=windows')],
    [0, DRIVER[0].replace('version=0.2.4 build=ok', 'build=ok version=0.2.4')],
    [0, DRIVER[0].replace('version=0.2.4', 'version=9')],
    [4, `${DRIVER[4]} run_id=1`],
    [4, DRIVER[4].replace('hosted-mac', 'hosted-linux')],
    [4, DRIVER[4].replace('candidate=webdriver', 'candidate=tauri-driver')],
    [4, DRIVER[4].replace('candidate=webdriver sessions=5', 'sessions=5 candidate=webdriver')],
    [4, DRIVER[4].replace('cell trial=driver', 'cell extra trial=driver')],
  ]) {
    refused('driver', file(replaced(DRIVER, DRIVER[at], bad), D), 'line_form');
  }
  for (const [from, bad] of [
    [HEAD_DIALOG[2], HEAD_DIALOG[2].replace(' close_file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs close_line=94', '')],
    [HEAD_DIALOG[2], HEAD_DIALOG[2].replace('machine=mac', 'machine=other')],
    [HEAD_DIALOG[2], HEAD_DIALOG[2].replace('form=plugin', 'form=alert')],
    [HEAD_DIALOG[2], HEAD_DIALOG[2].replace('default_line=71', 'default_line=0')],
    [HEAD_DIALOG[0], HEAD_DIALOG[0].replace('system=macos', 'system=linux')],
    [SOURCE_OUTCOMES[0], SOURCE_OUTCOMES[0].replace('outcome=failed_call', 'outcome=crash')],
    [SOURCE_OUTCOMES[0], SOURCE_OUTCOMES[0].replace('gives=allow', 'gives=maybe')],
    [HEAD_CELLS[0], HEAD_CELLS[0].replace('machine=hosted-mac', 'machine=hosted-win7')],
    [HEAD_CELLS[0], `${HEAD_CELLS[0]} owner=app`],
  ]) {
    refused('dialog', file(replaced(DIALOG, from, bad), G), 'line_form');
  }
});

test('U0.4b: a form reading holds a file and a line for each of default, cancel and close, and the old form with one of each is refused', () => {
  const refusedAt = (text, n, what) => assert.throws(() => checkFile('dialog', text), (e) => e.cause === 'line_form' && e.line === n, what);
  assert.equal(checkFile('dialog', file(DIALOG, HEAD_VERDICT)).computed, HEAD_VERDICT);
  refusedAt(file(replaced(DIALOG, HEAD_DIALOG[2], OLD_FORM), HEAD_VERDICT), 3, 'the old form');
  for (const key of ['default', 'cancel', 'close']) {
    for (const part of ['file', 'line']) {
      const line = HEAD_DIALOG[5].replace(new RegExp(` ${key}_${part}=\\S+`), '');
      assert.notEqual(line, HEAD_DIALOG[5], `${key}_${part}`);
      refusedAt(file(replaced(DIALOG, HEAD_DIALOG[5], line), HEAD_VERDICT), 6, `${key}_${part} left out`);
    }
  }
  for (const key of ['default', 'cancel', 'close']) {
    for (const [part, bad] of [['file', 'a:b'], ['line', '0'], ['line', '07'], ['line', 'x']]) {
      const line = HEAD_DIALOG[5].replace(new RegExp(` ${key}_${part}=\\S+`), ` ${key}_${part}=${bad}`);
      assert.notEqual(line, HEAD_DIALOG[5], `${key}_${part}=${bad}`);
      refusedAt(file(replaced(DIALOG, HEAD_DIALOG[5], line), HEAD_VERDICT), 6, `${key}_${part}=${bad}`);
    }
  }
  const moved = HEAD_DIALOG[5].replace(/ (default_file=\S+ default_line=\S+) (cancel_file=\S+ cancel_line=\S+)/, ' $2 $1');
  assert.notEqual(moved, HEAD_DIALOG[5]);
  refusedAt(file(replaced(DIALOG, HEAD_DIALOG[5], moved), HEAD_VERDICT), 6, 'the places in another order');
});

test('U0.4b: the committed dialog.md gives the verdict line that it gave with one file and line for three keys', () => {
  const text = fs.readFileSync(path.join(here, '..', '..', '..', '..', 'docs', 'r4', 'trials', 'dialog.md'), 'utf8');
  const forms = text.split('\n').filter((l) => / form=\w+ system=\w+ default=/.test(l));
  assert.equal(forms.length, 4);
  for (const l of forms) assert.match(l, / default_file=\S+ default_line=[1-9][0-9]* cancel_file=\S+ cancel_line=[1-9][0-9]* close_file=\S+ close_line=[1-9][0-9]*$/);
  const result = checkFile('dialog', text);
  assert.equal(result.verdict, HEAD_VERDICT);
  assert.equal(result.computed, HEAD_VERDICT);
});

test('D3: a reading missing for a driver or for a form on a system is refused', () => {
  refused('driver', file(dropped(DRIVER, DRIVER[0]), 'verdict driver hosted_mac=none hosted_win=none'), 'missing_reading');
  refused('driver', file(dropped(DRIVER, DRIVER[2]), 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'missing_reading');
  for (const gone of HEAD_DIALOG) {
    refused('dialog', file(dropped(DIALOG, gone), 'verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes'), 'missing_reading');
  }
});

test('D3: a figure line, a carriage return, a missing or doubled final line feed and a paragraph line in small letters are refused', () => {
  const ok = file(DRIVER, 'verdict driver hosted_mac=webdriver hosted_win=webdriver');
  refused('driver', file([...DRIVER.slice(0, 4), 'figure trial=driver median=5', ...DRIVER.slice(4)], 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'figure');
  refused('driver', ok.replace('\n', '\r\n'), 'carriage_return');
  refused('driver', ok.slice(0, -1), 'line_end');
  refused('driver', `${ok}\n`, 'line_end');
  refused('driver', `read on every target.\n\n${ok}`, 'paragraph');
  refused('driver', `\n${ok}`, 'paragraph');
  refused('driver', `A sentence.\n\n\n${ok}`, 'line_form');
  refused('driver', file([DRIVER[4], ...DRIVER.slice(0, 4), ...DRIVER.slice(5)], 'verdict driver hosted_mac=webdriver hosted_win=webdriver'), 'order');
  refused('driver', file(DRIVER, 'verdict driver hosted_mac=webdriver'), 'verdict_line');
  refused('driver', file(DRIVER, 'verdict dialog hosted_mac=webdriver hosted_win=webdriver'), 'verdict_line');
  refused('driver', `${DRIVER.join('\n')}\n`, 'verdict_line');
  assert.equal(checkFile('driver', `${DRIVER_PARAGRAPH}\nA second sentence.\n\n${ok}`).computed, 'verdict driver hosted_mac=webdriver hosted_win=webdriver');
});

function run(args) {
  return spawnSync(process.execPath, [tool, ...args], { encoding: 'utf8', timeout: 30000 });
}

test('D3: the exit code is 0 for the computed verdict, 1 for another, 2 on a refusal and 64 on a wrong call', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-verdict-test-'));
  const write = (name, text) => {
    const p = path.join(dir, name);
    fs.writeFileSync(p, text);
    return p;
  };
  const good = run(['driver', write('good.md', file(DRIVER, 'verdict driver hosted_mac=webdriver hosted_win=webdriver'))]);
  assert.equal(good.status, 0);
  assert.equal(good.stdout, 'verdict driver hosted_mac=webdriver hosted_win=webdriver\nverdict_matches=yes\n');
  assert.equal(good.stderr, '');
  const other = run(['driver', write('other.md', file(DRIVER, 'verdict driver hosted_mac=wdio hosted_win=webdriver'))]);
  assert.equal(other.status, 1);
  assert.equal(other.stdout, 'verdict driver hosted_mac=webdriver hosted_win=webdriver\nverdict_matches=no\n');
  const bad = run(['driver', write('bad.md', file(replaced(DRIVER, DRIVER[4], DRIVER[4].replace('sessions=5', 'sessions=4')), 'verdict driver hosted_mac=webdriver hosted_win=webdriver'))]);
  assert.equal(bad.status, 2);
  assert.equal(bad.stdout, '');
  assert.match(bad.stderr, /^REFUSED line_form: line 5: [^\n]+\n$/);
  const missing = run(['dialog', path.join(dir, 'no-such-file.md')]);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /^REFUSED file: line 0: [^\n]+\n$/);
  for (const args of [[], ['driver'], ['window', path.join(dir, 'good.md')], ['driver', path.join(dir, 'good.md'), 'x']]) {
    const wrong = run(args);
    assert.equal(wrong.status, 64, JSON.stringify(args));
    assert.equal(wrong.stdout, '');
    assert.match(wrong.stderr, /^usage: [^\n]+\n$/);
  }
});
