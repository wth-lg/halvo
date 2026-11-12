// The rule of the window and state-sync trial (contracts I11.3): the cell and figure lines, the
// figures over the counted runs and the two verdicts, and the result files made from raw output.
// usage: node rule.mjs make window|sync <reading file> <log>... | check window|sync <file>
// make prints the result file from the reading lines and the cell and figure lines of the logs, and
// refuses (exit 1) a log with a miss line or a figure line that its cells do not give; check reads a
// result file and exits 1 when its order, a figure or its verdict does not follow from its lines.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const DESTROY_SAVING = 16777216;
export const SHOW_BOUND_MS = 1000;
export const MIN_COUNTED = 8;
export const SYNC_GROWTH = 8388608;
export const MACHINES = ['hosted-mac', 'hosted-win'];
export const ARMS = ['main', 'a', 'a2', 'b'];
export const WAYS = ['get', 'channel', 'events'];
export const SYNC_ARMS = ['a2', 'b'];

const KINDS = ['reading', 'cell', 'figure', 'verdict'];
const WINDOW_KEYS = ['machine', 'arm', 'run', 'key_bytes', 'second_bytes', 'helpers', 'cpu_ms', 'ticks', 'frames',
  'load_x100', 'cpu_pct', 'counted', 'show_ms'];
const SYNC_KEYS = ['machine', 'way', 'arm', 'sent', 'last', 'order', 'after_hide', 'growth_bytes', 'hidden_ticks'];
const FIGURE_KEYS = ['machine', 'arm', 'median_bytes', 'range_bytes', 'runs'];

/** A line as `{ kind, pairs }` for the kinds reading, cell, figure, verdict and miss; else null. */
export function parseLine(line) {
  const words = String(line).replace(/\r$/, '').split(' ').filter((w) => w !== '');
  const kind = words[0];
  if (!['reading', 'cell', 'figure', 'verdict', 'miss'].includes(kind)) return null;
  const pairs = {};
  for (const word of words.slice(1)) {
    const at = word.indexOf('=');
    if (at > 0) pairs[word.slice(0, at)] = word.slice(at + 1);
  }
  return { kind, pairs };
}

/** The median of `values`: the middle value, or the floor of the mean of the two middle values. */
export function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((x, y) => x - y);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : Math.floor((sorted[mid - 1] + sorted[mid]) / 2);
}

const text = (value) => (value === null || value === undefined ? '-' : String(value));

export function windowCellLine(cell) {
  return `cell trial=window ${WINDOW_KEYS.map((k) => `${k}=${text(cell[k])}`).join(' ')}`;
}

export function syncCellLine(cell) {
  return `cell trial=sync ${SYNC_KEYS.map((k) => `${k}=${text(cell[k])}`).join(' ')}`;
}

export function figureLine(figure) {
  return `figure trial=window ${FIGURE_KEYS.map((k) => `${k}=${text(figure[k])}`).join(' ')}`;
}

const number = (value) => (/^-?\d+$/.test(String(value)) ? Number(value) : value);

function cellOf(pairs) {
  const cell = {};
  for (const [k, v] of Object.entries(pairs)) cell[k] = number(v);
  return cell;
}

const counted = (cells, machine, arm) => cells.filter((c) => c.machine === machine && c.arm === arm && c.counted === 'yes');

/** One figure per machine and arm that has cells, in the order of MACHINES and ARMS. */
export function windowFigures(cells) {
  const figures = [];
  for (const machine of MACHINES) {
    for (const arm of ARMS) {
      if (!cells.some((c) => c.machine === machine && c.arm === arm)) continue;
      const bytes = counted(cells, machine, arm).map((c) => c.key_bytes);
      figures.push({
        machine, arm,
        median_bytes: median(bytes),
        range_bytes: bytes.length ? Math.max(...bytes) - Math.min(...bytes) : null,
        runs: bytes.length,
      });
    }
  }
  return figures;
}

/** The verdict of one machine: `destroy`, `hidden` or `inconclusive`, and `quiet` (`yes`, `no` or `-`). */
export function machineVerdict(cells, machine) {
  const runs = (arm) => counted(cells, machine, arm);
  if (['a', 'a2', 'b'].some((arm) => runs(arm).length < MIN_COUNTED)) return { verdict: 'inconclusive', quiet: '-' };
  const med = (arm) => median(runs(arm).map((c) => c.key_bytes));
  const range = (arm) => {
    const bytes = runs(arm).map((c) => c.key_bytes);
    return Math.max(...bytes) - Math.min(...bytes);
  };
  const saving = med('a2') - med('b');
  const smaller = saving > range('a2') && saving >= DESTROY_SAVING;
  const noHelpers = runs('b').every((c) => c.helpers === 0);
  const quickShow = median(runs('b').map((c) => c.show_ms)) <= SHOW_BOUND_MS;
  if (smaller && noHelpers && quickShow) return { verdict: 'destroy', quiet: '-' };
  return { verdict: 'hidden', quiet: med('a') - med('a2') > range('a') ? 'yes' : 'no' };
}

export function windowVerdict(cells) {
  const mac = machineVerdict(cells, 'hosted-mac');
  const win = machineVerdict(cells, 'hosted-win');
  return {
    hosted_mac: mac.verdict, hosted_win: win.verdict, quiet_mac: mac.quiet, quiet_win: win.quiet,
    line: `verdict window hosted_mac=${mac.verdict} hosted_win=${win.verdict} quiet_mac=${mac.quiet} quiet_win=${win.quiet}`,
  };
}

function cellPasses(c) {
  return c.last === c.sent && c.order === 'ok' && c.after_hide === 0
    && (c.hidden_ticks === 0 || c.hidden_ticks === '-') && typeof c.growth_bytes === 'number' && c.growth_bytes <= SYNC_GROWTH;
}

/** Whether `way` passes on `machine`: each arm has a cell and every cell keeps the pass rule. */
export function syncPasses(cells, machine, way) {
  return SYNC_ARMS.every((arm) => {
    const mine = cells.filter((c) => c.machine === machine && c.way === way && c.arm === arm);
    return mine.length > 0 && mine.every(cellPasses);
  });
}

export function syncVerdict(cells) {
  const way = WAYS.find((w) => MACHINES.every((m) => syncPasses(cells, m, w))) ?? 'none';
  return { sync: way, line: `verdict sync=${way}` };
}

/** The lines of raw output without a job log's time stamp, a byte-order mark or a carriage return. */
export function rawLines(raw) {
  return String(raw).split('\n')
    .map((l) => l.replace(/^\ufeff/, '').replace(/\r$/, '').replace(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z /, ''))
    .filter((l) => l !== '');
}

function ofTrial(lines, trial, kind) {
  return lines.filter((l) => {
    const p = parseLine(l);
    return p && p.kind === kind && p.pairs.trial === trial;
  });
}

/** The result file of `trial` from the text of read.sh and the job logs, or why none can be made. */
export function makeFile(trial, readingText, logs) {
  const reading = ofTrial(rawLines(readingText), trial, 'reading');
  const cells = [];
  const figures = [];
  for (const [i, log] of logs.entries()) {
    const lines = rawLines(log);
    const miss = lines.find((l) => parseLine(l)?.kind === 'miss');
    if (miss) return { ok: false, why: `log ${i + 1} holds a miss line: ${miss}` };
    const mine = ofTrial(lines, trial, 'cell');
    cells.push(...mine);
    if (trial === 'window') {
      const given = ofTrial(lines, trial, 'figure');
      const wanted = windowFigures(mine.map((l) => cellOf(parseLine(l).pairs))).map(figureLine);
      if (given.join('\n') !== wanted.join('\n')) return { ok: false, why: `log ${i + 1}: its figure lines are not the figures of its cells` };
      figures.push(...given);
    }
  }
  const parsed = cells.map((l) => cellOf(parseLine(l).pairs));
  const verdict = trial === 'window' ? windowVerdict(parsed).line : syncVerdict(parsed).line;
  return { ok: true, text: `${[...reading, ...cells, ...figures, verdict].join('\n')}\n` };
}

/** Whether a result file holds its lines in the order of I11.1 and its figures and verdict follow. */
export function checkFile(trial, fileText) {
  const lines = String(fileText).split('\n').filter((l, i, all) => !(l === '' && i === all.length - 1));
  let last = 0;
  for (const line of lines) {
    const p = parseLine(line);
    const at = p ? KINDS.indexOf(p.kind) : -1;
    if (at < 0 || (p.kind !== 'verdict' && p.pairs.trial !== trial)) return { ok: false, why: `a line of no kind of ${trial}: ${line}` };
    if (at < last) return { ok: false, why: `a ${p.kind} line after a ${KINDS[last]} line` };
    last = at;
  }
  const cells = ofTrial(lines, trial, 'cell').map((l) => cellOf(parseLine(l).pairs));
  if (trial === 'window') {
    const wanted = windowFigures(cells).map(figureLine).join('\n');
    if (ofTrial(lines, trial, 'figure').join('\n') !== wanted) return { ok: false, why: 'the figure lines are not the figures of the cells' };
  }
  const verdict = trial === 'window' ? windowVerdict(cells).line : syncVerdict(cells).line;
  if (lines.filter((l) => l.startsWith('verdict ')).length !== 1 || lines.at(-1) !== verdict) {
    return { ok: false, why: `the last line is not ${verdict}` };
  }
  return { ok: true, why: verdict };
}

function main(argv) {
  const [mode, trial, ...files] = argv;
  const good = ['window', 'sync'].includes(trial)
    && ((mode === 'make' && files.length >= 2) || (mode === 'check' && files.length === 1));
  if (!good) {
    process.stderr.write('REFUSED usage: make window|sync <reading file> <log>... or check window|sync <file>\n');
    process.exit(2);
  }
  const read = (f) => fs.readFileSync(f, 'utf8');
  if (mode === 'make') {
    const made = makeFile(trial, read(files[0]), files.slice(1).map(read));
    if (!made.ok) {
      console.log(`rule make trial=${trial} ok=no why=${made.why}`);
      process.exit(1);
    }
    process.stdout.write(made.text);
    return;
  }
  const checked = checkFile(trial, read(files[0]));
  console.log(`rule check trial=${trial} ok=${checked.ok ? 'yes' : 'no'} ${checked.why}`);
  process.exit(checked.ok ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
