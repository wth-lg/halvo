// The readers of the trial's native dialog, on macOS and Windows; they press a control by its accessibility action and post no input event.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const READ_MS = 60000;
const BUILD_MS = 600000;

const WORDS = {
  found: ['yes', 'no'],
  trusted: ['yes', 'no'],
  text: ['same', 'differ', '-'],
  default: ['reject', 'allow', 'none', 'other', 'unread', '-'],
  cancel: ['reject', 'allow', 'none', 'other', 'via_close', 'unread', '-'],
  close: ['yes', 'no', 'unread', '-'],
  owner: ['app', 'other', '-'],
};

export function parseRead(line) {
  const found = /^dialog found=(\S+) trusted=(\S+) text=(\S+) default=(\S+) cancel=(\S+) close=(\S+) owner=(\S+)$/.exec(line ?? '');
  if (!found) throw new Error('the dialog reader gave no state');
  const [, ...values] = found;
  const out = {};
  Object.keys(WORDS).forEach((key, i) => {
    if (!WORDS[key].includes(values[i])) throw new Error(`the dialog reader gave ${key}=${values[i]}`);
    out[key] = values[i];
  });
  if ((out.found === 'no') !== (out.text === '-')) throw new Error('the dialog reader gave keys that do not fit found');
  return out;
}

export function parsePressed(line) {
  if (line === 'pressed=yes') return true;
  if (line === 'pressed=no') return false;
  throw new Error('the dialog reader gave no press result');
}

function macosReader() {
  const source = path.join(here, 'dialog', 'macos.swift');
  const digest = createHash('sha256').update(fs.readFileSync(source)).digest('hex').slice(0, 16);
  const dir = path.join(os.tmpdir(), `r4-driver-dialog-${digest}`);
  const bin = path.join(dir, 'macos');
  if (!fs.existsSync(bin)) {
    fs.mkdirSync(dir, { recursive: true });
    const partial = path.join(dir, `macos-${process.pid}`);
    execFileSync('swiftc', ['-O', '-o', partial, source], { stdio: ['ignore', 'ignore', 'inherit'], timeout: BUILD_MS });
    fs.renameSync(partial, bin);
  }
  return bin;
}

function run(args) {
  let out;
  if (process.platform === 'darwin') {
    out = execFileSync(macosReader(), args, { encoding: 'utf8', timeout: READ_MS });
  } else if (process.platform === 'win32') {
    const flags = ['-Mode', args[0], '-Id', args[1]];
    if (args[2]) flags.push('-Press', args[2]);
    out = execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
      '-File', path.join(here, 'dialog', 'windows.ps1'), ...flags], { encoding: 'utf8', timeout: READ_MS });
  } else {
    throw new Error(`no dialog reader for ${process.platform}`);
  }
  return out.split('\n').map((line) => line.replace(/\r$/, '')).filter((line) => line !== '');
}

/** What the accessibility interface shows of the trial's dialog in the process `pid`. */
export function readDialog(pid) {
  return parseRead(run(['read', String(pid)])[0]);
}

/** Presses `which` (`close`, `allow` or `reject`) of the dialog by its accessibility action. */
export function pressDialog(pid, which) {
  return parsePressed(run(['press', String(pid), which])[0]);
}

export function parseList(line) {
  if (!/^list (trusted=(yes|no) )?windows=\d+ (titled=\d+ )?buttons=\d+ reject_named=\d+ allow_named=\d+ classes=[A-Za-z0-9_#,-]+$/.test(line ?? '')) {
    throw new Error('the dialog reader gave no list');
  }
  return line;
}

/** Counts of the windows of the process `pid` and of their buttons, for a trace line. */
export function listDialog(pid) {
  return parseList(run(['list', String(pid)])[0]);
}

/** Prepares the reader once, so that the first read of a cell does not wait for a build. */
export function prepare() {
  if (process.platform === 'darwin') macosReader();
}
