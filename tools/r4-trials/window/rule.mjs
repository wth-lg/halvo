// The rule of the window and state-sync trial (contracts I11.3): the cell and figure lines, the
// figures over the counted runs and the two verdicts, and the result files made from raw output.
// usage: node rule.mjs make window|sync <reading file> <log>... | check window|sync <file> [<log>...]
// make prints the result file from the reading lines and the cell and figure lines of the logs, and
// refuses (exit 1) what makeFile names; check reads a result file and exits 1 when what checkFile
// names does not hold. A window file is checked with its job logs, which give each machine's
// processor count; without them check says mac_counted=unchecked.
import { createHash } from 'node:crypto';
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
const FIGURE_KEYS = ['machine', 'arm', 'median_bytes', 'range_bytes', 'runs', 'max_bytes', 'median_show_ms', 'max_show_ms', 'show_over_1000'];
const FIGURE_KEYS_OF_RUN_43325502435 = FIGURE_KEYS.slice(0, 5);
export const RUNS_PER_ARM = 10;

/** The SHA-256 of the 80 cell lines of docs/r4/trials/window.md as run 43325502435 gave them, joined
 * by line feeds: the one set of cells whose file may keep the five-key figure form that its jobs
 * printed (ruling RD-57 E4). */
export const CELLS_OF_RUN_43325502435 = 'dc206d0cc10b4439658edd49e70faceb6a88be31fcc7d237c27ba4cfb4f5622a';

const NAMED = /(?<![A-Za-z0-9_=])(nocell|miss)\s+trial=/i;
const FIRST_WORD = /^\s*([A-Za-z]+)/;

/** The kind of a trial's line in a job log, or null for a line that is not the trial's: after the time
 * stamp and leading white space its first word (the first run of letters) is `cell`, `nocell` or
 * `miss` in any letter case, or it names `nocell` or `miss` before `trial=` (NAMED). Every other line
 * is not the trial's, whatever it holds further on (ruling RD-60). */
export function trialKind(line) {
  const first = FIRST_WORD.exec(line)?.[1].toLowerCase();
  if (first === 'cell' || first === 'nocell' || first === 'miss') return first;
  const named = NAMED.exec(line);
  return named ? named[1].toLowerCase() : null;
}

/** A line as `{ kind, pairs }` for the kinds reading, cell, figure, verdict, miss and nocell; else null. */
export function parseLine(line) {
  const words = String(line).replace(/\r$/, '').split(' ').filter((w) => w !== '');
  const kind = words[0];
  if (!['reading', 'cell', 'figure', 'verdict', 'miss', 'nocell'].includes(kind)) return null;
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

/** The figure line of I11.3 with, beside its medians, the largest key_bytes and show_ms of the counted
 * runs and how many of them show above the line of 1,000 ms; `old` gives the form that the jobs of
 * run 43325502435 printed, without them. */
export function figureLine(figure, { old = false } = {}) {
  const keys = old ? FIGURE_KEYS_OF_RUN_43325502435 : FIGURE_KEYS;
  return `figure trial=window ${keys.map((k) => `${k}=${text(figure[k])}`).join(' ')}`;
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
      const runs = counted(cells, machine, arm);
      const bytes = runs.map((c) => c.key_bytes);
      const shows = runs.map((c) => c.show_ms);
      figures.push({
        machine, arm,
        median_bytes: median(bytes),
        range_bytes: bytes.length ? Math.max(...bytes) - Math.min(...bytes) : null,
        runs: bytes.length,
        max_bytes: bytes.length ? Math.max(...bytes) : null,
        median_show_ms: median(shows),
        max_show_ms: shows.length ? Math.max(...shows) : null,
        show_over_1000: shows.length ? shows.filter((ms) => ms > SHOW_BOUND_MS).length : null,
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

/** `counted` as the rule gives it: from `cpu_pct` on Windows, from `load_x100` and the processor
 * count `ncpu` on macOS; `null` where the cell and `ncpu` do not give it. */
export function countedFrom(cell, ncpu) {
  if (typeof cell.cpu_pct === 'number') return cell.cpu_pct <= 50 ? 'yes' : 'no';
  if (typeof cell.load_x100 === 'number' && typeof ncpu === 'number') return cell.load_x100 <= ncpu * 100 ? 'yes' : 'no';
  return null;
}

function twice(trial, cells) {
  const seen = new Set();
  for (const c of cells) {
    const key = trial === 'window' ? `machine=${c.machine} arm=${c.arm} run=${c.run}` : `machine=${c.machine} way=${c.way} arm=${c.arm}`;
    if (seen.has(key)) return key;
    seen.add(key);
  }
  return null;
}

const OBJECT_LINE = /^reading trial=sync machine=mac object=\S+$/;

const UINT = /^\d+$/;
const UINT_OR_DASH = /^(\d+|-)$/;
const RUN = /^([1-9]|10)$/;
const WINDOW_FORM = {
  machine: (v) => MACHINES.includes(v), arm: (v) => ARMS.includes(v), run: (v) => RUN.test(v),
  key_bytes: (v) => UINT.test(v), second_bytes: (v) => UINT.test(v), helpers: (v) => UINT.test(v), cpu_ms: (v) => UINT.test(v),
  ticks: (v) => UINT_OR_DASH.test(v), frames: (v) => UINT_OR_DASH.test(v), load_x100: (v) => UINT_OR_DASH.test(v),
  cpu_pct: (v) => UINT_OR_DASH.test(v), counted: (v) => v === 'yes' || v === 'no', show_ms: (v) => UINT.test(v),
};
const SYNC_FORM = {
  machine: (v) => MACHINES.includes(v), way: (v) => WAYS.includes(v), arm: (v) => SYNC_ARMS.includes(v), sent: (v) => v === '200000',
  last: (v) => UINT.test(v), order: (v) => v === 'ok' || v === 'miss', after_hide: (v) => UINT.test(v),
  growth_bytes: (v) => /^-?\d+$/.test(v), hidden_ticks: (v) => UINT_OR_DASH.test(v),
};
const READINGS = {
  window: [
    /^reading trial=window machine=mac arm=a2 system=(macos|windows) call=[a-z_]+ value=[a-z0-9_]+$/,
    /^reading trial=window machine=mac figure=(key_bytes|second_bytes|cpu_ms) system=(macos|windows) command=[A-Za-z_-]+$/,
    /^reading trial=window machine=mac arm=a2 system=macos expect=same_as_a source=[a-z0-9.-]+ policy_set=[a-z_]+$/,
  ],
  sync: [/^reading trial=sync machine=mac object=\S+$/, /^reading trial=sync machine=mac way=channel order_by=index$/],
};

/** Why a cell line is not of its form in I11.3, or null: the keys in their order and no other, a
 * number where the form has one, the closed sets of machines, arms and ways, `sent=200000`,
 * `hidden_ticks` a number in arm a2 and `-` in arm b, runs 1 to 10, and on each machine the load
 * that its sampler gives (`load_x100` on hosted-mac, `cpu_pct` on hosted-win). */
export function cellFormError(trial, line) {
  const words = String(line).replace(/\r$/, '').split(' ').filter((w) => w !== '');
  const form = trial === 'window' ? WINDOW_FORM : SYNC_FORM;
  const keys = ['trial', ...Object.keys(form)];
  if (words[0] !== 'cell' || words.length !== keys.length + 1) return 'its keys are not those of the form';
  const pairs = {};
  for (const [i, word] of words.slice(1).entries()) {
    const at = word.indexOf('=');
    if (at <= 0 || word.slice(0, at) !== keys[i]) return `its key ${i + 1} is not ${keys[i]}`;
    pairs[keys[i]] = word.slice(at + 1);
  }
  if (pairs.trial !== trial) return 'it is of another trial';
  for (const [key, ok] of Object.entries(form)) if (!ok(pairs[key])) return `${key}=${pairs[key]} is not of the form`;
  if (trial === 'sync' && (pairs.arm === 'a2') !== UINT.test(pairs.hidden_ticks)) return 'hidden_ticks is a number in arm a2 alone';
  if (trial === 'window') {
    const mac = pairs.machine === 'hosted-mac';
    if (mac ? !(UINT.test(pairs.load_x100) && pairs.cpu_pct === '-') : !(UINT.test(pairs.cpu_pct) && pairs.load_x100 === '-')) {
      return `the load is not the one that the sampler of ${pairs.machine} gives`;
    }
  }
  return null;
}

/** Why the reading lines of a file are not those that the unit prints, or null: each of a form of
 * read.sh with `machine=mac`, and in a window file each sampler command and a quieting call for
 * each system and the line of TA-5; in a sync file the object line and the line of channel order. */
export function readingError(trial, reading) {
  const bad = reading.find((l) => !READINGS[trial].some((re) => re.test(l)));
  if (bad) return `a reading line of no form that read.sh prints: ${bad}`;
  const wanted = trial === 'window'
    ? [...['macos', 'windows'].flatMap((sys) => [
      ...['key_bytes', 'second_bytes', 'cpu_ms'].map((f) => new RegExp(`^reading trial=window machine=mac figure=${f} system=${sys} `)),
      new RegExp(`^reading trial=window machine=mac arm=a2 system=${sys} call=`)]), / expect=same_as_a /]
    : READINGS.sync;
  const missing = wanted.find((re) => !reading.some((l) => re.test(l)));
  return missing ? `no reading line of the form ${missing}` : null;
}

/** Why the cells of a file are not one for each run of I11.3, or null: a window file holds exactly 10
 * cells for each machine and arm, and a sync file one cell for each machine, way and arm. */
export function countError(trial, cellLines) {
  const pairs = cellLines.map((l) => parseLine(l).pairs);
  if (trial === 'window') {
    for (const machine of MACHINES) {
      for (const arm of ARMS) {
        const n = pairs.filter((p) => p.machine === machine && p.arm === arm).length;
        if (n !== RUNS_PER_ARM) return `${machine} arm ${arm} holds ${n} cells, not ${RUNS_PER_ARM}`;
      }
    }
    return null;
  }
  for (const machine of MACHINES) {
    for (const way of WAYS) {
      for (const arm of SYNC_ARMS) {
        const n = pairs.filter((p) => p.machine === machine && p.way === way && p.arm === arm).length;
        if (n === 0) return `no cell of machine=${machine} way=${way} arm=${arm}`;
        if (n > 1) return `${n} cells of machine=${machine} way=${way} arm=${arm}`;
      }
    }
  }
  return null;
}

function logCounts(logs) {
  const counts = {};
  for (const log of logs) {
    const lines = rawLines(log);
    const machines = new Set(ofTrial(lines, 'window', 'cell').map((l) => parseLine(l).pairs.machine));
    const ncpus = new Set(lines.map((l) => /^trace trial=window .* ncpu=(\d+)$/.exec(l)).filter(Boolean).map((m) => Number(m[1])));
    for (const machine of machines) {
      counts[machine] = counts[machine] ?? new Set();
      for (const n of ncpus) counts[machine].add(n);
    }
  }
  return counts;
}

/** The result file of `trial` from the text of read.sh and the job logs, or why none can be made:
 * a log with a `miss` or `nocell` line or a `cell` line that it does not read exactly (`trialKind`), a run that appears twice, a `counted` that its load does not give, a
 * figure over all cells of a machine and arm that no log printed, or a sync file without its
 * object line. */
export function makeFile(trial, readingText, logs) {
  const reading = ofTrial(rawLines(readingText), trial, 'reading');
  if (trial === 'sync' && !reading.some((l) => OBJECT_LINE.test(l))) return { ok: false, why: 'no reading line names the object of call.js' };
  const cells = [];
  const given = new Set();
  for (const [i, log] of logs.entries()) {
    const lines = rawLines(log);
    for (const line of lines) {
      const kind = trialKind(line);
      if (kind === 'nocell' || kind === 'miss') return { ok: false, why: `log ${i + 1} holds a ${kind} line: ${line}` };
      if (kind === 'cell' && !line.startsWith('cell ')) return { ok: false, why: `log ${i + 1} holds a cell line that it does not read exactly: ${line}` };
    }
    const mine = ofTrial(lines, trial, 'cell');
    cells.push(...mine);
    if (trial !== 'window') continue;
    for (const l of ofTrial(lines, trial, 'figure')) given.add(l);
    const counts = new Set(lines.map((l) => /^trace trial=window .* ncpu=(\d+)$/.exec(l)).filter(Boolean).map((m) => Number(m[1])));
    const ncpu = counts.size === 1 ? [...counts][0] : null;
    for (const line of mine) {
      const cell = cellOf(parseLine(line).pairs);
      const wanted = countedFrom(cell, ncpu);
      if (wanted === null) return { ok: false, why: `log ${i + 1} gives no single processor count for a cell with a load: ${line}` };
      if (wanted !== cell.counted) return { ok: false, why: `log ${i + 1}: counted is not ${wanted} in ${line}` };
    }
  }
  const parsed = cells.map((l) => cellOf(parseLine(l).pairs));
  const again = twice(trial, parsed);
  if (again) return { ok: false, why: `a run appears twice: ${again}` };
  const figures = [];
  const ofRun43325502435 = createHash('sha256').update(cells.join('\n')).digest('hex') === CELLS_OF_RUN_43325502435;
  if (trial === 'window') {
    for (const figure of windowFigures(parsed)) {
      if (!given.has(figureLine(figure)) && !given.has(figureLine(figure, { old: true }))) {
        return { ok: false, why: `no log printed the figure ${figureLine(figure)}` };
      }
      const keep = ofRun43325502435 && !given.has(figureLine(figure));
      figures.push(keep ? figureLine(figure, { old: true }) : figureLine(figure));
    }
  }
  const verdict = trial === 'window' ? windowVerdict(parsed).line : syncVerdict(parsed).line;
  const text = `${[...reading, ...cells, ...figures, verdict].join('\n')}\n`;
  const checked = checkFile(trial, text, trial === 'window' ? logs : undefined);
  if (!checked.ok) return { ok: false, why: checked.why };
  return { ok: true, text };
}

/** Whether a result file holds its lines in the order of I11.1, each line, a carriage return at its
 * end taken off, of its form (`cellFormError`, `readingError`), one cell for each run (`countError`),
 * `counted` as the load gives it, its figures and its one verdict line as its cells give them, the
 * five-key figure form only for the cells of run 43325502435, and, for the sync trial, no figure line. A window file's macOS `counted` needs each machine's processor
 * count from the job `logs`; without logs `macCounted` is `unchecked`. */
export function checkFile(trial, fileText, logs = undefined) {
  const result = checkLines(trial, fileText, logs);
  return { ...result, macCounted: trial !== 'window' ? '-' : logs ? 'checked' : 'unchecked' };
}

function checkLines(trial, fileText, logs) {
  const lines = String(fileText).split('\n').map((l) => l.replace(/\r$/, '')).filter((l, i, all) => !(l === '' && i === all.length - 1));
  const kinds = trial === 'window' ? KINDS : KINDS.filter((k) => k !== 'figure');
  let last = 0;
  for (const line of lines) {
    const p = parseLine(line);
    const at = p ? kinds.indexOf(p.kind) : -1;
    if (at < 0 || (p.kind !== 'verdict' && p.pairs.trial !== trial)) return { ok: false, why: `a line of no kind of ${trial}: ${line}` };
    if (at < last) return { ok: false, why: `a ${p.kind} line after a ${kinds[last]} line` };
    last = at;
  }
  if (trial === 'sync' && !lines.some((l) => OBJECT_LINE.test(l))) return { ok: false, why: 'no reading line names the object of call.js' };
  const readingWhy = readingError(trial, ofTrial(lines, trial, 'reading'));
  if (readingWhy) return { ok: false, why: readingWhy };
  const cellLines = ofTrial(lines, trial, 'cell');
  if (cellLines.length === 0) return { ok: false, why: 'the file holds no cell line' };
  const countWhy = countError(trial, cellLines);
  if (countWhy) return { ok: false, why: countWhy };
  for (const line of cellLines) {
    const why = cellFormError(trial, line);
    if (why) return { ok: false, why: `a cell not of its form (${why}): ${line}` };
  }
  const cells = cellLines.map((l) => cellOf(parseLine(l).pairs));
  const again = twice(trial, cells);
  if (again) return { ok: false, why: `a run appears twice: ${again}` };
  if (trial === 'window') {
    const counts = logs ? logCounts(logs) : {};
    for (const c of cells) {
      const known = counts[c.machine];
      const ncpu = known && known.size === 1 ? [...known][0] : null;
      if (logs && c.machine === 'hosted-mac' && ncpu === null) return { ok: false, why: `the logs give no single processor count for ${c.machine}` };
      const wanted = countedFrom(c, ncpu);
      if (wanted !== null && wanted !== c.counted) return { ok: false, why: `counted is not ${wanted} in run ${c.run} of ${c.machine} arm ${c.arm}` };
      if (wanted === null && c.machine !== 'hosted-mac') return { ok: false, why: `counted has neither load nor use in run ${c.run} of ${c.machine} arm ${c.arm}` };
    }
    const figures = windowFigures(cells);
    const given = ofTrial(lines, trial, 'figure');
    const same = given.length === figures.length
      && figures.every((f, i) => given[i] === figureLine(f) || given[i] === figureLine(f, { old: true }));
    if (!same) return { ok: false, why: 'the figure lines are not the figures of the cells' };
    const old = given.some((l, i) => l !== figureLine(figures[i]));
    if (old && createHash('sha256').update(cellLines.join('\n')).digest('hex') !== CELLS_OF_RUN_43325502435) {
      return { ok: false, why: 'the five-key figure form is taken only for the cells of run 43325502435' };
    }
  }
  const verdict = trial === 'window' ? windowVerdict(cells).line : syncVerdict(cells).line;
  if (lines.filter((l) => parseLine(l)?.kind === 'verdict').length !== 1 || lines.at(-1) !== verdict) {
    return { ok: false, why: `the last line is not ${verdict}` };
  }
  return { ok: true, why: verdict };
}

function main(argv) {
  const [mode, trial, ...files] = argv;
  const good = ['window', 'sync'].includes(trial)
    && ((mode === 'make' && files.length >= 2) || (mode === 'check' && files.length >= 1 && (trial === 'window' || files.length === 1)));
  if (!good) {
    process.stderr.write('REFUSED usage: make window|sync <reading file> <log>... or check window <file> [<log>...] or check sync <file>\n');
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
  const logs = files.length > 1 ? files.slice(1).map(read) : undefined;
  const checked = checkFile(trial, read(files[0]), logs);
  const counted = trial === 'window' ? ` mac_counted=${checked.macCounted}` : '';
  console.log(`rule check trial=${trial} ok=${checked.ok ? 'yes' : 'no'}${counted} ${checked.why}`);
  process.exit(checked.ok ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
