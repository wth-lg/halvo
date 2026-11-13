import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { createServer, connect } from 'node:net';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { END_MS, alive, exitWithin, goneBy, scratchDir, startServer, stopServer } from './servers.mjs';

const PARENT = fileURLToPath(new URL('./stand-in-parent.mjs', import.meta.url));
const CHILD = fileURLToPath(new URL('./runner-child.mjs', import.meta.url));
const RUN_MS = 120000;
const scratch = scratchDir('fixture-server-lifetime-');
after(scratch.remove);

const SITE = join(scratch.dir, 'acorn-park.json');
writeFileSync(SITE, `${JSON.stringify({
  v: 1,
  service: 'acorn-park',
  category: 'other',
  kind: 'web',
  host: 'acorn-park.test',
  app: null,
  parameters: [],
  values: {},
  pages: [{ page: 'map', title: 'Map - Acorn Park', text: 'Paths.', password_input: false, card_title: false }],
  flows: [{ flow: 'walk', role: 'other', pages: ['map'] }],
})}\n`);

function assertEndedBySignal(ended) {
  if (process.platform === 'win32') assert.deepEqual({ killed: ended.killed, gone: ended.gone }, { killed: false, gone: true });
  else assert.deepEqual(ended, { code: 0, signal: null, killed: false, gone: true });
}

function sleep(ms) {
  return new Promise((resolve) => { setTimeout(resolve, ms); });
}

function hold(port, text) {
  return new Promise((resolve, reject) => {
    const socket = connect({ host: '127.0.0.1', port, allowHalfOpen: true });
    socket.on('error', () => {});
    socket.once('connect', () => {
      socket.write(text, 'latin1');
      setTimeout(() => resolve(socket), 500);
    });
    socket.once('error', reject);
  });
}

function get(port) {
  return new Promise((resolve) => {
    const socket = connect({ host: '127.0.0.1', port });
    let head = '';
    socket.setEncoding('latin1');
    socket.on('data', (chunk) => { head += chunk; });
    socket.on('error', () => resolve(0));
    socket.on('close', () => resolve(Number(head.slice(9, 12)) || 0));
    socket.write(`GET /map HTTP/1.1\r\nHost: acorn-park.test:${port}\r\nConnection: close\r\n\r\n`, 'latin1');
  });
}

function recorded(file) {
  try {
    return readFileSync(file, 'utf8').split('\n').filter((line) => line !== '').map(Number);
  } catch {
    return [];
  }
}

function outerEnv() {
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  return env;
}

async function runner(mode, pids) {
  const child = spawn(process.execPath, ['--test', '--test-reporter=tap', CHILD], {
    env: { ...outerEnv(), FIXTURE_SITE: SITE, FIXTURE_PIDS: pids, FIXTURE_MODE: mode },
    stdio: ['ignore', 'ignore', 'ignore'],
    windowsHide: true,
  });
  return child;
}

function cleanup(pids, ...children) {
  for (const child of children) if (child && child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  for (const pid of pids) if (alive(pid)) process.kill(pid, 'SIGKILL');
}

test('lifetime: SIGTERM to the node --test runner leaves none of the servers that its test started', async () => {
  const pids = join(scratch.dir, 'runner-sigterm.pids');
  const child = await runner('wait', pids);
  let ids = [];
  try {
    const until = Date.now() + RUN_MS;
    while (recorded(pids).length < 2 && Date.now() < until && child.exitCode === null) await sleep(200);
    ids = recorded(pids);
    assert.equal(ids.length, 2, 'two servers recorded');
    assert.ok(ids.every(alive), 'both servers alive before the signal');
    const deadline = Date.now() + END_MS;
    child.kill('SIGTERM');
    const gone = [];
    for (const pid of ids) gone.push(await goneBy(pid, deadline));
    console.log(`lifetime runner-sigterm recorded=${ids.length} alive_after_5s=${gone.filter((g) => !g).length}`);
    assert.deepEqual(gone, [true, true]);
  } finally {
    cleanup(ids.length > 0 ? ids : recorded(pids), child);
  }
});

test('lifetime: SIGKILL of the process that started a server ends the server', async () => {
  const parent = spawn(process.execPath, [PARENT, SITE], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
  let pid = null;
  try {
    const line = await new Promise((resolve) => {
      let out = '';
      const timer = setTimeout(() => resolve(''), RUN_MS);
      parent.stdout.setEncoding('utf8');
      parent.stdout.on('data', (chunk) => {
        out += chunk;
        if (out.includes('\n')) {
          clearTimeout(timer);
          resolve(out);
        }
      });
    });
    const match = /^server (\d+) (\d+)\n/.exec(line);
    assert.ok(match, 'the stand-in parent printed its server');
    pid = Number(match[1]);
    assert.equal(await get(Number(match[2])), 200);
    const deadline = Date.now() + END_MS;
    parent.kill('SIGKILL');
    const gone = await goneBy(pid, deadline);
    console.log(`lifetime parent-sigkill recorded=1 alive_after_5s=${gone ? 0 : 1}`);
    assert.equal(gone, true);
  } finally {
    cleanup(pid === null ? [] : [pid], parent);
  }
});

test('lifetime: a test that throws before its cleanup ends its run and leaves no server', async () => {
  const pids = join(scratch.dir, 'runner-throw.pids');
  const child = await runner('throw', pids);
  try {
    const ended = await exitWithin(child, RUN_MS);
    const ids = recorded(pids);
    assert.notEqual(ended, null, 'the run ended');
    assert.equal(ended.code, 1, 'the run failed');
    assert.equal(ids.length, 1, 'one server recorded');
    const gone = await goneBy(ids[0], Date.now() + END_MS);
    console.log(`lifetime test-throws recorded=${ids.length} run_ended=true alive_after_5s=${gone ? 0 : 1}`);
    assert.equal(gone, true);
  } finally {
    cleanup(recorded(pids), child);
  }
});

test('lifetime: a server whose input ends while a request is in flight ends with status 0', async () => {
  const server = await startServer(['--port', '0', SITE]);
  let held = null;
  try {
    held = await hold(server.port, `GET /map HTTP/1.1\r\nHost: acorn-park.test:${server.port}\r\n`);
    server.child.stdin.end();
    const ended = await exitWithin(server.child, END_MS);
    console.log(`lifetime input-ends recorded=1 alive_after_5s=${ended === null ? 1 : 0}`);
    assert.deepEqual(ended, { code: 0, signal: null });
  } finally {
    held?.destroy();
    await stopServer(server);
  }
});

if (process.platform !== 'win32') {
  test('lifetime: a server whose input fails ends with status 0', async () => {
    const listener = createServer();
    await new Promise((resolve) => { listener.listen({ host: '127.0.0.1', port: 0 }, resolve); });
    const accepted = new Promise((resolve) => { listener.once('connection', resolve); });
    const input = connect({ host: '127.0.0.1', port: listener.address().port });
    await new Promise((resolve) => { input.once('connect', resolve); });
    const other = await accepted;
    let server = null;
    try {
      server = await startServer(['--port', '0', SITE], { input });
      input.destroy();
      other.resetAndDestroy();
      const ended = await exitWithin(server.child, END_MS);
      console.log(`lifetime input-fails recorded=1 alive_after_5s=${ended === null ? 1 : 0}`);
      assert.deepEqual(ended, { code: 0, signal: null });
    } finally {
      listener.close();
      if (server !== null) await stopServer(server);
    }
  });
}

test('lifetime: a server with a null input serves on until the test ends it', async () => {
  const server = await startServer(['--port', '0', SITE], { input: 'ignore' });
  try {
    await sleep(1000);
    assert.equal(alive(server.pid), true);
    assert.equal(await get(server.port), 200);
  } finally {
    const ended = await stopServer(server);
    console.log(`lifetime null-input recorded=1 killed=${ended.killed} alive_after=${ended.gone ? 0 : 1}`);
    assertEndedBySignal(ended);
  }
});

test('control: a server that gets no signal is still running at the bound, and the check of a signalled end refuses it', async () => {
  const server = await startServer(['--port', '0', SITE]);
  try {
    const ended = await stopServer(server, null);
    console.log(`control no-signal recorded=1 killed=${ended.killed} alive_after=${ended.gone ? 0 : 1}`);
    assert.equal(ended.killed, true);
    assert.equal(ended.gone, true);
    assert.throws(() => assertEndedBySignal(ended), assert.AssertionError);
  } finally {
    await stopServer(server);
  }
});

const HELD = [
  ['a listed GET with an upgrade offer', (port) => `GET /map HTTP/1.1\r\nHost: acorn-park.test:${port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`],
  ['CONNECT', (port) => `CONNECT acorn-park.test:${port} HTTP/1.1\r\nHost: acorn-park.test:${port}\r\n\r\n`],
  ['an unlisted GET with an upgrade offer', (port) => `GET /nothing HTTP/1.1\r\nHost: acorn-park.test:${port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`],
  ['a half-sent request', (port) => `GET /map HTTP/1.1\r\nHost: acorn-park.test:${port}\r\n`],
];

for (const [kind, text] of HELD) {
  test(`shutdown: SIGTERM ends the server with status 0 while a client holds ${kind}`, async () => {
    const server = await startServer(['--port', '0', SITE]);
    let held = null;
    try {
      held = await hold(server.port, text(server.port));
      const ended = await stopServer(server);
      console.log(`shutdown held=${kind.replaceAll(' ', '-')} exit=${ended.code} killed=${ended.killed} alive_after=${ended.gone ? 0 : 1}`);
      assertEndedBySignal(ended);
    } finally {
      held?.destroy();
      await stopServer(server);
    }
  });
}
