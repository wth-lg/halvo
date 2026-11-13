// The cells of the driver and native dialog trial (contracts.md I11.5), on a hosted runner.
// usage: node run.mjs --candidate webdriver|wdio|all | --dialog plugin|system|all
// Exits 0 when it printed its cells, 1 when a cell could not be made, 2 on a refusal.
import { spawnSync } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { Control, launch, stop } from '../app/selfcheck.mjs';
import { readTree, readWindow } from '../app/system/system.mjs';
import { listDialog, prepare, pressDialog, readDialog } from './dialog.mjs';
import * as wd from './webdriver.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

export const CANDIDATES = ['webdriver', 'wdio'];
export const FORMS = ['plugin', 'system'];
export const SESSIONS = 5;
export const ROWS = 20;

const READY_MS = 30000;
const SHOW_MS = 40000;
const QUIT_MS = 15000;
const COUNT_MS = 5000;
const FIND_MS = 15000;
const ANSWER_MS = 15000;
const WINDOW_READS = 10;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export class Refused extends Error {
  constructor(cause, detail) {
    super(`REFUSED ${cause}: ${detail}`);
    this.cause = cause;
  }
}

class Miss extends Error {
  constructor(line) {
    super(line);
    this.line = line;
  }
}

export function parseArgs(argv) {
  const usage = 'give --candidate webdriver|wdio|all or --dialog plugin|system|all';
  if (argv.length !== 2) throw new Refused('usage', usage);
  const [flag, value] = argv;
  if (flag === '--candidate' && (value === 'all' || CANDIDATES.includes(value))) {
    return { kind: 'driver', names: value === 'all' ? CANDIDATES : [value] };
  }
  if (flag === '--dialog' && (value === 'all' || FORMS.includes(value))) {
    return { kind: 'dialog', forms: value === 'all' ? FORMS : [value] };
  }
  throw new Refused('usage', usage);
}

export function machineOf(env, platform) {
  const set = (name) => typeof env[name] === 'string' && env[name] !== '';
  const system = platform === 'win32' ? 'win' : platform === 'darwin' ? 'mac' : null;
  if (!set('CI') && !set('R4_GUEST')) throw new Refused('not_hosted', 'neither CI nor R4_GUEST is set');
  if (system === null) throw new Refused('usage', `no trial for ${platform}`);
  return `${set('CI') ? 'hosted' : 'guest'}-${system}`;
}

export function driverCell(machine, name, ok) {
  return `cell trial=driver machine=${machine} candidate=${name} sessions=${SESSIONS} ok=${ok}`;
}

export function dialogCell(machine, form, keys) {
  const k = keys ?? {};
  const v = (key) => k[key] ?? '-';
  return `cell trial=dialog machine=${machine} form=${form} text=${v('text')} default=${v('default')} cancel=${v('cancel')} close=${v('close')} allow_control=${v('allow_control')} reject_control=${v('reject_control')} driven=${keys ? 'yes' : 'no'}`;
}

export function engineOf(userAgent) {
  if (typeof userAgent !== 'string') return 'other';
  if (userAgent.includes('Edg/')) return 'webview2';
  if (userAgent.includes('AppleWebKit/') && !userAgent.includes('Chrome/')) return 'wkwebview';
  return 'other';
}

export function sessionCounts({ onscreen, rows, count }) {
  return onscreen && rows === ROWS && count === '1';
}

export function cancelOf(read, closeAnswer) {
  if (read.cancel !== 'via_close') return read.cancel;
  return read.close === 'yes' ? closeAnswer : 'none';
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

function binary() {
  return path.join(here, 'target', 'release', process.platform === 'win32' ? 'r4-driver-trial.exe' : 'r4-driver-trial');
}

function build(machine, label, features) {
  const started = Date.now();
  const args = ['build', '--release', '--locked', '--manifest-path', path.join(here, 'Cargo.toml')];
  if (features) args.push('--features', features);
  const done = spawnSync('cargo', args, { stdio: ['ignore', 'inherit', 'inherit'] });
  const ok = done.status === 0;
  console.log(`build trial=driver machine=${machine} build=${label} result=${ok ? 'ok' : 'fail'} seconds=${Math.round((Date.now() - started) / 1000)}`);
  return ok;
}

function waitLine(app, test, ms) {
  const seen = app.lines.find(test);
  if (seen !== undefined) return Promise.resolve(seen);
  return new Promise((resolve) => {
    let waiter;
    const timer = setTimeout(() => {
      app.waiters = app.waiters.filter((w) => w !== waiter);
      resolve(null);
    }, ms);
    waiter = (line) => {
      if (!test(line)) return false;
      clearTimeout(timer);
      resolve(line);
      return true;
    };
    app.waiters.push(waiter);
    app.exited.then(() => {
      clearTimeout(timer);
      resolve(app.lines.find(test) ?? null);
    });
  });
}

async function windowState(pid, want) {
  let state = null;
  for (let i = 0; i < WINDOW_READS; i += 1) {
    state = readWindow(pid);
    if (state === want) return state;
    await sleep(300);
  }
  return state;
}

async function quit(app, control) {
  if (control) {
    await control.send('quit', QUIT_MS);
    control.close();
  }
  const ended = await Promise.race([app.exited, sleep(QUIT_MS).then(() => 'late')]);
  if (ended === 'late') stop(app);
}

async function session(machine, name, run, bin) {
  const port = await freePort();
  const app = await launch(bin, ['--webdriver-port', String(port)]);
  let control = null;
  const result = { onscreen: false, rows: 0, count: null, tauri: false, engine: 'other' };
  try {
    if (!readTree(app.pid).count) throw new Miss(`miss trial=driver machine=${machine} arm=main run=${run} step=show`);
    if (!(await wd.waitReady(port, READY_MS, sleep))) return result;
    const id = await wd.newSession(port).catch(() => null);
    if (id === null) return result;
    control = await Control.connect(app.port);
    const shown = await control.send('show', SHOW_MS);
    const state = await windowState(app.pid, 'onscreen');
    if (typeof shown.answer !== 'string' || !shown.answer.startsWith('ok show_ms=') || state !== 'onscreen') {
      throw new Miss(`miss trial=driver machine=${machine} arm=main run=${run} step=show`);
    }
    result.onscreen = true;
    try {
      result.rows = (await wd.findAll(port, id, '[data-row]')).length;
      result.tauri = (await wd.execute(port, id, 'return typeof window.__TAURI_INTERNALS__')) === 'object';
      result.engine = engineOf(await wd.execute(port, id, 'return navigator.userAgent'));
      const act = await wd.find(port, id, '#act');
      const count = await wd.find(port, id, '#count');
      await wd.click(port, id, act);
      const until = Date.now() + COUNT_MS;
      do {
        result.count = await wd.text(port, id, count);
        if (result.count === '1') break;
        await sleep(100);
      } while (Date.now() < until);
    } finally {
      await wd.endSession(port, id).catch(() => false);
    }
    return result;
  } finally {
    await quit(app, control).catch(() => stop(app));
  }
}

async function driverCells(machine, name) {
  if (!build(machine, name, name)) {
    console.log(driverCell(machine, name, 0));
    return true;
  }
  let ok = 0;
  for (let run = 1; run <= SESSIONS; run += 1) {
    let result;
    try {
      result = await session(machine, name, run, binary());
    } catch (e) {
      if (e instanceof Miss) {
        console.log(e.line);
        return false;
      }
      result = { onscreen: false, rows: 0, count: null, tauri: false, engine: 'other' };
    }
    const counted = sessionCounts(result);
    if (counted) ok += 1;
    console.log(`session trial=driver machine=${machine} candidate=${name} run=${run} onscreen=${result.onscreen ? 'yes' : 'no'} rows=${result.rows} count_one=${result.count === '1' ? 'yes' : 'no'} tauri=${result.tauri ? 'yes' : 'no'} engine=${result.engine} counted=${counted ? 'yes' : 'no'}`);
  }
  console.log(driverCell(machine, name, ok));
  return true;
}

export class ReaderError extends Error {}

export function ownerFits(form, owner) {
  return form === 'system' ? owner === 'app' : owner === 'app' || owner === 'other';
}

export async function readUntilFound(pid, read1 = readDialog, waitMs = 500) {
  const until = Date.now() + FIND_MS;
  let read;
  do {
    try {
      read = read1(pid);
    } catch (e) {
      throw new ReaderError(e.message);
    }
    if (read.found === 'yes') return read;
    await sleep(waitMs);
  } while (Date.now() < until);
  return read;
}

async function answerOf(machine, form, app, n, press) {
  const shown = await waitLine(app, (line) => line === `dialog show form=${form} n=${n}`, ANSWER_MS);
  if (!shown) throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=show`);
  let read;
  try {
    read = await readUntilFound(app.pid);
  } catch (e) {
    if (e instanceof ReaderError) throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=read`);
    throw e;
  }
  if (read.found !== 'yes') return { read, answer: null };
  if (!ownerFits(form, read.owner)) throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=owner`);
  const which = typeof press === 'function' ? press(read) : press;
  let pressed = false;
  try {
    pressed = pressDialog(app.pid, which);
  } catch {
    pressed = false;
  }
  if (!pressed) return { read, answer: 'miss', which };
  const answer = await waitLine(app, (line) => line.startsWith(`dialog answer form=${form} n=${n} `), ANSWER_MS);
  if (!answer) throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=${which}`);
  return { read, answer: /result=(allow|reject)$/.exec(answer)[1], which };
}

async function dialogCellOf(machine, form, bin) {
  const app = await launch(bin, ['--dialog', form, '--shows', '3']);
  let control = null;
  try {
    if (!readTree(app.pid).count) throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=show`);
    control = await Control.connect(app.port);
    const first = await answerOf(machine, form, app, 1, (read) => (read.close === 'yes' ? 'close' : 'reject'));
    if (first.answer === null) {
      const lines = app.lines.filter((line) => /^dialog (show|answer) /.test(line)).length;
      let list = 'list=error';
      try {
        list = listDialog(app.pid).replace(/^list /, '');
      } catch {
        list = 'list=error';
      }
      console.log(`trace trial=dialog machine=${machine} form=${form} app_dialog_lines=${lines} ${list}`);
      stop(app);
      return null;
    }
    const { read } = first;
    for (const key of ['default', 'cancel', 'close']) {
      if (read[key] === 'other' || read[key] === 'unread') throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=${key}`);
    }
    if (first.answer === 'miss') throw new Miss(`miss trial=dialog machine=${machine} form=${form} run=1 step=${first.which}`);
    const close = first.which === 'close' ? first.answer : 'none';
    const second = await answerOf(machine, form, app, 2, 'allow');
    const third = await answerOf(machine, form, app, 3, 'reject');
    return {
      text: read.text,
      default: read.default,
      cancel: cancelOf(read, close),
      close,
      allow_control: second.answer === 'allow' ? 'allow' : 'miss',
      reject_control: third.answer === 'reject' ? 'reject' : 'miss',
    };
  } finally {
    await quit(app, control).catch(() => stop(app));
  }
}

async function dialogCells(machine, forms) {
  if (!build(machine, 'none', null)) return false;
  prepare();
  for (const form of forms) {
    try {
      console.log(dialogCell(machine, form, await dialogCellOf(machine, form, binary())));
    } catch (e) {
      if (e instanceof Miss) {
        console.log(e.line);
        return false;
      }
      throw e;
    }
  }
  return true;
}

async function main(argv, env) {
  let args;
  let machine;
  try {
    args = parseArgs(argv);
    machine = machineOf(env, process.platform);
  } catch (e) {
    if (e instanceof Refused) {
      process.stderr.write(`${e.message}\n`);
      return 2;
    }
    throw e;
  }
  let made = true;
  if (args.kind === 'driver') {
    for (const name of args.names) made = (await driverCells(machine, name)) && made;
  } else {
    made = await dialogCells(machine, args.forms);
  }
  return made ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = await main(process.argv.slice(2), process.env);
}
