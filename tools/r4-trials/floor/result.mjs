// usage: node tools/r4-trials/floor/result.mjs check [--file FILE] [--list FILE]
// Checks docs/r4/trials/floor.md (contracts.md I11.1, I11.4): its reading lines are the lines
// that read.sh prints, then its cell lines, its figure lines and last the verdict line, which is
// the verdict that the rule gives for its cells and the list, and no other line (I11.1).
// Exit: 0 when every check holds, 1 when one misses, 2 on a refusal.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { compareVersions, HERE, LIST, parseList, ROOT } from './list.mjs';

export const RESULT = path.join(ROOT, 'docs', 'r4', 'trials', 'floor.md');
const VERSION = '[0-9]+(?:\\.[0-9]+)*';
const CELL = new RegExp(`^cell trial=floor machine=(hosted-mac|hosted-win|guest-mac14) system=(${VERSION}) webkit=(${VERSION}) features=(\\d+) missing=(\\d+) missing_names=([a-z0-9_,]+|-) aria_notify=(function|undefined)$`);
const READING = /^reading trial=floor machine=mac fact=[a-z0-9_]{1,32} value=\S+ date=[0-9]{4}-[0-9]{2}-[0-9]{2}$/;

/** A cell line as `{ machine, system, webkit, features, missing, names, aria }`, or `null`,
 * also where `missing` is not the count of `missing_names` (0 with `-`). */
export function parseCell(line) {
  const m = CELL.exec(line);
  if (!m) return null;
  const missing = Number(m[5]);
  if (missing !== (m[6] === '-' ? 0 : m[6].split(',').length)) return null;
  return { machine: m[1], system: m[2], webkit: m[3], features: Number(m[4]), missing, names: m[6], aria: m[7] };
}

/** The verdict line by the rule of I11.4, or `null` where the rule does not hold: every allow
 * line at or under the floor, `missing` 0 in every cell, each cell counting every allow line,
 * and a cell of macOS to give `oldest_run`; `guest` only with a cell of `guest-mac14`. */
export function verdictOf(cells, features) {
  const allow = features.filter((f) => f.kind === 'allow');
  if (cells.length === 0 || allow.some((f) => compareVersions(f.version, '17.0') > 0)) return null;
  if (cells.some((c) => c.missing !== 0 || c.features !== allow.length)) return null;
  const mac = cells.filter((c) => c.machine !== 'hosted-win').map((c) => c.system);
  if (mac.length === 0) return null;
  const oldest = mac.reduce((a, b) => (compareVersions(b, a) < 0 ? b : a));
  const branch = cells.some((c) => c.machine === 'guest-mac14') ? 'guest' : 'rule-only';
  return `verdict floor=${branch} oldest_run=${oldest} missing=0`;
}

/** The problems of a result file's text against the reading lines and the list. */
export function checkResult(text, readings, features) {
  const problems = [];
  if (!text.endsWith('\n') || text.includes('\r')) problems.push('the file does not end in one line feed or holds a carriage return');
  const lines = text.split('\n').slice(0, -1);
  const order = { reading: 0, cell: 1, figure: 2, verdict: 3 };
  let at = 0;
  const cells = [];
  lines.forEach((line, i) => {
    const kind = line.split(' ')[0];
    if (!Object.hasOwn(order, kind)) {
      problems.push(`line ${i + 1} is no reading, cell, figure or verdict line`);
      return;
    }
    if (order[kind] < at) problems.push(`line ${i + 1}: a ${kind} line after a later kind`);
    at = Math.max(at, order[kind]);
    if (kind === 'reading' && !READING.test(line)) problems.push(`line ${i + 1}: a reading line out of form`);
    if (kind === 'cell') {
      const c = parseCell(line);
      if (c) cells.push(c);
      else problems.push(`line ${i + 1}: a cell line out of form`);
    }
    if (kind === 'figure') problems.push(`line ${i + 1}: the floor trial has no figure line`);
  });
  for (const machine of ['hosted-mac', 'hosted-win']) {
    if (!cells.some((c) => c.machine === machine)) problems.push(`the file holds no cell of ${machine}`);
  }
  const got = lines.filter((l) => l.startsWith('reading '));
  if (got.join('\n') !== readings.join('\n')) problems.push('the reading lines are not the lines that read.sh prints');
  const last = lines[lines.length - 1] ?? '';
  if (lines.filter((l) => l.startsWith('verdict ')).length !== 1 || !last.startsWith('verdict ')) problems.push('the file does not end in one verdict line of I11.4');
  const rule = verdictOf(cells, features);
  if (rule === null) problems.push('the rule gives no verdict for the cells');
  else if (last !== rule) problems.push(`the verdict line is not the rule's: ${rule}`);
  return problems;
}

function refuse(detail) {
  process.stderr.write(`REFUSED usage: ${detail}\n`);
  return 2;
}

function main(argv) {
  const usage = 'node tools/r4-trials/floor/result.mjs check [--file FILE] [--list FILE]';
  const options = { file: RESULT, list: LIST };
  if (argv[0] !== 'check') return refuse(usage);
  for (let i = 1; i < argv.length; i += 2) {
    if ((argv[i] === '--file' || argv[i] === '--list') && argv[i + 1]) options[argv[i].slice(2)] = argv[i + 1];
    else return refuse(usage);
  }
  const texts = {};
  for (const key of ['file', 'list']) {
    try {
      texts[key] = fs.readFileSync(options[key], 'utf8');
    } catch {
      return refuse(`the ${key} ${path.basename(options[key])} cannot be read`);
    }
  }
  const readings = execFileSync('sh', [path.join(HERE, 'read.sh')], { encoding: 'utf8' }).split('\n').filter((l) => l !== '');
  const problems = checkResult(texts.file, readings, parseList(texts.list).features);
  for (const p of problems) process.stdout.write(`miss ${p}\n`);
  process.stdout.write(`result problems=${problems.length}\n`);
  return problems.length === 0 ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
