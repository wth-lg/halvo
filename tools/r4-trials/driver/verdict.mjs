// The rules of the driver and native dialog trial (contracts.md I11.5) over a result file.
// usage: node verdict.mjs driver|dialog FILE
// Prints the verdict line that the file's reading and cell lines give and `verdict_matches=<yes|no>`;
// exits 0 when the file's last line is that verdict, 1 when it is not, 2 on a refusal.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const ORDER = ['reading', 'cell', 'figure', 'verdict'];
const MACHINES = [['hosted-mac', 'mac', 'macos'], ['hosted-win', 'win', 'windows']];
const CANDIDATES = ['webdriver', 'wdio'];
const FORMS = ['plugin', 'system'];
const SAFE = ['reject', 'none'];

export function splitLines(text) {
  if (!text.endsWith('\n') || text.includes('\r')) throw new Error('a result file ends in one LF and holds no CR');
  return text.slice(0, -1).split('\n');
}

export function parseLine(line) {
  const words = line.split(' ');
  const head = [];
  while (words.length > 0 && /^[a-z]+$/.test(words[0])) head.push(words.shift());
  const [kind, ...rest] = head;
  if (!kind || words.length === 0) throw new Error(`not a line of a result file: ${JSON.stringify(line)}`);
  const keys = {};
  for (const word of words) {
    const found = /^([a-z_]+)=(\S+)$/.exec(word);
    if (!found || Object.hasOwn(keys, found[1])) throw new Error(`not a key=value pair, or a key twice: ${JSON.stringify(word)}`);
    keys[found[1]] = found[2];
  }
  return rest.length > 0 ? { kind, words: rest, keys } : { kind, keys };
}

function only(list, test, what) {
  const found = list.filter(test);
  if (found.length > 1) throw new Error(`two lines for ${what}`);
  return found[0] ?? null;
}

export function driverVerdict(readings, cells) {
  const pick = MACHINES.map(([machine, key]) => {
    const winner = CANDIDATES.find((name) => {
      const r = only(readings, (l) => l.keys.candidate === name, `the reading of ${name}`);
      const c = only(cells, (l) => l.keys.machine === machine && l.keys.candidate === name, `${name} on ${machine}`);
      return r !== null && c !== null && r.keys.build === 'ok' && r.keys.licences === 'ok' && r.keys.input_calls === '0'
        && c.keys.ok === '5';
    });
    return `hosted_${key}=${winner ?? 'none'}`;
  });
  return `verdict driver ${pick.join(' ')}`;
}

function dialogPasses(cell, reading) {
  if (cell === null) return false;
  const k = cell.keys;
  if (k.driven === 'yes') {
    return k.text === 'same' && SAFE.includes(k.default) && SAFE.includes(k.cancel) && SAFE.includes(k.close)
      && k.allow_control === 'allow' && k.reject_control === 'reject';
  }
  return k.driven === 'no' && reading !== null
    && SAFE.includes(reading.keys.default) && SAFE.includes(reading.keys.cancel) && SAFE.includes(reading.keys.close);
}

export function dialogVerdict(readings, cells) {
  const forms = [];
  const driven = [];
  for (const [machine, key, system] of MACHINES) {
    let winner = null;
    for (const form of FORMS) {
      const c = only(cells, (l) => l.keys.machine === machine && l.keys.form === form, `${form} on ${machine}`);
      const r = only(readings, (l) => l.keys.form === form && l.keys.system === system, `the reading of ${form} on ${system}`);
      if (dialogPasses(c, r)) {
        winner = { form, driven: c.keys.driven };
        break;
      }
    }
    forms.push(`hosted_${key}=${winner?.form ?? 'none'}`);
    driven.push(`driven_${key}=${winner?.driven === 'yes' ? 'yes' : 'no'}`);
  }
  return `verdict dialog ${forms.join(' ')} ${driven.join(' ')}`;
}

export function checkFile(trial, text) {
  const lines = splitLines(text);
  const parsed = lines.map(parseLine);
  let at = 0;
  parsed.forEach((line, i) => {
    const rank = ORDER.indexOf(line.kind);
    if (rank < at) throw new Error(`line ${i + 1}: a ${line.kind} line after a ${ORDER[at]} line`);
    at = rank;
    const of = line.kind === 'verdict' ? line.words?.join(' ') : line.keys.trial;
    if (of !== trial || (line.kind !== 'verdict' && line.words)) throw new Error(`line ${i + 1}: not a line of the ${trial} trial`);
  });
  const verdicts = parsed.filter((l) => l.kind === 'verdict');
  if (verdicts.length !== 1 || parsed.at(-1).kind !== 'verdict') throw new Error('a result file ends in its one verdict line');
  const readings = parsed.filter((l) => l.kind === 'reading');
  const cells = parsed.filter((l) => l.kind === 'cell');
  const computed = trial === 'driver' ? driverVerdict(readings, cells) : dialogVerdict(readings, cells);
  return { verdict: lines.at(-1), computed };
}

function main(argv) {
  if (argv.length !== 2 || !['driver', 'dialog'].includes(argv[0])) {
    process.stderr.write('REFUSED usage: give driver|dialog and the result file\n');
    return 2;
  }
  const { verdict, computed } = checkFile(argv[0], fs.readFileSync(argv[1], 'utf8'));
  console.log(computed);
  console.log(`verdict_matches=${verdict === computed ? 'yes' : 'no'}`);
  return verdict === computed ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
