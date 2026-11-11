import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  armLine, checkHosted, parseAnswer, parseArgs, parseEnd, quietKey, Refused, SENT, SENT_B, syncKey, SYSTEM
} from '../selfcheck.mjs';

const script = fileURLToPath(new URL('../selfcheck.mjs', import.meta.url));

function run(args) {
  const env = { ...process.env };
  delete env.CI;
  delete env.R4_GUEST;
  return spawnSync(process.execPath, [script, ...args], { env, encoding: 'utf8', timeout: 20000 });
}

test('the self-check refuses with not_hosted where neither CI nor R4_GUEST is set', () => {
  for (const args of [[], ['--arm', 'b'], ['--trace', '--bin', 'no-such-binary']]) {
    const ended = run(args);
    assert.equal(ended.status, 2);
    assert.equal(ended.stdout, '');
    assert.equal(ended.stderr, 'REFUSED not_hosted: neither CI nor R4_GUEST is set\n');
  }
});

test('the self-check refuses a bad argument with usage before anything else', () => {
  for (const args of [['--arm', 'c'], ['--arm'], ['--window'], ['--bin'], ['--arm', 'a', '--arm', 'a'], ['--bin', 'x', '--bin', 'y']]) {
    const ended = run(args);
    assert.equal(ended.status, 2, String(args));
    assert.equal(ended.stdout, '');
    assert.match(ended.stderr, /^REFUSED usage: \S.*\n$/);
    assert.equal(ended.stderr.split('\n').length, 2);
  }
});

test('hosted means CI or R4_GUEST set to a value', () => {
  assert.throws(() => checkHosted({}), (e) => e instanceof Refused && e.cause === 'not_hosted');
  assert.throws(() => checkHosted({ CI: '', R4_GUEST: '' }), Refused);
  checkHosted({ CI: 'true' });
  checkHosted({ R4_GUEST: '1' });
});

test('the arguments name the binary and the arms, all four by default', () => {
  assert.deepEqual(parseArgs([]), { bin: null, arms: ['main', 'a', 'a2', 'b'], trace: false });
  assert.deepEqual(parseArgs(['--bin', 'x', '--arm', 'b', '--arm', 'a2', '--trace']),
    { bin: 'x', arms: ['b', 'a2'], trace: true });
});

test('answer and end lines are read as the app prints them', () => {
  assert.deepEqual(parseAnswer('ok show_ms=12'), { word: 'ok', cause: null, pairs: { show_ms: '12' } });
  assert.deepEqual(parseAnswer('ok pid=7 ticks=- frames=-'), { word: 'ok', cause: null, pairs: { pid: '7', ticks: '-', frames: '-' } });
  assert.deepEqual(parseAnswer('err timeout'), { word: 'err', cause: 'timeout', pairs: {} });
  assert.equal(parseAnswer(undefined), null);
  assert.deepEqual(parseEnd('page sync end last=10000 order=ok after_hide=0 hidden_ticks=3'), { last: 10000, order: 'ok', afterHide: 0 });
  assert.equal(parseEnd('page sync end last=1 order=ok'), null);
});

test('a sync key needs every change in order, the end of the send, and for get nothing after the hide', () => {
  const end = { last: SENT, order: 'ok', afterHide: 0 };
  for (const way of ['get', 'channel', 'events']) assert.equal(syncKey(way, end, 'done', true), 'ok');
  assert.equal(syncKey('get', end, 'done', false), 'miss');
  assert.equal(syncKey('channel', end, 'done', false), 'ok');
  assert.equal(syncKey('get', { ...end, afterHide: 1 }, 'done', true), 'miss');
  assert.equal(syncKey('channel', { ...end, afterHide: 40 }, 'done', true), 'ok');
  assert.equal(syncKey('events', { ...end, last: SENT - 1 }, 'done', true), 'miss');
  assert.equal(syncKey('channel', { ...end, order: 'miss' }, 'done', true), 'miss');
  assert.equal(syncKey('get', null, 'done', true), 'miss');
  assert.equal(syncKey('get', end, null, true), 'miss');
  assert.equal(syncKey('channel', { ...end, last: SENT_B }, 'done', true, SENT_B), 'ok');
  assert.equal(syncKey('channel', end, 'done', true, SENT_B), 'miss');
});

test('quiet reads the core quiet lines of arm a2 and wants none elsewhere', () => {
  const mac = ['core quiet system=macos call=background_throttling result=ok'];
  const done = { made: 1, hides: 1, shows: 3 };
  assert.equal(quietKey('a2', 'macos', mac, done), 'ok');
  assert.equal(quietKey('a2', 'macos', [], done), 'miss');
  assert.equal(quietKey('a2', 'macos', ['core quiet system=macos call=background_throttling result=err'], done), 'miss');
  const win = [];
  for (let i = 0; i < 4; i += 1) {
    win.push('core quiet system=windows call=set_is_visible result=ok');
    win.push('core quiet system=windows call=set_memory_usage_target_level result=ok');
  }
  assert.equal(quietKey('a2', 'windows', win, done), 'ok');
  assert.equal(quietKey('a2', 'windows', win.slice(1), done), 'miss');
  assert.equal(quietKey('a2', 'windows', [...win.slice(1), 'core quiet system=windows call=set_is_visible result=err'], done), 'miss');
  assert.equal(quietKey('main', 'windows', [], done), '-');
  assert.equal(quietKey('a', 'macos', mac, done), 'miss');
});

test('the line of an arm has its keys in the ruled order', () => {
  const keys = { show: 'ok', hide: '-', destroy: 'ok', counts: 'ok', pid: 'ok', quiet: '-', sync_get: 'ok', sync_channel: 'miss', sync_events: 'ok', quit: 'ok' };
  assert.equal(armLine('b', keys),
    `selfcheck arm=b system=${SYSTEM} show=ok hide=- destroy=ok counts=ok pid=ok quiet=- sync_get=ok sync_channel=miss sync_events=ok quit=ok`);
});
