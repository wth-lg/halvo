import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  checkFile, countedFrom, machineVerdict, parseLine, windowFigures, figureLine, windowVerdict, syncVerdict,
} from '../rule.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const docs = path.join(here, '..', '..', '..', '..', 'docs', 'r4', 'trials');
const WINDOW = fs.readFileSync(path.join(docs, 'window.md'), 'utf8');
const SYNC = fs.readFileSync(path.join(docs, 'sync.md'), 'utf8');

const lines = (text) => text.split('\n').filter((l) => l !== '');
const num = (v) => (/^-?\d+$/.test(String(v)) ? Number(v) : v);
const cellOf = (line) => Object.fromEntries(Object.entries(parseLine(line).pairs).map(([k, v]) => [k, num(v)]));

function rebuild(trial, all) {
  const reading = all.filter((l) => l.startsWith('reading '));
  const cells = all.filter((l) => l.startsWith('cell '));
  const parsed = cells.map(cellOf);
  if (trial === 'window') {
    return `${[...reading, ...cells, ...windowFigures(parsed).map(figureLine), windowVerdict(parsed).line].join('\n')}\n`;
  }
  return `${[...reading, ...cells, syncVerdict(parsed).line].join('\n')}\n`;
}

const edit = (text, test, change) => lines(text).map((l) => (test(l) ? change(l) : l));
const set = (line, key, value) => line.replace(new RegExp(` ${key}=\\S+`), ` ${key}=${value}`);

function logOf(machine, ncpu) {
  const own = lines(WINDOW).filter((l) => l.includes(` machine=${machine} `));
  const trace = ncpu === undefined ? [] : [`trace trial=window arm=a run=1 sample load_x100=58 cpu_pct=- ncpu=${ncpu}`];
  return [...trace, ...own].map((l) => `2026-11-12T23:40:00.0000000Z ${l}`).join('\n');
}

const LOGS = [logOf('hosted-mac', 3), logOf('hosted-win', 4)];

test('the committed result files pass the check, the window file with its logs and without them', () => {
  const withLogs = checkFile('window', WINDOW, LOGS);
  assert.equal(withLogs.ok, true, withLogs.why);
  assert.equal(withLogs.macCounted, 'checked');
  const without = checkFile('window', WINDOW);
  assert.equal(without.ok, true, without.why);
  assert.equal(without.macCounted, 'unchecked');
  assert.equal(checkFile('sync', SYNC).ok, true);
});

test('E1: with the logs, a macOS counted that its load and the processor count do not give is refused (W05, W06)', () => {
  const w05 = rebuild('window', edit(WINDOW, (l) => / machine=hosted-mac arm=b run=[123] /.test(l), (l) => set(l, 'counted', 'no')));
  assert.match(w05, /hosted_mac=inconclusive/);
  const w06 = rebuild('window', edit(WINDOW, (l) => l.includes(' machine=hosted-mac arm=a run=1 '), (l) => set(l, 'load_x100', '900')));
  for (const [name, file] of [['W05', w05], ['W06', w06]]) {
    const r = checkFile('window', file, LOGS);
    assert.equal(r.ok, false, name);
    assert.match(r.why, /counted/, name);
    const blind = checkFile('window', file);
    assert.equal(blind.macCounted, 'unchecked', name);
  }
  const noCount = checkFile('window', WINDOW, [logOf('hosted-mac'), logOf('hosted-win', 4)]);
  assert.equal(noCount.ok, false);
  assert.match(noCount.why, /processor count/);
});

const TAMPER = {
  'W03 a cell of another machine': () => rebuild('window', [...lines(WINDOW), set(lines(WINDOW).find((l) => l.startsWith('cell ')), 'machine', 'hosted-linux')]),
  'W03b a Windows cell of another machine': () => rebuild('window', [...lines(WINDOW), set(lines(WINDOW).find((l) => l.includes(' machine=hosted-win ')), 'machine', 'hosted-linux')]),
  'W24 a macOS cell with a use in place of no use': () => edit(WINDOW, (l) => l.includes(' machine=hosted-mac arm=main run=1 '), (l) => set(l, 'cpu_pct', '5')),
  'W04 a cell of another arm': () => rebuild('window', [...lines(WINDOW), set(set(lines(WINDOW).find((l) => l.startsWith('cell ')), 'arm', 'c'), 'run', '1')]),
  'W08 a Windows cell with cpu_pct - and counted yes': () => edit(WINDOW, (l) => l.includes(' machine=hosted-win arm=a run=1 '), (l) => set(l, 'cpu_pct', '-')),
  'W14 a file of the verdict line alone': () => ['verdict window hosted_mac=inconclusive hosted_win=inconclusive quiet_mac=- quiet_win=-'],
  'W15 no reading lines': () => lines(WINDOW).filter((l) => !l.startsWith('reading ')),
  'W15b the TA-5 reading line taken out': () => lines(WINDOW).filter((l) => !l.includes(' expect=same_as_a ')),
  'W17 twelve runs in one arm': () => rebuild('window', [...lines(WINDOW), ...[11, 12].map((n) => set(lines(WINDOW).find((l) => l.includes(' machine=hosted-win arm=b run=1 ')), 'run', String(n)))]),
  'W18 key_bytes not a number': () => rebuild('window', edit(WINDOW, (l) => l.includes(' machine=hosted-mac arm=main run=1 '), (l) => set(l, 'key_bytes', '12x'))),
  'W18b a Windows cell with neither load nor use': () => edit(WINDOW, (l) => l.includes(' machine=hosted-win arm=a run=1 '), (l) => set(set(l, 'cpu_pct', '-'), 'load_x100', '-')),
  'W19 an extra key': () => edit(WINDOW, (l) => l.includes(' machine=hosted-mac arm=main run=1 '), (l) => `${l} note=x`),
  'W19b a reading line of machine hosted-mac': () => edit(WINDOW, (l) => l.startsWith('reading '), (l) => l.replace('machine=mac', 'machine=hosted-mac')),
  'W19c a reading line of no form the unit prints': () => [lines(WINDOW)[0], 'reading trial=window machine=mac note=x', ...lines(WINDOW).slice(1)],
  'W20 Windows b cells without helpers': () => rebuild('window', edit(WINDOW, (l) => l.includes(' machine=hosted-win arm=b '), (l) => l.replace(' helpers=0', ''))),
  'W21 keys in another order': () => edit(WINDOW, (l) => l.includes(' machine=hosted-mac arm=main run=1 '), (l) => l.replace(' machine=hosted-mac arm=main ', ' arm=main machine=hosted-mac ')),
  'W22 run 0': () => edit(WINDOW, (l) => l.includes(' machine=hosted-mac arm=main run=1 '), (l) => set(l, 'run', '0')),
  'W23 a file with no cell line': () => lines(WINDOW).filter((l) => l.startsWith('reading ')).concat(['verdict window hosted_mac=inconclusive hosted_win=inconclusive quiet_mac=- quiet_win=-']),
  'S04 get in a2 with hidden_ticks - and verdict get': () => rebuild('sync', edit(SYNC, (l) => l.includes(' way=get '), (l) => set(l.includes(' arm=a2 ') ? set(l, 'hidden_ticks', '-') : l, 'growth_bytes', '1000'))),
  'S05 get with sent=5 last=5 and verdict get': () => rebuild('sync', edit(SYNC, (l) => l.includes(' way=get '), (l) => set(set(set(l, 'sent', '5'), 'last', '5'), 'growth_bytes', '1000'))),
  'S06 a cell of an unknown way': () => [...lines(SYNC).slice(0, -1), set(lines(SYNC).find((l) => l.startsWith('cell ')), 'way', 'poll'), lines(SYNC).at(-1)],
  'S07 hidden_ticks a number in arm b': () => edit(SYNC, (l) => l.includes(' way=get arm=b '), (l) => set(l, 'hidden_ticks', '0')),
  'S09 a cell of another machine': () => [...lines(SYNC).slice(0, -1), set(lines(SYNC).find((l) => l.startsWith('cell ')), 'machine', 'hosted-linux'), lines(SYNC).at(-1)],
  'S10 a cell of arm a': () => [...lines(SYNC).slice(0, -1), set(set(lines(SYNC).find((l) => l.startsWith('cell ')), 'arm', 'a'), 'way', 'events'), lines(SYNC).at(-1)],
  'S11 the channel order reading line taken out': () => lines(SYNC).filter((l) => !l.includes(' order_by=')),
  'S12 growth not a number': () => edit(SYNC, (l) => l.includes(' way=get arm=b '), (l) => set(l, 'growth_bytes', 'x')),
};

for (const [name, make] of Object.entries(TAMPER)) {
  test(`E2: ${name} is refused`, () => {
    const made = make();
    const text = typeof made === 'string' ? made : `${made.join('\n')}\n`;
    const trial = name.startsWith('S') ? 'sync' : 'window';
    const r = checkFile(trial, text, trial === 'window' ? LOGS : undefined);
    assert.equal(r.ok, false, `${name}: ${r.why}`);
  });
}

test('N2: a copy of each file with CRLF line ends passes, as a line is read with its carriage return taken off', () => {
  assert.equal(checkFile('window', WINDOW.replace(/\n/g, '\r\n'), LOGS).ok, true);
  assert.equal(checkFile('sync', SYNC.replace(/\n/g, '\r\n')).ok, true);
});

test('E4: a second verdict line is refused', () => {
  const all = lines(WINDOW);
  const r = checkFile('window', `${[...all.slice(0, -1), all.at(-1), all.at(-1)].join('\n')}\n`, LOGS);
  assert.equal(r.ok, false);
  const s = lines(SYNC);
  assert.equal(checkFile('sync', `${[...s.slice(0, -1), 'verdict sync=get', s.at(-1)].join('\n')}\n`).ok, false);
});

test('E4: helpers must be 0 in every counted run of b for destroy, not in some', () => {
  const cells = lines(WINDOW).filter((l) => l.startsWith('cell ')).map(cellOf);
  assert.equal(machineVerdict(cells, 'hosted-win').verdict, 'destroy');
  const mixed = cells.map((c) => (c.machine === 'hosted-win' && c.arm === 'b' && c.run === 4 ? { ...c, helpers: 1 } : c));
  assert.equal(machineVerdict(mixed, 'hosted-win').verdict, 'hidden');
});

test('V20: a run at cpu_pct 50 counts and at 51 does not, in countedFrom and in the check', () => {
  assert.equal(countedFrom({ cpu_pct: 50, load_x100: '-' }, null), 'yes');
  assert.equal(countedFrom({ cpu_pct: 51, load_x100: '-' }, null), 'no');
  const at = (pct, counted) => checkFile('window', rebuild('window', edit(WINDOW, (l) => l.includes(' machine=hosted-win arm=main run=1 '), (l) => set(set(l, 'cpu_pct', String(pct)), 'counted', counted))), LOGS).ok;
  assert.equal(at(50, 'yes'), true);
  assert.equal(at(50, 'no'), false);
  assert.equal(at(51, 'no'), true);
  assert.equal(at(51, 'yes'), false);
});
