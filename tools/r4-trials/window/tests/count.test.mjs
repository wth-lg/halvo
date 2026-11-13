import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkFile, makeFile, parseLine, windowFigures, figureLine, windowVerdict, syncVerdict } from '../rule.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const docs = path.join(here, '..', '..', '..', '..', 'docs', 'r4', 'trials');
const WINDOW = fs.readFileSync(path.join(docs, 'window.md'), 'utf8');
const SYNC = fs.readFileSync(path.join(docs, 'sync.md'), 'utf8');

const lines = (text) => text.split('\n').filter((l) => l !== '');
const num = (v) => (/^-?\d+$/.test(String(v)) ? Number(v) : v);
const cellOf = (line) => Object.fromEntries(Object.entries(parseLine(line).pairs).map(([k, v]) => [k, num(v)]));
const set = (line, key, value) => line.replace(new RegExp(` ${key}=\\S+`), ` ${key}=${value}`);
const READING = lines(WINDOW).filter((l) => l.startsWith('reading ')).join('\n');

function rebuild(trial, all) {
  const reading = all.filter((l) => l.startsWith('reading '));
  const cells = all.filter((l) => l.startsWith('cell '));
  const parsed = cells.map(cellOf);
  if (trial === 'window') {
    return `${[...reading, ...cells, ...windowFigures(parsed).map(figureLine), windowVerdict(parsed).line].join('\n')}\n`;
  }
  return `${[...reading, ...cells, syncVerdict(parsed).line].join('\n')}\n`;
}

function logOf(machine, ncpu, extra = []) {
  const own = lines(WINDOW).filter((l) => l.startsWith('cell ') && l.includes(` machine=${machine} `));
  const figures = windowFigures(own.map(cellOf)).map(figureLine);
  const trace = ncpu === undefined ? [] : [`trace trial=window arm=a run=1 sample load_x100=58 cpu_pct=- ncpu=${ncpu}`];
  return [...trace, ...own, ...figures, ...extra].map((l) => `2026-11-12T23:40:00.0000000Z ${l}`).join('\n');
}

const LOGS = [logOf('hosted-mac', 3), logOf('hosted-win', 4)];

test('a window file with one cell line taken out is refused, also with its figures and verdict made again', () => {
  const gone = lines(WINDOW).filter((l) => !l.includes(' machine=hosted-win arm=b run=7 '));
  const r = checkFile('window', rebuild('window', gone), LOGS);
  assert.equal(r.ok, false, r.why);
  assert.match(r.why, /hosted-win arm b holds 9 cells, not 10/);
});

test('a window file with eleven cells for one machine and arm is refused for its count', () => {
  const eleventh = set(lines(WINDOW).find((l) => l.includes(' machine=hosted-mac arm=a2 run=10 ')), 'run', '11');
  const r = checkFile('window', rebuild('window', [...lines(WINDOW), eleventh]), LOGS);
  assert.equal(r.ok, false, r.why);
  assert.match(r.why, /hosted-mac arm a2 holds 11 cells, not 10/);
});

test('make refuses a job log that holds a nocell line, as it refuses one with a miss line', () => {
  assert.equal(makeFile('window', READING, LOGS).ok, true);
  const nocell = 'nocell trial=window machine=hosted-win arm=b run=4 step=sample cause=sampler';
  const made = makeFile('window', READING, [LOGS[0], logOf('hosted-win', 4, [nocell])]);
  assert.equal(made.ok, false);
  assert.match(made.why, /log 2 holds a nocell line/);
});

test('a sync file with one cell taken out is refused (S06 of review 19)', () => {
  const gone = lines(SYNC).filter((l) => !l.includes(' machine=hosted-win way=get arm=b '));
  const r = checkFile('sync', rebuild('sync', gone));
  assert.equal(r.ok, false, r.why);
  assert.match(r.why, /no cell of machine=hosted-win way=get arm=b/);
});

test('each figure prints beside its medians the largest value and the count of runs above the line of 1,000 ms, read from the cells', () => {
  const cells = lines(WINDOW).filter((l) => l.startsWith('cell ')).map(cellOf);
  const figures = Object.fromEntries(windowFigures(cells).map((f) => [`${f.machine} ${f.arm}`, f]));
  const macB = figures['hosted-mac b'];
  assert.deepEqual([macB.median_bytes, macB.max_bytes, macB.median_show_ms, macB.max_show_ms, macB.show_over_1000], [86048768, 87687168, 1307, 1426, 10]);
  const winB = figures['hosted-win b'];
  assert.deepEqual([winB.median_show_ms, winB.max_show_ms, winB.show_over_1000], [462, 560, 0]);
  assert.equal(figureLine(winB), 'figure trial=window machine=hosted-win arm=b median_bytes=19431424 range_bytes=7286784 runs=10 max_bytes=25632768 median_show_ms=462 max_show_ms=560 show_over_1000=0');
  const slow = cells.map((c) => (c.machine === 'hosted-win' && c.arm === 'b' && c.run === 3 ? { ...c, show_ms: 1348 } : c));
  const after = windowFigures(slow).find((f) => f.machine === 'hosted-win' && f.arm === 'b');
  assert.deepEqual([after.max_show_ms, after.show_over_1000], [1348, 1]);
  const uncounted = cells.map((c) => (c.machine === 'hosted-win' && c.arm === 'b' && c.run === 3 ? { ...c, show_ms: 1348, cpu_pct: 90, counted: 'no' } : c));
  const left = windowFigures(uncounted).find((f) => f.machine === 'hosted-win' && f.arm === 'b');
  assert.deepEqual([left.max_show_ms, left.show_over_1000, left.runs], [560, 0, 9]);
});

test('the committed window file passes in its form of run 43325502435, and a file made again in the new form passes too', () => {
  const kept = checkFile('window', WINDOW, LOGS);
  assert.equal(kept.ok, true, kept.why);
  const again = rebuild('window', lines(WINDOW));
  assert.match(again, /show_over_1000=10/);
  const r = checkFile('window', again, LOGS);
  assert.equal(r.ok, true, r.why);
  assert.equal(lines(again).at(-1), lines(WINDOW).at(-1));
  const wrong = again.replace('max_show_ms=1426 show_over_1000=10', 'max_show_ms=1426 show_over_1000=9');
  assert.equal(checkFile('window', wrong, LOGS).ok, false);
});

test('a run at exactly 1,000 ms is not above the line, and one at 1,001 is', () => {
  const cells = lines(WINDOW).filter((l) => l.startsWith('cell ')).map(cellOf);
  const at = (ms) => windowFigures(cells.map((c) => (c.machine === 'hosted-win' && c.arm === 'b' && c.run === 1 ? { ...c, show_ms: ms } : c)))
    .find((f) => f.machine === 'hosted-win' && f.arm === 'b').show_over_1000;
  assert.equal(at(1000), 0);
  assert.equal(at(1001), 1);
});

test('make takes job logs whose figure lines are of the form that run 43325502435 printed, and keeps that form for its cells', () => {
  const oldLog = (machine, ncpu) => {
    const own = lines(WINDOW).filter((l) => l.startsWith('cell ') && l.includes(` machine=${machine} `));
    const figures = windowFigures(own.map(cellOf)).map((f) => figureLine(f, { old: true }));
    return [`trace trial=window arm=a run=1 sample load_x100=58 cpu_pct=- ncpu=${ncpu}`, ...own, ...figures].join('\n');
  };
  const made = makeFile('window', READING, [oldLog('hosted-mac', 3), oldLog('hosted-win', 4)]);
  assert.equal(made.ok, true, made.why);
  assert.equal(made.text, WINDOW);
});

test('make refuses a log with an indented nocell line, as it refuses an indented miss line (review of U0.2b)', () => {
  const nocell = '  nocell trial=window machine=hosted-win arm=b run=4 step=sample cause=sampler';
  const made = makeFile('window', READING, [LOGS[0], logOf('hosted-win', 4, [nocell])]);
  assert.equal(made.ok, false);
  assert.match(made.why, /log 2 holds a nocell line/);
});

test('a second verdict line is refused also when it is indented (review of U0.2b)', () => {
  const all = lines(WINDOW);
  const second = '  verdict window hosted_mac=destroy hosted_win=destroy quiet_mac=- quiet_win=-';
  const r = checkFile('window', `${[...all.slice(0, -1), second, all.at(-1)].join('\n')}\n`, LOGS);
  assert.equal(r.ok, false, r.why);
});

test('a file whose figure lines mix the two forms, each the figure of its cells, passes, and make builds it from such logs (review of U0.2b)', () => {
  const oldLog = (machine, ncpu) => {
    const own = lines(WINDOW).filter((l) => l.startsWith('cell ') && l.includes(` machine=${machine} `));
    const figures = windowFigures(own.map(cellOf)).map((f) => figureLine(f, { old: true }));
    return [`trace trial=window arm=a run=1 sample load_x100=58 cpu_pct=- ncpu=${ncpu}`, ...own, ...figures].join('\n');
  };
  const made = makeFile('window', READING, [oldLog('hosted-mac', 3), logOf('hosted-win', 4)]);
  assert.equal(made.ok, true, made.why);
  assert.equal(checkFile('window', made.text, LOGS).ok, true);
  assert.equal(checkFile('window', made.text.replace(/(arm=b median_bytes=19431424 range_bytes=7286784 runs=10 max_bytes=25632768 median_show_ms=462 max_show_ms=560 show_over_1000=)0/, '$11'), LOGS).ok, false);
});

test('a file with an extra figure line is refused, in either form', () => {
  const all = lines(WINDOW);
  const figure = all.find((l) => l.startsWith('figure '));
  const at = all.lastIndexOf(all.filter((l) => l.startsWith('figure ')).at(-1));
  assert.equal(checkFile('window', `${[...all.slice(0, at + 1), figure, ...all.slice(at + 1)].join('\n')}\n`, LOGS).ok, false);
});

test('RD-57 E1: a file in which run 9 is replaced by a copy of run 10 is refused, with the reason that names the run', () => {
  const ten = lines(WINDOW).find((l) => l.includes(' machine=hosted-mac arm=a run=10 '));
  const all = lines(WINDOW).map((l) => (l.includes(' machine=hosted-mac arm=a run=9 ') ? ten : l));
  const r = checkFile('window', rebuild('window', all), LOGS);
  assert.equal(r.ok, false);
  assert.match(r.why, /^a run appears twice: machine=hosted-mac arm=a run=10$/);
});

test('RD-57 E2: median_show_ms is taken over the counted runs alone', () => {
  const cells = lines(WINDOW).filter((l) => l.startsWith('cell ')).map(cellOf);
  const slow = cells.map((c) => (c.machine === 'hosted-win' && c.arm === 'b' && c.run <= 4 ? { ...c, show_ms: 5000 + c.run, cpu_pct: 90, counted: 'no' } : c));
  const f = windowFigures(slow).find((x) => x.machine === 'hosted-win' && x.arm === 'b');
  const counted = slow.filter((c) => c.machine === 'hosted-win' && c.arm === 'b' && c.counted === 'yes').map((c) => c.show_ms).sort((x, y) => x - y);
  assert.equal(f.runs, 6);
  assert.equal(f.median_show_ms, Math.floor((counted[2] + counted[3]) / 2));
  assert.ok(f.median_show_ms < 1000, 'four uncounted runs above 5,000 ms stay out of the median');
});

test('RD-57 E3: make refuses a log line that names nocell or miss in any form that it does not read exactly', () => {
  const shapes = {
    tab: '\tnocell trial=window machine=hosted-win arm=b run=4 step=sample cause=sampler',
    'tab after the stamp': '2026-11-12T23:40:00.0000000Z \tmiss trial=window machine=hosted-win arm=b run=4 step=hide',
    'two blanks': '  miss trial=window machine=hosted-win arm=b run=4 step=hide',
    'other letter case': 'NOCELL trial=window machine=hosted-win arm=b run=4 step=sample cause=sampler',
    'Miss in a capital': 'Miss trial=window machine=hosted-win arm=b run=4 step=hide',
    'a prefix': '##[error]nocell trial=window machine=hosted-win arm=b run=4 step=sample cause=sampler',
    'a no-break space': ' miss trial=window machine=hosted-win arm=b run=4 step=hide',
  };
  for (const [name, line] of Object.entries(shapes)) {
    const made = makeFile('window', READING, [LOGS[0], `${LOGS[1]}\n${line}`]);
    assert.equal(made.ok, false, name);
    assert.match(made.why, /^log 2 holds a (miss|nocell) line/, name);
  }
});

test("RD-60: make reads a line as the trial's by its first word or by NAMED, refuses such a line that it does not read exactly, and passes over every other line", () => {
  const refused = {
    'f14 another key before trial=, first word nocell': 'nocell machine=hosted-win trial=window arm=b run=4 step=sample cause=sampler',
    'another key before trial=, first word miss': 'miss arm=b trial=window machine=hosted-win run=4 step=hide',
    'f15 no trial= at all': 'nocell machine=hosted-win arm=b run=4 step=sample cause=sampler',
    'f16 a bare nocell': 'nocell',
    'f17 a bare miss': 'miss',
    'f18 a blank before the =': 'nocell trial =window machine=hosted-win arm=b run=4',
    'E3c two blanks between the word and trial=': 'miss  trial=window machine=hosted-win arm=b run=4 step=hide',
    'a bare NOCELL': 'NOCELL',
    'Miss in a capital without trial=': 'Miss machine=hosted-win arm=b run=4 step=hide',
    'mixed case': 'NoCeLl trial=window machine=hosted-win arm=b run=4',
    'the word after another key, before trial= (NAMED)': 'step=x nocell trial=window machine=hosted-win arm=b run=4',
    'a colon after the word': 'nocell:trial=window machine=hosted-win arm=b run=4',
    'a zero-width space before trial=': 'nocell\u200btrial=window machine=hosted-win arm=b run=4',
    'a carriage return before the word': '\rmiss trial=window machine=hosted-win arm=b run=4 step=hide',
    'a cell line in capitals': 'CELL trial=window machine=hosted-win arm=b run=4',
    'a cell line after a tab': '\tcell trial=sync machine=hosted-win way=get arm=b sent=200000 last=200000 order=ok after_hide=0 growth_bytes=1 hidden_ticks=-',
  };
  for (const [name, line] of Object.entries(refused)) {
    const made = makeFile('window', READING, [LOGS[0], `${LOGS[1]}\n${line}`]);
    assert.equal(made.ok, false, name);
    assert.match(made.why, /^log 2 holds a (miss|nocell|cell) line/, name);
  }
  const passed = {
    'a node test name that holds miss as a word': 'trial node_test ✔ the channel way counts what came while hidden and a step back as a miss (0.90925ms)',
    'another key first and no trial=': 'step=x nocell arm=b run=4',
    'a sync cell read exactly with order=miss': 'cell trial=sync machine=hosted-win way=get arm=b sent=200000 last=200000 order=miss after_hide=0 growth_bytes=1 hidden_ticks=-',
    'the letters inside another word': 'dismiss xnocell remiss missing cellular',
    'a line whose first word is no run of letters': '0 miss 1 nocell',
  };
  for (const [name, line] of Object.entries(passed)) {
    const made = makeFile('window', READING, [LOGS[0], `${LOGS[1]}\n${line}`]);
    assert.equal(made.ok, true, `${name}: ${made.why}`);
  }
});

test("RD-60: job logs of the form of run 43325502435, with node test names that hold miss, give window.md back byte for byte", () => {
  const saved = (machine, ncpu) => {
    const own = lines(WINDOW).filter((l) => l.startsWith('cell ') && l.includes(` machine=${machine} `));
    const figures = windowFigures(own.map(cellOf)).map((f) => figureLine(f, { old: true }));
    return [
      '2026-11-12T23:40:00.0000000Z trial node_test ✔ a step that the system does not show is a miss line and no cell (61106.595875ms)',
      `2026-11-12T23:40:00.0000000Z trace trial=window arm=a run=1 sample load_x100=58 cpu_pct=- ncpu=${ncpu}`,
      ...own.map((l) => `2026-11-12T23:40:00.0000000Z ${l}\r`),
      ...figures.map((l) => `2026-11-12T23:40:00.0000000Z ${l}\r`),
      '2026-11-12T23:40:00.0000000Z trial node_test ✔ a sampler that finds no process is a run without a cell, which is no miss (121.244166ms)',
    ].join('\n');
  };
  const made = makeFile('window', READING, [saved('hosted-mac', 3), saved('hosted-win', 4)]);
  assert.equal(made.ok, true, made.why);
  assert.equal(made.text, WINDOW);
});
test('RD-57 E4: the old figure form is taken only for the committed cells of run 43325502435, and make writes the new form', () => {
  const again = rebuild('window', lines(WINDOW));
  const one = lines(again).find((l) => l.includes(' machine=hosted-mac arm=main run=1 '));
  const changed = rebuild('window', lines(again).map((l) => (l === one ? set(l, 'second_bytes', '1') : l)));
  const toOld = (text) => text.replace(/ max_bytes=\S+ median_show_ms=\S+ max_show_ms=\S+ show_over_1000=\S+$/gm, '');
  assert.equal(checkFile('window', changed, LOGS).ok, true, 'the new form of the changed cells passes');
  const r = checkFile('window', toOld(changed), LOGS);
  assert.equal(r.ok, false);
  assert.match(r.why, /five-key figure form/);
  assert.equal(checkFile('window', toOld(again), LOGS).ok, true, 'the committed cells in the old form pass');
  const made = makeFile('window', READING, LOGS.map((log) => toOld(log)));
  assert.equal(made.ok, true, made.why);
  assert.equal(made.text, WINDOW, 'the cells of run 43325502435 keep the form that their logs printed (RD-60)');
  const changedLogs = [LOGS[0], toOld(LOGS[1])].map((log) => log.replace(/(machine=hosted-win arm=main run=1 key_bytes=\d+ second_bytes=)\d+/, '$11'));
  const other = makeFile('window', READING, changedLogs);
  assert.equal(other.ok, true, other.why);
  assert.doesNotMatch(other.text, /^figure .* runs=\d+$/m, 'any other cells are written in the new form');
});

test('RD-57 E5 (V05): a window file numbered 1 to 9 and 11 is refused for its run form', () => {
  const all = lines(WINDOW).map((l) => (l.includes(' machine=hosted-mac arm=a2 run=10 ') ? set(l, 'run', '11') : l));
  const r = checkFile('window', rebuild('window', all), LOGS);
  assert.equal(r.ok, false);
  assert.match(r.why, /run=11 is not of the form/);
});

test('RD-57 E5 (V14): an old-form figure line with a value that its cells do not give is refused', () => {
  const r = checkFile('window', WINDOW.replace('median_bytes=86048768', 'median_bytes=86049768'), LOGS);
  assert.equal(r.ok, false);
  assert.match(r.why, /figure lines are not the figures of the cells/);
});

test('RD-57 E5 (V24): a group with no counted run prints - for show_over_1000 through windowFigures', () => {
  const cells = lines(WINDOW).filter((l) => l.startsWith('cell ')).map(cellOf);
  const none = cells.map((c) => (c.machine === 'hosted-win' && c.arm === 'b' ? { ...c, cpu_pct: 90, counted: 'no' } : c));
  const f = windowFigures(none).find((x) => x.machine === 'hosted-win' && x.arm === 'b');
  assert.deepEqual([f.runs, f.show_over_1000, f.max_show_ms, f.median_show_ms], [0, null, null, null]);
  assert.match(figureLine(f), / runs=0 max_bytes=- median_show_ms=- max_show_ms=- show_over_1000=-$/);
});
