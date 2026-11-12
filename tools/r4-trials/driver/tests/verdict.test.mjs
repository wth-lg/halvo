import assert from 'node:assert/strict';
import test from 'node:test';

import { checkFile, dialogVerdict, driverVerdict, parseLine, splitLines } from '../verdict.mjs';

const R = {
  webdriverOk: 'reading trial=driver machine=mac candidate=webdriver version=0.2.4 build=ok licences=ok input_calls=0 banned=1',
  wdioMiss: 'reading trial=driver machine=mac candidate=wdio version=1.5.1 build=ok licences=miss input_calls=0 banned=1',
  wdioOk: 'reading trial=driver machine=mac candidate=wdio version=1.5.1 build=ok licences=ok input_calls=0 banned=1',
};
const cell = (m, c, ok) => `cell trial=driver machine=${m} candidate=${c} sessions=5 ok=${ok}`;

function driver(lines) {
  const parsed = lines.map(parseLine);
  return driverVerdict(parsed.filter((l) => l.kind === 'reading'), parsed.filter((l) => l.kind === 'cell'));
}

test('the first candidate that passes on a machine wins there', () => {
  assert.equal(driver([R.webdriverOk, R.wdioOk, cell('hosted-mac', 'webdriver', 5), cell('hosted-win', 'webdriver', 5)]),
    'verdict driver hosted_mac=webdriver hosted_win=webdriver');
  assert.equal(driver([R.webdriverOk, R.wdioOk, cell('hosted-mac', 'webdriver', 4), cell('hosted-mac', 'wdio', 5),
    cell('hosted-win', 'webdriver', 5), cell('hosted-win', 'wdio', 5)]),
  'verdict driver hosted_mac=wdio hosted_win=webdriver');
  assert.equal(driver([R.webdriverOk, R.wdioMiss, cell('hosted-mac', 'webdriver', 0), cell('hosted-mac', 'wdio', 5)]),
    'verdict driver hosted_mac=none hosted_win=none');
});

test('a candidate needs build=ok, licences=ok and input_calls=0 in its reading', () => {
  const cells = [cell('hosted-mac', 'webdriver', 5), cell('hosted-win', 'webdriver', 5)];
  for (const reading of [
    R.webdriverOk.replace('build=ok', 'build=fail'),
    R.webdriverOk.replace('licences=ok', 'licences=miss'),
    R.webdriverOk.replace('input_calls=0', 'input_calls=1'),
  ]) {
    assert.equal(driver([reading, ...cells]), 'verdict driver hosted_mac=none hosted_win=none', reading);
  }
  assert.equal(driver(cells), 'verdict driver hosted_mac=none hosted_win=none');
  assert.equal(driver([R.webdriverOk.replace('banned=1', 'banned=3'), ...cells]), 'verdict driver hosted_mac=webdriver hosted_win=webdriver');
});

test('a reading of an entry that no built target compiles does not stand for the candidate', () => {
  const entry = 'reading trial=driver machine=mac candidate=webdriver entry=target-lexicon version=0.13.5 licence=Apache-2.0_WITH_LLVM-exception built_targets=- rulings=P7-52,CP-102';
  assert.equal(driver([R.webdriverOk, entry, cell('hosted-mac', 'webdriver', 5), cell('hosted-win', 'webdriver', 5)]),
    'verdict driver hosted_mac=webdriver hosted_win=webdriver');
  assert.equal(driver([entry, cell('hosted-mac', 'webdriver', 5)]), 'verdict driver hosted_mac=none hosted_win=none');
});

test('two cells of one candidate on one machine are refused', () => {
  assert.throws(() => driver([R.webdriverOk, cell('hosted-mac', 'webdriver', 5), cell('hosted-mac', 'webdriver', 4)]));
});

const D = {
  attrs: 'reading trial=dialog machine=mac system=macos default_attr=AXDefaultButton cancel_attr=AXCancelButton',
  pluginMac: 'reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none file=a line=1',
  pluginWin: 'reading trial=dialog machine=mac form=plugin system=windows default=reject cancel=allow close=allow file=b line=2',
  systemWin: 'reading trial=dialog machine=mac form=system system=windows default=reject cancel=reject close=reject file=c line=3',
};
const dcell = (m, form, keys) => {
  const k = { text: 'same', default: 'reject', cancel: 'none', close: 'none', allow_control: 'allow', reject_control: 'reject', driven: 'yes', ...keys };
  return `cell trial=dialog machine=${m} form=${form} text=${k.text} default=${k.default} cancel=${k.cancel} close=${k.close} allow_control=${k.allow_control} reject_control=${k.reject_control} driven=${k.driven}`;
};
const undriven = (m, form) => `cell trial=dialog machine=${m} form=${form} text=- default=- cancel=- close=- allow_control=- reject_control=- driven=no`;

function dialog(lines) {
  const parsed = lines.map(parseLine);
  return dialogVerdict(parsed.filter((l) => l.kind === 'reading'), parsed.filter((l) => l.kind === 'cell'));
}

test('a driven form passes with the same text, no allowing default, cancel or close, and both controls', () => {
  assert.equal(dialog([D.attrs, dcell('hosted-mac', 'plugin'), dcell('hosted-win', 'plugin', { close: 'allow' }), dcell('hosted-win', 'system', { cancel: 'reject', close: 'reject' })]),
    'verdict dialog hosted_mac=plugin hosted_win=system driven_mac=yes driven_win=yes');
  for (const keys of [{ text: 'differ' }, { default: 'allow' }, { cancel: 'allow' }, { close: 'allow' },
    { allow_control: 'miss' }, { reject_control: 'miss' }]) {
    assert.equal(dialog([dcell('hosted-mac', 'plugin', keys)]), 'verdict dialog hosted_mac=none hosted_win=none driven_mac=no driven_win=no', JSON.stringify(keys));
  }
  for (const keys of [{ default: 'none' }, { cancel: 'reject' }, { close: 'reject' }]) {
    assert.equal(dialog([dcell('hosted-mac', 'plugin', keys)]), 'verdict dialog hosted_mac=plugin hosted_win=none driven_mac=yes driven_win=no', JSON.stringify(keys));
  }
});

test('an undriven form passes on the reading of its source for that system, and is driven=no', () => {
  assert.equal(dialog([D.pluginMac, undriven('hosted-mac', 'plugin')]), 'verdict dialog hosted_mac=plugin hosted_win=none driven_mac=no driven_win=no');
  assert.equal(dialog([D.pluginWin, D.systemWin, undriven('hosted-win', 'plugin'), undriven('hosted-win', 'system')]),
    'verdict dialog hosted_mac=none hosted_win=system driven_mac=no driven_win=no');
  assert.equal(dialog([undriven('hosted-mac', 'plugin')]), 'verdict dialog hosted_mac=none hosted_win=none driven_mac=no driven_win=no');
  assert.equal(dialog([D.pluginWin, undriven('hosted-mac', 'plugin')]), 'verdict dialog hosted_mac=none hosted_win=none driven_mac=no driven_win=no');
});

test('the earlier form wins even when the later one also passes', () => {
  assert.equal(dialog([dcell('hosted-mac', 'plugin'), dcell('hosted-mac', 'system')]),
    'verdict dialog hosted_mac=plugin hosted_win=none driven_mac=yes driven_win=no');
  assert.equal(dialog([dcell('hosted-mac', 'plugin', { default: 'allow' }), undriven('hosted-mac', 'system'),
    D.pluginMac.replace('form=plugin', 'form=system')]),
  'verdict dialog hosted_mac=system hosted_win=none driven_mac=no driven_win=no');
});

test('a result file holds readings, cells, figures and one verdict last', () => {
  const ok = [R.webdriverOk, cell('hosted-mac', 'webdriver', 5), cell('hosted-win', 'webdriver', 5), 'verdict driver hosted_mac=webdriver hosted_win=webdriver'];
  assert.deepEqual(checkFile('driver', `${ok.join('\n')}\n`), { verdict: ok[3], computed: ok[3] });
  const wrong = [...ok.slice(0, 3), 'verdict driver hosted_mac=wdio hosted_win=webdriver'];
  assert.equal(checkFile('driver', `${wrong.join('\n')}\n`).computed, ok[3]);
  for (const bad of [
    [cell('hosted-mac', 'webdriver', 5), R.webdriverOk, ok[3]],
    [R.webdriverOk, ok[3], cell('hosted-mac', 'webdriver', 5)],
    [R.webdriverOk, cell('hosted-mac', 'webdriver', 5)],
    [R.webdriverOk, ok[3], ok[3]],
    [R.webdriverOk, 'figure trial=driver x=1', cell('hosted-mac', 'webdriver', 5), ok[3]],
    [R.webdriverOk, 'note something', ok[3]],
    [R.webdriverOk, 'cell trial=dialog machine=hosted-mac form=plugin text=same', ok[3]],
  ]) {
    assert.throws(() => checkFile('driver', `${bad.join('\n')}\n`), bad.join(' | '));
  }
  assert.throws(() => checkFile('driver', ok.join('\n')));
  assert.throws(() => checkFile('driver', `${ok.join('\r\n')}\r\n`));
});

test('a line is a word and key=value pairs, each key once, no value empty', () => {
  assert.deepEqual(parseLine('cell trial=driver machine=hosted-mac'), { kind: 'cell', keys: { trial: 'driver', machine: 'hosted-mac' } });
  for (const bad of ['cell trial=driver trial=dialog', 'cell trial=', 'cell trial', 'cell  trial=driver', '', 'cell trial=a b']) {
    assert.throws(() => parseLine(bad), bad);
  }
  assert.deepEqual(splitLines('a\nb\n'), ['a', 'b']);
});
