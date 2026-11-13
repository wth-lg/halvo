import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { KEYS, joined } from './run.mjs';

const PASSED = ['violations=0', 'keyboard=ok', 'structure=same', 'size=ok', 'modes=ok', 'markup=ok'];

export const RECORD_FILE = join('ui', 'tests', 'r4', 'page', 'record.txt');
const RECORD_PATH = 'ui/tests/r4/page/record.txt';

/** The tracked paths whose files the whole page check reads: the views, the fixtures, the structure files, page.mjs with its files, this check and the harness. */
export const STAMPED = ['ui/app/views', 'ui/tests/fixtures/coord', 'ui/tests/r4/snapshots', 'ui/tests/r4/needs', 'ui/tests/r4/page', 'tools/r4-page', 'docs/r4/page-features.txt', 'scripts/r4/ci.d/40-r4-page.sh'];

function sha256(buffer) {
  return createHash('sha256').update(buffer).digest('hex');
}

/** The stamp of the files that the page check reads, from the index's list of them and their text with every carriage return taken out. */
export function stampOf(repo, list) {
  const files = (list || execFileSync('git', ['-C', repo, 'ls-files', '-z', '--', ...STAMPED]).toString('utf8').split('\0'))
    .filter((f) => f !== '' && f !== RECORD_PATH)
    .sort();
  const all = createHash('sha256');
  for (const f of files) {
    const body = Buffer.from(readFileSync(join(repo, f)).toString('latin1').replace(/\r/g, ''), 'latin1');
    all.update(f + '\0' + sha256(body) + '\n');
  }
  return { stamp: all.digest('hex'), files: files.length };
}

/** The text of a record of a whole run: its stamp, its browser line, every line of the run and its result. */
export function recordText({ stamp, files, browser, lines, cause }) {
  return (
    'page record: sh scripts/r4/ci.d/40-r4-page.sh --record\n' +
    'stamp=' + stamp + ' files=' + files + '\n' +
    (browser || 'browser -') + '\n' +
    lines.join('\n') + '\n' +
    'result=' + (cause || 'ok') + '\n'
  );
}

/** Reads the committed record against the states and the stamp of the head: returns the fault that holds, or null. */
export function readRecord(repo, states, stamp, labels) {
  const path = join(repo, RECORD_FILE);
  if (!existsSync(path)) return { fault: 'missing' };
  const lines = readFileSync(path, 'utf8').replace(/\r/g, '').split('\n').filter(Boolean);
  const stamps = lines.filter((l) => l.startsWith('stamp='));
  if (stamps.length !== 1) return { fault: 'no_stamp' };
  if (stamps[0].split(' ')[0] !== 'stamp=' + stamp) return { fault: 'stamp' };
  const results = lines.filter((l) => l.startsWith('result='));
  if (results.length !== 1 || results[0] !== 'result=ok') return { fault: 'result' };
  const own = lines.filter((l) => l.startsWith('page state='));
  const needs = lines.filter((l) => l.startsWith('needs page state='));
  const stateOf = (l) => /state=(\S+)/.exec(l)[1];
  const ownStates = own.map(stateOf).sort();
  if (ownStates.length !== states.length || ownStates.some((s, i) => s !== states[i])) return { fault: 'states' };
  if (states.some((s) => !needs.some((l) => stateOf(l) === s))) return { fault: 'states' };
  if ([...own, ...needs].some((l) => / misses=|region=miss/.test(l))) return { fault: 'result' };
  if (own.some((l) => (/ views=(\S+)/.exec(l) || [])[1] !== labels[stateOf(l)])) return { fault: 'views' };
  const browser = lines.find((l) => l.startsWith('browser name=')) || 'browser -';
  const joinedLine = lines.filter((l) => l.startsWith('page states='));
  if (joinedLine.length !== 1 || joinedLine[0].split(' ')[1] !== 'states=' + states.length) return { fault: 'states' };
  if ([...own, ...needs, joinedLine[0]].some((l) => !PASSED.every((kv) => (' ' + l + ' ').includes(' ' + kv + ' ')))) return { fault: 'result' };
  return { fault: null, browser, line: joinedLine[0], own: own.length, needs: needs.length };
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
