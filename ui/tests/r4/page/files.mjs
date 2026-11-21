import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, relative, sep } from 'node:path';

export const NAMES = [
  'setTimeout',
  'setInterval',
  'requestAnimationFrame',
  'requestIdleCallback',
  'localStorage',
  'sessionStorage',
  'indexedDB',
  'fetch',
  'XMLHttpRequest',
  'WebSocket',
  'EventSource',
  'sendBeacon',
  'innerHTML',
  'outerHTML',
  'insertAdjacentHTML',
  'setHTMLUnsafe',
  'parseHTMLUnsafe',
  'createContextualFragment',
  'DOMParser',
  'document.write',
  'attachShadow',
];
const PREFIXES = ['__playwright', '__r4'];
const ID = '[\\p{ID_Continue}$\\u200c\\u200d]';

export const VIEWS_DIR = join('ui', 'app', 'views');
export const FIXTURES_DIR = join('ui', 'tests', 'fixtures', 'coord');
export const SNAPSHOTS_DIR = join('ui', 'tests', 'r4', 'snapshots');
export const ACTS_FILE = join('ui', 'tests', 'r4', 'page', 'acts.json');
export const RUN_VIEWS = { 'needs.js': '/app/views/needs.js', 'agents.js': '/app/views/agents.js' };
const OTHER_STRUCTURE = [
  join('ui', 'tests', 'r4', 'needs', 'structure'),
  join('ui', 'tests', 'r4', 'agents', 'structure'),
  join('tools', 'r4-page', 'sample'),
];

/** The count of view files that the index holds under ui/app/views/, or null outside a git work tree. */
function viewsOfCommit(repo) {
  try {
    return execFileSync('git', ['-C', repo, 'ls-files', '-z', '--', VIEWS_DIR.split(sep).join('/')], { stdio: ['ignore', 'pipe', 'ignore'] }).toString('utf8').split('\0').filter(Boolean).length;
  } catch {
    return null;
  }
}

function filesUnder(dir) {
  const out = [];
  const visit = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      if (e.isDirectory()) visit(join(d, e.name));
      else out.push(relative(dir, join(d, e.name)).split(sep).join('/'));
    }
  };
  visit(dir);
  return out;
}

/** The states of a fixture directory, read from the names of its state files and fixed nowhere else. */
export function statesOf(dir) {
  return readdirSync(dir)
    .map((f) => /^state-(.+)\.json$/.exec(f))
    .filter(Boolean)
    .map((m) => m[1])
    .sort();
}

function escape(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

const nameRes = NAMES.map((n) => [n, new RegExp('(?<!' + ID + ')' + n.split('.').map(escape).join('\\s*\\.\\s*') + '(?!' + ID + ')', 'u')]);
const prefixRes = PREFIXES.map((p) => [p, new RegExp('(?<!' + ID + ')' + escape(p), 'u')]);

function hitsIn(text) {
  const out = [];
  for (const [n, re] of nameRes) if (re.test(text)) out.push(n);
  for (const [p, re] of prefixRes) if (re.test(text)) out.push(p + '*');
  return out;
}

function walk(node, visit) {
  if (!node || typeof node.type !== 'string') return;
  visit(node);
  for (const key of Object.keys(node)) {
    const v = node[key];
    if (Array.isArray(v)) for (const c of v) walk(c, visit);
    else if (v && typeof v === 'object' && typeof v.type === 'string') walk(v, visit);
  }
}

/** The text of an expression that is a string literal or a join of string literals with +, else null. */
function joinedText(node) {
  if (node.type === 'Literal' && typeof node.value === 'string') return node.value;
  if (node.type === 'BinaryExpression' && node.operator === '+') {
    const a = joinedText(node.left);
    const b = a === null ? null : joinedText(node.right);
    return a === null || b === null ? null : a + b;
  }
  return null;
}

function keyName(node) {
  if (!node.computed && node.property.type === 'Identifier') return node.property.name;
  return node.computed ? joinedText(node.property) : null;
}

/** The rules of contracts.md I8.4 on one view file's text: the parse at ECMAScript 2022, template strings, the 21 names and the two prefixes (in the text, in decoded names and strings, in a key or a value that joins string literals, a member write or writeln of any object, a with statement) and the deny patterns. */
export function viewRules(acorn, text, denies) {
  const found = { syntax: [], template: [], names: [], deny: [] };
  let ast = null;
  try {
    ast = acorn.parse(text, { ecmaVersion: 2022, sourceType: 'script', allowHashBang: false, allowReserved: false });
  } catch {
    found.syntax.push('parse');
  }
  const names = new Set(hitsIn(text));
  if (ast) {
    walk(ast, (node) => {
      if (node.type === 'TemplateLiteral' || node.type === 'TaggedTemplateExpression') found.template.push('template');
      if (node.type === 'Identifier' || node.type === 'PrivateIdentifier') hitsIn(node.name).forEach((h) => names.add(h));
      if (node.type === 'Literal' && typeof node.value === 'string') hitsIn(node.value).forEach((h) => names.add(h));
      if (node.type === 'BinaryExpression') {
        const joined = joinedText(node);
        if (joined !== null) hitsIn(joined).forEach((h) => names.add(h));
      }
      if (node.type === 'WithStatement') names.add('with');
      if (node.type === 'MemberExpression') {
        const k = keyName(node);
        if (k !== null) {
          hitsIn(k).forEach((h) => names.add(h));
          if (k === 'write' || k === 'writeln') names.add('document.write');
        }
      }
    });
  }
  found.names = [...names].sort();
  for (const [feature, re] of denies) {
    re.lastIndex = 0;
    if (re.test(text)) found.deny.push(feature);
  }
  return found;
}

/** The deny lines of docs/r4/page-features.txt as [feature, RegExp with the flags g and u]; throws on a line of no known form. */
export function denyLines(text) {
  const out = [];
  for (const raw of text.split('\n')) {
    const line = raw.replace(/\r$/, '');
    if (line === '' || line.startsWith('#')) continue;
    const cells = line.split('\t');
    if (cells.length !== 4 || !/^(allow|deny)$/.test(cells[0]) || cells.some((c) => c === '')) throw new Error('features');
    if (cells[0] === 'deny') out.push([cells[1], new RegExp(cells[3], 'gu')]);
  }
  return out;
}

function spreadLine(line) {
  const m = /^ *- (\{.*\})$/.exec(line);
  if (!m) return false;
  let v;
  try {
    v = JSON.parse(m[1]);
  } catch {
    return false;
  }
  const keys = Object.keys(v);
  return keys.length > 0 && keys.every((k, i) => k === String(i));
}

/** The lines of a structure file that hold a text node spread into an object of characters. */
export function spreadCount(text) {
  return text.split('\n').filter(spreadLine).length;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function actsOf(repo, states) {
  const path = join(repo, ACTS_FILE);
  const acts = readJson(path);
  if (!acts || typeof acts !== 'object' || Array.isArray(acts)) throw new Error('acts');
  let unknown = 0;
  for (const [state, list] of Object.entries(acts)) {
    if (!states.includes(state)) unknown++;
    if (!Array.isArray(list) || list.length === 0) throw new Error('acts');
    for (const a of list) {
      if (!a || typeof a !== 'object' || Array.isArray(a) || typeof a.act !== 'string') throw new Error('acts');
      for (const k of Object.keys(a)) if (!['act', 'param', 'n', 'text'].includes(k)) throw new Error('acts');
    }
  }
  return { acts, unknown };
}

function pinOf(path) {
  if (!existsSync(path)) return null;
  const p = readJson(path);
  const v = (p.dependencies && p.dependencies.playwright) || (p.devDependencies && p.devDependencies.playwright);
  return typeof v === 'string' ? v : '';
}

/** Runs every rule that reads files alone; returns the lines to print and the first cause that holds, or null. */
export function fileRules(repo, acornDir) {
  const lines = [];
  const acorn = createRequire(join(acornDir, 'package.json'))('acorn');
  let denies;
  try {
    denies = denyLines(readFileSync(join(repo, 'docs', 'r4', 'page-features.txt'), 'utf8'));
  } catch {
    lines.push('files features=malformed');
    return { lines, cause: 'features' };
  }
  const views = filesUnder(join(repo, VIEWS_DIR)).sort();
  const committed = viewsOfCommit(repo);
  const total = { syntax: 0, template: 0, names: 0, deny: 0 };
  let unrun = 0;
  for (const v of views) {
    const found = viewRules(acorn, readFileSync(join(repo, VIEWS_DIR, v), 'utf8'), denies);
    for (const rule of Object.keys(total)) {
      total[rule] += found[rule].length;
      for (const what of found[rule]) lines.push('files view=' + v + ' rule=' + rule + ' what=' + what);
    }
    if (!(v in RUN_VIEWS)) {
      unrun++;
      lines.push('files view=' + v + ' rule=unrun');
    }
  }
  lines.push(
    'files views=' + views.length + ' syntax=' + total.syntax + ' template=' + total.template + ' names=' + total.names + ' deny=' + total.deny +
      ' deny_lines=' + denies.length + ' unrun=' + unrun + ' commit_views=' + (committed === null ? '-' : committed),
  );
  const states = statesOf(join(repo, FIXTURES_DIR));
  let acts;
  try {
    acts = actsOf(repo, states);
  } catch {
    lines.push('acts malformed=yes');
    return { lines, cause: 'acts' };
  }
  lines.push('acts states=' + Object.keys(acts.acts).length + ' unknown=' + acts.unknown);
  const snaps = existsSync(join(repo, SNAPSHOTS_DIR)) ? readdirSync(join(repo, SNAPSHOTS_DIR)).filter((f) => !f.startsWith('.')) : [];
  let missing = states.filter((s) => !snaps.includes(s + '.txt')).length;
  let stale = snaps.filter((f) => !f.endsWith('.txt') || !states.includes(f.slice(0, -4))).length;
  const items = Object.keys(readJson(join(repo, 'ui', 'tests', 'r4', 'needs', 'items.json')));
  for (const [dir, names] of [[OTHER_STRUCTURE[0], [...states, ...items]], [OTHER_STRUCTURE[1], states]]) {
    const have = existsSync(join(repo, dir)) ? readdirSync(join(repo, dir)).filter((f) => !f.startsWith('.')) : [];
    missing += names.filter((s) => !have.includes(s + '.txt')).length;
    stale += have.filter((f) => !f.endsWith('.txt') || !names.includes(f.slice(0, -4))).length;
  }
  let spread = 0;
  let read = 0;
  for (const dir of [SNAPSHOTS_DIR, ...OTHER_STRUCTURE]) {
    const full = join(repo, dir);
    if (!existsSync(full)) continue;
    for (const f of readdirSync(full).filter((x) => x.endsWith('.txt'))) {
      read++;
      spread += spreadCount(readFileSync(join(full, f), 'utf8'));
    }
  }
  lines.push('structure states=' + states.length + ' files=' + snaps.length + ' missing=' + missing + ' stale=' + stale + ' read=' + read + ' spread=' + spread);
  const ours = pinOf(join(repo, 'tools', 'r4-page', 'package.json'));
  const theirs = pinOf(join(repo, 'tools', 'fixture-server', 'package.json'));
  lines.push('pin playwright=' + (ours || '-') + ' fixture_server=' + (theirs === null ? '-' : theirs || 'none'));
  const cause =
    total.syntax ? 'syntax'
    : total.template ? 'template'
    : total.names ? 'names'
    : total.deny ? 'deny'
    : unrun ? 'unrun'
    : views.length === 0 || (committed !== null && committed !== views.length) ? 'views'
    : states.length === 0 ? 'states'
    : acts.unknown ? 'acts'
    : missing || stale ? 'structure'
    : spread ? 'spread'
    : !ours || (theirs !== null && theirs !== ours) ? 'pin'
    : null;
  return { lines, cause };
}
