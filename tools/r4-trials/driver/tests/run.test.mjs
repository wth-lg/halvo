import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { parseList, parsePressed, parseRead } from '../dialog.mjs';
import {
  ownerFits, ReaderError, readUntilFound, cancelOf, dialogCell, driverCell, engineOf, machineOf, parseArgs, Refused, sessionCounts,
} from '../run.mjs';
import { ELEMENT, elementId } from '../webdriver.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const crate = path.join(here, '..');

function shell(script, args) {
  const env = { ...process.env };
  delete env.CI;
  delete env.R4_GUEST;
  return spawnSync('sh', [path.join(crate, script), ...args], { env, encoding: 'utf8', timeout: 30000 });
}

test('run.sh refuses with not_hosted where neither CI nor R4_GUEST is set', () => {
  for (const args of [['--candidate', 'webdriver'], ['--candidate', 'all'], ['--dialog', 'plugin'], ['--dialog', 'all']]) {
    const done = shell('run.sh', args);
    assert.equal(done.status, 2, JSON.stringify(args));
    assert.equal(done.stdout, '');
    assert.match(done.stderr, /^REFUSED not_hosted: [^\n]+\n$/);
  }
});

test('run.sh and read.sh refuse a bad argument with usage', () => {
  for (const [script, args] of [['run.sh', []], ['run.sh', ['--candidate', 'tauri-driver']], ['run.sh', ['--dialog']],
    ['read.sh', []], ['read.sh', ['--dialog', 'all']], ['read.sh', ['--candidate', 'all']]]) {
    const done = shell(script, args);
    assert.equal(done.status, 2, `${script} ${args.join(' ')}`);
    assert.equal(done.stdout, '');
    assert.match(done.stderr, /^REFUSED usage: [^\n]+\n$/);
  }
});

test('the arguments name candidates or forms in the order of I11.5', () => {
  assert.deepEqual(parseArgs(['--candidate', 'all']), { kind: 'driver', names: ['webdriver', 'wdio'] });
  assert.deepEqual(parseArgs(['--candidate', 'wdio']), { kind: 'driver', names: ['wdio'] });
  assert.deepEqual(parseArgs(['--dialog', 'all']), { kind: 'dialog', forms: ['plugin', 'system'] });
  assert.deepEqual(parseArgs(['--dialog', 'system']), { kind: 'dialog', forms: ['system'] });
  for (const bad of [[], ['--candidate'], ['--dialog', 'none'], ['--candidate', 'webdriver', 'x'], ['--arm', 'a']]) {
    assert.throws(() => parseArgs(bad), (e) => e instanceof Refused && e.cause === 'usage', JSON.stringify(bad));
  }
});

test('the machine is hosted with CI and a guest with R4_GUEST alone', () => {
  assert.equal(machineOf({ CI: 'true' }, 'darwin'), 'hosted-mac');
  assert.equal(machineOf({ CI: 'true' }, 'win32'), 'hosted-win');
  assert.equal(machineOf({ CI: 'true', R4_GUEST: '1' }, 'darwin'), 'hosted-mac');
  assert.equal(machineOf({ R4_GUEST: '1' }, 'darwin'), 'guest-mac');
  for (const env of [{}, { CI: '' }, { R4_GUEST: '' }]) {
    assert.throws(() => machineOf(env, 'darwin'), (e) => e instanceof Refused && e.cause === 'not_hosted');
  }
  assert.throws(() => machineOf({ CI: 'true' }, 'linux'), (e) => e instanceof Refused && e.cause === 'usage');
});

test('the cells hold the keys of I11.5', () => {
  assert.equal(driverCell('hosted-mac', 'webdriver', 5), 'cell trial=driver machine=hosted-mac candidate=webdriver sessions=5 ok=5');
  assert.equal(driverCell('hosted-win', 'wdio', 0), 'cell trial=driver machine=hosted-win candidate=wdio sessions=5 ok=0');
  assert.equal(
    dialogCell('hosted-win', 'system', { text: 'same', default: 'reject', cancel: 'reject', close: 'reject', allow_control: 'allow', reject_control: 'reject' }),
    'cell trial=dialog machine=hosted-win form=system text=same default=reject cancel=reject close=reject allow_control=allow reject_control=reject driven=yes',
  );
  assert.equal(
    dialogCell('hosted-mac', 'plugin', null),
    'cell trial=dialog machine=hosted-mac form=plugin text=- default=- cancel=- close=- allow_control=- reject_control=- driven=no',
  );
});

test('a session counts with the window on screen, 20 rows and the count 1', () => {
  assert.equal(sessionCounts({ onscreen: true, rows: 20, count: '1' }), true);
  assert.equal(sessionCounts({ onscreen: false, rows: 20, count: '1' }), false);
  assert.equal(sessionCounts({ onscreen: true, rows: 19, count: '1' }), false);
  assert.equal(sessionCounts({ onscreen: true, rows: 21, count: '1' }), false);
  assert.equal(sessionCounts({ onscreen: true, rows: 20, count: '0' }), false);
  assert.equal(sessionCounts({ onscreen: true, rows: 20, count: '2' }), false);
  assert.equal(sessionCounts({ onscreen: true, rows: 20, count: null }), false);
});

test('the engine is named from the web view user agent', () => {
  assert.equal(engineOf('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)'), 'wkwebview');
  assert.equal(engineOf('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0'), 'webview2');
  assert.equal(engineOf('Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36'), 'other');
  assert.equal(engineOf(undefined), 'other');
});

test('the cancel control falls back to the close control only where the reader says so', () => {
  const read = (cancel, close) => ({ cancel, close });
  assert.equal(cancelOf(read('via_close', 'yes'), 'allow'), 'allow');
  assert.equal(cancelOf(read('via_close', 'yes'), 'reject'), 'reject');
  assert.equal(cancelOf(read('via_close', 'no'), 'none'), 'none');
  assert.equal(cancelOf(read('none', 'yes'), 'allow'), 'none');
  assert.equal(cancelOf(read('reject', 'yes'), 'allow'), 'reject');
});

test('the dialog reader lines are parsed with their closed words', () => {
  assert.deepEqual(parseRead('dialog found=yes trusted=yes text=same default=reject cancel=none close=no owner=app'), {
    found: 'yes', trusted: 'yes', text: 'same', default: 'reject', cancel: 'none', close: 'no', owner: 'app',
  });
  assert.equal(parseRead('dialog found=no trusted=no text=- default=- cancel=- close=- owner=-').found, 'no');
  assert.equal(parseRead('dialog found=yes trusted=yes text=differ default=other cancel=via_close close=yes owner=other').cancel, 'via_close');
  for (const bad of [undefined, '', 'dialog found=maybe trusted=yes text=same default=reject cancel=none close=no',
    'dialog found=yes trusted=yes text=same default=pay cancel=none close=no',
    'dialog found=yes trusted=yes text=- default=reject cancel=none close=no',
    'dialog found=no trusted=yes text=same default=- cancel=- close=-',
    'dialog found=yes trusted=yes text=same default=reject cancel=none close=no extra=1']) {
    assert.throws(() => parseRead(bad), String(bad));
  }
  assert.equal(parseRead('dialog found=yes trusted=yes text=same default=unread cancel=unread close=unread owner=app').close, 'unread');
  assert.equal(parsePressed('pressed=yes'), true);
  assert.equal(parsePressed('pressed=no'), false);
  assert.throws(() => parsePressed('pressed=maybe'));
});

test('an element reference is read by the W3C key', () => {
  assert.equal(elementId({ [ELEMENT]: 'e1' }), 'e1');
  assert.throws(() => elementId({ ELEMENT: 'e1' }));
  assert.throws(() => elementId(null));
});

test('both readers hold the dialog text of the Rust card', () => {
  const card = fs.readFileSync(path.join(crate, 'src', 'card.rs'), 'utf8');
  const swift = fs.readFileSync(path.join(crate, 'dialog', 'macos.swift'), 'utf8');
  const ps = fs.readFileSync(path.join(crate, 'dialog', 'windows.ps1'), 'utf8');
  for (const text of ['Trial title', "Don't pay", '"Pay"', '"one"', '"two"', '"<b>bold</b> &amp;"', '"five"', 'repeat(200)']) {
    assert.ok(card.includes(text), `card.rs: ${text}`);
  }
  for (const text of ['"Trial title"', `"Don't pay"`, '"Pay"', '"Label 1: one"', '"Label 2: two"', '"Label 3: "', 'count: 200', '"Label 4: <b>bold</b> &amp;"', '"Label 5: five"', 'joined(separator: "\\n")']) {
    assert.ok(swift.includes(text), `macos.swift: ${text}`);
  }
  for (const text of ["'Trial title'", `"Don't pay"`, "'Pay'", "'Label 1: one'", "'Label 2: two'", "('x' * 200)", "'Label 4: <b>bold</b> &amp;'", "'Label 5: five'", '-join "`n"']) {
    assert.ok(ps.includes(text), `windows.ps1: ${text}`);
  }
});

test('the readers pin the attributes and values that name the default, cancel and close controls', () => {
  const swift = fs.readFileSync(path.join(crate, 'dialog', 'macos.swift'), 'utf8');
  const ps = fs.readFileSync(path.join(crate, 'dialog', 'windows.ps1'), 'utf8');
  for (const text of ['let defaultAttr = "AXDefaultButton"', 'let cancelAttr = "AXCancelButton"', 'let closeAttr = "AXCloseButton"',
    'let systemAlerts = "com.apple.UserNotificationCenter"',
    'AXUIElementPerformAction(control, kAXPressAction as CFString)']) {
    assert.ok(swift.includes(text), `macos.swift: ${text}`);
  }
  for (const text of ['$STATE_SYSTEM_DEFAULT = 0x100', 'public const int STATE_SYSTEM_DEFAULT = 0x100;', 'public const int IDCANCEL = 2;',
    'public const int ROLE_SYSTEM_PUSHBUTTON = 0x2B;', 'public const int STATE_SYSTEM_INVISIBLE = 0x8000;', 'public const int STATE_SYSTEM_UNAVAILABLE = 0x1;',
    'const uint OBJID_TITLEBAR = 0xFFFFFFFE;', 'const int TITLEBAR_CLOSE = 5;', 'bar.accDoDefaultAction(TITLEBAR_CLOSE)', 'n.Acc.accDoDefaultAction(n.Child)']) {
    assert.ok(ps.includes(text), `windows.ps1: ${text}`);
  }
});

test('the list line of a reader holds counts and class names alone', () => {
  assert.equal(parseList('list windows=2 buttons=2 reject_named=1 allow_named=1 classes=#32770,Tao_Thread_Event_Target'),
    'list windows=2 buttons=2 reject_named=1 allow_named=1 classes=#32770,Tao_Thread_Event_Target');
  assert.equal(parseList('list windows=1 titled=1 buttons=0 reject_named=0 allow_named=0 classes=#32770').length > 0, true);
  assert.equal(parseList('list trusted=yes windows=0 buttons=0 reject_named=0 allow_named=0 classes=-').length > 0, true);
  for (const bad of [undefined, '', 'list windows=1', "list windows=1 buttons=2 reject_named=1 allow_named=1 classes=Don't pay"]) {
    assert.throws(() => parseList(bad), String(bad));
  }
});

test('a reader that fails is a reader error and no dialog not reached, and a found dialog ends the wait', async () => {
  await assert.rejects(readUntilFound(1, () => { throw new Error('exit 1'); }, 1), (e) => e instanceof ReaderError);
  let calls = 0;
  const read = await readUntilFound(1, () => {
    calls += 1;
    return { found: calls < 3 ? 'no' : 'yes' };
  }, 1);
  assert.equal(read.found, 'yes');
  assert.equal(calls, 3);
});

test('the system form is read in the app process alone; the plugin form may be drawn by the system', () => {
  assert.equal(ownerFits('system', 'app'), true);
  assert.equal(ownerFits('system', 'other'), false);
  assert.equal(ownerFits('plugin', 'app'), true);
  assert.equal(ownerFits('plugin', 'other'), true);
  assert.equal(ownerFits('plugin', '-'), false);
});

test('the macOS reader prints unread where an attribute read fails, and reads each control by its own attribute', () => {
  const swift = fs.readFileSync(path.join(crate, 'dialog', 'macos.swift'), 'utf8');
  for (const text of ['default=\\(name(of: control(window, defaultAttr)))', 'cancel=\\(name(of: control(window, cancelAttr)))',
    'case .noValue, .attributeUnsupported: return .absent', 'default: return .unread', 'case .unread: return "unread"',
    'let closeWord: String', '?? "unread"']) {
    assert.ok(swift.includes(text), `macos.swift: ${text}`);
  }
  const ps = fs.readFileSync(path.join(crate, 'dialog', 'windows.ps1'), 'utf8');
  for (const text of ["if (($rs -band $STATE_SYSTEM_DEFAULT) -ne 0) { $defaultWord = 'reject' } elseif (($as -band $STATE_SYSTEM_DEFAULT) -ne 0) { $defaultWord = 'allow' }",
    "$closeWord = if ($closeState -lt 0) { 'unread' } elseif (($closeState -band $hidden) -eq 0) { 'yes' } else { 'no' }"]) {
    assert.ok(ps.includes(text), `windows.ps1: ${text}`);
  }
});

test('D4: each rule of parseRead refuses a reader line that is whole but for that rule', () => {
  const whole = 'dialog found=yes trusted=yes text=same default=reject cancel=none close=no owner=app';
  assert.equal(parseRead(whole).owner, 'app');
  for (const bad of [
    whole.replace('default=reject', 'default=pay'),
    whole.replace('found=yes', 'found=maybe'),
    whole.replace('owner=app', 'owner=system'),
    whole.replace('close=no', 'close=maybe'),
    `${whole} extra=1`,
    whole.replace('text=same', 'text=-'),
    'dialog found=no trusted=yes text=same default=- cancel=- close=- owner=-',
  ]) {
    assert.throws(() => parseRead(bad), bad);
  }
});

test('D4: an element reference whose id is no string is refused', () => {
  for (const bad of [{ [ELEMENT]: 5 }, { [ELEMENT]: null }, { [ELEMENT]: ['e1'] }, {}]) {
    assert.throws(() => elementId(bad), JSON.stringify(bad));
  }
});
