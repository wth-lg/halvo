import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { FIXTURES_DIR, statesOf } from './files.mjs';
import { KEYS, joined, plan } from './run.mjs';

export const RECORD_FILE = join('ui', 'tests', 'r4', 'page', 'record.txt');
const RECORD_PATH = 'ui/tests/r4/page/record.txt';
const FIRST = 'page record: sh scripts/r4/ci.d/40-r4-page.sh --record';
const PASSED = 'keyboard=ok structure=same size=ok modes=ok modes_off=- markup=ok';

/**
 * The tracked paths whose files a whole run reads: every file under ui/, which the harness serves to the pages whole, the harness under
 * tools/r4-page/, the feature list and this check's script. The stamp holds what the index holds for them, which the dirty gate ties to
 * HEAD. It does not hold the browser build, the installed packages beyond the copies that packagesCheck compares, Node or the system.
 */
export const STAMPED = ['ui', 'tools/r4-page', 'docs/r4/page-features.txt', 'scripts/r4/ci.d/40-r4-page.sh'];

/** The stamp of the stamped files as the index holds them: the SHA-256 of each path with its mode and blob id, in path order, the record left out. */
export function stampOf(repo) {
  const out = execFileSync('git', ['-C', repo, 'ls-files', '-s', '-z', '--', ...STAMPED]).toString('utf8');
  const entries = out
    .split('\0')
    .filter(Boolean)
    .map((e) => {
      const tab = e.indexOf('\t');
      return [e.slice(tab + 1), e.slice(0, tab)];
    })
    .filter(([path]) => path !== RECORD_PATH)
    .sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  const all = createHash('sha256');
  for (const [path, head] of entries) all.update(path + '\0' + head + '\n');
  return { stamp: all.digest('hex'), files: entries.length };
}

/** The engine that tools/r4-page/browser.txt names for a Mac, the engine of the record. */
export function macEngine(repo) {
  const m = /^mac (\S+)$/m.exec(readFileSync(join(repo, 'tools', 'r4-page', 'browser.txt'), 'utf8').replace(/\r/g, ''));
  return m ? m[1] : '-';
}

function sum(values) {
  return values.reduce((a, b) => a + b, 0);
}

/**
 * The text of the record of a whole run that passed, the one form that the writer writes and the reader accepts. `runs` is the plan
 * (state and views label) of every state, `items` the names of the test items of page.mjs, `own` and `needs` the review count of each
 * run by name; every other value is that of a passing run, and the totals are the sums of the counts.
 */
export function recordForm({ stamp, files, engine, version, runs, items, own, needs }) {
  const count = (map, name) => (map.has(name) ? map.get(name) : '?');
  const states = runs.map((r) => r.state);
  const ownSum = sum(states.map((s) => Number(count(own, s)) || 0));
  const needsSum = sum(states.map((s) => Number(count(needs, s)) || 0));
  const browser = 'browser name=' + engine + ' version=' + version;
  return (
    [
      FIRST,
      'stamp=' + stamp + ' files=' + files,
      browser,
      ...runs.map((r) => 'page state=' + r.state + ' views=' + r.label + ' violations=0 review=' + count(own, r.state) + ' ' + PASSED + ' region=ok'),
      'page regions read=' + runs.length + ' rebuilt=0 say=ok',
      'needs ' + browser,
      ...[...states, ...items].map((n) => 'needs page state=' + n + ' violations=0 review=' + count(needs, n) + ' ' + PASSED),
      'needs page test_items=' + items.length + ' failed=0',
      'needs page states=' + states.length + ' violations=0 review=' + needsSum + ' ' + PASSED + ' engine=offline',
      'page needs_alone states=' + states.length + ' test_items=' + items.length + ' failed=0 exit=0',
      'page states=' + states.length + ' violations=0 review=' + (ownSum + needsSum) + ' ' + PASSED,
      'result=ok',
    ].join('\n') + '\n'
  );
}

/**
 * The browsers directory against the commit: the copies of the harness's manifest and lock that fetch.sh made there must be the
 * committed ones, and each installed package must have the version of its entry in the lock. Returns the lines to print and the cause.
 */
export function packagesCheck(repo, dir) {
  const harness = join(repo, 'tools', 'r4-page');
  const lines = [];
  let differ = 0;
  for (const f of ['package.json', 'package-lock.json']) {
    const same = existsSync(join(dir, f)) && readFileSync(join(dir, f)).equals(readFileSync(join(harness, f)));
    if (!same) differ++;
  }
  let wrong = 0;
  let read = 0;
  const lock = JSON.parse(readFileSync(join(harness, 'package-lock.json'), 'utf8'));
  for (const [key, entry] of Object.entries(lock.packages || {})) {
    if (!key.startsWith('node_modules/')) continue;
    read++;
    const manifest = join(dir, key, 'package.json');
    const version = existsSync(manifest) ? JSON.parse(readFileSync(manifest, 'utf8')).version : null;
    if (version !== entry.version) wrong++;
  }
  lines.push('page packages copies_differ=' + differ + ' packages=' + read + ' versions_differ=' + wrong);
  return { lines, cause: differ || wrong || read === 0 ? 'browser' : null };
}

/** The facts of a tree that a record names: the states and their plan, the test items of page.mjs, the engine for a Mac and the stamp. */
export function factsOf(repo) {
  const fx = join(repo, FIXTURES_DIR);
  const all = statesOf(fx);
  const items = Object.keys(JSON.parse(readFileSync(join(repo, 'ui', 'tests', 'r4', 'needs', 'items.json'), 'utf8')));
  return { all, items, runs: plan(fx, all), engine: macEngine(repo), ...stampOf(repo) };
}

/** The tracked files that differ from the index and the files that git does not track, the record left out. */
export function treeBeside(repo) {
  const git = (...a) => execFileSync('git', ['-C', repo, ...a]).toString('utf8').split('\0').filter(Boolean);
  return [...git('diff', '--name-only', '-z'), ...git('ls-files', '--others', '--exclude-standard', '-z')].filter((f) => f !== RECORD_PATH);
}

/** The first line at which two texts differ, counted from 1, with that line of each, or null when they are equal. */
export function firstDiff(a, b) {
  if (a === b) return null;
  const x = a.split('\n');
  const y = b.split('\n');
  let i = 0;
  while (i < x.length && i < y.length && x[i] === y[i]) i++;
  return { line: i + 1, committed: i < x.length ? x[i] : '<end>', fresh: i < y.length ? y[i] : '<end>' };
}

/** The committed record's text, or null. */
export function committedRecord(repo) {
  const path = join(repo, RECORD_FILE);
  return existsSync(path) ? readFileSync(path, 'utf8') : null;
}

/** Whether the committed record names the stamp of the head: the fault 'missing' or 'stamp', or null. */
export function stampFault(text, stamp, files) {
  if (text === null) return 'missing';
  return text.split('\n')[1] === 'stamp=' + stamp + ' files=' + files ? null : 'stamp';
}

/**
 * Reads the committed record against the facts of the head: it takes the review counts and the browser's version from the file, writes
 * the record of a passing run with them through recordForm and requires the file to be that text byte for byte.
 */
export function readRecord(text, facts) {
  const fault = stampFault(text, facts.stamp, facts.files);
  if (fault) return { fault };
  const own = new Map();
  const needs = new Map();
  let version = '-';
  for (const l of text.split('\n')) {
    let m = /^page state=(\S+) views=\S+ violations=\S+ review=(\d+) /.exec(l);
    if (m && !own.has(m[1])) own.set(m[1], Number(m[2]));
    m = /^needs page state=(\S+) violations=\S+ review=(\d+) /.exec(l);
    if (m && !needs.has(m[1])) needs.set(m[1], Number(m[2]));
    m = /^browser name=\S+ version=(\S+)$/.exec(l);
    if (m && version === '-') version = m[1];
  }
  const want = recordForm({ ...facts, version, own, needs });
  const diff = firstDiff(text, want);
  if (diff) return { fault: 'form', diff };
  const lines = want.split('\n');
  return { fault: null, browser: lines[2], line: lines[lines.length - 3], own: facts.runs.length };
}

/** The names of a list in the order of the SHA-256 of the commit and the name, so that each commit reads others and a second run of one commit reads the same. */
export function rotation(commit, names) {
  const key = (n) => createHash('sha256').update(commit + '\0' + n).digest('hex');
  return names.slice().sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));
}

/** The line of contracts.md I9.3 for a push: the record's line, with what a run of this push missed and its violations added. */
export function pushLine(count, recordLine, results, needs) {
  const v = Object.fromEntries(recordLine.split(' ').slice(1).map((kv) => kv.split('=')));
  const live = joined(count, results, needs);
  const l = Object.fromEntries(live.split(' ').slice(1).map((kv) => kv.split('=')));
  const out = { violations: Number(v.violations) + Number(l.violations), review: Number(v.review) };
  for (const k of KEYS) out[k] = v[k] === 'ok' || v[k] === 'same' ? l[k] : v[k];
  const off = new Set([...(v.modes_off === '-' ? [] : v.modes_off.split(',')), ...(l.modes_off === '-' ? [] : l.modes_off.split(','))]);
  return (
    'page states=' + count + ' violations=' + out.violations + ' review=' + out.review + ' keyboard=' + out.keyboard + ' structure=' + out.structure +
    ' size=' + out.size + ' modes=' + out.modes + ' modes_off=' + (off.size ? [...off].sort().join(',') : '-') + ' markup=' + out.markup
  );
}
