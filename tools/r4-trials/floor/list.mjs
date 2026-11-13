// usage: node tools/r4-trials/floor/list.mjs check [--list FILE] [--cases FILE] [--probe FILE] [VIEW...]
//        node tools/r4-trials/floor/list.mjs probe [--list FILE] [--write]
// check: the form of docs/r4/page-features.txt (contracts.md I11.4), every deny pattern against
// the cases of cases.txt, probe.js against the list, and every view file given against every
// deny pattern.
// probe: prints the source of probe.js for the allow lines of the list, or writes it with --write.
// Exit: 0 when every check holds, 1 when one misses, 2 on a refusal.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const HERE = path.dirname(fileURLToPath(import.meta.url));
export const ROOT = path.resolve(HERE, '..', '..', '..');
export const LIST = path.join(ROOT, 'docs', 'r4', 'page-features.txt');
export const CASES = path.join(HERE, 'cases.txt');
export const PROBE = path.join(HERE, 'probe.js');
export const FLOOR = '17.0';

const NAME = /^[a-z0-9_]{1,32}$/;
const VERSION = /^[0-9]+(?:\.[0-9]+){0,2}$/;
const EVAL = /\b(?:eval|Function)\s*\(/;
const SOURCE = /^# https:\/\/\S+.*, Safari ([0-9]+(?:\.[0-9]+){0,2}) of (\S+)$/;
const LANGUAGE = new Set(['true', 'false', 'null', 'undefined', 'NaN', 'Infinity', 'typeof', 'void', 'new', 'in', 'instanceof',
  'delete', 'function', 'return', 'var', 'let', 'const', 'if', 'else', 'this']);

function isDate(day) {
  const parsed = new Date(`${day}T00:00:00Z`);
  return !Number.isNaN(parsed.getTime()) && parsed.toISOString().slice(0, 10) === day;
}

/** Whether an expression, with its string literals taken out, names anything other than the
 * literals and keywords of the language, so that it can be false on some page. */
export function namesThePage(cell) {
  const bare = cell.replace(/'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*"/g, '""');
  return (bare.match(/[A-Za-z_$][\w$]*/g) ?? []).some((word) => !LANGUAGE.has(word));
}

/** Compares two versions field by field as numbers, a missing field being 0. */
export function compareVersions(a, b) {
  const x = a.split('.').map(Number);
  const y = b.split('.').map(Number);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return Math.sign(d);
  }
  return 0;
}

/** The feature lines of the list as `{ kind, name, version, cell, line }`, with one problem per
 * line that breaks the form of I11.4: four cells separated by tabs, none with a space at either
 * end, a name of I0.6's form used once, a version, an allow line at or under the floor and a deny
 * line above it, the comment directly above naming a page and a real date of the version that the
 * line gives, an allow expression that parses, names something of the page and calls neither eval
 * nor Function, which the policy of the trial app's pages refuses, and a deny pattern that
 * compiles with the flags g and u. */
export function parseList(text) {
  const features = [];
  const problems = [];
  const names = new Set();
  if (text.includes('\r')) problems.push('the list holds a carriage return');
  const lines = text.split('\n');
  if (lines[lines.length - 1] !== '') problems.push('the list does not end with a line feed');
  if (lines[lines.length - 1] === '') lines.pop();
  if (!/^# .*https:\/\/developer\.apple\.com\/documentation\/safari-release-notes\/safari-17-release-notes/.test(lines[0] ?? '')) {
    problems.push('line 1 names no page of the floor\'s Safari release notes');
  }
  lines.forEach((line, i) => {
    const at = `line ${i + 1}`;
    if (line.startsWith('#')) return;
    const cells = line.split('\t');
    if (cells.length !== 4) {
      problems.push(`${at}: ${cells.length} cells, not 4`);
      return;
    }
    const [kind, name, version, cell] = cells;
    if (cells.some((c) => c !== c.trim())) problems.push(`${at}: a cell starts or ends with a space`);
    if (kind !== 'allow' && kind !== 'deny') problems.push(`${at}: kind ${JSON.stringify(kind)} is neither allow nor deny`);
    if (!NAME.test(name)) problems.push(`${at}: the name is not of I0.6's form`);
    else if (names.has(name)) problems.push(`${at}: the name ${name} is used twice`);
    names.add(name);
    if (!VERSION.test(version)) {
      problems.push(`${at}: ${name}: the version is not a version`);
    } else if (kind === 'allow' && compareVersions(version, FLOOR) > 0) {
      problems.push(`${at}: ${name}: an allow line has the version ${version}, above the floor ${FLOOR}`);
    } else if (kind === 'deny' && compareVersions(version, FLOOR) <= 0) {
      problems.push(`${at}: ${name}: a deny line has the version ${version}, at or under the floor ${FLOOR}`);
    }
    const above = SOURCE.exec(lines[i - 1] ?? '');
    if (!above || !isDate(above[2])) problems.push(`${at}: ${name}: the line above names no page and date of its version`);
    else if (above[1] !== version) problems.push(`${at}: ${name}: the comment above gives the version ${above[1]}, the line ${version}`);
    if (cell === '') problems.push(`${at}: ${name}: the fourth cell is empty`);
    if (kind === 'allow') {
      try {
        new Function(`return (${cell});`);
      } catch {
        problems.push(`${at}: ${name}: the expression does not parse`);
      }
      if (EVAL.test(cell)) problems.push(`${at}: ${name}: the expression calls eval or Function, which the policy of the app's pages refuses`);
      if (!namesThePage(cell)) problems.push(`${at}: ${name}: the expression names nothing of the page, so it cannot be false`);
    } else if (kind === 'deny') {
      try {
        new RegExp(cell, 'gu');
      } catch {
        problems.push(`${at}: ${name}: the pattern does not compile with the flags g and u`);
      }
    }
    features.push({ kind, name, version, cell, line: i + 1 });
  });
  return { features, problems };
}

/** The deny lines whose pattern finds a use in `text`, by name. */
export function denied(features, text) {
  const found = [];
  for (const f of features) {
    if (f.kind !== 'deny') continue;
    let re;
    try {
      re = new RegExp(f.cell, 'gu');
    } catch {
      continue;
    }
    if (re.test(text)) found.push(f.name);
  }
  return found;
}

/** The cases of cases.txt: `use`, a tab, a deny name, a tab and a text that must be found by
 * that line's pattern; or `free`, a tab and a text that no deny pattern may find. */
export function parseCases(text) {
  const cases = [];
  const problems = [];
  text.split('\n').forEach((line, i) => {
    if (line === '' || line.startsWith('#')) return;
    const cells = line.split('\t');
    if (cells[0] === 'use' && cells.length === 3 && NAME.test(cells[1]) && cells[2] !== '') {
      cases.push({ kind: 'use', name: cells[1], text: cells[2], line: i + 1 });
    } else if (cells[0] === 'free' && cells.length === 2 && cells[1] !== '') {
      cases.push({ kind: 'free', text: cells[1], line: i + 1 });
    } else {
      problems.push(`cases line ${i + 1} is neither "use, name, text" nor "free, text"`);
    }
  });
  return { cases, problems };
}

/** The problems of the cases against the deny lines: a deny line with no use case, a use case
 * of a name that is no deny line, a use that its own pattern misses, and a free text that a
 * pattern finds. */
export function checkCases(features, cases) {
  const problems = [];
  const deny = new Set(features.filter((f) => f.kind === 'deny').map((f) => f.name));
  const used = new Set();
  for (const c of cases) {
    const found = denied(features, c.text);
    if (c.kind === 'use') {
      used.add(c.name);
      if (!deny.has(c.name)) problems.push(`cases line ${c.line}: ${c.name} is no deny line`);
      else if (!found.includes(c.name)) problems.push(`cases line ${c.line}: the pattern of ${c.name} misses its use`);
    } else if (found.length > 0) {
      problems.push(`cases line ${c.line}: a free text is found by ${found.join(',')}`);
    }
  }
  for (const name of deny) if (!used.has(name)) problems.push(`${name} has no use case`);
  return problems;
}

/** The source of probe.js: one entry per allow line, in the order of the list, each a function
 * that evaluates the line's expression, so that the page needs no eval under the app's policy. */
export function probeSource(features) {
  const entries = features.filter((f) => f.kind === 'allow').map((f) =>
    `  [${JSON.stringify(f.name)}, function () { return (${f.cell}); }]`);
  return [
    '(function () {',
    "  'use strict';",
    '  var FEATURES = [',
    entries.join(',\n'),
    '  ];',
    '  var missing = [];',
    '  FEATURES.forEach(function (feature) {',
    '    var present = false;',
    '    try {',
    '      present = feature[1]() === true;',
    '    } catch (e) {',
    '      present = false;',
    '    }',
    '    if (!present) missing.push(feature[0]);',
    '  });',
    "  var agent = String(navigator.userAgent);",
    "  var webkit = /AppleWebKit\\/([0-9.]+)/.exec(agent);",
    "  var edge = /Edg\\/([0-9.]+)/.exec(agent);",
    "  var aria = typeof document.ariaNotify === 'function' ? 'function' : 'undefined';",
    "  window.r4app.report('probe features=' + FEATURES.length + ' missing=' + missing.length +",
    "    ' missing_names=' + (missing.length ? missing.join(',') : '-') +",
    "    ' webkit=' + (webkit ? webkit[1] : '-') + ' edg=' + (edge ? edge[1] : '-') + ' aria_notify=' + aria);",
    '})();',
    '',
  ].join('\n');
}

function read(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return null;
  }
}

class Refused extends Error {}

function parseArgs(argv) {
  const [mode, ...rest] = argv;
  const options = { mode, list: LIST, cases: CASES, probe: PROBE, write: false, views: [] };
  if (mode !== 'check' && mode !== 'probe') throw new Refused('give check or probe');
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    if ((a === '--list' || a === '--cases' || (a === '--probe' && mode === 'check')) && rest[i + 1] !== undefined) options[a.slice(2)] = rest[++i];
    else if (a === '--write' && mode === 'probe') options.write = true;
    else if (mode === 'check' && !a.startsWith('-')) options.views.push(a);
    else throw new Refused(`unknown argument ${JSON.stringify(a)}`);
  }
  return options;
}

/** Runs the command line and returns the exit status. */
export function main(argv, out = (s) => process.stdout.write(s), err = (s) => process.stderr.write(s)) {
  let options;
  try {
    options = parseArgs(argv);
  } catch (e) {
    if (!(e instanceof Refused)) throw e;
    err(`REFUSED usage: ${e.message}\n`);
    return 2;
  }
  const listText = read(options.list);
  if (listText === null) {
    err('REFUSED usage: the list cannot be read\n');
    return 2;
  }
  const { features, problems } = parseList(listText);
  if (options.mode === 'probe') {
    if (problems.length > 0) {
      for (const p of problems) out(`miss ${p}\n`);
      return 1;
    }
    const source = probeSource(features);
    if (!options.write) {
      out(source);
      return 0;
    }
    fs.writeFileSync(PROBE, source);
    out(`probe written features=${features.filter((f) => f.kind === 'allow').length}\n`);
    return 0;
  }
  const casesText = read(options.cases);
  if (casesText === null) {
    err('REFUSED usage: the cases cannot be read\n');
    return 2;
  }
  const parsed = parseCases(casesText);
  const all = [...problems, ...parsed.problems, ...checkCases(features, parsed.cases)];
  if (read(options.probe) !== probeSource(features)) all.push('probe.js is not the probe of the list: run node tools/r4-trials/floor/list.mjs probe --write');
  let found = 0;
  for (const view of options.views) {
    const text = read(view);
    if (text === null) {
      all.push(`the view ${path.basename(view)} cannot be read`);
      continue;
    }
    const names = denied(features, text);
    found += names.length;
    for (const name of names) all.push(`the view ${path.basename(view)} uses ${name}, which the list denies`);
  }
  for (const p of all) out(`miss ${p}\n`);
  const allow = features.filter((f) => f.kind === 'allow').length;
  out(`list allow=${allow} deny=${features.length - allow} cases=${parsed.cases.length} views=${options.views.length} found=${found} problems=${all.length}\n`);
  return all.length === 0 ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
