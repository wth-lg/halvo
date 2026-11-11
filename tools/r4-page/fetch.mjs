import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { HERE, Refusal, refuse, choose, buildDir, marker } from './machine.mjs';

const USAGE = 'sh tools/r4-page/fetch.sh DIR [--browser webkit|chromium]';

function parse(argv) {
  let dir = null;
  let browser = null;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--browser' && browser === null && (argv[i + 1] === 'webkit' || argv[i + 1] === 'chromium')) {
      browser = argv[++i];
    } else if (a !== '' && !a.startsWith('-') && dir === null) {
      dir = a;
    } else {
      throw new Refusal('usage', USAGE);
    }
  }
  if (dir === null) throw new Refusal('usage', USAGE);
  return { dir: resolve(dir), browser };
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function matches(bytes, line) {
  return bytes.length === line.size && sha256(bytes) === line.sha256;
}

const HOPS = 10;
const IDLE_MS = 60000;
const TOTAL_MS = 1800000;

async function download(line) {
  const total = AbortSignal.timeout(TOTAL_MS);
  const idle = new AbortController();
  const signal = AbortSignal.any([total, idle.signal]);
  let timer = setTimeout(() => idle.abort(), IDLE_MS);
  const awake = () => {
    clearTimeout(timer);
    timer = setTimeout(() => idle.abort(), IDLE_MS);
  };
  try {
    let url = new URL(line.address);
    let res;
    for (let hop = 0; ; hop++) {
      if (url.protocol !== 'https:') return 'error=redirect_not_https';
      if (hop > HOPS) return 'error=redirects';
      res = await fetch(url, { redirect: 'manual', signal });
      awake();
      if (res.status < 300 || res.status > 399) break;
      const next = res.headers.get('location');
      await res.body?.cancel();
      if (!next) return 'status=' + res.status;
      url = new URL(next, url);
    }
    if (!res.ok) {
      await res.body?.cancel();
      return 'status=' + res.status;
    }
    const chunks = [];
    let size = 0;
    for await (const chunk of res.body) {
      awake();
      size += chunk.length;
      if (size > line.size) {
        idle.abort();
        throw new Refusal('hash', line.address + ' sends more than the ' + line.size + ' bytes of its line in tools/r4-page/files.tsv; nothing is written');
      }
      chunks.push(chunk);
    }
    return Buffer.concat(chunks);
  } catch (err) {
    if (err instanceof Refusal) throw err;
    if (signal.aborted) return 'error=timeout';
    throw err;
  } finally {
    clearTimeout(timer);
  }
}

function failed(step, detail) {
  process.stdout.write('fetch failed step=' + step + ' ' + detail + '\n');
  process.exitCode = 1;
}

function unzip(archive, dest) {
  let r = spawnSync('unzip', ['-q', '-o', archive, '-d', dest], { stdio: ['ignore', 'ignore', 'inherit'] });
  if (r.error && r.error.code === 'ENOENT' && process.platform === 'win32') {
    const tar = join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe');
    r = spawnSync(tar, ['-xf', archive, '-C', dest], { stdio: ['ignore', 'ignore', 'inherit'] });
  }
  return !r.error && r.status === 0;
}

function sameFile(a, b) {
  return existsSync(b) && readFileSync(a).equals(readFileSync(b));
}

async function main() {
  const { dir, browser } = parse(process.argv.slice(2));
  const pick = choose(browser);
  const line = pick.line;
  if (!line) throw new Refusal('files', 'tools/r4-page/files.tsv has no line for ' + pick.browser + ' on ' + pick.system);
  const archive = join(dir, 'archives', line.build + '-' + line.system + '.zip');
  let archiveState = 'kept';
  if (!(existsSync(archive) && matches(readFileSync(archive), line))) {
    let bytes;
    try {
      bytes = await download(line);
    } catch (err) {
      if (err instanceof Refusal) throw err;
      return failed('download', 'error=' + String(err && err.code ? err.code : 'network'));
    }
    if (typeof bytes === 'string') return failed('download', bytes);
    if (!matches(bytes, line)) {
      throw new Refusal('hash', line.address + ' has size ' + bytes.length + ' and SHA-256 ' + sha256(bytes) + ', and its line in tools/r4-page/files.tsv has size ' + line.size + ' and SHA-256 ' + line.sha256);
    }
    mkdirSync(dirname(archive), { recursive: true });
    writeFileSync(archive, bytes);
    archiveState = 'downloaded';
  }
  const dest = buildDir(dir, line);
  const mark = marker(dir, line);
  let extracted = 'no';
  if (!(existsSync(mark) && readFileSync(mark, 'utf8').trim() === line.sha256)) {
    mkdirSync(dirname(dest), { recursive: true });
    const fresh = mkdtempSync(dest + '.new-');
    if (!unzip(archive, fresh)) return failed('extract', 'build=' + line.build);
    writeFileSync(join(fresh, basename(mark)), line.sha256 + '\n');
    if (existsSync(dest)) {
      let n = 1;
      while (existsSync(dest + '.old-' + n)) n++;
      renameSync(dest, dest + '.old-' + n);
    }
    renameSync(fresh, dest);
    extracted = 'yes';
  }
  let pkg = 'kept';
  const lockChanged = !sameFile(join(HERE, 'package-lock.json'), join(dir, 'package-lock.json')) || !sameFile(join(HERE, 'package.json'), join(dir, 'package.json'));
  if (lockChanged || !existsSync(join(dir, 'node_modules', '.package-lock.json'))) {
    copyFileSync(join(HERE, 'package.json'), join(dir, 'package.json'));
    copyFileSync(join(HERE, 'package-lock.json'), join(dir, 'package-lock.json'));
    const env = {
      ...process.env,
      npm_config_cache: join(dir, 'npm-cache'),
      npm_config_update_notifier: 'false',
      npm_config_fund: 'false',
      npm_config_audit: 'false',
    };
    const r = spawnSync('npm', ['ci', '--ignore-scripts'], { cwd: dir, env, shell: process.platform === 'win32', stdio: ['ignore', 2, 2] });
    if (r.error || r.status !== 0) return failed('package', 'status=' + (r.error ? 'spawn' : r.status));
    pkg = 'installed';
  }
  process.stdout.write('fetch browser=' + pick.browser + ' build=' + line.build + ' system=' + line.system + ' bytes=' + line.size + ' archive=' + archiveState + ' extracted=' + extracted + ' package=' + pkg + '\n');
}

main().catch((err) => {
  if (refuse(err)) return;
  process.stdout.write('fetch failed step=internal error=' + String(err && err.name) + '\n');
  process.exitCode = 1;
});
