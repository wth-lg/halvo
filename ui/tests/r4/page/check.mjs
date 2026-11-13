import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { FIXTURES_DIR, fileRules, statesOf } from './files.mjs';
import { RECORD_FILE, pushLine, readRecord, recordText, rotation, stampOf } from './record.mjs';
import { joined, pageCheck, plan } from './run.mjs';

const USAGE =
  'usage: node ui/tests/r4/page/check.mjs files --repo DIR --acorn DIR | page --repo DIR [--write] | record --repo DIR | push --repo DIR --commit SHA --own K --needs M | selftest --repo DIR --acorn DIR';

function arg(list, name) {
  const i = list.indexOf(name);
  return i >= 0 && i + 1 < list.length ? list[i + 1] : null;
}

function refuse() {
  console.log(USAGE);
  console.log('check result=usage');
  process.exit(2);
}

function count(value) {
  return /^[1-9][0-9]*$/.test(value || '') ? Number(value) : null;
}

let browserLine = null;
const write = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, ...more) => {
  const m = /^browser name=\S+ version=\S+/m.exec(String(chunk));
  if (m && !browserLine) browserLine = m[0];
  return write(chunk, ...more);
};

const lines = [];
const log = (l) => {
  lines.push(l);
  console.log(l);
};

const [command, ...rest] = process.argv.slice(2);
const repo = arg(rest, '--repo');
if (!repo) refuse();

if (command === 'files') {
  const acorn = arg(rest, '--acorn');
  if (!acorn || rest.length !== 4) refuse();
  const { lines: out, cause } = fileRules(repo, acorn);
  for (const l of out) console.log(l);
  console.log('check result=' + (cause || 'ok'));
  process.exit(cause ? 1 : 0);
} else if (command === 'page' || command === 'record') {
  const write = rest.includes('--write');
  if (rest.length !== (write ? 3 : 2) || (write && command === 'record')) refuse();
  const before = command === 'record' ? stampOf(repo) : null;
  const r = await pageCheck({ repo, write, needs: !write, log });
  log(joined(r.all.length, r.results, r.needs));
  if (command === 'record') {
    const after = stampOf(repo);
    const cause = r.cause || (after.stamp !== before.stamp ? 'dirty' : null);
    const browser = browserLine;
    writeFileSync(join(repo, RECORD_FILE), recordText({ ...before, browser, lines: lines.filter((l) => !l.startsWith('browser name=')), cause }));
    console.log('page record written stamp=' + before.stamp + ' files=' + before.files + ' result=' + (cause || 'ok'));
    console.log('check result=' + (cause || 'ok'));
    process.exit(cause ? 1 : 0);
  }
  console.log('check result=' + (r.cause || 'ok'));
  process.exit(r.cause ? 1 : 0);
} else if (command === 'push') {
  const commit = arg(rest, '--commit');
  const own = count(arg(rest, '--own'));
  const needsCount = arg(rest, '--needs') === '0' ? 0 : count(arg(rest, '--needs'));
  if (rest.length !== 8 || !/^[0-9a-f]{40}$/.test(commit || '') || own === null || needsCount === null) refuse();
  const all = statesOf(join(repo, FIXTURES_DIR));
  const { stamp, files } = stampOf(repo);
  const labels = Object.fromEntries(plan(join(repo, FIXTURES_DIR), all).map((r) => [r.state, r.label]));
  const rec = readRecord(repo, all, stamp, labels);
  console.log('page record fault=' + (rec.fault || 'none') + ' stamp=' + stamp + ' files=' + files + (rec.browser ? ' recorded_' + rec.browser.replace(/^browser /, '').replace(' ', ' recorded_') : ''));
  if (rec.fault) {
    console.log('check result=record');
    process.exit(1);
  }
  const items = Object.keys(JSON.parse(readFileSync(join(repo, 'ui', 'tests', 'r4', 'needs', 'items.json'), 'utf8')));
  const only = rotation(commit, all).slice(0, own);
  const needsOnly = rotation(commit, [...all, ...items]).slice(0, needsCount);
  const r = await pageCheck({ repo, needs: true, only, needsOnly, log });
  console.log('page live own=' + only.length + ' of ' + all.length + ' needs=' + needsOnly.length + ' of ' + (all.length + items.length) + ' recorded=' + rec.own);
  console.log(pushLine(all.length, rec.line, r.results, r.needs));
  console.log('check result=' + (r.cause || 'ok'));
  process.exit(r.cause ? 1 : 0);
} else if (command === 'selftest') {
  const acorn = arg(rest, '--acorn');
  if (!acorn || rest.length !== 4) refuse();
  const { selfTest } = await import('./selftest.mjs');
  const ok = await selfTest({ repo, acorn, log: (l) => console.log(l) });
  process.exit(ok ? 0 : 1);
} else {
  refuse();
}
