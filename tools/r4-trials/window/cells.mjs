// The cells of the window trial and of the state-sync trial (contracts I11.3), called by run.sh.
// usage: node cells.mjs --arm main|a|a2|b --runs N | --sync get|channel|events --arm a2|b
//        node cells.mjs --check <the same arguments>: refuses or prints nothing, starts nothing
// Each step's answer is compared with what the readers of the system show (TA-9, TA-12): a step
// whose read differs from its answer, or a line with no answer inside its own limit (TA-16), prints
// `miss trial=<t> machine=<m> arm=<arm> run=<i> step=<step>`. A cell that cannot be made for another
// cause (an err answer, a sampler that found no process, a page line that never came) prints
// `nocell trial=<t> machine=<m> arm=<arm> run=<i> step=<step> cause=<word>`; one err answer makes a
// cell: `err timeout` of the second show of a sync cell, when the window reads on screen. A sync
// cell's `order` is a miss when any page of the run saw a step back (its `sync order_miss` or
// `after_end ... order=miss` line). Exit: 0 when every cell was printed, 1 when one was not, 2 on a
// refusal.
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Control, launch, stop, stepKey, checkHosted, Refused, STEP_STATES, SYSTEM_READERS } from '../app/selfcheck.mjs';
import { figureLine, syncCellLine, windowCellLine, windowFigures } from './rule.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

/** The timing of a window run: 5 s shown, then 330 s hidden or 120 s destroyed (I11.3). */
export const WINDOW = Object.freeze({ shownMs: 5000, hiddenMs: 330000, destroyedMs: 120000 });

/** The sync run: 200,000 changes at 1,000 a second, the hide at change 100,000, the show 60 s
 * later; `settleMs` lets the page subscribe before the first change. */
export const SYNC = Object.freeze({ count: 200000, rate: 1000, hideAt: 100000, showAfterMs: 60000, settleMs: 2000, endMs: 120000 });

/** The time limit of each control line (TA-16): show waits up to 30 s in the app. */
export const LIMITS = Object.freeze({ show: 45000, hide: 20000, destroy: 20000, sample: 20000, send: 10000, quit: 15000 });

const APP_LINE = /^(core send_done |core quiet |page sync (end|stalled|closed|after_end|order_miss) |page sync mark=|page window shown |page policy )/;

const READ_TRIES = 20;
const READ_EVERY_MS = 500;
const GONE_MS = 30000;

const ARMS = ['main', 'a', 'a2', 'b'];
const WAYS = ['get', 'channel', 'events'];

/** The arguments as `{ trial, arm, runs }` or `{ trial, way, arm }`, or `Refused` with `usage`. */
export function parseArgs(argv) {
  const seen = new Map();
  for (let i = 0; i < argv.length; i += 2) {
    const [flag, value] = [argv[i], argv[i + 1]];
    if (!['--arm', '--runs', '--sync'].includes(flag)) throw new Refused('usage', `unknown argument ${JSON.stringify(flag)}`);
    if (value === undefined) throw new Refused('usage', `${flag} needs a value`);
    if (seen.has(flag)) throw new Refused('usage', `${flag} given twice`);
    seen.set(flag, value);
  }
  const arm = seen.get('--arm');
  if (seen.has('--sync')) {
    const way = seen.get('--sync');
    if (!WAYS.includes(way)) throw new Refused('usage', '--sync takes get, channel or events');
    if (!['a2', 'b'].includes(arm)) throw new Refused('usage', 'a sync cell takes --arm a2 or b');
    if (seen.has('--runs')) throw new Refused('usage', 'a sync cell takes no --runs');
    return { trial: 'sync', way, arm };
  }
  if (!ARMS.includes(arm)) throw new Refused('usage', '--arm takes main, a, a2 or b');
  const runs = seen.get('--runs');
  if (!/^[1-9]\d{0,2}$/.test(runs ?? '')) throw new Refused('usage', '--runs takes a count from 1 to 999');
  return { trial: 'window', arm, runs: Number(runs) };
}

/** The machine of the lines: `hosted-mac` or `hosted-win` where CI is set, else `guest`. */
export function machineOf(env, platform) {
  if (typeof env.CI === 'string' && env.CI !== '') return platform === 'win32' ? 'hosted-win' : 'hosted-mac';
  return 'guest';
}

/** A sampler line of sample.sh or sample.ps1 as figures, or `null`. */
export function parseSample(lines) {
  const head = /^sample pid=(\d+) key_bytes=(\d+) second_bytes=(\d+) helpers=(\d+) load_x100=(\d+|-) cpu_pct=(\d+|-) ncpu=(\d+)$/.exec(lines[0] ?? '');
  if (!head) return null;
  const n = (v) => (v === '-' ? '-' : Number(v));
  const procs = lines.slice(1).map((l) => /^proc pid=(\d+) start=(\d+) cpu_ms=(\d+)$/.exec(l)).filter(Boolean)
    .map((m) => ({ pid: Number(m[1]), start: m[2], cpu_ms: Number(m[3]) }));
  return {
    pid: Number(head[1]), key_bytes: Number(head[2]), second_bytes: Number(head[3]), helpers: Number(head[4]),
    load_x100: n(head[5]), cpu_pct: n(head[6]), ncpu: Number(head[7]), procs,
  };
}

/** The sampler of this system: sample.sh on macOS, sample.ps1 on Windows. */
export function systemSampler(pid) {
  const run = process.platform === 'win32'
    ? spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'sample.ps1'), '-Id', String(pid)], { encoding: 'utf8', timeout: 120000 })
    : spawnSync('sh', [path.join(here, 'sample.sh'), String(pid)], { encoding: 'utf8', timeout: 120000 });
  if (run.status !== 0) return null;
  return parseSample(run.stdout.split('\n').map((l) => l.replace(/\r$/, '')).filter((l) => l !== ''));
}

/** The processor time between two samples: per process alive in both its difference, per process
 * that appeared between them its time, and nothing for one that left. */
export function cpuBetween(first, second) {
  let sum = 0;
  for (const p of second.procs) {
    const before = first.procs.find((q) => q.pid === p.pid && q.start === p.start);
    sum += Math.max(0, p.cpu_ms - (before ? before.cpu_ms : 0));
  }
  return sum;
}

/** Whether a run counts: the load at or under the processor count, or the use at or under 50 %. */
export function countedOf(figures) {
  if (typeof figures.load_x100 === 'number') return figures.load_x100 <= figures.ncpu * 100 ? 'yes' : 'no';
  if (typeof figures.cpu_pct === 'number') return figures.cpu_pct <= 50 ? 'yes' : 'no';
  return 'no';
}

const count = (word) => (/^\d+$/.test(word ?? '') ? Number(word) : null);

/** `ok` when the answer and the read agree; `miss` when the read differs from an `ok` answer, when
 * an `err` answer comes with the state that an `ok` answer names (but for `err timeout` of `show`,
 * which says that no frame came and names no state), or when no answer came inside the line's
 * limit; else `fail` for an `err` answer. */
export function stepCheck(step, answer, read) {
  if (typeof answer !== 'string') return 'miss';
  if (answer.startsWith('err ')) {
    const namesNoState = step === 'show' && answer === 'err timeout';
    return !namesNoState && read === STEP_STATES[step] ? 'miss' : 'fail';
  }
  return stepKey(step, answer, read);
}

/** The check of a sample: the window is in the state of the last step and the app's process lives,
 * whatever the answer; the answer names the app's pid, and no counts where the window is absent. */
export function sampleCheck(answer, pid, windowRead, expected, tree) {
  if (typeof answer !== 'string') return 'miss';
  if (windowRead !== expected || !(tree.count >= 1)) return 'miss';
  if (answer.startsWith('err ')) return 'fail';
  const m = /^ok pid=(\d+) ticks=(\d+|-) frames=(\d+|-)$/.exec(answer);
  if (!m || Number(m[1]) !== pid) return 'miss';
  if (windowRead === 'absent' && (m[2] !== '-' || m[3] !== '-')) return 'miss';
  return 'ok';
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

class Stop extends Error {
  constructor(kind, step, cause) {
    super(`${kind} ${step}`);
    this.kind = kind;
    this.step = step;
    this.cause = cause;
  }
}

function binary() {
  const target = process.env.CARGO_TARGET_DIR || path.join(here, '..', 'app', 'target');
  return path.join(target, 'release', process.platform === 'win32' ? 'r4-trial-app.exe' : 'r4-trial-app');
}

/** The default dependencies of a cell: the built app, the readers of the system and the sampler. */
export function systemDeps() {
  return { bin: binary(), readers: SYSTEM_READERS, sampler: systemSampler, print: (line) => console.log(line) };
}

function session(deps, label) {
  const state = { app: null, control: null, procs: [] };
  const trace = (text) => deps.print(`trace ${label} ${text}`);
  const look = async (expected) => {
    let read = null;
    for (let i = 0; i < READ_TRIES; i += 1) {
      read = deps.readers.window(state.app.pid).state;
      if (read === expected) break;
      await sleep(READ_EVERY_MS);
    }
    return read;
  };
  const tree = () => {
    const t = deps.readers.tree(state.app.pid);
    for (const p of t.procs) if (!state.procs.some((q) => q.pid === p.pid && q.start === p.start)) state.procs.push(p);
    return t;
  };
  const send = async (line, step) => {
    const result = await state.control.send(line, LIMITS[step]);
    const answer = result.answer;
    trace(`step=${step} answer=${typeof answer === 'string' ? answer.replace(/ /g, ',') : 'none'} ms=${result.ms}`);
    return answer;
  };
  return {
    state,
    trace,
    async start(args) {
      state.app = await launch(deps.bin, args);
      state.control = await Control.connect(state.app.port);
      tree();
    },
    async line(line, step) {
      const answer = await send(line, step);
      if (typeof answer !== 'string') throw new Stop('miss', step);
      if (answer !== 'ok') throw new Stop('nocell', step, answer.slice(4).replace(/[^a-z_]/g, '') || 'err');
    },
    async step(step, line, { allowTimeout = false } = {}) {
      const answer = await send(line, step);
      const read = typeof answer === 'string' ? await look(step === 'destroy' ? 'absent' : step === 'hide' ? 'hidden' : 'onscreen') : '-';
      trace(`step=${step} read=${read}`);
      const timedOut = allowTimeout && answer === 'err timeout' && read === 'onscreen';
      const checked = timedOut ? 'ok' : stepCheck(step, answer, read);
      if (checked === 'miss') throw new Stop('miss', step);
      if (checked === 'fail') throw new Stop('nocell', step, answer.slice(4).replace(/[^a-z_]/g, '') || 'err');
      return answer;
    },
    async sample(expected) {
      const windowRead = deps.readers.window(state.app.pid).state;
      const t = tree();
      const answer = await send('sample', 'sample');
      const checked = sampleCheck(answer, state.app.pid, windowRead, expected, t);
      trace(`step=sample read=${windowRead} tree=${t.count}`);
      if (checked === 'miss') throw new Stop('miss', 'sample');
      if (checked === 'fail') throw new Stop('nocell', 'sample', 'err');
      const figures = deps.sampler(state.app.pid);
      if (!figures) throw new Stop('nocell', 'sample', 'sampler');
      trace(`sample load_x100=${figures.load_x100} cpu_pct=${figures.cpu_pct} ncpu=${figures.ncpu}`);
      const [, ticks, frames] = /ticks=(\S+) frames=(\S+)$/.exec(answer);
      return { ...figures, ticks: count(ticks), frames: count(frames) };
    },
    async quit() {
      const answer = await send('quit', 'quit');
      const deadline = Date.now() + GONE_MS;
      let gone = false;
      while (Date.now() < deadline) {
        gone = state.app.exitCode !== undefined && deps.readers.alive(state.procs) === 0;
        if (gone) break;
        await sleep(250);
      }
      const read = gone && state.app.exitCode === 0 ? 'gone' : 'alive';
      trace(`step=quit read=${read}`);
      const checked = stepCheck('quit', answer, read);
      if (checked === 'miss') throw new Stop('miss', 'quit');
      if (checked === 'fail') throw new Stop('nocell', 'quit', 'err');
    },
    end() {
      if (state.control) state.control.close();
      stop(state.app);
      let afterEnd = 0;
      for (const line of state.app?.lines ?? []) {
        if (!APP_LINE.test(line)) continue;
        if (line.startsWith('page sync after_end ') && (afterEnd += 1) > 20) continue;
        trace(line);
      }
    },
  };
}

function stopLine(error, trial, machine, arm, run, way) {
  const at = way ? `way=${way} ` : '';
  if (error instanceof Stop && error.kind === 'miss') return `miss trial=${trial} machine=${machine} arm=${arm} run=${run} step=${error.step}`;
  if (error instanceof Stop) return `nocell trial=${trial} machine=${machine} ${at}arm=${arm} run=${run} step=${error.step} cause=${error.cause}`;
  return `nocell trial=${trial} machine=${machine} ${at}arm=${arm} run=${run} step=start cause=start`;
}

/** One run of the window trial; returns the cell, or `null` after printing its miss or nocell line. */
export async function windowCell({ machine, arm, run, timing = WINDOW }, deps) {
  const s = session(deps, `trial=window arm=${arm} run=${run}`);
  try {
    await s.start(['--arm', arm]);
    await s.step('show', 'show');
    await sleep(timing.shownMs);
    const away = arm === 'b' ? 'destroy' : 'hide';
    const expected = arm === 'b' ? 'absent' : 'hidden';
    await s.step(away, away);
    const first = await s.sample(expected);
    await sleep(arm === 'b' ? timing.destroyedMs : timing.hiddenMs);
    const second = await s.sample(expected);
    const shown = await s.step('show', 'show');
    await s.quit();
    const diff = (k) => (first[k] === null || second[k] === null ? null : second[k] - first[k]);
    const cell = {
      trial: 'window', machine, arm, run,
      key_bytes: second.key_bytes, second_bytes: second.second_bytes, helpers: second.helpers,
      cpu_ms: cpuBetween(first, second), ticks: diff('ticks'), frames: diff('frames'),
      load_x100: second.load_x100, cpu_pct: second.cpu_pct, counted: countedOf(second),
      show_ms: Number(/show_ms=(\d+)/.exec(shown)[1]),
    };
    deps.print(windowCellLine(cell));
    return cell;
  } catch (error) {
    deps.print(stopLine(error, 'window', machine, arm, run));
    return null;
  } finally {
    s.end();
  }
}

async function waitFor(app, test, from, ms) {
  const deadline = Date.now() + ms;
  for (;;) {
    const found = app.lines.slice(from).find(test);
    if (found !== undefined) return found;
    if (Date.now() >= deadline || app.exitCode !== undefined) return null;
    await sleep(100);
  }
}

const PAGE_STATE = /^page sync (end|stalled) last=(\d+) order=(ok|miss) after_hide=(\d+) hidden_ticks=(\d+)$/;

/** One cell of the sync trial; returns the cell, or `null` after printing its miss or nocell line. */
export async function syncCell({ machine, way, arm, timing = SYNC }, deps) {
  const s = session(deps, `trial=sync way=${way} arm=${arm}`);
  try {
    await s.start(['--arm', arm, '--sync', way]);
    const { app } = s.state;
    await s.step('show', 'show');
    await sleep(timing.settleMs);
    const base = await s.sample('onscreen');
    await s.line(`send ${timing.count} ${timing.rate}`, 'send');
    const sendMs = (timing.count * 1000) / timing.rate;
    const mark = await waitFor(app, (l) => l === `page sync mark=${timing.hideAt}`, 0, sendMs + timing.endMs);
    if (!mark) throw new Stop('nocell', 'send', 'no_mark');
    const hideFrom = app.lines.length;
    await s.step(arm === 'b' ? 'destroy' : 'hide', arm === 'b' ? 'destroy' : 'hide');
    await sleep(timing.showAfterMs);
    const showFrom = app.lines.length;
    await s.step('show', 'show', { allowTimeout: true });
    if (way === 'get' && arm === 'a2' && !app.lines.slice(hideFrom, showFrom).some((l) => /^page sync closed last=\d+$/.test(l))) {
      throw new Stop('miss', 'hide');
    }
    const done = await waitFor(app, (l) => l === `core send_done sent=${timing.count}`, 0, sendMs + timing.endMs);
    if (!done) throw new Stop('nocell', 'send', 'no_send_done');
    const doneAt = app.lines.indexOf(done);
    let endLine = await waitFor(app, (l) => l.startsWith('page sync end '), 0, timing.endMs);
    if (!endLine) {
      const stalls = app.lines.slice(doneAt).filter((l) => l.startsWith('page sync stalled '));
      endLine = stalls.at(-1) ?? null;
    }
    const end = PAGE_STATE.exec(endLine ?? '');
    if (!end) throw new Stop('nocell', 'send', 'no_end');
    let afterHide = Number(end[4]);
    let hiddenTicks = '-';
    if (arm === 'a2') {
      const shown = /^page window shown after_hide=(\d+) hidden_ticks=(\d+) hide_seen=(yes|no)$/.exec(await waitFor(app, (l) => l.startsWith('page window shown '), showFrom, 5000) ?? '');
      if (!shown) throw new Stop('nocell', 'show', 'no_shown_line');
      if (shown[3] !== 'yes') throw new Stop('nocell', 'hide', 'hide_unseen');
      afterHide = Number(shown[1]);
      hiddenTicks = Number(shown[2]);
    }
    const last = await s.sample('onscreen');
    const late = app.lines.some((l) => /^page sync (after_end seq=\d+ last=\d+ order=miss|order_miss seq=\d+ last=\d+)$/.test(l));
    await s.quit();
    const cell = {
      trial: 'sync', machine, way, arm, sent: Number(done.slice(done.indexOf('=') + 1)), last: Number(end[2]), order: late ? 'miss' : end[3],
      after_hide: afterHide, growth_bytes: last.second_bytes - base.second_bytes, hidden_ticks: hiddenTicks,
    };
    deps.print(syncCellLine(cell));
    return cell;
  } catch (error) {
    deps.print(stopLine(error, 'sync', machine, arm, 1, way));
    return null;
  } finally {
    s.end();
  }
}

/** Makes the cells that `options` names, prints them and, for the window trial, the figure of the
 * arm over its counted runs; returns the exit status. */
export async function main(options, machine, deps, timing = undefined) {
  if (options.trial === 'sync') {
    return (await syncCell({ machine, way: options.way, arm: options.arm, timing: timing ?? SYNC }, deps)) ? 0 : 1;
  }
  const cells = [];
  for (let run = 1; run <= options.runs; run += 1) {
    const cell = await windowCell({ machine, arm: options.arm, run, timing: timing ?? WINDOW }, deps);
    if (cell) cells.push(cell);
  }
  for (const figure of windowFigures(cells)) deps.print(figureLine(figure));
  return cells.length === options.runs ? 0 : 1;
}

async function cli(argv) {
  const check = argv[0] === '--check';
  let options;
  try {
    options = parseArgs(check ? argv.slice(1) : argv);
    checkHosted(process.env);
  } catch (error) {
    if (error instanceof Refused) {
      process.stderr.write(`${error.message}\n`);
      process.exit(2);
    }
    throw error;
  }
  if (check) return;
  process.exit(await main(options, machineOf(process.env, process.platform), systemDeps()));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await cli(process.argv.slice(2));
}
