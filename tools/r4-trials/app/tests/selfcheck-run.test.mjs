import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { windowRun } from '../selfcheck.mjs';
import { fakeReaders } from './fake-readers.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const fake = path.join(here, 'fake-app.mjs');

async function run(arm, fault = '') {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-selfcheck-run-'));
  const state = path.join(dir, 'state');
  process.env.FAKE_STATE = state;
  process.env.FAKE_FAULT = fault;
  try {
    return await windowRun({ bin: fake, trace: false }, arm, dir, fakeReaders(state));
  } finally {
    delete process.env.FAKE_STATE;
    delete process.env.FAKE_FAULT;
  }
}

test('the window run reads the system through the readers it is handed, and every key is ok for a sound app', async () => {
  assert.deepEqual(await run('main'), { show: 'ok', hide: 'ok', destroy: '-', counts: 'ok', pid: 'ok', quiet: '-', quit: 'ok' });
  assert.deepEqual(await run('a2'), { show: 'ok', hide: 'ok', destroy: '-', counts: 'ok', pid: 'ok', quiet: 'ok', quit: 'ok' });
  assert.deepEqual(await run('b'), { show: 'ok', hide: '-', destroy: 'ok', counts: 'ok', pid: 'ok', quiet: '-', quit: 'ok' });
});

test('a step that the system does not show is a miss of that key, whatever the app answered', async () => {
  assert.equal((await run('main', 'show_kept_first')).show, 'miss');
  assert.equal((await run('main', 'show_kept_later')).show, 'miss');
  assert.equal((await run('a', 'hide_kept')).hide, 'miss');
  assert.equal((await run('b', 'destroy_kept')).destroy, 'miss');
  assert.equal((await run('main', 'quit_stays')).quit, 'miss');
});
