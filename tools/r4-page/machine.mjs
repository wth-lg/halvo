import { readFileSync, existsSync } from 'node:fs';
import { cpus, platform, release } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const HERE = dirname(fileURLToPath(import.meta.url));

export const BROWSERS = ['webkit', 'chromium'];

export class Refusal extends Error {
  constructor(cause, detail) {
    super(detail);
    this.code = cause;
  }
}

export function refuse(err) {
  if (!(err instanceof Refusal)) return false;
  process.stderr.write('REFUSED ' + err.code + ': ' + err.message + '\n');
  process.exitCode = 2;
  return true;
}

export function browsersDir() {
  const dir = process.env.R4_PAGE_BROWSERS;
  return dir ? resolve(dir) : join(HERE, '.browsers');
}

export function machine() {
  if (!process.env.CI) return 'mac';
  if (platform() === 'darwin') return 'hosted-mac';
  if (platform() === 'win32') return 'hosted-win';
  return null;
}

export function system() {
  if (platform() === 'darwin') {
    const major = parseInt(release().split('.')[0], 10);
    if (!(major >= 20)) return null;
    const version = major < 25 ? 11 + (major - 20) : Math.min(major + 1, 26);
    const arm = cpus().some((cpu) => cpu.model.includes('Apple'));
    return 'mac' + version + (arm ? '-arm64' : '');
  }
  if (platform() === 'win32' && process.arch === 'x64') return 'win64';
  return null;
}

export function browserFor(name) {
  const lines = readFileSync(join(HERE, 'browser.txt'), 'utf8').split('\n').filter((line) => line !== '');
  for (const line of lines) {
    const m = /^(\S+) (webkit|chromium)$/.exec(line);
    if (!m) throw new Refusal('browser', 'tools/r4-page/browser.txt holds a line that is not "<machine> <webkit|chromium>"');
    if (m[1] === name) return m[2];
  }
  return null;
}

export function buildLine(browser, sys) {
  const lines = readFileSync(join(HERE, 'files.tsv'), 'utf8').split('\n');
  for (const line of lines) {
    if (line === '' || line.startsWith('#')) continue;
    const f = line.split('\t');
    const ok = f.length === 5 && /^(webkit|chromium)-[0-9]+$/.test(f[0]) && /^https:\/\/[^\s]+\.zip$/.test(f[2]) && /^[0-9]+$/.test(f[3]) && /^[0-9a-f]{64}$/.test(f[4]);
    if (!ok) throw new Refusal('files', 'tools/r4-page/files.tsv holds a line that is not "build, system, address, size, sha256" separated by tabs');
    if (f[0].startsWith(browser + '-') && f[1] === sys) {
      return { build: f[0], system: f[1], address: f[2], size: Number(f[3]), sha256: f[4] };
    }
  }
  return null;
}

const EXECUTABLES = {
  webkit: (sys) => (sys === 'win64' ? ['Playwright.exe'] : ['pw_run.sh']),
  chromium: (sys) => {
    if (sys === 'win64') return ['chrome-win64', 'chrome.exe'];
    const dir = sys.endsWith('-arm64') ? 'chrome-mac-arm64' : 'chrome-mac-x64';
    return [dir, 'Google Chrome for Testing.app', 'Contents', 'MacOS', 'Google Chrome for Testing'];
  },
};

export function buildDir(dir, line) {
  return join(dir, 'builds', line.build + '-' + line.system);
}

export function marker(dir, line) {
  return join(buildDir(dir, line), '.r4-sha256');
}

export function executable(dir, browser, line) {
  return join(buildDir(dir, line), ...EXECUTABLES[browser](line.system));
}

export function choose(explicit) {
  let browser = explicit;
  const name = machine();
  if (!browser) {
    if (!name) throw new Refusal('browser', 'no machine of tools/r4-page/browser.txt matches this system with CI set');
    browser = browserFor(name);
    if (!browser) throw new Refusal('browser', 'tools/r4-page/browser.txt names no browser for the machine ' + name);
  }
  const sys = system();
  if (!sys) throw new Refusal('browser', 'Playwright 1.65.0 has no build for this system');
  const line = buildLine(browser, sys);
  return { machine: name, browser, system: sys, line };
}

export function installed(explicit) {
  const dir = browsersDir();
  const pick = choose(explicit);
  if (!pick.line) throw new Refusal('browser', 'tools/r4-page/files.tsv has no line for ' + pick.browser + ' on ' + pick.system);
  const exe = executable(dir, pick.browser, pick.line);
  const mark = marker(dir, pick.line);
  const pkg = join(dir, 'node_modules', 'playwright', 'index.mjs');
  if (!existsSync(exe) || !existsSync(mark) || readFileSync(mark, 'utf8').trim() !== pick.line.sha256 || !existsSync(pkg)) {
    throw new Refusal('browser', 'no fetched build of ' + pick.line.build + ' for ' + pick.system + ' under the browsers directory; run sh tools/r4-page/fetch.sh DIR first');
  }
  return { ...pick, dir, exe, pkg, walkKey: walkKey(pick.browser) };
}

export function walkKey(browser) {
  return browser === 'webkit' && platform() === 'darwin' ? 'Alt+Tab' : 'Tab';
}
