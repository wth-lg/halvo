import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const dir = path.join(here, '..');
const windows = process.platform === 'win32';

function sample(args) {
  const command = windows
    ? ['powershell', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(dir, 'sample.ps1'), ...args]]
    : ['sh', [path.join(dir, 'sample.sh'), ...args]];
  const run = spawnSync(command[0], command[1], { encoding: 'utf8', timeout: 120000 });
  const lines = (text) => text.split('\n').map((l) => l.replace(/\r$/, '')).filter((l) => l !== '');
  return { status: run.status, out: lines(run.stdout ?? ''), err: lines(run.stderr ?? '') };
}

function made(pid) {
  let r = sample(idArgs(pid));
  for (let i = 0; i < 4 && r.status === 1 && r.out[0] === `sample pid=${pid} found=partial`; i += 1) r = sample(idArgs(pid));
  return r;
}

function idArgs(pid) {
  return windows ? ['-Id', String(pid)] : [String(pid)];
}

function withChild(fn) {
  const child = spawn(process.execPath, ['-e', 'setInterval(() => { let x = 0; for (let i = 0; i < 1e6; i += 1) x += i; }, 50)'], { stdio: 'ignore' });
  return new Promise((resolve) => setTimeout(resolve, 1500)).then(() => fn(child)).finally(() => child.kill('SIGKILL'));
}

const LINE = /^sample pid=(\d+) key_bytes=(\d+) second_bytes=(\d+) helpers=(\d+) load_x100=(\d+|-) cpu_pct=(\d+|-) ncpu=(\d+)$/;

test('the sampler reads the figures of a live process and of each process of its tree', async () => {
  await withChild((child) => {
    const r = made(child.pid);
    assert.equal(r.status, 0, r.out.concat(r.err).join('\n'));
    assert.deepEqual(r.err, []);
    const m = LINE.exec(r.out[0]);
    assert.ok(m, r.out[0]);
    assert.equal(Number(m[1]), child.pid);
    assert.ok(Number(m[2]) > 1024 * 1024, 'key_bytes holds at least the child');
    assert.ok(Number(m[3]) > 1024 * 1024, 'second_bytes holds at least the child');
    if (windows) {
      assert.equal(m[5], '-');
      assert.ok(Number(m[6]) >= 0 && Number(m[6]) <= 100);
    } else {
      assert.match(m[5], /^\d+$/);
      assert.equal(m[6], '-');
    }
    assert.ok(Number(m[7]) >= 1);
    const procs = r.out.slice(1).map((l) => /^proc pid=(\d+) start=(\d+) cpu_ms=(\d+)$/.exec(l));
    assert.ok(procs.every(Boolean), r.out.join('\n'));
    assert.equal(procs.length, Number(m[4]) + 1, 'helpers are the processes of the tree other than the child');
    const own = procs.find((p) => Number(p[1]) === child.pid);
    assert.ok(own, r.out.join('\n'));
    assert.ok(Number(own[3]) > 0, 'the busy child used processor time');
  });
});

test('the processor time of a process grows between two samples', async () => {
  await withChild(async (child) => {
    const cpu = (r) => Number(new RegExp(`^proc pid=${child.pid} start=\\d+ cpu_ms=(\\d+)$`).exec(r.out.find((l) => l.startsWith(`proc pid=${child.pid} `)))[1]);
    const first = made(child.pid);
    await new Promise((resolve) => setTimeout(resolve, 1500));
    const second = made(child.pid);
    assert.equal(first.status, 0);
    assert.equal(second.status, 0);
    assert.ok(cpu(second) > cpu(first), `${cpu(first)} then ${cpu(second)}`);
  });
});

test('a process that does not exist is a sample that could not be made', () => {
  const gone = spawnSync(process.execPath, ['-e', 'console.log(process.pid)'], { encoding: 'utf8' });
  const pid = Number(gone.stdout.trim());
  const r = sample(idArgs(pid));
  assert.equal(r.status, 1);
  assert.deepEqual(r.out, [`sample pid=${pid} found=no`]);
});

test('the sampler refuses a missing or a bad process id with usage', () => {
  for (const args of windows ? [[], ['-Id', '0']] : [[], ['x'], ['1', '2'], ['']]) {
    const r = sample(args);
    assert.equal(r.status, 2, JSON.stringify(args));
    assert.deepEqual(r.out, []);
    assert.equal(r.err.length, 1);
    assert.match(r.err[0], /^REFUSED usage: /);
  }
});

test('E4 and E7: the processor time folds minutes into seconds, and the load is the 1-minute value', { skip: windows }, async () => {
  const stub = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-stub-'));
  fs.writeFileSync(path.join(stub, 'ps'), '#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\n  if [ "$1" = -p ]; then list=$2; fi\n  shift\ndone\nprintf "%s\\n" "$list" | tr , "\\n" | while read -r p; do printf "%s 4096 1:05.00\\n" "$p"; done\n');
  fs.writeFileSync(path.join(stub, 'sysctl'), '#!/bin/sh\ncase "$2" in\n  vm.loadavg) printf "{ 1.11 2.22 3.33 }\\n" ;;\n  hw.ncpu) printf "8\\n" ;;\nesac\n');
  fs.chmodSync(path.join(stub, 'ps'), 0o755);
  fs.chmodSync(path.join(stub, 'sysctl'), 0o755);
  await withChild((child) => {
    const env = { ...process.env, PATH: `${stub}${path.delimiter}${path.dirname(process.execPath)}${path.delimiter}/usr/bin${path.delimiter}/bin` };
    let r;
    for (let i = 0; i < 5; i += 1) {
      r = spawnSync('sh', [path.join(dir, 'sample.sh'), String(child.pid)], { encoding: 'utf8', timeout: 120000, env });
      if (r.status === 0) break;
    }
    assert.equal(r.status, 0, r.stdout + r.stderr);
    const out = r.stdout.split('\n').filter(Boolean);
    assert.match(out[0], / load_x100=111 cpu_pct=- ncpu=8$/);
    assert.ok(out.includes(`proc pid=${child.pid} start=${/^proc pid=\d+ start=(\d+)/.exec(out.find((l) => l.startsWith(`proc pid=${child.pid} `)))[1]} cpu_ms=65000`), out.join('\n'));
  });
});
