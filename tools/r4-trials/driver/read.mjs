// The reading lines of the driver and native dialog trial (contracts.md I11.5), on this Mac.
// usage: node read.mjs --candidate webdriver|wdio | --dialog plugin|system | --attrs
// It builds the trial crate with a candidate, reads the lock and the sources, and starts no app.
import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

export const CANDIDATES = {
  webdriver: { crate: 'tauri-plugin-webdriver', feature: 'webdriver', npm: null },
  wdio: { crate: 'tauri-plugin-wdio-webdriver', feature: 'wdio', npm: '@wdio/tauri-service' },
};

export const ALLOWED = ['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'Unicode-3.0'];

export const BUILT_TARGETS = ['aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc'];

export const RULINGS = 'P7-52,CP-102';

export const BANNED = ['reqwest', 'hyper', 'ureq', 'rustls', 'ring', 'aws-lc-rs', 'native-tls', 'openssl-sys'];

const SOURCE_EXTENSIONS = new Set(['.rs', '.js', '.mjs', '.cjs', '.ts', '.swift', '.kt', '.java', '.m', '.mm', '.c', '.h']);

export class Refused extends Error {
  constructor(cause, detail) {
    super(`REFUSED ${cause}: ${detail}`);
    this.cause = cause;
  }
}

export function parseArgs(argv) {
  if (argv.length === 1 && argv[0] === '--attrs') return { mode: 'attrs' };
  if (argv.length !== 2) throw new Refused('usage', 'give --candidate webdriver|wdio, --dialog plugin|system or --attrs');
  const [flag, value] = argv;
  if (flag === '--candidate' && Object.hasOwn(CANDIDATES, value)) return { mode: 'candidate', name: value };
  if (flag === '--dialog' && Object.hasOwn(DIALOG_SOURCES, value)) return { mode: 'dialog', form: value };
  throw new Refused('usage', 'give --candidate webdriver|wdio, --dialog plugin|system or --attrs');
}

export function parsePatterns(text) {
  const calls = [];
  const crates = [];
  for (const raw of text.split('\n')) {
    const line = raw.replace(/\r$/, '');
    if (line === '' || line.startsWith('#')) continue;
    const found = /^(call|crate) (\S.*)$/.exec(line);
    if (!found) throw new Error(`input-patterns.txt: a line is neither call nor crate: ${JSON.stringify(line)}`);
    if (found[1] === 'call') calls.push(new RegExp(found[2], 'gu'));
    else crates.push(found[2]);
  }
  if (calls.length === 0 || crates.length === 0) throw new Error('input-patterns.txt: no call or no crate line');
  return { calls, crates };
}

export function countInputCalls(patterns, texts, names) {
  let n = 0;
  for (const text of texts) {
    for (const line of text.split('\n')) {
      for (const re of patterns.calls) {
        re.lastIndex = 0;
        n += [...line.matchAll(re)].length;
      }
    }
  }
  for (const name of names) {
    if (patterns.crates.includes(name)) n += 1;
  }
  return n;
}

function tokenize(expr) {
  return expr.replace(/\(/g, ' ( ').replace(/\)/g, ' ) ').split(/\s+/).filter(Boolean);
}

export function licenceAllowed(expr, allowed = ALLOWED) {
  if (typeof expr !== 'string' || expr.trim() === '') return false;
  const tokens = tokenize(expr.replace(/\//g, ' OR '));
  let at = 0;
  function primary() {
    const token = tokens[at++];
    if (token === undefined) throw new Error('end');
    if (token === '(') {
      const value = orExpr();
      if (tokens[at++] !== ')') throw new Error('paren');
      return value;
    }
    if (['AND', 'OR', 'WITH', ')'].includes(token)) throw new Error('operator');
    if (tokens[at] === 'WITH') {
      at += 2;
      return false;
    }
    return allowed.includes(token);
  }
  function andExpr() {
    let value = primary();
    while (tokens[at] === 'AND') {
      at += 1;
      value = primary() && value;
    }
    return value;
  }
  function orExpr() {
    let value = andExpr();
    while (tokens[at] === 'OR') {
      at += 1;
      value = andExpr() || value;
    }
    return value;
  }
  try {
    const value = orExpr();
    return at === tokens.length && value;
  } catch {
    return false;
  }
}

export function closure(metadata, rootName) {
  const byId = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const roots = metadata.packages.filter((p) => p.name === rootName);
  if (roots.length !== 1) throw new Error(`the metadata holds ${roots.length} packages named ${rootName}`);
  const seen = new Set();
  const stack = [roots[0].id];
  while (stack.length > 0) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) stack.push(dep.pkg);
  }
  return [...seen].map((id) => byId.get(id));
}

export function addedPackages(metadata, candidateCrate, baseCrate, baseMetadata = metadata) {
  const base = new Set(closure(baseMetadata, baseCrate).map((p) => p.id));
  return closure(metadata, candidateCrate).filter((p) => !base.has(p.id));
}

export function addedOnTargets(metaByTarget, candidateCrate, baseCrate, baseByTarget = metaByTarget) {
  const byId = new Map();
  for (const [target, meta] of Object.entries(metaByTarget)) {
    for (const pkg of addedPackages(meta, candidateCrate, baseCrate, baseByTarget[target])) {
      if (!byId.has(pkg.id)) byId.set(pkg.id, { pkg, targets: [] });
      byId.get(pkg.id).targets.push(target);
    }
  }
  return [...byId.values()];
}

export function unbuiltEntries(allTargets, built) {
  const compiled = new Set(built.map((a) => a.pkg.id));
  return allTargets.filter((p) => !compiled.has(p.id) && !licenceAllowed(p.license));
}

export function entryLine(name, pkg) {
  return `reading trial=driver machine=mac candidate=${name} entry=${pkg.name} version=${pkg.version} licence=${String(pkg.license).replace(/\s+/g, '_')} built_targets=- rulings=${RULINGS}`;
}

export function npmTree(lock, rootPackage) {
  const packages = lock.packages ?? {};
  const out = [];
  for (const [key, entry] of Object.entries(packages)) {
    if (key === '') continue;
    const name = key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length);
    out.push({ name, version: entry.version, license: entry.license ?? null });
  }
  if (!out.some((p) => p.name === rootPackage)) throw new Error(`the lock holds no ${rootPackage}`);
  return out;
}

export function sourceTexts(dir) {
  const texts = [];
  const walk = (at) => {
    for (const entry of fs.readdirSync(at, { withFileTypes: true })) {
      const full = path.join(at, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== 'node_modules' && entry.name !== 'target') walk(full);
      } else if (entry.isFile() && SOURCE_EXTENSIONS.has(path.extname(entry.name))) {
        texts.push(fs.readFileSync(full, 'utf8'));
      }
    }
  };
  walk(dir);
  return texts;
}

export function readingLine({ name, version, build, licences, inputCalls, banned }) {
  return `reading trial=driver machine=mac candidate=${name} version=${version} build=${build} licences=${licences ? 'ok' : 'miss'} input_calls=${inputCalls} banned=${banned}`;
}

export const DIALOG_KEYS = ['default', 'cancel', 'close'];

const CANCELLABLE = /^\s*dwFlags: TDF_ALLOW_DIALOG_CANCELLATION \| TDF_SIZE_TO_CONTENT,$/;
const PLUGIN_CALL = [
  { source: 'card', find: /^\s*\(REJECT, ALLOW\)$/ },
  { source: 'card', find: /^\s*MessageDialogResult::Custom\(label\) if label == ALLOW => Answer::Allow,$/ },
  { source: 'native', find: /^\s*let \(first, second\) = card::plugin_slots\(\);$/ },
  { source: 'native', after: /^\s*\.buttons\(MessageDialogButtons::OkCancelCustom\($/, find: /^\s*first\.to_string\(\),$/ },
];
const CF_ALERT = { source: 'rfdMac', find: /^\s*CFUserNotificationDisplayAlert\($/ };
const NS_ALERT = { source: 'native', find: /^\s*let alert = NSAlert::new\(mtm\);$/ };
const PLUGIN_CANCEL = { source: 'plugin', find: /^\s*MessageDialogButtons::OkCancelCustom\(_ok, cancel\),$/ };
const TASK_ELSE = { source: 'card', find: /^\s*if shown && button == ALLOW_ID \{$/ };
const ELSE_REJECT = [/^\s*Answer::Allow$/, /^\s*\} else \{$/, /^\s*Answer::Reject$/];
const PLUGIN_PASS = [
  { source: 'plugin', find: /^\s*MessageDialogButtons::OkCancelCustom\(ok, cancel\) => Self::OkCancelCustom\(ok, cancel\),$/ },
  { source: 'card', seq: [PLUGIN_CALL[1].find, /^\s*_ => Answer::Reject,$/] },
];

/** For each form and system: the value of each key and the one source line that decides it, and the required lines, orders of lines and absence that the review and the verifier named in the read sources. The reading refuses when a cited or a required text changes; it reads text and does not follow the code, so a change of another line is not seen. */
export const DIALOG_SOURCES = {
  plugin: [
    {
      system: 'macos',
      requires: [
        ...PLUGIN_CALL,
        ...PLUGIN_PASS,
        { source: 'native', find: /\.parent\(/, absent: true },
        { source: 'rfdMac', after: /^\s*MessageButtons::OkCancelCustom\(ok_text, cancel_text\) => \[$/, find: /^\s*Some\(ok_text\.to_string\(\)\),$/ },
        { source: 'rfdMac', find: /^\s*MessageButtons::OkCancelCustom\(custom, _\)$/ },
        { source: 'rfdMac', seq: [/^\s*MessageButtons::OkCancelCustom\(custom, _\)$/, /^\s*if response == kCFUserNotificationDefaultResponse =>$/] },
        { source: 'rfdMac', seq: [/^\s*default_button_title\.as_deref\(\),$/, /^\s*alternate_button_title\.as_deref\(\),$/] },
      ],
      keys: {
        default: { value: 'reject', source: 'rfdMac', find: /^\s*default_button_title: buttons\[0\]\.take\(\),$/ },
        cancel: { value: 'none', ...CF_ALERT },
        close: { value: 'none', ...CF_ALERT },
      },
    },
    {
      system: 'windows',
      requires: [
        ...PLUGIN_CALL,
        { source: 'rfdWin', after: /^\s*\(ID_CUSTOM_OK, str_to_vec_u16\(ok_text\)\),$/, find: /^\s*\(ID_CUSTOM_CANCEL, str_to_vec_u16\(cancel_text\)\),$/ },
        { source: 'rfdWin', find: CANCELLABLE },
        { source: 'rfdWin', after: /^\s*MessageButtons::OkCancelCustom\(ok_text, cancel_text\) => match custom \{$/, find: /^\s*ID_CUSTOM_OK => MessageDialogResult::Custom\(ok_text\),$/ },
        ...PLUGIN_PASS,
        { source: 'plugin', seq: [/^\s*rfd::MessageDialogResult::Cancel,$/, PLUGIN_CANCEL.find] },
        { source: 'plugin', seq: [PLUGIN_CANCEL.find, /^\s*\) => rfd::MessageDialogResult::Custom\(cancel\),$/] },
        { source: 'rfdWin', seq: [/^\s*match pn_button \{$/, /^\s*IDOK => MessageDialogResult::Ok,$/, /^\s*IDYES => MessageDialogResult::Yes,$/, /^\s*IDCANCEL => MessageDialogResult::Cancel,$/] },
        { source: 'native', seq: [/^\s*first\.to_string\(\),$/, /^\s*second\.to_string\(\),$/] },
      ],
      keys: {
        default: { value: 'reject', source: 'rfdWin', find: /^\s*nDefaultButton: 0,$/ },
        cancel: { value: 'allow', ...PLUGIN_CANCEL },
        close: { value: 'allow', ...PLUGIN_CANCEL },
      },
    },
  ],
  system: [
    {
      system: 'macos',
      requires: [
        { source: 'card', find: /^pub const REJECT: &str = "Don't pay";$/ },
        { source: 'card', find: /^pub const ALLOW: &str = "Pay";$/ },
        { source: 'card', find: /^pub const ALERT_SECOND: isize = 1001;$/ },
        { source: 'card', find: /^\s*if response == ALERT_SECOND \{$/ },
        { source: 'card', seq: [/^\s*if response == ALERT_SECOND \{$/, ...ELSE_REJECT] },
        { source: 'native', seq: [/^\s*alert\.addButtonWithTitle\(&NSString::from_str\(card::REJECT\)\);$/, /^\s*alert\.addButtonWithTitle\(&NSString::from_str\(card::ALLOW\)\);$/] },
      ],
      keys: {
        default: { value: 'reject', source: 'native', find: /^\s*alert\.addButtonWithTitle\(&NSString::from_str\(card::REJECT\)\);$/ },
        cancel: { value: 'none', ...NS_ALERT },
        close: { value: 'none', ...NS_ALERT },
      },
    },
    {
      system: 'windows',
      requires: [
        { source: 'card', find: /^pub const REJECT_ID: i32 = 101;$/ },
        { source: 'card', find: /^pub const ALLOW_ID: i32 = 100;$/ },
        { source: 'native', find: CANCELLABLE },
        { source: 'native', find: /^\s*card::from_task_dialog\(shown, pressed\)$/ },
        { source: 'card', seq: [TASK_ELSE.find, ...ELSE_REJECT] },
        {
          source: 'native',
          seq: [/^\s*nButtonID: card::REJECT_ID,$/, /^\s*pszButtonText: PCWSTR\(reject\.as_ptr\(\)\),$/, /^\s*\},$/,
            /^\s*TASKDIALOG_BUTTON \{$/, /^\s*nButtonID: card::ALLOW_ID,$/, /^\s*pszButtonText: PCWSTR\(allow\.as_ptr\(\)\),$/],
        },
      ],
      keys: {
        default: { value: 'reject', source: 'native', find: /^\s*nDefaultButton: card::REJECT_ID,$/ },
        cancel: { value: 'reject', ...TASK_ELSE },
        close: { value: 'reject', ...TASK_ELSE },
      },
    },
  ],
};

export const ATTRS = [
  { system: 'macos', file: 'dialog/macos.swift', default: 'AXDefaultButton', cancel: 'AXCancelButton' },
  { system: 'windows', file: 'dialog/windows.ps1', default: 'STATE_SYSTEM_DEFAULT', cancel: 'IDCANCEL' },
];

export function findLine(text, re) {
  const lines = text.split('\n');
  const hits = [];
  lines.forEach((line, i) => {
    if (re.test(line.replace(/\r$/, ''))) hits.push(i + 1);
  });
  if (hits.length !== 1) throw new Error(`${hits.length} lines match ${re}`);
  return hits[0];
}

function runs(text, seq) {
  const lines = textLines(text);
  let n = 0;
  lines.forEach((line, i) => {
    if (!seq[0].test(line)) return;
    let at = i;
    for (const re of seq.slice(1)) {
      do at += 1; while (at < lines.length && lines[at].trim() === '');
      if (at >= lines.length || !re.test(lines[at])) return;
    }
    n += 1;
  });
  return n;
}

function holds(text, r) {
  if (r.absent) {
    const n = textLines(text).filter((line) => r.find.test(line)).length;
    if (n !== 0) throw new Error(`${n} lines match ${r.find}`);
  } else if (r.seq) {
    const n = runs(text, r.seq);
    if (n !== 1) throw new Error(`${n} lines match ${r.seq[0]} with ${r.seq.slice(1).join(' then ')} after them`);
  } else if (r.after) nextAfter(text, findLine(text, r.after), r.find, String(r.after));
  else findLine(text, r.find);
}

function named(kind, label, check) {
  try {
    return check();
  } catch (e) {
    throw new Error(`${kind} ${label}: ${e.message}`);
  }
}

/** The reading line of each system of `form`: each key's value, then the file and line that decide each key. Throws, naming the file, when a cited or a required line does not match exactly once, a required order of lines is not found exactly once, or a required line does not follow its anchor. */
export function dialogLines(form, readSource) {
  return DIALOG_SOURCES[form].map((s) => {
    for (const r of s.requires) {
      const { label, text } = readSource(SOURCES[r.source]);
      named('required', label, () => holds(text, r));
    }
    const values = DIALOG_KEYS.map((key) => `${key}=${s.keys[key].value}`);
    const places = DIALOG_KEYS.flatMap((key) => {
      const { label, text } = readSource(SOURCES[s.keys[key].source]);
      return [`${key}_file=${label}`, `${key}_line=${named('cited', label, () => findLine(text, s.keys[key].find))}`];
    });
    return `reading trial=dialog machine=mac form=${form} system=${s.system} ${values.join(' ')} ${places.join(' ')}`;
  });
}

export const OUTCOMES = ['failed_call', 'cancel_response', 'unknown_response', 'timeout'];

export const SOURCES = {
  rfdMac: { crate: 'rfd', file: 'src/backend/macos/utils/user_alert.rs' },
  rfdWin: { crate: 'rfd', file: 'src/backend/win_cid/message_dialog.rs' },
  plugin: { crate: 'tauri-plugin-dialog', file: 'src/desktop.rs' },
  card: { crate: null, file: 'src/card.rs' },
  native: { crate: null, file: 'src/native.rs' },
};

function textLines(text) {
  return text.split('\n').map((line) => line.replace(/\r$/, ''));
}

function at(text, re) {
  const n = findLine(text, re);
  return { n, line: textLines(text)[n - 1], match: re.exec(textLines(text)[n - 1]) };
}

function nextAfter(text, n, re, what) {
  const lines = textLines(text);
  for (let i = n; i < lines.length; i += 1) {
    if (lines[i].trim() === '') continue;
    const match = re.exec(lines[i]);
    if (!match) throw new Error(`the line after ${what} is not ${re}`);
    return { n: i + 1, match };
  }
  throw new Error(`no line after ${what}`);
}

function firstAfter(text, anchorRe, re) {
  const start = findLine(text, anchorRe);
  const lines = textLines(text);
  for (let i = start; i < lines.length; i += 1) {
    const match = re.exec(lines[i]);
    if (match) return { n: i + 1, match };
  }
  throw new Error(`no line matches ${re} after ${anchorRe}`);
}

function word(answer) {
  return answer === 'Allow' ? 'allow' : answer === 'Reject' ? 'reject' : 'unknown';
}

function pluginAnswer(src, variant) {
  const card = src('card').text;
  const slots = at(card, /^\s*\((REJECT|ALLOW), (REJECT|ALLOW)\)$/).match;
  const allows = at(card, /^\s*MessageDialogResult::Custom\(label\) if label == (REJECT|ALLOW) => Answer::(Allow|Reject),$/);
  const rest = nextAfter(card, allows.n, /^\s*_ => Answer::(Allow|Reject),$/, 'the custom arm of from_plugin').match[1];
  const app = (label) => word(label === allows.match[1] ? allows.match[2] : rest);
  const plugin = src('plugin');
  if (variant === 'Cancel' || variant === 'Ok') {
    const arm = new RegExp(`\\(\\s*rfd::MessageDialogResult::${variant},\\s*MessageDialogButtons::OkCancelCustom\\((\\w+), (\\w+)\\),\\s*\\) => rfd::MessageDialogResult::Custom\\((\\w+)\\),`, 'g');
    const arms = [...plugin.text.matchAll(arm)];
    if (arms.length > 1) throw new Error(`${arms.length} arms of the plugin map ${variant}`);
    if (arms.length === 1) {
      const [whole, first, second, picked] = arms[0];
      const n = plugin.text.slice(0, arms[0].index + whole.indexOf('MessageDialogButtons')).split('\n').length;
      const slot = picked === first ? slots[1] : picked === second ? slots[2] : null;
      return { gives: slot === null ? 'unknown' : app(slot), label: plugin.label, n };
    }
  }
  const through = at(plugin.text, /^\s*\(result, _\) => result,$/);
  return { gives: ['Yes', 'No', 'Cancel', 'Ok'].includes(variant) ? app(null) : 'unknown', label: plugin.label, n: through.n };
}

function rfdVariant(text, re, what, nextLine) {
  const found = at(text, re);
  if (!nextLine) return found.match[1];
  return nextAfter(text, found.n, /^\s*return MessageDialogResult::(\w+);$/, what).match[1];
}

function elseOf(text, re, what) {
  const found = at(text, re);
  const branch = nextAfter(text, found.n, /^\s*Answer::(Allow|Reject)$/, what);
  nextAfter(text, branch.n, /^\s*\} else \{$/, `${what}, its first branch`);
  const other = nextAfter(text, branch.n + 1, /^\s*Answer::(Allow|Reject)$/, `${what}, its else`);
  return { found, first: branch.match[1], other: other.match[1] };
}

const DERIVE = {
  plugin: {
    macos(src) {
      const rfd = src('rfdMac');
      const timeout = at(rfd.text, /^\s*timeout: ([0-9][0-9_a-z.]*),$/);
      const seconds = Number(timeout.match[1].replace(/_f64$/, '').replace(/_/g, ''));
      return {
        failed_call: pluginAnswer(src, rfdVariant(rfd.text, /^\s*if is_cancel != 0 \{$/, 'the failed call', true)),
        cancel_response: pluginAnswer(src, rfdVariant(rfd.text, /^\s*if response == kCFUserNotificationCancelResponse \{$/, 'the cancel response', true)),
        unknown_response: pluginAnswer(src, rfdVariant(rfd.text, /^\s*_ => MessageDialogResult::(\w+),$/, 'the unlisted response', false)),
        timeout: { gives: seconds === 0 ? 'none' : 'unknown', label: rfd.label, n: timeout.n },
      };
    },
    windows(src) {
      const rfd = src('rfdWin');
      const callback = at(rfd.text, /^\s*pfCallback: (\S+),$/);
      const cancel = firstAfter(rfd.text, /^\s*match pn_button \{$/, /^\s*IDCANCEL => MessageDialogResult::(\w+),$/);
      const unlisted = firstAfter(rfd.text, /^\s*MessageButtons::OkCancelCustom\(ok_text, cancel_text\) => match custom \{$/, /^\s*_ => MessageDialogResult::(\w+),$/);
      return {
        failed_call: pluginAnswer(src, rfdVariant(rfd.text, /^\s*if ret != 0 \{$/, 'the failed call', true)),
        cancel_response: pluginAnswer(src, cancel.match[1]),
        unknown_response: pluginAnswer(src, unlisted.match[1]),
        timeout: { gives: callback.match[1] === 'None' ? 'none' : 'unknown', label: rfd.label, n: callback.n },
      };
    },
  },
  system: {
    macos(src) {
      const native = src('native');
      const card = src('card');
      const failed = at(native.text, /^\s*_ => Answer::(Allow|Reject),$/);
      at(native.text, /^\s*let _ = tx\.send\(Some\(alert\.runModal\(\)\)\);$/);
      const wait = at(native.text, /^\s*match \(queued, rx\.(\w+)\(/);
      const alert = elseOf(card.text, /^\s*if response == (ALERT_FIRST|ALERT_SECOND) \{$/, 'from_alert');
      const other = { gives: word(alert.other), label: card.label, n: alert.found.n };
      return {
        failed_call: { gives: word(failed.match[1]), label: native.label, n: failed.n },
        cancel_response: other,
        unknown_response: other,
        timeout: { gives: wait.match[1] === 'recv' ? 'none' : 'unknown', label: native.label, n: wait.n },
      };
    },
    windows(src) {
      const native = src('native');
      const card = src('card');
      const flags = at(native.text, /^\s*dwFlags: (.+),$/);
      const task = elseOf(card.text, /^\s*if (shown && )?button == (ALLOW_ID|REJECT_ID) \{$/, 'from_task_dialog');
      const allowId = Number(at(card.text, /^pub const ALLOW_ID: i32 = ([0-9]+);$/).match[1]);
      const other = word(task.other);
      const line = { label: card.label, n: task.found.n };
      return {
        failed_call: { gives: task.found.match[1] ? other : 'unknown', ...line },
        cancel_response: { gives: allowId !== 2 ? other : 'unknown', ...line },
        unknown_response: { gives: other, ...line },
        timeout: { gives: flags.match[1].includes('TDF_CALLBACK_TIMER') ? 'unknown' : 'none', label: native.label, n: flags.n },
      };
    },
  },
};

export function outcomeLines(form, readSource) {
  const out = [];
  for (const system of ['macos', 'windows']) {
    const derived = DERIVE[form][system]((key) => readSource(SOURCES[key]));
    for (const outcome of OUTCOMES) {
      const d = derived[outcome];
      out.push(`reading trial=dialog machine=mac form=${form} system=${system} outcome=${outcome} gives=${d.gives} file=${d.label} line=${d.n}`);
    }
  }
  return out;
}

export function attrLines(read) {
  return ATTRS.map((a) => {
    const text = read(a.file);
    for (const name of [a.default, a.cancel]) {
      if (!text.includes(name)) throw new Error(`${a.file} does not name ${name}`);
    }
    return `reading trial=dialog machine=mac system=${a.system} default_attr=${a.default} cancel_attr=${a.cancel}`;
  });
}

function cargo(args, options = {}) {
  return spawnSync('cargo', args, { cwd: here, encoding: 'utf8', maxBuffer: 1 << 28, ...options });
}

function metadata(features, target) {
  const args = ['metadata', '--format-version', '1', '--locked'];
  if (features) args.push('--features', features);
  if (target) args.push('--filter-platform', target);
  const done = cargo(args);
  if (done.status !== 0) throw new Error(`cargo metadata exited ${done.status}`);
  return JSON.parse(done.stdout);
}

function candidateReading(name) {
  const candidate = CANDIDATES[name];
  const patterns = parsePatterns(fs.readFileSync(path.join(here, 'input-patterns.txt'), 'utf8'));
  const built = cargo(['build', '--release', '--locked', '--features', candidate.feature], { stdio: ['ignore', 'ignore', 'inherit'] });
  const build = built.status === 0 ? 'ok' : 'fail';
  const meta = metadata(candidate.feature);
  const crate = meta.packages.find((p) => p.name === candidate.crate);
  if (!crate) throw new Error(`the metadata holds no ${candidate.crate}`);
  const perTarget = Object.fromEntries(BUILT_TARGETS.map((t) => [t, metadata(candidate.feature, t)]));
  const basePerTarget = Object.fromEntries(BUILT_TARGETS.map((t) => [t, metadata(null, t)]));
  const onTargets = addedOnTargets(perTarget, candidate.crate, 'r4-trial-app', basePerTarget);
  const added = onTargets.map((a) => a.pkg);
  const details = BUILT_TARGETS.map((t) => `detail trial=driver candidate=${name} target=${t} added=${onTargets.filter((a) => a.targets.includes(t)).length}`);
  let licences = true;
  for (const { pkg: p, targets } of onTargets) {
    const ok = licenceAllowed(p.license);
    licences &&= ok;
    details.push(`detail trial=driver candidate=${name} tree=cargo package=${p.name} version=${p.version} licence=${String(p.license).replace(/\s+/g, '_')} targets=${targets.length} ok=${ok ? 'yes' : 'no'}`);
  }
  const entries = unbuiltEntries(addedPackages(meta, candidate.crate, 'r4-trial-app', metadata(null)), onTargets).map((p) => entryLine(name, p));
  const texts = sourceTexts(path.dirname(crate.manifest_path));
  const names = added.map((p) => p.name);
  if (candidate.npm) {
    const lock = JSON.parse(fs.readFileSync(path.join(here, name, 'package-lock.json'), 'utf8'));
    const tree = npmTree(lock, candidate.npm);
    for (const p of tree) {
      const ok = licenceAllowed(p.license);
      licences &&= ok;
      details.push(`detail trial=driver candidate=${name} tree=npm package=${p.name} version=${p.version} licence=${String(p.license).replace(/\s+/g, '_')} ok=${ok ? 'yes' : 'no'}`);
    }
    names.push(...tree.map((p) => p.name));
    const packed = packageSource(name, candidate.npm, lock);
    texts.push(...sourceTexts(packed));
  }
  const banned = names.filter((n) => BANNED.includes(n)).length;
  details.push(`detail trial=driver candidate=${name} added=${added.length} source_files=${texts.length}`);
  return [...details, readingLine({ name, version: crate.version, build, licences, inputCalls: countInputCalls(patterns, texts, names), banned }), ...entries];
}

function packageSource(name, npm, lock) {
  const entry = lock.packages[`node_modules/${npm}`];
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `r4-driver-read-${name}-`));
  const out = execFileSync('npm', ['pack', `${npm}@${entry.version}`, '--json', '--ignore-scripts', '--pack-destination', dir], { encoding: 'utf8' });
  const [packed] = JSON.parse(out);
  if (packed.integrity !== entry.integrity) throw new Error(`the packed ${npm} differs from the lock`);
  const unpacked = path.join(dir, 'unpacked');
  fs.mkdirSync(unpacked, { recursive: true });
  execFileSync('tar', ['-xzf', path.join(dir, packed.filename), '-C', unpacked]);
  return unpacked;
}

function dialogReading(form) {
  const meta = metadata(null);
  const readSource = (s) => {
    if (s.crate === null) {
      return { label: `tools/r4-trials/driver/${s.file}`, text: fs.readFileSync(path.join(here, s.file), 'utf8') };
    }
    const pkg = meta.packages.find((p) => p.name === s.crate);
    const dir = path.dirname(pkg.manifest_path);
    return { label: `${s.crate}-${pkg.version}/${s.file}`, text: fs.readFileSync(path.join(dir, s.file), 'utf8') };
  };
  return [...dialogLines(form, readSource), ...outcomeLines(form, readSource)];
}

function main(argv) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (e) {
    if (e instanceof Refused) {
      process.stderr.write(`${e.message}\n`);
      return 2;
    }
    throw e;
  }
  let lines;
  if (args.mode === 'candidate') lines = candidateReading(args.name);
  else if (args.mode === 'dialog') lines = dialogReading(args.form);
  else lines = attrLines((file) => fs.readFileSync(path.join(here, file), 'utf8'));
  for (const line of lines) process.stdout.write(`${line}\n`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
