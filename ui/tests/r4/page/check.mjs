import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileRules } from './files.mjs';
import { RECORD_FILE, committedRecord, factsOf, firstDiff, packagesCheck, readRecord, recordForm, treeBeside } from './record.mjs';
import { joined, pageCheck, stage } from './run.mjs';

const USAGE =
  'usage: node ui/tests/r4/page/check.mjs files --repo DIR --acorn DIR | page --repo DIR [--write] | whole --repo DIR --out FILE | record --repo DIR | selftest --repo DIR --acorn DIR --work DIR';
const FIX = 'fix: sh scripts/r4/ci.d/40-r4-page.sh --record, run through the helper for timed runs, and commit ui/tests/r4/page/record.txt';

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

function end(cause) {
  console.log('check result=' + (cause || 'ok'));
  process.exit(cause ? 1 : 0);
}

function stopped(message) {
  const first = String(message || 'unknown').split('\n')[0].replace(/\s+/g, ' ').slice(0, 200);
  console.log('page stopped state=' + stage.state + ' step=' + stage.step + ' message=' + first);
}

process.on('exit', (code) => {
  if (code === 13) stopped('a top-level await never settled');
});
for (const event of ['uncaughtException', 'unhandledRejection']) {
  process.on(event, (err) => {
    stopped(err && err.message ? err.message : err);
    console.log('check result=stopped');
    process.exit(1);
  });
}

let browserLine = null;
const write = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, ...more) => {
  const m = /^browser name=\S+ version=\S+/m.exec(String(chunk));
  if (m && !browserLine) browserLine = m[0];
  return write(chunk, ...more);
};

function browsersDir(repo) {
  return process.env.R4_PAGE_BROWSERS || join(repo, 'tools', 'r4-page', '.browsers');
}

/** The whole run: every state with its views, the regions, page.mjs on every state and every test item; the record's text when it passed. */
async function whole(repo, log) {
  const f = factsOf(repo);
  const r = await pageCheck({ repo, needs: true, log });
  log(joined(f.all.length, r.results, r.needs));
  if (r.cause) return { cause: r.cause, facts: f };
  const m = /^browser name=(\S+) version=(\S+)$/.exec(browserLine || '');
  const own = new Map(r.results.map((x) => [x.state, x.line.review]));
  const text = recordForm({ ...f, engine: m ? m[1] : '-', version: m ? m[2] : '-', own, needs: r.needs.reviews });
  return { cause: null, text, facts: f };
}

function packages(repo) {
  const p = packagesCheck(repo, browsersDir(repo));
  for (const l of p.lines) console.log(l);
  if (p.cause) end(p.cause);
}

const log = (l) => console.log(l);
const [command, ...rest] = process.argv.slice(2);
const repo = arg(rest, '--repo');
if (!repo) refuse();

if (command === 'files') {
  const acorn = arg(rest, '--acorn');
  if (!acorn || rest.length !== 4) refuse();
  const { lines, cause } = fileRules(repo, acorn);
  for (const l of lines) console.log(l);
  end(cause);
} else if (command === 'page') {
  const write = rest.includes('--write');
  if (rest.length !== (write ? 3 : 2)) refuse();
  if (!write) packages(repo);
  const r = await pageCheck({ repo, write, needs: !write, log });
  log('page ran states=' + r.results.length + ' of ' + r.all.length + ' needs_states=' + (r.needs && r.needs.parsed ? r.needs.states : '-') + ' test_items=' + (r.needs && r.needs.parsed ? r.needs.items : '-'));
  log(joined(r.all.length, r.results, r.needs));
  end(r.cause);
} else if (command === 'whole') {
  const out = arg(rest, '--out');
  if (!out || rest.length !== 4) refuse();
  packages(repo);
  const f = factsOf(repo);
  const text = committedRecord(repo);
  const rec = readRecord(text, f);
  console.log('page record fault=' + (rec.fault || 'none') + ' stamp=' + f.stamp + ' files=' + f.files + (rec.diff ? ' line=' + rec.diff.line : ''));
  if (rec.fault) {
    if (rec.fault !== 'form') console.log(FIX);
    end('record');
  }
  const w = await whole(repo, log);
  if (w.cause) end(w.cause);
  writeFileSync(out, w.text);
  const diff = firstDiff(text, w.text);
  if (diff) {
    console.log('page record differs line=' + diff.line + ' committed: ' + diff.committed);
    console.log('page record differs line=' + diff.line + ' fresh: ' + diff.fresh);
    end('record');
  }
  console.log('page record same=yes engine=' + w.facts.engine + ' states=' + w.facts.all.length);
  end(null);
} else if (command === 'record') {
  if (rest.length !== 2) refuse();
  packages(repo);
  const w = await whole(repo, log);
  if (w.cause) end(w.cause);
  writeFileSync(join(repo, RECORD_FILE), w.text);
  const others = treeBeside(repo);
  console.log('page record written stamp=' + w.facts.stamp + ' files=' + w.facts.files + ' other_changes=' + others.length);
  end(others.length ? 'dirty' : null);
} else if (command === 'selftest') {
  const acorn = arg(rest, '--acorn');
  const work = arg(rest, '--work');
  if (!acorn || !work || rest.length !== 6) refuse();
  const { selfTest } = await import('./selftest.mjs');
  const ok = await selfTest({ repo, acorn, work, log });
  process.exit(ok ? 0 : 1);
} else {
  refuse();
}
