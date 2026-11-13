// usage: node tools/r4-trials/floor/cell.mjs --bin PATH
// One cell of the floor trial (contracts.md I11.4): starts the built trial app on the pages of
// this directory, shows its window, loads probe.html, reads the probe's line and quits, reading
// the system after the show and the quit (I11.3, TA-9). Prints the cell line, or one miss or fail
// line. Refuses with not_hosted where neither CI nor R4_GUEST is set.
// Exit: 0 when it printed its cell, 1 when the cell could not be made, 2 on a refusal.
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkHosted, Control, launch, Refused, stepKey, stop } from '../app/selfcheck.mjs';
import { countAlive, readTree, readWindowMatch } from '../app/system/system.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SHOW_MS = 45000;
const LINE_MS = 15000;
const PROBE_MS = 30000;
const QUIT_MS = 10000;
const READ_TRIES = 10;
const READ_EVERY_MS = 300;

/** The machine of a cell: `guest-mac14` where R4_GUEST is set, else the hosted machine of the
 * system, or `null` on a system that the trial does not run on. */
export function machineOf(env, platform) {
  if (typeof env.R4_GUEST === 'string' && env.R4_GUEST !== '') return 'guest-mac14';
  if (platform === 'darwin') return 'hosted-mac';
  if (platform === 'win32') return 'hosted-win';
  return null;
}

/** The system version from the output of `sw_vers -productVersion` on macOS or of `cmd /c ver`
 * on Windows, or `null`. */
export function systemOf(platform, text) {
  const m = platform === 'win32'
    ? /Version ([0-9]+(?:\.[0-9]+)+)\]/.exec(text)
    : /^([0-9]+(?:\.[0-9]+){0,2})\s*$/.exec(text);
  return m ? m[1] : null;
}

/** The probe's report as the app prints it (`page probe ...`), or `null`. */
export function parseProbe(line) {
  const m = /^page probe features=(\d+) missing=(\d+) missing_names=([a-z0-9_,]+|-) webkit=([0-9.]+|-) edg=([0-9.]+|-) aria_notify=(function|undefined)$/.exec(line ?? '');
  if (!m) return null;
  return { features: Number(m[1]), missing: Number(m[2]), names: m[3], webkit: m[4], edg: m[5], aria: m[6] };
}

/** The cell line of I11.4: `webkit` is the AppleWebKit version on macOS and the Edg version on
 * Windows. */
export function cellLine(machine, platform, system, probe) {
  const webkit = platform === 'win32' ? probe.edg : probe.webkit;
  return `cell trial=floor machine=${machine} system=${system} webkit=${webkit} features=${probe.features} ` +
    `missing=${probe.missing} missing_names=${probe.names} aria_notify=${probe.aria}`;
}

/** The miss line of I11.3 for a step whose answer the system does not show. */
export function missLine(machine, step) {
  return `miss trial=floor machine=${machine} arm=main run=1 step=${step}`;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function waitFor(app, test, ms) {
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

async function look(pid, expected) {
  let state = null;
  for (let tries = 0; tries < READ_TRIES; tries++) {
    ({ state } = readWindowMatch(pid));
    if (state === expected) break;
    await sleep(READ_EVERY_MS);
  }
  return state;
}

function readSystem(platform) {
  const text = platform === 'win32'
    ? execFileSync('cmd', ['/c', 'ver'], { encoding: 'utf8', timeout: LINE_MS })
    : execFileSync('sw_vers', ['-productVersion'], { encoding: 'utf8', timeout: LINE_MS });
  return systemOf(platform, text);
}

/** Makes the cell with the app at `bin` and returns `{ code, line }`; throws `Refused` with
 * `not_hosted` before it starts anything where neither CI nor R4_GUEST is set in `env`. */
export async function cell(bin, env = process.env, platform = process.platform) {
  checkHosted(env);
  const machine = machineOf(env, platform);
  const fail = (step) => ({ code: 1, line: `fail trial=floor machine=${machine} step=${step}` });
  const system = readSystem(platform);
  if (!system) return fail('system');
  let app = null;
  let control = null;
  try {
    try {
      app = await launch(bin, ['--arm', 'main', '--pages', HERE]);
    } catch {
      return fail('launch');
    }
    control = await Control.connect(app.port);
    const shown = await control.send('show', SHOW_MS);
    if (stepKey('show', shown.answer, await look(app.pid, 'onscreen')) !== 'ok') return { code: 1, line: missLine(machine, 'show') };
    const tree = readTree(app.pid);
    const loaded = await control.send('load probe.html', LINE_MS);
    if (loaded.answer !== 'ok') return fail('load');
    const probe = parseProbe(await waitFor(app, (line) => line.startsWith('page probe '), PROBE_MS));
    if (!probe) return fail('probe');
    const atQuit = readTree(app.pid);
    const procs = [...tree.procs, ...atQuit.procs.filter((p) => !tree.procs.some((q) => q.pid === p.pid && q.start === p.start))];
    const quit = await control.send('quit', LINE_MS);
    const deadline = Date.now() + QUIT_MS;
    let gone = false;
    while (Date.now() < deadline) {
      gone = app.exitCode === 0 && countAlive(procs) === 0;
      if (gone) break;
      await sleep(250);
    }
    if (stepKey('quit', quit.answer, gone ? 'gone' : 'alive') !== 'ok') return { code: 1, line: missLine(machine, 'quit') };
    return { code: 0, line: cellLine(machine, platform, system, probe) };
  } catch {
    return fail('read');
  } finally {
    if (control) control.close();
    stop(app);
  }
}

function parseArgs(argv) {
  if (argv.length !== 2 || argv[0] !== '--bin' || argv[1] === '') throw new Refused('usage', 'node tools/r4-trials/floor/cell.mjs --bin PATH');
  return argv[1];
}

async function main() {
  let bin;
  try {
    bin = parseArgs(process.argv.slice(2));
    checkHosted(process.env);
    if (!machineOf(process.env, process.platform)) throw new Refused('usage', 'the floor trial runs on macOS and Windows');
  } catch (error) {
    if (error instanceof Refused) {
      process.stderr.write(`${error.message}\n`);
      process.exit(2);
    }
    throw error;
  }
  const { code, line } = await cell(bin);
  process.stdout.write(`${line}\n`);
  process.exit(code);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
