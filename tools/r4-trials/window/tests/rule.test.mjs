import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  parseLine, median, windowCellLine, syncCellLine, windowFigures, figureLine, windowVerdict,
  syncPasses, syncVerdict, rawLines, makeFile, checkFile, DESTROY_SAVING, SYNC_GROWTH,
} from '../rule.mjs';

const MiB = 1024 * 1024;
const RW = `reading trial=window machine=mac arm=a2 system=macos call=background_throttling value=suspend\nreading trial=window machine=mac arm=a2 system=windows call=set_is_visible value=false\nreading trial=window machine=mac arm=a2 system=windows call=set_memory_usage_target_level value=low\nreading trial=window machine=mac arm=a2 system=macos expect=same_as_a source=wry-0.57.1 policy_set=only_when_named\nreading trial=window machine=mac figure=key_bytes system=macos command=ps\nreading trial=window machine=mac figure=second_bytes system=macos command=proc_pid_rusage\nreading trial=window machine=mac figure=cpu_ms system=macos command=ps\nreading trial=window machine=mac figure=key_bytes system=windows command=Get-Process\nreading trial=window machine=mac figure=second_bytes system=windows command=Get-Process\nreading trial=window machine=mac figure=cpu_ms system=windows command=Get-Process\n`;
const RS = 'reading trial=sync machine=mac object=__TAURI_INTERNALS__\nreading trial=sync machine=mac way=channel order_by=index\n';

function wcell(machine, arm, run, over = {}) {
  return {
    trial: 'window', machine, arm, run, key_bytes: 200 * MiB, second_bytes: 150 * MiB, helpers: arm === 'b' ? 0 : 2,
    cpu_ms: 10, ticks: '-', frames: '-', load_x100: machine === 'hosted-mac' ? 100 : '-',
    cpu_pct: machine === 'hosted-win' ? 10 : '-', counted: 'yes', show_ms: 300, ...over,
  };
}

function machineCells(machine, { a = 200, a2 = 200, b = 150, spread = 1, helpers = 0, show = 300, counted = 10 } = {}) {
  const cells = [];
  for (const [arm, mib] of [['main', 210], ['a', a], ['a2', a2], ['b', b]]) {
    for (let i = 1; i <= 10; i += 1) {
      cells.push(wcell(machine, arm, i, {
        key_bytes: Math.round((mib + (i % 3) * spread) * MiB),
        helpers: arm === 'b' ? helpers : 2,
        show_ms: show,
        counted: i <= counted ? 'yes' : 'no',
      }));
    }
  }
  return cells;
}

test('a line is read as its kind and its pairs, after a carriage return is taken off', () => {
  assert.deepEqual(parseLine('cell trial=window machine=hosted-mac arm=a run=3 key_bytes=7\r'),
    { kind: 'cell', pairs: { trial: 'window', machine: 'hosted-mac', arm: 'a', run: '3', key_bytes: '7' } });
  assert.equal(parseLine('verdict sync=get').kind, 'verdict');
  assert.equal(parseLine('page sync end last=1'), null);
  assert.equal(parseLine(''), null);
});

test('the median of an even count is the floor of the mean of the two middle values', () => {
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(median([4, 1, 3, 2]), 2);
  assert.equal(median([1, 2]), 1);
  assert.equal(median([5]), 5);
  assert.equal(median([]), null);
});

test('the cell lines have the keys of I11.3 in their order', () => {
  assert.equal(windowCellLine(wcell('hosted-mac', 'a2', 4, { ticks: 0, frames: 0 })),
    `cell trial=window machine=hosted-mac arm=a2 run=4 key_bytes=${200 * MiB} second_bytes=${150 * MiB} helpers=2 cpu_ms=10 ticks=0 frames=0 load_x100=100 cpu_pct=- counted=yes show_ms=300`);
  assert.equal(syncCellLine({ machine: 'hosted-win', way: 'get', arm: 'b', sent: 200000, last: 200000, order: 'ok', after_hide: 0, growth_bytes: -5, hidden_ticks: '-' }),
    'cell trial=sync machine=hosted-win way=get arm=b sent=200000 last=200000 order=ok after_hide=0 growth_bytes=-5 hidden_ticks=-');
});

test('a figure is the median and the range of key_bytes over the counted runs of one machine and arm', () => {
  const cells = [
    wcell('hosted-mac', 'a', 1, { key_bytes: 30 }), wcell('hosted-mac', 'a', 2, { key_bytes: 10 }),
    wcell('hosted-mac', 'a', 3, { key_bytes: 1000, counted: 'no' }), wcell('hosted-mac', 'a', 4, { key_bytes: 20 }),
    wcell('hosted-win', 'a', 1, { key_bytes: 7 }),
  ];
  assert.deepEqual(windowFigures(cells), [
    { machine: 'hosted-mac', arm: 'a', median_bytes: 20, range_bytes: 20, runs: 3, max_bytes: 30, median_show_ms: 300, max_show_ms: 300, show_over_1000: 0 },
    { machine: 'hosted-win', arm: 'a', median_bytes: 7, range_bytes: 0, runs: 1, max_bytes: 7, median_show_ms: 300, max_show_ms: 300, show_over_1000: 0 },
  ]);
  assert.equal(figureLine(windowFigures(cells)[0]), 'figure trial=window machine=hosted-mac arm=a median_bytes=20 range_bytes=20 runs=3 max_bytes=30 median_show_ms=300 max_show_ms=300 show_over_1000=0');
  assert.equal(figureLine(windowFigures(cells)[0], { old: true }), 'figure trial=window machine=hosted-mac arm=a median_bytes=20 range_bytes=20 runs=3');
  assert.equal(figureLine({ machine: 'hosted-mac', arm: 'b', median_bytes: null, range_bytes: null, runs: 0 }),
    'figure trial=window machine=hosted-mac arm=b median_bytes=- range_bytes=- runs=0 max_bytes=- median_show_ms=- max_show_ms=- show_over_1000=-');
});

test('destroy needs all three lines of the rule; any one off gives hidden', () => {
  const win = machineCells('hosted-win');
  const v = (mac) => windowVerdict([...mac, ...win]);
  assert.equal(DESTROY_SAVING, 16777216);
  assert.equal(v(machineCells('hosted-mac', { b: 150 })).hosted_mac, 'destroy');
  assert.equal(v(machineCells('hosted-mac', { b: 150 })).quiet_mac, '-');
  assert.equal(v(machineCells('hosted-mac', { b: 185 })).hosted_mac, 'hidden', 'saving under 16 MiB');
  assert.equal(v(machineCells('hosted-mac', { b: 150, spread: 30 })).hosted_mac, 'hidden', 'saving not beyond the range of a2');
  assert.equal(v(machineCells('hosted-mac', { b: 150, helpers: 1 })).hosted_mac, 'hidden', 'a helper alive');
  assert.equal(v(machineCells('hosted-mac', { b: 150, show: 1001 })).hosted_mac, 'hidden', 'second show over 1,000 ms');
  assert.equal(v(machineCells('hosted-mac', { b: 150, show: 1000 })).hosted_mac, 'destroy', 'second show at 1,000 ms');
});

test('the saving is compared exactly with 16 MiB and strictly with the range', () => {
  const at = (saving, range) => {
    const cells = [];
    for (let i = 1; i <= 8; i += 1) {
      const wide = i === 1 ? range : 0;
      cells.push(wcell('hosted-mac', 'a2', i, { key_bytes: 100 * MiB + wide }));
      cells.push(wcell('hosted-mac', 'a', i, { key_bytes: 100 * MiB }));
      cells.push(wcell('hosted-mac', 'b', i, { key_bytes: 100 * MiB - saving }));
    }
    return windowVerdict(cells).hosted_mac;
  };
  assert.equal(at(DESTROY_SAVING, 0), 'destroy');
  assert.equal(at(DESTROY_SAVING - 1, 0), 'hidden');
  assert.equal(at(DESTROY_SAVING, DESTROY_SAVING), 'hidden', 'not more than the range');
  assert.equal(at(DESTROY_SAVING + 1, DESTROY_SAVING), 'destroy');
});

test('fewer than 8 counted runs in a, a2 or b is inconclusive; main does not count', () => {
  const win = machineCells('hosted-win');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { counted: 7 }), ...win]).hosted_mac, 'inconclusive');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { counted: 8 }), ...win]).hosted_mac, 'destroy');
  for (const arm of ['a', 'a2', 'b']) {
    const cells = machineCells('hosted-mac').map((c) => (c.arm === arm && c.run > 7 ? { ...c, counted: 'no' } : c));
    assert.equal(windowVerdict([...cells, ...win]).hosted_mac, 'inconclusive', arm);
  }
  const noMain = machineCells('hosted-mac').map((c) => (c.arm === 'main' ? { ...c, counted: 'no' } : c));
  assert.equal(windowVerdict([...noMain, ...win]).hosted_mac, 'destroy');
  assert.deepEqual(windowVerdict(machineCells('hosted-mac')).hosted_win, 'inconclusive');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { counted: 7 }), ...win]).quiet_mac, '-');
});

test('quiet is yes only with hidden and a2 below a by more than the range of a', () => {
  const win = machineCells('hosted-win');
  const hidden = { b: 199 };
  assert.equal(windowVerdict([...machineCells('hosted-mac', { ...hidden, a: 200, a2: 180 }), ...win]).quiet_mac, 'yes');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { ...hidden, a: 200, a2: 197 }), ...win]).quiet_mac, 'yes', 'beyond the range of a');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { ...hidden, a: 200, a2: 198 }), ...win]).quiet_mac, 'no', 'equal to the range of a');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { ...hidden, a: 200, a2: 199, spread: 1 }), ...win]).quiet_mac, 'no');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { ...hidden, a: 200, a2: 200 }), ...win]).quiet_mac, 'no');
  assert.equal(windowVerdict([...machineCells('hosted-mac', { b: 230, a: 200, a2: 220 }), ...win]).quiet_mac, 'no');
  const v = windowVerdict([...machineCells('hosted-mac', hidden), ...machineCells('hosted-win', { b: 199, a2: 150 })]);
  assert.equal(v.line, 'verdict window hosted_mac=hidden hosted_win=hidden quiet_mac=no quiet_win=yes');
});

function scell(machine, way, arm, over = {}) {
  return { trial: 'sync', machine, way, arm, sent: 200000, last: 200000, order: 'ok', after_hide: 0, growth_bytes: 1000, hidden_ticks: arm === 'b' ? '-' : 0, ...over };
}

function allSync(over = () => ({})) {
  const cells = [];
  for (const machine of ['hosted-mac', 'hosted-win']) {
    for (const way of ['get', 'channel', 'events']) {
      for (const arm of ['a2', 'b']) cells.push(scell(machine, way, arm, over(machine, way, arm)));
    }
  }
  return cells;
}

test('a way passes on a machine when both arms keep every line of the pass rule', () => {
  assert.equal(SYNC_GROWTH, 8388608);
  const ok = (over) => syncPasses([scell('hosted-mac', 'get', 'a2', over), scell('hosted-mac', 'get', 'b')], 'hosted-mac', 'get');
  assert.equal(ok({}), true);
  assert.equal(ok({ last: 199999 }), false);
  assert.equal(ok({ order: 'miss' }), false);
  assert.equal(ok({ after_hide: 1 }), false);
  assert.equal(ok({ hidden_ticks: 1 }), false);
  assert.equal(ok({ hidden_ticks: '-' }), true);
  assert.equal(ok({ growth_bytes: SYNC_GROWTH }), true);
  assert.equal(ok({ growth_bytes: SYNC_GROWTH + 1 }), false);
  assert.equal(ok({ growth_bytes: -SYNC_GROWTH * 4 }), true);
  assert.equal(syncPasses([scell('hosted-mac', 'get', 'a2')], 'hosted-mac', 'get'), false, 'arm b has no cell');
  assert.equal(syncPasses([scell('hosted-mac', 'get', 'b')], 'hosted-mac', 'get'), false, 'arm a2 has no cell');
  assert.equal(syncPasses([scell('hosted-mac', 'get', 'a2'), scell('hosted-mac', 'get', 'b'), scell('hosted-mac', 'get', 'b', { order: 'miss' })], 'hosted-mac', 'get'), false, 'every cell of an arm');
});

test('the first way in the order get, channel, events that passes on both machines wins', () => {
  assert.equal(syncVerdict(allSync()).line, 'verdict sync=get');
  assert.equal(syncVerdict(allSync((m, w) => (w === 'get' && m === 'hosted-win' ? { last: 1 } : {}))).line, 'verdict sync=channel');
  assert.equal(syncVerdict(allSync((m, w) => (w !== 'events' ? { after_hide: 3 } : {}))).line, 'verdict sync=events');
  assert.equal(syncVerdict(allSync(() => ({ order: 'miss' }))).line, 'verdict sync=none');
  assert.equal(syncVerdict(allSync().filter((c) => c.machine === 'hosted-mac')).line, 'verdict sync=none');
});

test('raw lines lose the time stamp of a job log and a carriage return', () => {
  assert.deepEqual(rawLines('2026-11-12T10:00:00.1234567Z cell trial=sync a=1\r\nplain line\n﻿2026-11-12T10:00:01.0000000Z verdict sync=get\n'),
    ['cell trial=sync a=1', 'plain line', 'verdict sync=get']);
});

function windowLog(machine, cells) {
  const figs = windowFigures(cells).map(figureLine);
  const trace = machine === 'hosted-mac' ? ['trace trial=window machine=hosted-mac arm=a run=1 sample=2 load_x100=100 cpu_pct=- ncpu=3'] : [];
  return [...trace, ...cells.map(windowCellLine), ...figs].map((l) => `2026-11-12T10:00:00.0000000Z ${l}\r`).join('\n');
}

test('a result file is made of the reading lines, the cells, the figures of the logs and the verdict', () => {
  const reading = `${RW}reading trial=sync machine=mac object=x\n`;
  const mac = machineCells('hosted-mac');
  const win = machineCells('hosted-win', { b: 199 });
  const made = makeFile('window', reading, [windowLog('hosted-mac', mac), windowLog('hosted-win', win)]);
  assert.equal(made.ok, true, made.why);
  const lines = made.text.trimEnd().split('\n');
  assert.equal(lines[0], RW.split('\n')[0]);
  assert.equal(lines.length, 10 + 80 + 8 + 1);
  assert.equal(lines.at(-1), 'verdict window hosted_mac=destroy hosted_win=hidden quiet_mac=- quiet_win=no');
  assert.equal(checkFile('window', made.text).ok, true);
});

test('a log with a miss line, or a figure that its cells do not give, makes no result file', () => {
  const mac = machineCells('hosted-mac');
  const log = windowLog('hosted-mac', mac);
  const missed = makeFile('window', RW, [`${log}\nmiss trial=window machine=hosted-mac arm=a run=2 step=hide`]);
  assert.equal(missed.ok, false);
  assert.match(missed.why, /miss/);
  const wrong = makeFile('window', RW, [log.replace(/median_bytes=\d+/, 'median_bytes=1')]);
  assert.equal(wrong.ok, false);
  assert.match(wrong.why, /figure/);
  const noFigure = makeFile('window', RW, [log.split('\n').filter((l) => !l.includes(' figure ')).join('\n')]);
  assert.equal(noFigure.ok, false);
});

test('a result file is checked for its order, its figures and its verdict', () => {
  const made = makeFile('sync', RS, [allSync().map(syncCellLine).join('\n')]);
  assert.equal(made.ok, true, made.why);
  assert.equal(made.text.trimEnd().split('\n').at(-1), 'verdict sync=get');
  assert.equal(checkFile('sync', made.text).ok, true);
  assert.equal(checkFile('sync', made.text.replace('verdict sync=get', 'verdict sync=channel')).ok, false);
  const lines = made.text.trimEnd().split('\n');
  assert.equal(checkFile('sync', [lines[2], lines[0], lines[1], ...lines.slice(3)].join('\n')).ok, false, 'a reading line after a cell');
  assert.equal(checkFile('sync', `${made.text}extra\n`).ok, false, 'a line of no kind');
  const w = makeFile('window', RW, [windowLog('hosted-mac', machineCells('hosted-mac')), winLog()]);
  assert.equal(checkFile('window', w.text.replace(/median_bytes=\d+/, 'median_bytes=2')).ok, false);
});

const winLog = () => windowLog('hosted-win', machineCells('hosted-win'));

const MAC_TRACE = '2026-11-12T10:00:00.0000000Z trace trial=window machine=hosted-mac arm=a run=1 sample=2 load_x100=100 cpu_pct=- ncpu=3';

test('the figures of a result file are one per machine and arm over all its cells, each found in the logs', () => {
  const mac = machineCells('hosted-mac');
  const win = machineCells('hosted-win', { b: 199 });
  const swapped = makeFile('window', RW, [windowLog('hosted-win', win), `${windowLog('hosted-mac', mac)}\n${MAC_TRACE}`]);
  assert.equal(swapped.ok, true, swapped.why);
  assert.equal(checkFile('window', swapped.text).ok, true);
  const b = mac.filter((c) => c.arm === 'b');
  const halves = [b.slice(0, 5), b.slice(5)].map((cells) => `${windowLog('hosted-mac', cells)}\n${MAC_TRACE}`);
  const split = makeFile('window', RW, halves);
  assert.equal(split.ok, false);
  assert.match(split.why, /figure/);
});

test('a run that appears twice makes no result file and fails the check', () => {
  const mac = machineCells('hosted-mac');
  const twice = [...mac, mac.find((c) => c.arm === 'b' && c.run === 3)];
  const made = makeFile('window', RW, [`${windowLog('hosted-mac', twice)}\n${MAC_TRACE}`]);
  assert.equal(made.ok, false);
  assert.match(made.why, /twice/);
  const good = makeFile('window', RW, [`${windowLog('hosted-mac', mac)}\n${MAC_TRACE}`, winLog()]);
  const lines = good.text.split('\n');
  const cell = lines.find((l) => l.includes(' arm=b run=3 '));
  assert.equal(checkFile('window', good.text.replace(cell, `${cell}\n${cell}`)).ok, false);
  const s = makeFile('sync', RS, [[...allSync(), scell('hosted-mac', 'get', 'b')].map(syncCellLine).join('\n')]);
  assert.equal(s.ok, false);
});

test('counted follows from the use on Windows and from the load and the processor count on macOS', () => {
  const win = machineCells('hosted-win').map((c) => (c.run === 1 ? { ...c, cpu_pct: 99 } : c));
  const wrongWin = makeFile('window', RW, [windowLog('hosted-win', win)]);
  assert.equal(wrongWin.ok, false);
  assert.match(wrongWin.why, /counted/);
  const winText = makeFile('window', RW, [`${windowLog('hosted-mac', machineCells('hosted-mac'))}\n${MAC_TRACE}`, winLog()]).text;
  assert.equal(checkFile('window', winText.replace(/cpu_pct=10 counted=yes/, 'cpu_pct=51 counted=yes')).ok, false);
  const mac = machineCells('hosted-mac').map((c) => (c.run === 1 ? { ...c, load_x100: 301 } : c));
  assert.equal(makeFile('window', RW, [`${windowLog('hosted-mac', mac)}\n${MAC_TRACE}`]).ok, false);
  const atLine = machineCells('hosted-mac').map((c) => (c.run === 1 ? { ...c, load_x100: 300 } : c));
  assert.equal(makeFile('window', RW, [`${windowLog('hosted-mac', atLine)}\n${MAC_TRACE}`, winLog()]).ok, true);
  const noCount = makeFile('window', RW, [windowLog('hosted-mac', machineCells('hosted-mac')).split('\n').filter((l) => !l.includes(' trace ')).join('\n')]);
  assert.equal(noCount.ok, false);
  assert.match(noCount.why, /processor count/);
});

test('a sync result file needs its object reading line before the cells and holds no figure line', () => {
  const none = makeFile('sync', '', [allSync().map(syncCellLine).join('\n')]);
  assert.equal(none.ok, false);
  const made = makeFile('sync', RS, [allSync().map(syncCellLine).join('\n')]);
  assert.equal(checkFile('sync', made.text.replace(/^reading trial=sync machine=mac object=__TAURI_INTERNALS__\n/, '')).ok, false);
  const lines = made.text.trimEnd().split('\n');
  assert.equal(checkFile('sync', [...lines.slice(0, -1), 'figure trial=sync machine=hosted-mac arm=b median_bytes=1 range_bytes=0 runs=1', lines.at(-1)].join('\n')).ok, false);
});

test('make refuses what the check of its own file refuses: a reading set without the line of TA-5', () => {
  const made = makeFile('window', RW.split('\n').filter((l) => !l.includes(' expect=same_as_a ')).join('\n'), [windowLog('hosted-mac', machineCells('hosted-mac'))]);
  assert.equal(made.ok, false);
  assert.match(made.why, /no reading line/);
});
