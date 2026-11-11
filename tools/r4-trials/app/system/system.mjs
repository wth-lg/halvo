// The readers of the system for the trial app, on macOS and Windows; they read and change nothing.
// usage: node system.mjs window <pid> | tree <pid> | alive <pid>:<start>...
// window prints window=onscreen|hidden|absent for the window titled Trial (on macOS, where the
// system gives no titles, the layer-0 window 620 points wide); tree prints tree count=<n>
// webview=<n> and one line pid=<n> start=<n> per process of the app's tree; alive prints alive=<n>.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const READ_MS = 60000;
const BUILD_MS = 600000;

export const TITLE = 'Trial';
export const WIDTH = 620;

function macosReader() {
  const source = path.join(here, 'macos.swift');
  const digest = createHash('sha256').update(fs.readFileSync(source)).digest('hex').slice(0, 16);
  const dir = path.join(os.tmpdir(), `r4-trial-system-${digest}`);
  const bin = path.join(dir, 'macos');
  if (!fs.existsSync(bin)) {
    fs.mkdirSync(dir, { recursive: true });
    const partial = path.join(dir, `macos-${process.pid}`);
    execFileSync('swiftc', ['-O', '-o', partial, source], { stdio: ['ignore', 'ignore', 'inherit'], timeout: BUILD_MS });
    fs.renameSync(partial, bin);
  }
  return bin;
}

function read(mode, args) {
  let out;
  if (process.platform === 'darwin') {
    out = execFileSync(macosReader(), [mode, ...args], { encoding: 'utf8', timeout: READ_MS });
  } else if (process.platform === 'win32') {
    const flags = mode === 'window' ? ['-Id', args[0], '-Title', args[1]]
      : mode === 'tree' ? ['-Id', args[0]]
        : ['-Pairs', args.join(',')];
    out = execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
      '-File', path.join(here, 'windows.ps1'), '-Mode', mode, ...flags], { encoding: 'utf8', timeout: READ_MS });
  } else {
    throw new Error(`no reader for ${process.platform}`);
  }
  return out.split('\n').map((line) => line.replace(/\r$/, '')).filter((line) => line !== '');
}

/** The state of the app's window: `onscreen`, `hidden` or `absent`. */
export function readWindow(pid) {
  const line = read('window', [String(pid), TITLE, String(WIDTH)])[0] ?? '';
  const state = /^window=(onscreen|hidden|absent)\b/.exec(line);
  if (!state) throw new Error('the window reader gave no state');
  return state[1];
}

/** The processes of the app's tree: `{ count, webview, procs: [{ pid, start }] }`. */
export function readTree(pid) {
  const lines = read('tree', [String(pid)]);
  const head = /^tree count=(\d+) webview=(\d+)$/.exec(lines[0] ?? '');
  if (!head) throw new Error('the tree reader gave no count');
  const procs = lines.slice(1).map((line) => /^pid=(\d+) start=(\d+)$/.exec(line))
    .filter(Boolean).map((m) => ({ pid: Number(m[1]), start: m[2] }));
  return { count: Number(head[1]), webview: Number(head[2]), procs };
}

/** How many of `procs` (from `readTree`) are still alive with the same start time. */
export function countAlive(procs) {
  if (procs.length === 0) return 0;
  const line = read('alive', procs.map((p) => `${p.pid}:${p.start}`))[0] ?? '';
  const alive = /^alive=(\d+)$/.exec(line);
  if (!alive) throw new Error('the alive reader gave no count');
  return Number(alive[1]);
}

function main(argv) {
  const [mode, ...rest] = argv;
  const pid = Number(rest[0]);
  if (mode === 'window' && rest.length === 1 && Number.isInteger(pid) && pid > 0) {
    console.log(`window=${readWindow(pid)}`);
  } else if (mode === 'tree' && rest.length === 1 && Number.isInteger(pid) && pid > 0) {
    const tree = readTree(pid);
    console.log(`tree count=${tree.count} webview=${tree.webview}`);
    for (const p of tree.procs) console.log(`pid=${p.pid} start=${p.start}`);
  } else if (mode === 'alive' && rest.every((pair) => /^\d+:\d+$/.test(pair))) {
    console.log(`alive=${countAlive(rest.map((pair) => ({ pid: pair.split(':')[0], start: pair.split(':')[1] })))}`);
  } else {
    process.stderr.write('REFUSED usage: window <pid>, tree <pid> or alive <pid>:<start>...\n');
    process.exit(2);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
