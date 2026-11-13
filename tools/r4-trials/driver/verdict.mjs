// The rules of the driver and native dialog trial (contracts.md I11.5, ruling RD-42) over a result file.
// usage: node verdict.mjs driver|dialog FILE
// A file is: one paragraph of lines that start with a capital letter and one empty line, which may be left out;
// then its reading lines, its cell lines and, last, its one verdict line, each in the exact form of its kind.
// Exit 0: the file's verdict line is the one that its lines give; it prints that line and `verdict_matches=yes`.
// Exit 1: the file's verdict line differs; it prints the computed line and `verdict_matches=no`.
// Exit 2: a refusal, no verdict: one line `REFUSED <cause>: line <n>: <detail>` on standard error, nothing on
// standard output. The causes: file, carriage_return, line_end, paragraph, figure, line_form, order, duplicate,
// missing_cell, missing_reading, verdict_line.
// Exit 64: a wrong call; one usage line on standard error.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export const MACHINES = [['hosted-mac', 'mac', 'macos'], ['hosted-win', 'win', 'windows']];
export const CANDIDATES = ['webdriver', 'wdio'];
export const FORMS = ['plugin', 'system'];
export const OUTCOMES = ['failed_call', 'cancel_response', 'unknown_response', 'timeout'];
const SAFE = ['reject', 'none'];

export class Refusal extends Error {
  constructor(cause, line, detail) {
    super(`REFUSED ${cause}: line ${line}: ${detail}`);
    this.cause = cause;
    this.line = line;
  }
}

const one = (...words) => (value) => words.includes(value);
const like = (re) => (value) => re.test(value);
const COUNT = like(/^(0|[1-9][0-9]*)$/);
const LINE_NO = like(/^[1-9][0-9]*$/);
const VERSION = like(/^[0-9]+\.[0-9]+\.[0-9]+$/);
const PATH = like(/^[A-Za-z0-9._/-]+$/);
const NAME = like(/^[A-Za-z_][A-Za-z0-9_]*$/);
const TRIAL = (trial) => ['trial', one(trial)];
const MAC = ['machine', one('mac')];
const MACHINE = ['machine', one(...MACHINES.map((m) => m[0]))];
const CANDIDATE = ['candidate', one(...CANDIDATES)];
const FORM = ['form', one(...FORMS)];
const SYSTEM = ['system', one('macos', 'windows')];
const CONTROL = one('reject', 'allow', 'none');
const CELL_CONTROL = one('reject', 'allow', 'none', '-');

export const LINE_FORMS = {
  driver: {
    reading: {
      candidate: [TRIAL('driver'), MAC, CANDIDATE, ['version', VERSION], ['build', one('ok', 'fail')],
        ['licences', one('ok', 'miss')], ['input_calls', COUNT], ['banned', COUNT]],
      entry: [TRIAL('driver'), MAC, CANDIDATE, ['entry', like(/^[a-z0-9_-]+$/)], ['version', VERSION],
        ['licence', like(/^[A-Za-z0-9.()_+-]+$/)], ['built_targets', like(/^(-|[a-z0-9_-]+(,[a-z0-9_-]+)*)$/)],
        ['rulings', like(/^[A-Z0-9-]+(,[A-Z0-9-]+)*$/)]],
    },
    cell: {
      cell: [TRIAL('driver'), MACHINE, CANDIDATE, ['sessions', one('5')], ['ok', one('0', '1', '2', '3', '4', '5')]],
    },
    verdict: [['hosted_mac', one('none', ...CANDIDATES)], ['hosted_win', one('none', ...CANDIDATES)]],
  },
  dialog: {
    reading: {
      attrs: [TRIAL('dialog'), MAC, SYSTEM, ['default_attr', NAME], ['cancel_attr', NAME]],
      form: [TRIAL('dialog'), MAC, FORM, SYSTEM, ['default', CONTROL], ['cancel', CONTROL], ['close', CONTROL],
        ['default_file', PATH], ['default_line', LINE_NO], ['cancel_file', PATH], ['cancel_line', LINE_NO],
        ['close_file', PATH], ['close_line', LINE_NO]],
      outcome: [TRIAL('dialog'), MAC, FORM, SYSTEM, ['outcome', one(...OUTCOMES)],
        ['gives', one('reject', 'allow', 'none', 'unknown')], ['file', PATH], ['line', LINE_NO]],
    },
    cell: {
      cell: [TRIAL('dialog'), MACHINE, FORM, ['text', one('same', 'differ', '-')], ['default', CELL_CONTROL],
        ['cancel', CELL_CONTROL], ['close', CELL_CONTROL], ['allow_control', one('allow', 'miss', '-')],
        ['reject_control', one('reject', 'miss', '-')], ['driven', one('yes', 'no')]],
    },
    verdict: [['hosted_mac', one('none', ...FORMS)], ['hosted_win', one('none', ...FORMS)],
      ['driven_mac', one('yes', 'no')], ['driven_win', one('yes', 'no')]],
  },
};

function pairs(words, n, what) {
  const keys = [];
  const values = {};
  for (const word of words) {
    const found = /^([a-z_]+)=(\S+)$/.exec(word);
    if (!found) throw new Refusal('line_form', n, `${what}: ${JSON.stringify(word)} is not a key=value pair`);
    keys.push(found[1]);
    values[found[1]] = found[2];
  }
  return { keys, values };
}

function fits(form, keys, values) {
  return keys.length === form.length && form.every(([key, ok], i) => keys[i] === key && ok(values[key]));
}

export function parseLine(trial, line, n) {
  const words = line.split(' ');
  const kind = words.shift();
  if (kind === 'figure') throw new Refusal('figure', n, 'the driver and dialog trial has no figure line');
  if (kind === 'verdict') {
    if (words.shift() !== trial) throw new Refusal('verdict_line', n, `not the verdict line of the ${trial} trial`);
    const { keys, values } = pairs(words, n, 'verdict line');
    if (!fits(LINE_FORMS[trial].verdict, keys, values)) throw new Refusal('verdict_line', n, 'not the keys and words of the verdict line');
    return { kind, n, name: 'verdict', keys: values };
  }
  const forms = LINE_FORMS[trial][kind];
  if (!forms) throw new Refusal('line_form', n, `a line starts with ${JSON.stringify(kind)}`);
  const { keys, values } = pairs(words, n, `${kind} line`);
  const name = Object.keys(forms).find((formName) => fits(forms[formName], keys, values));
  if (!name) throw new Refusal('line_form', n, `a ${kind} line that is no ${kind} form of the ${trial} trial, in its keys, their order or their words`);
  if (kind === 'cell' && trial === 'dialog') {
    const others = ['text', 'default', 'cancel', 'close', 'allow_control', 'reject_control'];
    const dashes = others.filter((key) => values[key] === '-').length;
    if (values.driven === 'no' && dashes !== others.length) throw new Refusal('line_form', n, 'a cell with driven=no holds - in its other keys');
    if (values.driven === 'yes' && dashes !== 0) throw new Refusal('line_form', n, 'a cell with driven=yes holds no -');
  }
  return { kind, n, name, keys: values };
}

function body(text) {
  const cr = text.indexOf('\r');
  if (cr >= 0) throw new Refusal('carriage_return', text.slice(0, cr).split('\n').length, 'a result file holds no carriage return');
  if (!text.endsWith('\n') || text.endsWith('\n\n')) throw new Refusal('line_end', text.split('\n').length, 'a result file ends in one line feed');
  const lines = text.slice(0, -1).split('\n');
  const blank = lines.indexOf('');
  let first = 0;
  if (blank >= 0) {
    const bad = lines.slice(0, blank).findIndex((l) => !/^[A-Z]/.test(l));
    if (blank === 0 || bad >= 0) throw new Refusal('paragraph', bad >= 0 ? bad + 1 : 1, 'only one paragraph of lines that start with a capital letter stands before the one empty line');
    first = blank + 1;
  }
  return lines.slice(first).map((line, i) => ({ line, n: first + i + 1 }));
}

function single(lines, test, what, end) {
  const found = lines.filter(test);
  if (found.length > 1) throw new Refusal('duplicate', found[1].n, `two lines for ${what}`);
  if (found.length === 0 && end) throw new Refusal(end.cause, end.n, `no line for ${what}`);
  return found[0] ?? null;
}

function driverVerdict(readings, cells, end) {
  for (const r of readings.filter((l) => l.name === 'entry')) {
    single(readings, (l) => l.name === 'entry' && l.keys.candidate === r.keys.candidate && l.keys.entry === r.keys.entry,
      `the entry ${r.keys.entry} of ${r.keys.candidate}`);
  }
  const reading = Object.fromEntries(CANDIDATES.map((c) => [c, single(readings, (l) => l.name === 'candidate' && l.keys.candidate === c,
    `the reading of ${c}`, { cause: 'missing_reading', n: end })]));
  const pick = MACHINES.map(([machine, key]) => {
    const cell = Object.fromEntries(CANDIDATES.map((c) => [c, single(cells, (l) => l.keys.machine === machine && l.keys.candidate === c,
      `${c} on ${machine}`, { cause: 'missing_cell', n: end })]));
    const winner = CANDIDATES.find((c) => reading[c].keys.build === 'ok' && reading[c].keys.licences === 'ok'
      && reading[c].keys.input_calls === '0' && cell[c].keys.ok === '5');
    return `hosted_${key}=${winner ?? 'none'}`;
  });
  return `verdict driver ${pick.join(' ')}`;
}

function dialogPasses(cell, reading, outcomes) {
  const k = cell.keys;
  const controls = k.driven === 'yes'
    ? k.text === 'same' && SAFE.includes(k.default) && SAFE.includes(k.cancel) && SAFE.includes(k.close)
      && k.allow_control === 'allow' && k.reject_control === 'reject'
    : SAFE.includes(reading.keys.default) && SAFE.includes(reading.keys.cancel) && SAFE.includes(reading.keys.close);
  return controls && OUTCOMES.every((o) => outcomes[o] !== null && SAFE.includes(outcomes[o].keys.gives));
}

function dialogVerdict(readings, cells, end) {
  for (const system of ['macos', 'windows']) {
    single(readings, (l) => l.name === 'attrs' && l.keys.system === system, `the attributes on ${system}`, { cause: 'missing_reading', n: end });
  }
  const forms = [];
  const driven = [];
  for (const [machine, key, system] of MACHINES) {
    let winner = null;
    for (const form of FORMS) {
      const cell = single(cells, (l) => l.keys.machine === machine && l.keys.form === form, `${form} on ${machine}`, { cause: 'missing_cell', n: end });
      const reading = single(readings, (l) => l.name === 'form' && l.keys.form === form && l.keys.system === system,
        `the reading of ${form} on ${system}`, { cause: 'missing_reading', n: end });
      const outcomes = Object.fromEntries(OUTCOMES.map((o) => [o, single(readings,
        (l) => l.name === 'outcome' && l.keys.form === form && l.keys.system === system && l.keys.outcome === o, `${o} of ${form} on ${system}`)]));
      if (winner === null && dialogPasses(cell, reading, outcomes)) winner = { form, driven: cell.keys.driven };
    }
    forms.push(`hosted_${key}=${winner?.form ?? 'none'}`);
    driven.push(`driven_${key}=${winner?.driven === 'yes' ? 'yes' : 'no'}`);
  }
  return `verdict dialog ${forms.join(' ')} ${driven.join(' ')}`;
}

/** Reads the text of a result file of `trial`: returns the file's verdict line and the computed one, or throws a Refusal. */
export function checkFile(trial, text) {
  const lines = body(text).map(({ line, n }) => parseLine(trial, line, n));
  const rank = { reading: 0, cell: 1, verdict: 2 };
  let at = 0;
  for (const l of lines) {
    if (rank[l.kind] < at) throw new Refusal('order', l.n, `a ${l.kind} line after a later kind`);
    at = rank[l.kind];
  }
  const verdicts = lines.filter((l) => l.kind === 'verdict');
  const last = lines.at(-1);
  if (verdicts.length !== 1 || last?.kind !== 'verdict') {
    throw new Refusal('verdict_line', verdicts[1]?.n ?? last?.n ?? 1, 'a result file ends in its one verdict line');
  }
  const readings = lines.filter((l) => l.kind === 'reading');
  const cells = lines.filter((l) => l.kind === 'cell');
  const computed = trial === 'driver' ? driverVerdict(readings, cells, last.n) : dialogVerdict(readings, cells, last.n);
  return { verdict: text.slice(0, -1).split('\n').at(-1), computed };
}

export function main(argv, out = process.stdout, err = process.stderr) {
  if (argv.length !== 2 || !['driver', 'dialog'].includes(argv[0])) {
    err.write('usage: node verdict.mjs driver|dialog FILE\n');
    return 64;
  }
  let text;
  try {
    text = fs.readFileSync(argv[1], 'utf8');
  } catch (e) {
    err.write(`REFUSED file: line 0: ${e.code ?? 'unreadable'}\n`);
    return 2;
  }
  let result;
  try {
    result = checkFile(argv[0], text);
  } catch (e) {
    if (!(e instanceof Refusal)) throw e;
    err.write(`${e.message}\n`);
    return 2;
  }
  out.write(`${result.computed}\nverdict_matches=${result.verdict === result.computed ? 'yes' : 'no'}\n`);
  return result.verdict === result.computed ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
