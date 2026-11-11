import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
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
      const res = await fetch(line.address);
      if (!res.ok) return failed('download', 'status=' + res.status);
      bytes = Buffer.from(await res.arrayBuffer());
    } catch (err) {
      return failed('download', 'error=' + String(err && err.code ? err.code : 'network'));
    }
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
    mkdirSync(dest, { recursive: true });
    if (!unzip(archive, dest)) return failed('extract', 'build=' + line.build);
    writeFileSync(mark, line.sha256 + '\n');
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
