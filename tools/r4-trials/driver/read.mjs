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

export function addedPackages(metadata, candidateCrate, baseCrate) {
  const base = new Set(closure(metadata, baseCrate).map((p) => p.id));
  return closure(metadata, candidateCrate).filter((p) => !base.has(p.id));
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

export const DIALOG_SOURCES = {
  plugin: [
    {
      system: 'macos', crate: 'rfd', file: 'src/backend/macos/message_dialog.rs',
      find: /^\s*unsafe \{ alert\.addButtonWithTitle\(&label\) \};$/,
      default: 'reject', cancel: 'none', close: 'none',
    },
    {
      system: 'windows', crate: 'tauri-plugin-dialog', file: 'src/desktop.rs',
      find: /^\s*\) => rfd::MessageDialogResult::Custom\(cancel\),$/,
      default: 'reject', cancel: 'allow', close: 'allow',
    },
  ],
  system: [
    {
      system: 'macos', crate: null, file: 'src/native.rs',
      find: /alert\.addButtonWithTitle\(&NSString::from_str\(card::REJECT\)\)/,
      default: 'reject', cancel: 'none', close: 'none',
    },
    {
      system: 'windows', crate: null, file: 'src/native.rs',
      find: /nDefaultButton: card::REJECT_ID,/,
      default: 'reject', cancel: 'reject', close: 'reject',
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

export function dialogLines(form, locate) {
  return DIALOG_SOURCES[form].map((s) => {
    const { label, text } = locate(s);
    const line = findLine(text, s.find);
    return `reading trial=dialog machine=mac form=${form} system=${s.system} default=${s.default} cancel=${s.cancel} close=${s.close} file=${label} line=${line}`;
  });
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

function metadata(features) {
  const args = ['metadata', '--format-version', '1', '--locked'];
  if (features) args.push('--features', features);
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
  const added = addedPackages(meta, candidate.crate, 'r4-trial-app');
  const details = [];
  let licences = true;
  for (const p of added) {
    const ok = licenceAllowed(p.license);
    licences &&= ok;
    details.push(`detail trial=driver candidate=${name} tree=cargo package=${p.name} version=${p.version} licence=${String(p.license).replace(/\s+/g, '_')} ok=${ok ? 'yes' : 'no'}`);
  }
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
  return [...details, readingLine({ name, version: crate.version, build, licences, inputCalls: countInputCalls(patterns, texts, names), banned })];
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
  return dialogLines(form, (s) => {
    if (s.crate === null) {
      return { label: `tools/r4-trials/driver/${s.file}`, text: fs.readFileSync(path.join(here, s.file), 'utf8') };
    }
    const pkg = meta.packages.find((p) => p.name === s.crate);
    const dir = path.dirname(pkg.manifest_path);
    return { label: `${s.crate}-${pkg.version}/${s.file}`, text: fs.readFileSync(path.join(dir, s.file), 'utf8') };
  });
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
