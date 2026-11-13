import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  addedOnTargets, addedPackages, attrLines, BANNED, BUILT_TARGETS, closure, entryLine, unbuiltEntries, countInputCalls, dialogLines, DIALOG_SOURCES, findLine,
  licenceAllowed, npmTree, outcomeLines, parseArgs, parsePatterns, readingLine, Refused, sourceTexts,
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
    'src/backend/macos/utils/user_alert.rs': 'x\n            default_button_title: buttons[0].take(),\n',
    'src/desktop.rs': 'a\nb\n                    MessageDialogButtons::OkCancelCustom(_ok, cancel),\n                ) => rfd::MessageDialogResult::Custom(cancel),\n',
  };
  const lines = dialogLines('plugin', (s) => ({ label: `${s.crate}/${s.file}`, text: texts[s.file] }));
  assert.deepEqual(lines, [
    'reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none file=rfd/src/backend/macos/utils/user_alert.rs line=2',
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

test('the licences are read on the four built targets of Halvo (rulings P7-52, CP-102)', () => {
  assert.deepEqual(BUILT_TARGETS, ['aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc']);
});

test('the added crates are the union over the built targets, each with the targets that compile it', () => {
  const m = meta();
  const win = meta();
  win.resolve.nodes = win.resolve.nodes.map((n) => (n.id === 'axum-id' ? { ...n, deps: [{ pkg: 'serde-id' }] } : n));
  const added = addedOnTargets({ mac: m, win }, 'cand', 'app');
  assert.deepEqual(added.map((a) => [a.pkg.name, a.targets]).sort(), [
    ['axum', ['mac', 'win']], ['cand', ['mac', 'win']], ['hyper', ['mac']],
  ]);
});

test('an entry outside the list that no built target compiles is named and not counted', () => {
  const pkg = (name, license) => ({ id: `${name}-id`, name, version: '0.13.5', license });
  const all = [pkg('cand', 'MIT'), pkg('target-lexicon', 'Apache-2.0 WITH LLVM-exception'), pkg('gpl', 'GPL-3.0')];
  const built = [{ pkg: all[0], targets: ['x'] }, { pkg: all[2], targets: ['x'] }];
  assert.deepEqual(unbuiltEntries(all, built).map((p) => p.name), ['target-lexicon']);
  assert.equal(entryLine('webdriver', all[1]),
    'reading trial=driver machine=mac candidate=webdriver entry=target-lexicon version=0.13.5 licence=Apache-2.0_WITH_LLVM-exception built_targets=- rulings=P7-52,CP-102');
});

test('the base tree is resolved without the candidate, so a crate that the candidate brings by a feature counts as added', () => {
  const pkg = (name) => ({ id: `${name}-id`, name, version: '1.0.0', license: 'MIT' });
  const graph = (deps) => ({ packages: Object.keys(deps).map(pkg), resolve: { nodes: Object.entries(deps).map(([n, d]) => ({ id: `${n}-id`, deps: d.map((x) => ({ pkg: `${x}-id` })) })) } });
  const withFeature = graph({ app: ['shared'], shared: ['extra'], extra: [], cand: ['shared'] });
  const without = graph({ app: ['shared'], shared: [] });
  assert.deepEqual(addedPackages(withFeature, 'cand', 'app', without).map((p) => p.name).sort(), ['cand', 'extra']);
  assert.deepEqual(addedOnTargets({ t: withFeature }, 'cand', 'app', { t: without }).map((a) => a.pkg.name).sort(), ['cand', 'extra']);
});

test('the pattern file finds the Swift, objc2 and core-graphics method forms and the npm and shell ways of posting input', () => {
  for (const line of ['let e = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true)', 'e?.post(tap: .cghidEventTap)',
    'e.postToPid(pid)', 'NSApp.postEvent(event, atStart: false)', 'CGEvent::post(CGEventTapLocation::HID, Some(&e));',
    'CGEvent::post_to_pid(pid, Some(&e));', 'ev.post_to_pid(pid);', 'CGEvent::new_scroll_wheel_event2(src, unit, 1, 0, 0, 0)',
    "execFileSync('osascript', ['-e', 'tell application \"System Events\" to keystroke \"a\"'])", "spawn('xdotool', ['key', 'Return'])",
    "[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')"]) {
    assert.ok(countInputCalls(patterns, [line], []) >= 1, line);
  }
  assert.equal(countInputCalls(patterns, [], ['robotjs', '@nut-tree/nut-js', '@nut-tree-fork/nut-js']), 3);
  assert.equal(countInputCalls(patterns, ['client.post(url).send()', 'router.post("/session", create)', 'http.post(tap)'], []), 0);
});

test('the Windows reading of the system form rests on a button table that puts the rejecting label on REJECT_ID first', () => {
  const native = fs.readFileSync(path.join(crate, 'src', 'native.rs'), 'utf8');
  assert.match(native, /TASKDIALOG_BUTTON \{\s*nButtonID: card::REJECT_ID,\s*pszButtonText: PCWSTR\(reject\.as_ptr\(\)\),\s*\},\s*TASKDIALOG_BUTTON \{\s*nButtonID: card::ALLOW_ID,\s*pszButtonText: PCWSTR\(allow\.as_ptr\(\)\),/);
  assert.match(native, /let reject = HSTRING::from\(card::REJECT\);\s*let allow = HSTRING::from\(card::ALLOW\);/);
  assert.match(native, /dwFlags: TDF_ALLOW_DIALOG_CANCELLATION \| TDF_SIZE_TO_CONTENT,/);
});

function pinned(dir, file) {
  const base = path.join(process.env.CARGO_HOME ?? path.join(os.homedir(), '.cargo'), 'registry', 'src');
  for (const index of fs.existsSync(base) ? fs.readdirSync(base) : []) {
    const p = path.join(base, index, dir, file);
    if (fs.existsSync(p)) return fs.readFileSync(p, 'utf8');
  }
  return null;
}

const OWN = { card: fs.readFileSync(path.join(crate, 'src', 'card.rs'), 'utf8'), native: fs.readFileSync(path.join(crate, 'src', 'native.rs'), 'utf8') };
const PINNED = {
  rfdMac: pinned('rfd-0.17.2', 'src/backend/macos/utils/user_alert.rs'),
  rfdWin: pinned('rfd-0.17.2', 'src/backend/win_cid/message_dialog.rs'),
  plugin: pinned('tauri-plugin-dialog-2.8.4', 'src/desktop.rs'),
};
const NOT_PINNED = Object.values(PINNED).some((t) => t === null) && 'the registry holds no rfd 0.17.2 or tauri-plugin-dialog 2.8.4 yet';

function sources(texts) {
  return ({ crate: c, file }) => {
    if (c === null) return { label: `tools/r4-trials/driver/${file}`, text: file === 'src/card.rs' ? (texts.card ?? OWN.card) : (texts.native ?? OWN.native) };
    const key = c === 'tauri-plugin-dialog' ? 'plugin' : file.includes('macos') ? 'rfdMac' : 'rfdWin';
    return { label: `${c === 'rfd' ? 'rfd-0.17.2' : 'tauri-plugin-dialog-2.8.4'}/${file}`, text: texts[key] };
  };
}

const SYN = {
  rfdMac: ['            timeout: 0_f64,', '        if is_cancel != 0 {', '            return MessageDialogResult::Cancel;', '        }',
    '        if response == kCFUserNotificationCancelResponse {', '            return MessageDialogResult::Cancel;', '        }',
    '            _ => MessageDialogResult::Cancel,', ''].join('\n'),
  rfdWin: ['            pfCallback: None,', '        if ret != 0 {', '            return MessageDialogResult::Cancel;', '        }',
    '        match pn_button {', '            IDCANCEL => MessageDialogResult::Cancel,', '                MessageButtons::OkCancelCustom(ok_text, cancel_text) => match custom {',
    '                    ID_CUSTOM_OK => MessageDialogResult::Custom(ok_text),', '                    _ => MessageDialogResult::Cancel,', ''].join('\n'),
  plugin: ['                (', '                    rfd::MessageDialogResult::Ok,', '                    MessageDialogButtons::OkCancelCustom(ok, _cancel),',
    '                ) => rfd::MessageDialogResult::Custom(ok),', '                (', '                    rfd::MessageDialogResult::Cancel,',
    '                    MessageDialogButtons::OkCancelCustom(_ok, cancel),', '                ) => rfd::MessageDialogResult::Custom(cancel),',
    '                (result, _) => result,', ''].join('\n'),
};
const gives = (lines) => lines.map((l) => /outcome=(\w+) gives=(\w+)/.exec(l).slice(1).join('=')).join(' ');
const swap = (text, from, to) => {
  assert.equal(text.split(from).length, 2, from);
  return text.replace(from, to);
};

test('D1: the plugin on macOS finds the three mappings to the allowing label at the pinned versions', { skip: NOT_PINNED }, () => {
  const lines = outcomeLines('plugin', sources(PINNED));
  assert.deepEqual(lines, [
    'reading trial=dialog machine=mac form=plugin system=macos outcome=failed_call gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238',
    'reading trial=dialog machine=mac form=plugin system=macos outcome=cancel_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238',
    'reading trial=dialog machine=mac form=plugin system=macos outcome=unknown_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238',
    'reading trial=dialog machine=mac form=plugin system=macos outcome=timeout gives=none file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs line=60',
    'reading trial=dialog machine=mac form=plugin system=windows outcome=failed_call gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238',
    'reading trial=dialog machine=mac form=plugin system=windows outcome=cancel_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238',
    'reading trial=dialog machine=mac form=plugin system=windows outcome=unknown_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238',
    'reading trial=dialog machine=mac form=plugin system=windows outcome=timeout gives=none file=rfd-0.17.2/src/backend/win_cid/message_dialog.rs line=153',
  ]);
  const rfd = PINNED.rfdMac.split('\n');
  assert.match(rfd[107], /if is_cancel != 0 \{/);
  assert.match(rfd[112], /return MessageDialogResult::Cancel;/);
  assert.match(rfd[167], /_ => MessageDialogResult::Cancel,/);
});

test('D1: the sources give the same words in the short form that the other tests change', () => {
  assert.equal(gives(outcomeLines('plugin', sources(SYN))),
    'failed_call=allow cancel_response=allow unknown_response=allow timeout=none failed_call=allow cancel_response=allow unknown_response=allow timeout=none');
});

test('D1: a source in which a mapping differs gives the other word', () => {
  const failedOk = { ...SYN, rfdMac: swap(SYN.rfdMac, 'if is_cancel != 0 {\n            return MessageDialogResult::Cancel;', 'if is_cancel != 0 {\n            return MessageDialogResult::Ok;') };
  assert.match(gives(outcomeLines('plugin', sources(failedOk))), /^failed_call=reject cancel_response=allow unknown_response=allow timeout=none /);
  const armFirst = { ...SYN, plugin: swap(SYN.plugin, 'OkCancelCustom(_ok, cancel),\n                ) => rfd::MessageDialogResult::Custom(cancel)', 'OkCancelCustom(ok, _cancel),\n                ) => rfd::MessageDialogResult::Custom(ok)') };
  assert.match(gives(outcomeLines('plugin', sources(armFirst))), /^failed_call=reject cancel_response=reject unknown_response=reject timeout=none /);
  const armOther = { ...SYN, plugin: swap(SYN.plugin, 'OkCancelCustom(_ok, cancel),\n                ) => rfd::MessageDialogResult::Custom(cancel)', 'OkCancelCustom(cancel, _ok),\n                ) => rfd::MessageDialogResult::Custom(cancel)') };
  assert.match(gives(outcomeLines('plugin', sources(armOther))), /^failed_call=reject cancel_response=reject unknown_response=reject /);
  const noArm = { ...SYN, plugin: swap(SYN.plugin, '                (\n                    rfd::MessageDialogResult::Cancel,\n                    MessageDialogButtons::OkCancelCustom(_ok, cancel),\n                ) => rfd::MessageDialogResult::Custom(cancel),\n', '') };
  assert.match(gives(outcomeLines('plugin', sources(noArm))), /^failed_call=reject cancel_response=reject unknown_response=reject /);
  const slots = { ...SYN, card: swap(OWN.card, '    (REJECT, ALLOW)\n', '    (ALLOW, REJECT)\n') };
  assert.match(gives(outcomeLines('plugin', sources(slots))), /^failed_call=reject cancel_response=reject unknown_response=reject /);
  const yes = { ...SYN, rfdMac: swap(SYN.rfdMac, '_ => MessageDialogResult::Cancel,', '_ => MessageDialogResult::Yes,') };
  assert.match(gives(outcomeLines('plugin', sources(yes))), /^failed_call=allow cancel_response=allow unknown_response=reject /);
  const weird = { ...SYN, rfdMac: swap(SYN.rfdMac, '_ => MessageDialogResult::Cancel,', '_ => MessageDialogResult::Other,') };
  assert.match(gives(outcomeLines('plugin', sources(weird))), /unknown_response=unknown timeout=none failed_call/);
  const timed = { ...SYN, rfdMac: swap(SYN.rfdMac, 'timeout: 0_f64,', 'timeout: 30_f64,') };
  assert.match(gives(outcomeLines('plugin', sources(timed))), /^failed_call=allow cancel_response=allow unknown_response=allow timeout=unknown /);
  const callback = { ...SYN, rfdWin: swap(SYN.rfdWin, 'pfCallback: None,', 'pfCallback: Some(timer),') };
  assert.match(gives(outcomeLines('plugin', sources(callback))), /timeout=unknown$/);
  const fromPlugin = { ...SYN, card: swap(OWN.card, '        _ => Answer::Reject,\n    }\n}\n\n/// The response', '        _ => Answer::Allow,\n    }\n}\n\n/// The response') };
  assert.match(gives(outcomeLines('plugin', sources({ ...noArm, card: fromPlugin.card }))), /^failed_call=allow cancel_response=allow unknown_response=allow /);
});

test('D1: a locating pattern that matches no line, or two, refuses', () => {
  assert.throws(() => outcomeLines('plugin', sources({ ...SYN, rfdMac: swap(SYN.rfdMac, '        if is_cancel != 0 {\n', '') })), /0 lines match/);
  assert.throws(() => outcomeLines('plugin', sources({ ...SYN, rfdMac: `${SYN.rfdMac}        if is_cancel != 0 {\n` })), /2 lines match/);
  assert.throws(() => outcomeLines('plugin', sources({ ...SYN, rfdWin: swap(SYN.rfdWin, '        match pn_button {\n', '') })), /0 lines match/);
  assert.throws(() => outcomeLines('plugin', sources({ ...SYN, plugin: `${SYN.plugin}                (result, _) => result,\n`, rfdMac: swap(SYN.rfdMac, '_ => MessageDialogResult::Cancel,', '_ => MessageDialogResult::No,') })), /2 lines match/);
  assert.throws(() => outcomeLines('system', sources({ native: swap(OWN.native, '        _ => Answer::Reject,\n', '') })), /0 lines match/);
});

test('D1: the system form rejects on every outcome by the app own mappings, and a changed mapping gives the other word', () => {
  assert.deepEqual(outcomeLines('system', sources({})), [
    'reading trial=dialog machine=mac form=system system=macos outcome=failed_call gives=reject file=tools/r4-trials/driver/src/native.rs line=80',
    'reading trial=dialog machine=mac form=system system=macos outcome=cancel_response gives=reject file=tools/r4-trials/driver/src/card.rs line=95',
    'reading trial=dialog machine=mac form=system system=macos outcome=unknown_response gives=reject file=tools/r4-trials/driver/src/card.rs line=95',
    'reading trial=dialog machine=mac form=system system=macos outcome=timeout gives=none file=tools/r4-trials/driver/src/native.rs line=78',
    'reading trial=dialog machine=mac form=system system=windows outcome=failed_call gives=reject file=tools/r4-trials/driver/src/card.rs line=104',
    'reading trial=dialog machine=mac form=system system=windows outcome=cancel_response gives=reject file=tools/r4-trials/driver/src/card.rs line=104',
    'reading trial=dialog machine=mac form=system system=windows outcome=unknown_response gives=reject file=tools/r4-trials/driver/src/card.rs line=104',
    'reading trial=dialog machine=mac form=system system=windows outcome=timeout gives=none file=tools/r4-trials/driver/src/native.rs line=108',
  ]);
  const alertElse = swap(OWN.card, '        Answer::Allow\n    } else {\n        Answer::Reject\n    }\n}\n\n/// The result', '        Answer::Allow\n    } else {\n        Answer::Allow\n    }\n}\n\n/// The result');
  assert.match(gives(outcomeLines('system', sources({ card: alertElse }))), /^failed_call=reject cancel_response=allow unknown_response=allow timeout=none /);
  assert.match(gives(outcomeLines('system', sources({ native: swap(OWN.native, '        _ => Answer::Reject,', '        _ => Answer::Allow,') }))), /^failed_call=allow /);
  assert.match(gives(outcomeLines('system', sources({ native: swap(OWN.native, 'match (queued, rx.recv()) {', 'match (queued, rx.recv_timeout(WAIT)) {') }))), /^failed_call=reject cancel_response=reject unknown_response=reject timeout=unknown /);
  assert.match(gives(outcomeLines('system', sources({ card: swap(OWN.card, 'if shown && button == ALLOW_ID {', 'if button == ALLOW_ID {') }))), /failed_call=unknown cancel_response=reject unknown_response=reject timeout=none$/);
  assert.match(gives(outcomeLines('system', sources({ card: swap(OWN.card, 'pub const ALLOW_ID: i32 = 100;', 'pub const ALLOW_ID: i32 = 2;') }))), /failed_call=reject cancel_response=unknown unknown_response=reject timeout=none$/);
  assert.match(gives(outcomeLines('system', sources({ native: swap(OWN.native, 'dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,', 'dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER,') }))), /timeout=unknown$/);
});

test('D4: a licence with an opening parenthesis that does not close is no licence', () => {
  for (const bad of ['(MIT Apache-2.0', '(MIT GPL-3.0', '(MIT OR Apache-2.0', '(MIT OR Apache-2.0 Zlib']) {
    assert.equal(licenceAllowed(bad), false, bad);
  }
  assert.equal(licenceAllowed('(MIT OR Apache-2.0)'), true);
});

test('D4: the Rust files of a source are read for input calls', () => {
  const texts = sourceTexts(path.join(crate, 'src'));
  assert.ok(texts.some((t) => t.includes('pub fn builder(')), 'lib.rs');
  assert.ok(texts.some((t) => t.includes('TaskDialogIndirect(')), 'native.rs');
  assert.equal(countInputCalls(patterns, sourceTexts(path.join(crate, 'src')), []), 0);
});
