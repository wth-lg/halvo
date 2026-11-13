import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const trials = path.join(here, '..', '..');
const mac = process.platform === 'darwin';

const WANTED = [
  'reading trial=window machine=mac arm=a2 system=macos call=background_throttling value=suspend',
  'reading trial=window machine=mac arm=a2 system=windows call=set_is_visible value=false',
  'reading trial=window machine=mac arm=a2 system=windows call=set_memory_usage_target_level value=low',
  'reading trial=window machine=mac arm=a2 system=macos expect=same_as_a source=wry-0.57.1 policy_set=only_when_named',
  'reading trial=window machine=mac figure=key_bytes system=macos command=ps',
  'reading trial=window machine=mac figure=second_bytes system=macos command=proc_pid_rusage',
  'reading trial=window machine=mac figure=cpu_ms system=macos command=ps',
  'reading trial=window machine=mac figure=key_bytes system=windows command=Get-Process',
  'reading trial=window machine=mac figure=second_bytes system=windows command=Get-Process',
  'reading trial=window machine=mac figure=cpu_ms system=windows command=Get-Process',
  'reading trial=sync machine=mac object=__TAURI_INTERNALS__',
  'reading trial=sync machine=mac way=channel order_by=index',
];

function read(args = []) {
  const r = spawnSync('sh', [path.join(trials, 'window', 'read.sh'), ...args], { encoding: 'utf8', timeout: 120000 });
  return { status: r.status, out: r.stdout.split('\n').filter((l) => l !== ''), err: r.stderr };
}

test('read.sh prints each reading line from the sources it names', { skip: !mac }, () => {
  const r = read();
  assert.equal(r.status, 0, r.err);
  assert.deepEqual(r.out, WANTED);
});

test('a reading whose source no longer says it is not printed, and read.sh says which', { skip: !mac }, () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-read-'));
  const files = execFileSync('git', ['-C', trials, 'ls-files', 'app', 'window'], { encoding: 'utf8' }).split('\n').filter(Boolean);
  for (const f of files) {
    fs.mkdirSync(path.join(root, path.dirname(f)), { recursive: true });
    fs.copyFileSync(path.join(trials, f), path.join(root, f));
  }
  const appRs = path.join(root, 'app', 'src', 'app.rs');
  fs.writeFileSync(appRs, fs.readFileSync(appRs, 'utf8').replace('COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW\n', 'COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL\n'));
  const swift = path.join(root, 'window', 'footprint.swift');
  fs.writeFileSync(swift, fs.readFileSync(swift, 'utf8').replace('info.ri_phys_footprint', 'info.ri_resident_size'));
  const r = read(['--root', root]);
  assert.equal(r.status, 1);
  assert.deepEqual(r.out.filter((l) => l.startsWith('reading ')), WANTED.filter((l) => !/set_memory_usage_target_level|second_bytes system=macos/.test(l)));
  assert.deepEqual(r.out.filter((l) => !l.startsWith('reading ')), [
    'read trial=window call=set_memory_usage_target_level found=no',
    'read trial=window figure=second_bytes system=macos found=no',
  ]);
});

test('read.sh refuses an argument it does not know with usage', () => {
  const r = spawnSync('sh', [path.join(trials, 'window', 'read.sh'), '--what'], { encoding: 'utf8' });
  assert.equal(r.status, 2);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, /^REFUSED usage: [^\n]+\n$/);
});

test('read.sh finds the wry source without cargo, so that a registry with no crate of another system is enough', { skip: !mac }, () => {
  const r = spawnSync('sh', [path.join(trials, 'window', 'read.sh')], { encoding: 'utf8', timeout: 120000, env: { ...process.env, PATH: `${path.dirname(process.execPath)}${path.delimiter}/usr/bin${path.delimiter}/bin` } });
  assert.equal(r.status, 0, r.stdout);
  assert.deepEqual(r.stdout.split('\n').filter((l) => l !== ''), WANTED);
});
