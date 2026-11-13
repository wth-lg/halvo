import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  parseArgs, machineOf, parseSample, cpuBetween, countedOf, stepCheck, sampleCheck, windowCell, syncCell, main, LIMITS, WINDOW, SYNC,
} from '../cells.mjs';
import { fakeReaders } from '../../app/tests/fake-readers.mjs';
import { cellFormError, makeFile } from '../rule.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const fake = path.join(here, '..', '..', 'app', 'tests', 'fake-app.mjs');
const READ_LINES = `reading trial=window machine=mac arm=a2 system=macos call=background_throttling value=suspend\nreading trial=window machine=mac arm=a2 system=windows call=set_is_visible value=false\nreading trial=window machine=mac arm=a2 system=windows call=set_memory_usage_target_level value=low\nreading trial=window machine=mac arm=a2 system=macos expect=same_as_a source=wry-0.57.1 policy_set=only_when_named\nreading trial=window machine=mac figure=key_bytes system=macos command=ps\nreading trial=window machine=mac figure=second_bytes system=macos command=proc_pid_rusage\nreading trial=window machine=mac figure=cpu_ms system=macos command=ps\nreading trial=window machine=mac figure=key_bytes system=windows command=Get-Process\nreading trial=window machine=mac figure=second_bytes system=windows command=Get-Process\nreading trial=window machine=mac figure=cpu_ms system=windows command=Get-Process\n`;
const FAST = { shownMs: 50, hiddenMs: 400, destroyedMs: 200 };
const FAST_SYNC = { count: 40000, rate: 20000, hideAt: 10000, showAfterMs: 300, settleMs: 100, endMs: 3000 };

test('the timing and the limits are the lines of I11.3 and TA-16', () => {
  assert.deepEqual(WINDOW, { shownMs: 5000, hiddenMs: 330000, destroyedMs: 120000 });
  assert.deepEqual(SYNC, { count: 200000, rate: 1000, hideAt: 100000, showAfterMs: 60000, settleMs: 2000, endMs: 120000 });
  assert.ok(LIMITS.show > 30000, 'show waits up to 30 s in the app');
  for (const step of ['hide', 'destroy', 'sample', 'send', 'quit']) assert.ok(LIMITS[step] > 0 && LIMITS[step] <= 20000, step);
});

test('the arguments are a window arm with its runs, or a sync way with arm a2 or b', () => {
  assert.deepEqual(parseArgs(['--arm', 'a2', '--runs', '10']), { trial: 'window', arm: 'a2', runs: 10 });
  assert.deepEqual(parseArgs(['--runs', '1', '--arm', 'main']), { trial: 'window', arm: 'main', runs: 1 });
  assert.deepEqual(parseArgs(['--sync', 'get', '--arm', 'b']), { trial: 'sync', way: 'get', arm: 'b' });
  assert.deepEqual(parseArgs(['--arm', 'a2', '--sync', 'events']), { trial: 'sync', way: 'events', arm: 'a2' });
  for (const bad of [[], ['--arm', 'a'], ['--arm', 'c', '--runs', '1'], ['--arm', 'a', '--runs', '0'], ['--arm', 'a', '--runs', 'x'],
    ['--arm', 'a', '--runs', '1000'], ['--sync', 'get', '--arm', 'a'], ['--sync', 'get', '--arm', 'main'], ['--sync', 'poll', '--arm', 'b'],
    ['--sync', 'get', '--arm', 'b', '--runs', '2'], ['--arm', 'a', '--arm', 'b', '--runs', '1'], ['--arm'], ['--window', 'a']]) {
    assert.throws(() => parseArgs(bad), (e) => e.cause === 'usage', JSON.stringify(bad));
  }
});

test('the machine is hosted-mac or hosted-win under CI and guest otherwise', () => {
  assert.equal(machineOf({ CI: 'true' }, 'darwin'), 'hosted-mac');
  assert.equal(machineOf({ CI: 'true' }, 'win32'), 'hosted-win');
  assert.equal(machineOf({ R4_GUEST: '1' }, 'darwin'), 'guest');
  assert.equal(machineOf({ CI: '', R4_GUEST: '1' }, 'darwin'), 'guest');
});

test('a sampler line is read with its processes; any other first line is no sample', () => {
  const s = parseSample(['sample pid=7 key_bytes=100 second_bytes=90 helpers=2 load_x100=150 cpu_pct=- ncpu=3', 'proc pid=7 start=11 cpu_ms=40', 'proc pid=9 start=12 cpu_ms=5']);
  assert.deepEqual(s, { pid: 7, key_bytes: 100, second_bytes: 90, helpers: 2, load_x100: 150, cpu_pct: '-', ncpu: 3,
    procs: [{ pid: 7, start: '11', cpu_ms: 40 }, { pid: 9, start: '12', cpu_ms: 5 }] });
  assert.equal(parseSample(['sample pid=7 found=no']), null);
  assert.equal(parseSample([]), null);
});

test('the processor time between two samples counts a process that appeared and none that left', () => {
  const first = { procs: [{ pid: 1, start: 'a', cpu_ms: 100 }, { pid: 2, start: 'b', cpu_ms: 50 }, { pid: 3, start: 'c', cpu_ms: 9 }] };
  const second = { procs: [{ pid: 1, start: 'a', cpu_ms: 130 }, { pid: 3, start: 'z', cpu_ms: 4 }, { pid: 4, start: 'd', cpu_ms: 7 }] };
  assert.equal(cpuBetween(first, second), 30 + 4 + 7);
  assert.equal(cpuBetween(first, first), 0);
});

test('a run counts at a load at or under the processor count or a use at or under 50 %', () => {
  assert.equal(countedOf({ load_x100: 300, cpu_pct: '-', ncpu: 3 }), 'yes');
  assert.equal(countedOf({ load_x100: 301, cpu_pct: '-', ncpu: 3 }), 'no');
  assert.equal(countedOf({ load_x100: '-', cpu_pct: 50, ncpu: 4 }), 'yes');
  assert.equal(countedOf({ load_x100: '-', cpu_pct: 51, ncpu: 4 }), 'no');
  assert.equal(countedOf({ load_x100: '-', cpu_pct: '-', ncpu: 4 }), 'no');
});

test('a step is ok when the read is the state its answer names, a miss when it is not or no answer came, else no cell', () => {
  assert.equal(stepCheck('hide', 'ok', 'hidden'), 'ok');
  assert.equal(stepCheck('hide', 'ok', 'onscreen'), 'miss');
  assert.equal(stepCheck('destroy', 'ok', 'hidden'), 'miss');
  assert.equal(stepCheck('show', 'ok show_ms=5', 'onscreen'), 'ok');
  assert.equal(stepCheck('show', 'ok', 'onscreen'), 'miss');
  assert.equal(stepCheck('show', undefined, '-'), 'miss');
  assert.equal(stepCheck('quit', null, '-'), 'miss');
  assert.equal(stepCheck('show', 'err timeout', 'onscreen'), 'fail');
  assert.equal(stepCheck('quit', 'ok', 'alive'), 'miss');
});

test('a sample is ok for the app pid, the window in the state of the last step and a living process', () => {
  const tree = { count: 3 };
  assert.equal(sampleCheck('ok pid=5 ticks=1 frames=2', 5, 'hidden', 'hidden', tree), 'ok');
  assert.equal(sampleCheck('ok pid=5 ticks=- frames=-', 5, 'absent', 'absent', tree), 'ok');
  assert.equal(sampleCheck('ok pid=6 ticks=1 frames=2', 5, 'hidden', 'hidden', tree), 'miss');
  assert.equal(sampleCheck('ok pid=5 ticks=1 frames=2', 5, 'onscreen', 'hidden', tree), 'miss');
  assert.equal(sampleCheck('ok pid=5 ticks=1 frames=2', 5, 'hidden', 'hidden', { count: 0 }), 'miss');
  assert.equal(sampleCheck(undefined, 5, 'hidden', 'hidden', tree), 'miss');
  assert.equal(sampleCheck('err busy', 5, 'hidden', 'hidden', tree), 'fail');
});

function deps(fault = '', sampler) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-cells-'));
  const state = path.join(dir, 'state');
  process.env.FAKE_STATE = state;
  process.env.FAKE_FAULT = fault;
  const printed = [];
  let calls = 0;
  const fakeSampler = (pid) => {
    calls += 1;
    return { pid, key_bytes: 1000 + calls, second_bytes: 500 + 10 * calls, helpers: 0, load_x100: 100, cpu_pct: '-', ncpu: 3,
      procs: [{ pid, start: '1', cpu_ms: 20 * calls }] };
  };
  return { bin: fake, readers: fakeReaders(state), sampler: sampler ?? fakeSampler, print: (l) => printed.push(l), printed };
}

const cells = (d) => d.printed.filter((l) => /^(cell|figure|miss|nocell) /.test(l));

test('a window run of arm a makes a cell from the second sample, with the counts of the hidden time', async () => {
  const d = deps();
  const cell = await windowCell({ machine: 'hosted-mac', arm: 'a', run: 3, timing: FAST }, d);
  assert.ok(cell);
  assert.deepEqual(cells(d), ['cell trial=window machine=hosted-mac arm=a run=3 key_bytes=1002 second_bytes=520 helpers=0 cpu_ms=20 ticks=0 frames=0 load_x100=100 cpu_pct=- counted=yes show_ms=12']);
});

test('a window run of arm main counts the ticks of its hidden page, and arm b has no page to count', async () => {
  const main = deps();
  const m = await windowCell({ machine: 'hosted-mac', arm: 'main', run: 1, timing: FAST }, main);
  assert.ok(m.ticks >= 3 && m.frames >= 20, JSON.stringify(m));
  const b = deps();
  const cell = await windowCell({ machine: 'hosted-win', arm: 'b', run: 1, timing: FAST }, b);
  assert.equal(cell.ticks, null);
  assert.match(cells(b)[0], / ticks=- frames=- /);
});

test('a step that the system does not show is a miss line and no cell', async () => {
  for (const [arm, fault, step] of [['a', 'hide_kept', 'hide'], ['b', 'destroy_kept', 'destroy'], ['main', 'show_kept', 'show'],
    ['a2', 'wrong_pid', 'sample'], ['main', 'quit_stays', 'quit']]) {
    const d = deps(fault);
    assert.equal(await windowCell({ machine: 'hosted-mac', arm, run: 2, timing: FAST }, d), null, fault);
    assert.deepEqual(cells(d), [`miss trial=window machine=hosted-mac arm=${arm} run=2 step=${step}`], fault);
  }
});

test('a sampler that finds no process is a run without a cell, which is no miss', async () => {
  const d = deps('', () => null);
  assert.equal(await windowCell({ machine: 'hosted-mac', arm: 'a', run: 1, timing: FAST }, d), null);
  assert.deepEqual(cells(d), ['nocell trial=window machine=hosted-mac arm=a run=1 step=sample cause=sampler']);
});

test('the window main prints a cell per run and the figure of the arm', async () => {
  const d = deps();
  const status = await main({ trial: 'window', arm: 'a2', runs: 2 }, 'hosted-mac', d, FAST);
  assert.equal(status, 0);
  const lines = cells(d);
  assert.equal(lines.length, 3);
  assert.match(lines[2], /^figure trial=window machine=hosted-mac arm=a2 median_bytes=1003 range_bytes=2 runs=2 max_bytes=1004 median_show_ms=12 max_show_ms=12 show_over_1000=0$/);
});

test('a sync cell of the way get in arm a2 holds every change, none after the hide and no hidden tick', async () => {
  const d = deps();
  const cell = await syncCell({ machine: 'hosted-mac', way: 'get', arm: 'a2', timing: FAST_SYNC }, d);
  assert.ok(cell, d.printed.join('\n'));
  assert.deepEqual(cells(d), ['cell trial=sync machine=hosted-mac way=get arm=a2 sent=40000 last=40000 order=ok after_hide=0 growth_bytes=10 hidden_ticks=0']);
});

test('a hide that leaves the way get open is a miss of the hide (TA-13)', async () => {
  const d = deps('get_open');
  assert.equal(await syncCell({ machine: 'hosted-mac', way: 'get', arm: 'a2', timing: FAST_SYNC }, d), null);
  assert.deepEqual(cells(d), ['miss trial=sync machine=hosted-mac arm=a2 run=1 step=hide']);
});

test('the way channel in arm a2 counts what the core sent after the hide; arm b has no hidden ticks', async () => {
  const d = deps();
  const cell = await syncCell({ machine: 'hosted-win', way: 'channel', arm: 'a2', timing: FAST_SYNC }, d);
  assert.ok(cell && cell.after_hide > 0, d.printed.join('\n'));
  const b = deps();
  const cellB = await syncCell({ machine: 'hosted-win', way: 'events', arm: 'b', timing: FAST_SYNC }, b);
  assert.ok(cellB, b.printed.join('\n'));
  assert.equal(cellB.hidden_ticks, '-');
  assert.equal(cellB.last, 40000);
});

test('a way that loses its last change still makes a cell, from the page line of its stall', async () => {
  const d = deps('lose_last');
  const cell = await syncCell({ machine: 'hosted-mac', way: 'events', arm: 'a2', timing: { ...FAST_SYNC, endMs: 1500 } }, d);
  assert.ok(cell, d.printed.join('\n'));
  assert.equal(cell.sent, 40000);
  assert.equal(cell.last, 39999);
});

test('run.sh refuses a bad argument with usage and, on this Mac, a cell with not_hosted, before it builds anything', () => {
  const env = { ...process.env };
  delete env.CI;
  delete env.R4_GUEST;
  const run = (args) => spawnSync('sh', [path.join(here, '..', 'run.sh'), ...args], { encoding: 'utf8', env, timeout: 60000 });
  for (const [args, cause] of [[['--arm', 'a'], 'usage'], [['--sync', 'get', '--arm', 'a'], 'usage'], [['--arm', 'a', '--runs', '1'], 'not_hosted'],
    [['--sync', 'get', '--arm', 'b'], 'not_hosted']]) {
    const r = run(args);
    assert.equal(r.status, 2, JSON.stringify(args));
    assert.equal(r.stdout, '');
    assert.match(r.stderr, new RegExp(`^REFUSED ${cause}: [^\\n]+\\n$`));
  }
  const help = run(['--help']);
  assert.equal(help.status, 0);
  assert.match(help.stdout, /^usage: sh tools\/r4-trials\/window\/run\.sh/);
});

test('an err answer while the system shows what an ok answer would have shown is a miss', () => {
  assert.equal(stepCheck('hide', 'err timeout', 'hidden'), 'miss');
  assert.equal(stepCheck('destroy', 'err timeout', 'absent'), 'miss');
  assert.equal(stepCheck('show', 'err no_window', 'onscreen'), 'miss');
  assert.equal(stepCheck('hide', 'err timeout', 'onscreen'), 'fail');
  assert.equal(stepCheck('show', 'err timeout', 'onscreen'), 'fail', 'a show timeout claims no frame, not a state');
});

test('a sample compares the window and the tree before its answer, and counts with no window are a miss', () => {
  const tree = { count: 3 };
  assert.equal(sampleCheck('ok pid=5 ticks=12 frames=30', 5, 'absent', 'absent', tree), 'miss');
  assert.equal(sampleCheck('err busy', 5, 'onscreen', 'hidden', tree), 'miss');
  assert.equal(sampleCheck('err busy', 5, 'hidden', 'hidden', { count: 0 }), 'miss');
  assert.equal(sampleCheck('err busy', 5, 'hidden', 'hidden', tree), 'fail');
});

test('the window run prints the processor count of each sample, and its lines pass every rule of make but the count of 10 runs', async () => {
  const d = deps();
  assert.equal(await main({ trial: 'window', arm: 'a', runs: 2 }, 'hosted-mac', d, FAST), 0);
  assert.ok(d.printed.some((l) => /^trace trial=window arm=a run=2 sample load_x100=100 cpu_pct=- ncpu=3$/.test(l)), d.printed.join('\n'));
  const made = makeFile('window', READ_LINES, [d.printed.join('\n')]);
  assert.equal(made.ok, false);
  assert.match(made.why, /^hosted-mac arm main holds 0 cells, not 10$/);
  const printed = d.printed.filter((l) => l.startsWith('cell '));
  assert.equal(printed.length, 2);
  for (const line of printed) assert.equal(cellFormError('window', line), null, line);
});

test('a sync cell puts the app lines that its keys come from into the log', async () => {
  const d = deps();
  assert.ok(await syncCell({ machine: 'hosted-mac', way: 'get', arm: 'a2', timing: FAST_SYNC }, d));
  for (const want of [/^trace trial=sync way=get arm=a2 core send_done sent=40000$/, /^trace trial=sync way=get arm=a2 page sync end last=40000 order=ok /,
    /^trace trial=sync way=get arm=a2 page sync closed last=\d+$/, /^trace trial=sync way=get arm=a2 page window shown after_hide=0 hidden_ticks=0 hide_seen=yes$/,
    /^trace trial=sync way=get arm=a2 page sync mark=10000$/]) {
    assert.ok(d.printed.some((l) => want.test(l)), `${want}\n${d.printed.join('\n')}`);
  }
});

test('a change at or under the last after the end line makes the order a miss', async () => {
  const d = deps('repeat_last');
  const cell = await syncCell({ machine: 'hosted-mac', way: 'events', arm: 'b', timing: FAST_SYNC }, d);
  assert.ok(cell, d.printed.join('\n'));
  assert.equal(cell.order, 'miss');
});

test('a page that never saw the hide gives no sync cell', async () => {
  const d = deps('hide_unseen');
  assert.equal(await syncCell({ machine: 'hosted-mac', way: 'channel', arm: 'a2', timing: FAST_SYNC }, d), null);
  assert.deepEqual(cells(d), ['nocell trial=sync machine=hosted-mac way=channel arm=a2 run=1 step=hide cause=hide_unseen']);
});

test('E3: in arm b a step back of the first page makes the order a miss, as it does in arm a2', async () => {
  for (const [way, arm] of [['events', 'b'], ['channel', 'b'], ['events', 'a2']]) {
    const d = deps('repeat_early');
    const cell = await syncCell({ machine: 'hosted-mac', way, arm, timing: FAST_SYNC }, d);
    assert.ok(cell, d.printed.join('\n'));
    assert.equal(cell.order, 'miss', `${way} ${arm}`);
    assert.ok(d.printed.some((l) => new RegExp(`^trace trial=sync way=${way} arm=${arm} page sync order_miss seq=4999 last=5000$`).test(l)), d.printed.join('\n'));
  }
});

test('E4: show_ms is that of the second show', async () => {
  const d = deps('shows_differ');
  const cell = await windowCell({ machine: 'hosted-mac', arm: 'a', run: 1, timing: FAST }, d);
  assert.ok(cell, d.printed.join('\n'));
  assert.equal(cell.show_ms, 22);
});

test('E4: a quit is a miss while a process of the tree lives, and unless the app ends with 0', async () => {
  for (const fault of ['child_alive', 'exit_one']) {
    const d = deps(fault);
    try {
      assert.equal(await windowCell({ machine: 'hosted-mac', arm: 'main', run: 1, timing: FAST }, d), null, fault);
      assert.deepEqual(cells(d), ['miss trial=window machine=hosted-mac arm=main run=1 step=quit'], fault);
    } finally {
      const child = Number(fs.readFileSync(process.env.FAKE_STATE, 'utf8').trim().split(' ')[2]);
      try {
        if (child) process.kill(child, 'SIGKILL');
      } catch {
        assert.equal(fault, 'exit_one', 'the child of child_alive outlives the app until this test ends it');
      }
    }
  }
});

test('E5: a window arm whose run gave no cell prints its nocell line and exits 1, and run.sh ends with that exit', async () => {
  const d = deps();
  let calls = 0;
  const sampler = (pid) => {
    calls += 1;
    return calls === 1 ? null : { pid, key_bytes: 1000, second_bytes: 500, helpers: 0, load_x100: 100, cpu_pct: '-', ncpu: 3, procs: [{ pid, start: '1', cpu_ms: calls }] };
  };
  const status = await main({ trial: 'window', arm: 'a', runs: 2 }, 'hosted-mac', { ...d, sampler }, FAST);
  assert.equal(status, 1);
  const out = cells(d);
  assert.equal(out[0], 'nocell trial=window machine=hosted-mac arm=a run=1 step=sample cause=sampler');
  assert.match(out[1], /^cell trial=window machine=hosted-mac arm=a run=2 /);
  const runSh = fs.readFileSync(path.join(here, '..', 'run.sh'), 'utf8').trimEnd().split('\n');
  assert.equal(runSh.at(-1), 'node "$here/cells.mjs" "$@"');
});

test('E6 (V34): helpers, key_bytes, second_bytes and the load of a cell are those of the second sample', async () => {
  const d = deps();
  let calls = 0;
  const sampler = (pid) => {
    calls += 1;
    return calls === 1
      ? { pid, key_bytes: 9000, second_bytes: 8000, helpers: 3, load_x100: 900, cpu_pct: '-', ncpu: 3, procs: [{ pid, start: '1', cpu_ms: 5 }] }
      : { pid, key_bytes: 1000, second_bytes: 700, helpers: 0, load_x100: 100, cpu_pct: '-', ncpu: 3, procs: [{ pid, start: '1', cpu_ms: 9 }] };
  };
  const cell = await windowCell({ machine: 'hosted-mac', arm: 'b', run: 1, timing: FAST }, { ...d, sampler });
  assert.ok(cell, d.printed.join('\n'));
  assert.deepEqual([cell.helpers, cell.key_bytes, cell.second_bytes, cell.load_x100, cell.counted], [0, 1000, 700, 100, 'yes']);
});

test('E6 (V44): a sample whose window the readers show in another state than its step left is a miss of the sample', async () => {
  const d = deps();
  let hiddenSeen = false;
  const readers = {
    ...d.readers,
    window(pid) {
      const read = d.readers.window(pid);
      if (hiddenSeen) return { ...read, state: 'onscreen' };
      if (read.state === 'hidden') hiddenSeen = true;
      return read;
    },
  };
  assert.equal(await windowCell({ machine: 'hosted-mac', arm: 'a', run: 1, timing: FAST }, { ...d, readers }), null);
  assert.deepEqual(cells(d), ['miss trial=window machine=hosted-mac arm=a run=1 step=sample']);
});

test('E7 (V28): counts of frames where the window is absent are a miss even with ticks -', () => {
  assert.equal(sampleCheck('ok pid=5 ticks=- frames=3', 5, 'absent', 'absent', { count: 2 }), 'miss');
  assert.equal(sampleCheck('ok pid=5 ticks=3 frames=-', 5, 'absent', 'absent', { count: 2 }), 'miss');
});

test('E8 (N1): a closed line before the hide does not stand for one after it', async () => {
  const d = deps('closed_early,get_open');
  assert.equal(await syncCell({ machine: 'hosted-mac', way: 'get', arm: 'a2', timing: FAST_SYNC }, d), null);
  assert.deepEqual(cells(d), ['miss trial=sync machine=hosted-mac arm=a2 run=1 step=hide']);
});
