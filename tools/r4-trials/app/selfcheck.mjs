// The self-check of the trial app: it starts the built app per arm, sends the lines of a short
// run, reads the system after each answer and prints one line per arm, with one key per step.
// usage: node tools/r4-trials/app/selfcheck.mjs [--bin PATH] [--arm main|a|a2|b]... [--trace]
// Refuses with not_hosted where neither CI nor R4_GUEST is set (contracts I11.1, I0.3).
// Exit: 0 when no key is miss, 1 when a key is miss or the binary is missing, 2 on a refusal.
// A window read is repeated, at most 10 times 300 ms apart, until it shows the state that the
// step expects: on macOS the window list follows the window's own report with a delay.
// A sync run sends 1,000 changes at 100 a second and hides 2 s in, so that the call of the way
// get waits at the core when the hide comes; in arm b it sends 3,000, so that a window made again
// slowly (8.6 s once on Windows) still gets the end of the send.
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { countAlive, readTree, readWindowMatch } from './system/system.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

/** The readers of the system that the self-check uses where it is handed none. */
export const SYSTEM_READERS = Object.freeze({ window: readWindowMatch, tree: readTree, alive: countAlive });

export const ARMS = ['main', 'a', 'a2', 'b'];
export const WAYS = ['get', 'channel', 'events'];
export const SYSTEM = process.platform === 'win32' ? 'windows' : 'macos';
export const SENT = 1000;
export const SENT_B = 3000;
export const RATE = 100;
const START_MS = 60000;
const CONNECT_MS = 10000;
const READ_TRIES = 10;
const READ_EVERY_MS = 300;
const ANSWER_MS = 45000;
const QUIT_MS = 10000;
const SLOW_FRAME_MS = 1400;

/** A refusal: `REFUSED <cause>: <detail>` on standard error and exit 2. */
export class Refused extends Error {
  constructor(cause, detail) {
    super(`REFUSED ${cause}: ${detail}`);
    this.cause = cause;
  }
}

/** The options of the command line, or `Refused` with `usage`. */
export function parseArgs(argv) {
  const options = { bin: null, arms: [], trace: false };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    if (flag === '--bin') {
      if (i + 1 >= argv.length || argv[i + 1] === '') throw new Refused('usage', '--bin takes a path');
      if (options.bin !== null) throw new Refused('usage', '--bin given twice');
      i += 1;
      options.bin = argv[i];
    } else if (flag === '--arm') {
      const arm = argv[i + 1];
      if (!ARMS.includes(arm)) throw new Refused('usage', '--arm takes main, a, a2 or b');
      if (options.arms.includes(arm)) throw new Refused('usage', `--arm ${arm} given twice`);
      i += 1;
      options.arms.push(arm);
    } else if (flag === '--trace') {
      options.trace = true;
    } else {
      throw new Refused('usage', `unknown argument ${JSON.stringify(flag)}`);
    }
  }
  if (options.arms.length === 0) options.arms = [...ARMS];
  return options;
}

/** `Refused` with `not_hosted` where neither `CI` nor `R4_GUEST` is set to a value. */
export function checkHosted(env) {
  const set = (value) => typeof value === 'string' && value !== '';
  if (!set(env.CI) && !set(env.R4_GUEST)) throw new Refused('not_hosted', 'neither CI nor R4_GUEST is set');
}

/** An answer line as `{ word, cause, pairs }`; `null` for no line. */
export function parseAnswer(line) {
  if (typeof line !== 'string') return null;
  const [word, ...rest] = line.split(' ');
  if (word === 'err') return { word, cause: rest[0] ?? '', pairs: {} };
  const pairs = {};
  for (const item of rest) {
    const at = item.indexOf('=');
    if (at > 0) pairs[item.slice(0, at)] = item.slice(at + 1);
  }
  return { word, cause: null, pairs };
}

/** The page's end line of a sync run as `{ last, order, afterHide }`, or `null`. */
export function parseEnd(line) {
  const m = /^page sync end last=(\d+) order=(ok|miss) after_hide=(\d+) hidden_ticks=(\d+)$/.exec(line ?? '');
  return m ? { last: Number(m[1]), order: m[2], afterHide: Number(m[3]) } : null;
}

/** The key of a sync run: the page's end line holds the last change in order, the core ended
 * its send, and for the way `get` nothing that the core sent after a hide reached the page and,
 * after a hide, the page's waiting call got the reply `closed` (`closedAfterHide`, the line
 * `page sync closed last=<n>`; TA-13). In an arm that hides its window and keeps its page
 * (`keptHidden`, arms `a` and `a2`), the ways `channel` and `events` must show a change counted
 * after the hide, so that a core that marks nothing it sends after the hide is a miss (TA-14). */
export function syncKey(way, end, sendDone, closedAfterHide, sent = SENT, keptHidden = false) {
  if (!end || !sendDone) return 'miss';
  if (end.last !== sent || end.order !== 'ok') return 'miss';
  if (way === 'get' && (end.afterHide !== 0 || closedAfterHide !== true)) return 'miss';
  if (way !== 'get' && keptHidden && !(end.afterHide > 0)) return 'miss';
  return 'ok';
}

/** The state that the `ok` answer of each step names, as the readers of the system show it:
 * the window on screen, not visible or absent, and for `quit` the app's tree gone. */
export const STEP_STATES = Object.freeze({ show: 'onscreen', hide: 'hidden', destroy: 'absent', quit: 'gone' });

/** The key of a step that compares an answer with what the reader of the system shows: `ok`
 * for the step's `ok` answer (`ok show_ms=<n>` for `show`, `ok` for the others) together with
 * the state that the answer names, `miss` for every other pair (TA-15). */
export function stepKey(step, answer, read) {
  if (!Object.hasOwn(STEP_STATES, step)) throw new Error(`no step key for ${step}`);
  const answered = step === 'show' ? /^ok show_ms=\d+$/.test(answer ?? '') : answer === 'ok';
  return answered && read === STEP_STATES[step] ? 'ok' : 'miss';
}

/** The key `quiet` from the app's `core quiet` lines: in arm `a2` one line per window made on
 * macOS, and on Windows one line per call at each hide and show, each with `result=ok`. */
export function quietKey(arm, system, lines, { made, hides, shows }) {
  const quiet = lines.filter((line) => line.startsWith('core quiet '));
  if (arm !== 'a2') return quiet.length === 0 ? '-' : 'miss';
  if (quiet.some((line) => !line.endsWith(' result=ok'))) return 'miss';
  const count = (call) => quiet.filter((line) => line === `core quiet system=${system} call=${call} result=ok`).length;
  if (system === 'macos') return count('background_throttling') === made && quiet.length === made ? 'ok' : 'miss';
  const calls = hides + shows;
  const ok = count('set_is_visible') === calls && count('set_memory_usage_target_level') === calls
    && quiet.length === 2 * calls;
  return ok ? 'ok' : 'miss';
}

/** The line of one arm. */
export function armLine(arm, keys) {
  const order = ['show', 'hide', 'destroy', 'counts', 'pid', 'quiet', 'sync_get', 'sync_channel', 'sync_events', 'quit'];
  return `selfcheck arm=${arm} system=${SYSTEM} ${order.map((k) => `${k}=${keys[k]}`).join(' ')}`;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Starts the app with `args` and waits for its port line; a `bin` that ends in `.mjs` runs under
 * this Node. */
export async function launch(bin, args) {
  const [command, first] = bin.endsWith('.mjs') ? [process.execPath, [bin]] : [bin, []];
  const child = spawn(command, [...first, ...args], { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: false });
  const app = { child, pid: child.pid, lines: [], firstLine: null, port: null, exitCode: undefined, waiters: [] };
  let buffer = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', (chunk) => {
    buffer += chunk;
    let at;
    while ((at = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, at).replace(/\r$/, '');
      buffer = buffer.slice(at + 1);
      if (app.firstLine === null) app.firstLine = line;
      app.lines.push(line);
      app.waiters = app.waiters.filter((w) => !w(line));
    }
  });
  child.stderr.resume();
  app.exited = new Promise((resolve) => child.on('exit', (code) => {
    app.exitCode = code;
    resolve(code);
  }));
  child.on('error', () => {});
  const port = await waitLine(app, (line) => /^control port=\d+$/.test(line), START_MS);
  if (!port) throw new Error('no control port line');
  app.port = Number(port.slice('control port='.length));
  return app;
}

function waitLine(app, test, ms) {
  const seen = app.lines.find(test);
  if (seen !== undefined) return Promise.resolve(seen);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      app.waiters = app.waiters.filter((w) => w !== waiter);
      resolve(null);
    }, ms);
    const waiter = (line) => {
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

/** Ends the app and its tree where they still run. */
export function stop(app) {
  if (!app || app.exitCode !== undefined) return;
  try {
    if (process.platform === 'win32') {
      execFileSync('taskkill', ['/PID', String(app.pid), '/T', '/F'], { stdio: 'ignore', timeout: 30000 });
    } else {
      app.child.kill('SIGKILL');
    }
  } catch {
    app.child.kill('SIGKILL');
  }
}

/** One connection to the control port; `send` gives the answer line, `null` when the
 * connection closed, `undefined` when no answer came within its bound. */
export class Control {
  static connect(port, ms = CONNECT_MS) {
    return new Promise((resolve, reject) => {
      const socket = net.connect(port, '127.0.0.1');
      const control = new Control(socket);
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new Error('no connection within its bound'));
      }, ms);
      socket.once('connect', () => {
        clearTimeout(timer);
        resolve(control);
      });
      socket.once('error', (error) => {
        clearTimeout(timer);
        reject(error);
      });
    });
  }

  constructor(socket) {
    this.socket = socket;
    this.buffer = '';
    this.answers = [];
    this.waiters = [];
    this.closed = false;
    socket.setEncoding('utf8');
    socket.on('data', (chunk) => {
      this.buffer += chunk;
      let at;
      while ((at = this.buffer.indexOf('\n')) >= 0) {
        const line = this.buffer.slice(0, at).replace(/\r$/, '');
        this.buffer = this.buffer.slice(at + 1);
        const waiter = this.waiters.shift();
        if (waiter) waiter(line); else this.answers.push(line);
      }
    });
    socket.on('error', () => {});
    socket.on('close', () => {
      this.closed = true;
      for (const waiter of this.waiters.splice(0)) waiter(null);
    });
  }

  async send(line, ms = ANSWER_MS) {
    const started = Date.now();
    if (this.closed) return { answer: null, ms: 0 };
    this.socket.write(`${line}\n`);
    const answer = await new Promise((resolve) => {
      if (this.answers.length) return resolve(this.answers.shift());
      let waiter;
      const timer = setTimeout(() => {
        this.waiters = this.waiters.filter((w) => w !== waiter);
        resolve(undefined);
      }, ms);
      waiter = (got) => {
        clearTimeout(timer);
        resolve(got);
      };
      this.waiters.push(waiter);
    });
    return { answer, ms: Date.now() - started };
  }

  close() {
    this.socket.destroy();
  }
}

function makePages() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-selfcheck-'));
  for (const from of [path.join(here, 'pages'), path.join(here, 'selfcheck-pages')]) {
    for (const name of fs.readdirSync(from)) fs.copyFileSync(path.join(from, name), path.join(dir, name));
  }
  return dir;
}

function tracer(options, arm, run) {
  return (text) => {
    if (options.trace) console.log(`trace arm=${arm} run=${run} ${text}`);
  };
}

function words(result) {
  if (result.answer === undefined) return 'none';
  if (result.answer === null) return 'closed';
  return result.answer;
}

/** The keys of the window run of `arm`, read through `readers` (`SYSTEM_READERS` or a stand-in). */
export async function windowRun(options, arm, pages, readers = SYSTEM_READERS) {
  const keys = {
    show: 'miss',
    hide: arm === 'b' ? '-' : 'miss',
    destroy: arm === 'b' ? 'miss' : '-',
    counts: 'miss',
    pid: 'miss',
    quiet: arm === 'a2' ? 'miss' : '-',
    quit: 'miss',
  };
  const trace = tracer(options, arm, 'window');
  let app = null;
  let control = null;
  try {
    app = await launch(options.bin, ['--arm', arm, '--pages', pages]);
    trace(`first_line_is_port=${app.firstLine === app.lines[0] && /^control port=/.test(app.firstLine) ? 'yes' : 'no'}`);
    control = await Control.connect(app.port);
    const send = async (line) => {
      const result = await control.send(line);
      trace(`sent=${line.replace(/ /g, '_')} answer=${words(result).replace(/ /g, ',')} ms=${result.ms}`);
      return { ...result, parsed: parseAnswer(result.answer) };
    };
    const look = async (expected) => {
      let state = null;
      let by = '-';
      let tries = 0;
      while (tries < READ_TRIES) {
        tries += 1;
        ({ state, by } = readers.window(app.pid));
        if (state === expected) break;
        await sleep(READ_EVERY_MS);
      }
      trace(`read=window state=${state} matched_by=${by} tries=${tries}`);
      return state;
    };
    const shows = [];
    const first = await send('show');
    const firstMs = Number(first.parsed?.pairs.show_ms);
    shows.push(/^control port=\d+$/.test(app.firstLine ?? '') && first.ms <= firstMs + 1500);
    shows.push(stepKey('show', first.answer, await look('onscreen')) === 'ok');
    const tree = readers.tree(app.pid);
    trace(`read=tree count=${tree.count} webview=${tree.webview}`);
    const s1 = (await send('sample')).parsed;
    keys.pid = s1?.word === 'ok' && Number(s1.pairs.pid) === app.pid ? 'ok' : 'miss';
    await sleep(1200);
    const s2 = (await send('sample')).parsed;
    const num = (s, k) => Number(s?.pairs?.[k]);
    let counts = [s1, s2].every((s) => s?.word === 'ok' && /^\d+$/.test(s.pairs.ticks ?? '') && /^\d+$/.test(s.pairs.frames ?? ''))
      && num(s2, 'ticks') > num(s1, 'ticks') && num(s2, 'frames') > num(s1, 'frames');
    let hides = 0;
    if (arm === 'b') {
      const d = await send('destroy');
      const destroyed = stepKey('destroy', d.answer, await look('absent'));
      const after = (await send('sample')).parsed;
      keys.destroy = destroyed === 'ok' && after?.word === 'ok'
        && after.pairs.ticks === '-' && after.pairs.frames === '-' ? 'ok' : 'miss';
    } else {
      const h = await send('hide');
      hides += 1;
      keys.hide = stepKey('hide', h.answer, await look('hidden'));
      if (arm === 'a' || arm === 'a2') {
        const s3 = (await send('sample')).parsed;
        await sleep(3000);
        const s4 = (await send('sample')).parsed;
        counts = counts && [s3, s4].every((s) => s?.word === 'ok' && /^\d+$/.test(s.pairs.ticks ?? ''))
          && num(s4, 'ticks') === num(s3, 'ticks');
      }
    }
    keys.counts = counts ? 'ok' : 'miss';
    const second = await send('show');
    shows.push(stepKey('show', second.answer, await look('onscreen')) === 'ok');
    const load = await send('load slow.html');
    shows.push(load.answer === 'ok');
    await sleep(1000);
    const slow = await send('show');
    shows.push(slow.parsed?.word === 'ok' && Number(slow.parsed.pairs.show_ms) >= SLOW_FRAME_MS);
    keys.show = shows.every(Boolean) ? 'ok' : 'miss';
    await sleep(500);
    keys.quiet = quietKey(arm, SYSTEM, app.lines, { made: arm === 'a2' ? 1 : 0, hides, shows: 3 });
    const atQuit = readers.tree(app.pid);
    trace(`read=tree count=${atQuit.count} webview=${atQuit.webview}`);
    const procs = [...tree.procs, ...atQuit.procs.filter((p) => !tree.procs.some((q) => q.pid === p.pid && q.start === p.start))];
    const quit = await send('quit');
    const deadline = Date.now() + QUIT_MS;
    let exited = false;
    let alive = -1;
    while (Date.now() < deadline) {
      exited = app.exitCode !== undefined;
      alive = readers.alive(procs);
      if (exited && alive === 0) break;
      await sleep(250);
    }
    trace(`quit exit=${app.exitCode ?? 'none'} alive=${alive}`);
    const quitRead = exited && app.exitCode === 0 && alive === 0 ? 'gone' : 'alive';
    keys.quit = stepKey('quit', quit.answer, quitRead) === 'ok'
      && tree.webview >= 1 && atQuit.webview >= 1 ? 'ok' : 'miss';
  } catch (error) {
    trace(`error=${String(error && error.message).replace(/[^A-Za-z0-9_.-]+/g, '_').slice(0, 80)}`);
  } finally {
    if (control) control.close();
    stop(app);
  }
  return keys;
}

async function syncRun(options, arm, way, pages) {
  const trace = tracer(options, arm, way);
  let app = null;
  let control = null;
  try {
    app = await launch(options.bin, ['--arm', arm, '--sync', way, '--pages', pages]);
    control = await Control.connect(app.port);
    const send = async (line) => {
      const result = await control.send(line);
      trace(`sent=${line.replace(/ /g, '_')} answer=${words(result).replace(/ /g, ',')} ms=${result.ms}`);
      return result.answer;
    };
    const sent = arm === 'b' ? SENT_B : SENT;
    const steps = [await send('show'), await send(`send ${sent} ${RATE}`)];
    await sleep(2000);
    const beforeHide = app.lines.length;
    if (arm === 'a' || arm === 'a2') steps.push(await send('hide'));
    if (arm === 'b') steps.push(await send('destroy'));
    if (arm !== 'main') {
      await sleep(1500);
      steps.push(await send('show'));
    }
    const endLine = await waitLine(app, (line) => line.startsWith('page sync end '), 45000);
    const doneLine = await waitLine(app, (line) => line === `core send_done sent=${sent}`, 5000);
    const end = parseEnd(endLine);
    const closedLines = app.lines.slice(beforeHide).filter((line) => /^page sync closed last=\d+$/.test(line)).length;
    const hidden = arm === 'a' || arm === 'a2';
    trace(`end=${end ? `last=${end.last},order=${end.order},after_hide=${end.afterHide}` : 'none'} send_done=${doneLine ? 'yes' : 'no'} closed_lines_after_hide=${closedLines}`);
    for (const line of app.lines.filter((l) => /^page (sync closed|window shown) /.test(l))) trace(`page_line=${line.slice(5).replace(/ /g, ',')}`);
    const key = steps.every((answer) => typeof answer === 'string' && answer.startsWith('ok'))
      ? syncKey(way, end, doneLine, hidden ? closedLines > 0 : true, sent, hidden) : 'miss';
    await send('quit');
    await Promise.race([app.exited, sleep(QUIT_MS)]);
    return key;
  } catch (error) {
    trace(`error=${String(error && error.message).replace(/[^A-Za-z0-9_.-]+/g, '_').slice(0, 80)}`);
    return 'miss';
  } finally {
    if (control) control.close();
    stop(app);
  }
}

/** Runs the self-check for `options` and returns the exit status. */
export async function selfcheck(options) {
  const bin = options.bin ?? path.join(here, 'target', 'release', process.platform === 'win32' ? 'r4-trial-app.exe' : 'r4-trial-app');
  if (!fs.existsSync(bin)) {
    console.log(`selfcheck binary=missing`);
    return 1;
  }
  const pages = makePages();
  let missed = false;
  for (const arm of options.arms) {
    const keys = await windowRun({ ...options, bin }, arm, pages);
    for (const way of WAYS) keys[`sync_${way}`] = await syncRun({ ...options, bin }, arm, way, pages);
    const line = armLine(arm, keys);
    console.log(line);
    if (/=miss\b/.test(line)) missed = true;
  }
  return missed ? 1 : 0;
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
    checkHosted(process.env);
  } catch (error) {
    if (error instanceof Refused) {
      process.stderr.write(`${error.message}\n`);
      process.exit(2);
    }
    throw error;
  }
  process.exit(await selfcheck(options));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
