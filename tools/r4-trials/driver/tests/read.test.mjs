import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  addedPackages, attrLines, BANNED, closure, countInputCalls, dialogLines, DIALOG_SOURCES, findLine,
  licenceAllowed, npmTree, parseArgs, parsePatterns, readingLine, Refused,
} from '../read.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const crate = path.join(here, '..');
const patterns = parsePatterns(fs.readFileSync(path.join(crate, 'input-patterns.txt'), 'utf8'));

test('the arguments are one candidate, one dialog form or --attrs', () => {
  assert.deepEqual(parseArgs(['--candidate', 'webdriver']), { mode: 'candidate', name: 'webdriver' });
  assert.deepEqual(parseArgs(['--candidate', 'wdio']), { mode: 'candidate', name: 'wdio' });
  assert.deepEqual(parseArgs(['--dialog', 'plugin']), { mode: 'dialog', form: 'plugin' });
  assert.deepEqual(parseArgs(['--dialog', 'system']), { mode: 'dialog', form: 'system' });
  assert.deepEqual(parseArgs(['--attrs']), { mode: 'attrs' });
  for (const bad of [[], ['--candidate'], ['--candidate', 'tauri-driver'], ['--dialog', 'none'],
    ['--candidate', 'webdriver', '--dialog', 'plugin'], ['--attrs', 'x'], ['--help']]) {
    assert.throws(() => parseArgs(bad), (e) => e instanceof Refused && e.cause === 'usage', JSON.stringify(bad));
  }
});

test('the pattern file names the calls and crates of the contract', () => {
  for (const name of ['CGEventPost', 'SendInput', 'keybd_event']) {
    assert.equal(countInputCalls(patterns, [`unsafe { ${name}(a, b) };`], []), 1, name);
  }
  assert.equal(countInputCalls(patterns, ['mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);'], []), 1);
  assert.equal(countInputCalls(patterns, ['let e = enigo::Enigo::new();'], []), 1);
  assert.equal(countInputCalls(patterns, [], ['enigo']), 1);
  assert.equal(countInputCalls(patterns, [], ['rdev', 'serde', 'autopilot']), 2);
  assert.equal(countInputCalls(patterns, ['CGEventPostToPid(pid, e);'], []), 1);
  assert.equal(countInputCalls(patterns, ['let e = CGEventCreateKeyboardEvent(None, 0, true);'], []), 1);
  assert.equal(countInputCalls(patterns, ['ev.post(CGEventTapLocation::HID);'], []), 1);
  assert.equal(countInputCalls(patterns, ['[NSApp postEvent:e atStart:NO];'], []), 1);
});

test('a script that builds a synthetic page event is no input call', () => {
  const lines = [
    'let mouse_event = |name: &str| format!("new MouseEvent({name})");',
    'mouse_event(mouse_event_name)',
    'el.dispatchEvent(new KeyboardEvent("keydown", { key }));',
    'suppress_mouse_events: bool,',
    '// SendInputs are not used here',
    'let sendinput = 0;',
  ];
  assert.equal(countInputCalls(patterns, lines, ['serde', 'tauri', 'mouse']), 0);
});

test('counts add up over files, lines and names', () => {
  assert.equal(countInputCalls(patterns, ['SendInput(); SendInput();', 'x\nkeybd_event(1)'], ['enigo', 'rdev']), 5);
});

test('a pattern file without call or crate lines, or with another line, is refused', () => {
  assert.throws(() => parsePatterns('# only a comment\n'));
  assert.throws(() => parsePatterns('call \\bX\\b\n'));
  assert.throws(() => parsePatterns('crate x\n'));
  assert.throws(() => parsePatterns('call \\bX\\b\ncrate x\nother y\n'));
  const p = parsePatterns('call \\bX\\b\r\ncrate x\r\n');
  assert.equal(p.crates[0], 'x');
});

test('a licence passes when one side of each OR stays inside the list', () => {
  for (const ok of ['MIT', 'Apache-2.0', 'MIT OR Apache-2.0', 'Apache-2.0 OR MIT', 'MIT/Apache-2.0',
    '(MIT OR Apache-2.0) AND Unicode-3.0', 'Zlib OR Apache-2.0 OR MIT', 'BSD-3-Clause', 'ISC', 'BSD-2-Clause',
    'MIT OR GPL-3.0', 'Unicode-3.0']) {
    assert.equal(licenceAllowed(ok), true, ok);
  }
  for (const bad of ['MPL-2.0', 'GPL-3.0', 'MIT AND MPL-2.0', 'Apache-2.0 WITH LLVM-exception', '', null, undefined,
    'MIT OR', '(MIT', 'MIT)', 'AND MIT', 'mit', 'Unlicense', 'BSD-2-Clause AND (MPL-2.0 OR GPL-2.0)']) {
    assert.equal(licenceAllowed(bad), false, String(bad));
  }
});

function meta() {
  const pkg = (name, license = 'MIT') => ({ id: `${name}-id`, name, version: '1.0.0', license });
  const packages = ['root', 'app', 'tauri', 'cand', 'axum', 'hyper', 'serde', 'dialog'].map((n) => pkg(n));
  const deps = {
    root: ['app', 'cand', 'dialog'], app: ['tauri', 'serde'], tauri: ['serde'], cand: ['tauri', 'axum'],
    axum: ['hyper', 'serde'], hyper: [], serde: [], dialog: ['axum'],
  };
  const nodes = Object.entries(deps).map(([n, d]) => ({ id: `${n}-id`, deps: d.map((x) => ({ pkg: `${x}-id` })) }));
  return { packages, resolve: { nodes } };
}

test('the candidate adds the crates of its closure that the trial app does not hold', () => {
  const m = meta();
  assert.deepEqual(closure(m, 'app').map((p) => p.name).sort(), ['app', 'serde', 'tauri']);
  assert.deepEqual(addedPackages(m, 'cand', 'app').map((p) => p.name).sort(), ['axum', 'cand', 'hyper']);
  assert.throws(() => closure(m, 'missing'));
});

test('the banned names are the eight of the billing run', () => {
  assert.deepEqual(BANNED, ['reqwest', 'hyper', 'ureq', 'rustls', 'ring', 'aws-lc-rs', 'native-tls', 'openssl-sys']);
});

test('the npm tree is every package of the lock with its licence', () => {
  const lock = {
    packages: {
      '': { name: 'x' },
      'node_modules/@wdio/tauri-service': { version: '1.5.1', license: 'MIT' },
      'node_modules/a/node_modules/b': { version: '2.0.0' },
    },
  };
  assert.deepEqual(npmTree(lock, '@wdio/tauri-service'), [
    { name: '@wdio/tauri-service', version: '1.5.1', license: 'MIT' },
    { name: 'b', version: '2.0.0', license: null },
  ]);
  assert.throws(() => npmTree({ packages: {} }, '@wdio/tauri-service'));
});

test('the reading line holds the keys of I11.5 in order', () => {
  assert.equal(
    readingLine({ name: 'webdriver', version: '0.2.4', build: 'ok', licences: true, inputCalls: 0, banned: 1 }),
    'reading trial=driver machine=mac candidate=webdriver version=0.2.4 build=ok licences=ok input_calls=0 banned=1',
  );
  assert.equal(
    readingLine({ name: 'wdio', version: '1.5.1', build: 'fail', licences: false, inputCalls: 2, banned: 0 }),
    'reading trial=driver machine=mac candidate=wdio version=1.5.1 build=fail licences=miss input_calls=2 banned=0',
  );
});

test('a dialog reading names one line that matches once', () => {
  assert.equal(findLine('a\nb\r\nc', /^b$/), 2);
  assert.throws(() => findLine('b\nb', /^b$/));
  assert.throws(() => findLine('a', /^b$/));
  const texts = {
    'src/backend/macos/message_dialog.rs': 'x\n            unsafe { alert.addButtonWithTitle(&label) };\n',
    'src/desktop.rs': 'a\nb\n                ) => rfd::MessageDialogResult::Custom(cancel),\n',
  };
  const lines = dialogLines('plugin', (s) => ({ label: `${s.crate}/${s.file}`, text: texts[s.file] }));
  assert.deepEqual(lines, [
    'reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none file=rfd/src/backend/macos/message_dialog.rs line=2',
    'reading trial=dialog machine=mac form=plugin system=windows default=reject cancel=allow close=allow file=tauri-plugin-dialog/src/desktop.rs line=3',
  ]);
});

test('the dialog readings of the system form match the crate own source once each', () => {
  const lines = dialogLines('system', (s) => ({
    label: `tools/r4-trials/driver/${s.file}`, text: fs.readFileSync(path.join(crate, s.file), 'utf8'),
  }));
  assert.equal(lines.length, 2);
  assert.match(lines[0], /^reading trial=dialog machine=mac form=system system=macos default=reject cancel=none close=none file=tools\/r4-trials\/driver\/src\/native\.rs line=[1-9][0-9]*$/);
  assert.match(lines[1], /^reading trial=dialog machine=mac form=system system=windows default=reject cancel=reject close=reject file=tools\/r4-trials\/driver\/src\/native\.rs line=[1-9][0-9]*$/);
  assert.deepEqual(Object.keys(DIALOG_SOURCES), ['plugin', 'system']);
});

test('the attribute lines name what each reader reads', () => {
  const lines = attrLines((file) => fs.readFileSync(path.join(crate, file), 'utf8'));
  assert.deepEqual(lines, [
    'reading trial=dialog machine=mac system=macos default_attr=AXDefaultButton cancel_attr=AXCancelButton',
    'reading trial=dialog machine=mac system=windows default_attr=STATE_SYSTEM_DEFAULT cancel_attr=IDCANCEL',
  ]);
  assert.throws(() => attrLines(() => 'nothing'));
});
