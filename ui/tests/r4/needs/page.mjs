import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { HERE, ROOT, VIEW, check, fixtures, open, serve } from './lib.mjs';

const argv = process.argv.slice(2);
const write = argv.includes('--write');
const noStructure = argv.includes('--no-structure');
if (write) mkdirSync(join(HERE, 'structure'), { recursive: true });
const i = argv.indexOf('--states');
const only = i >= 0 && argv[i + 1] ? argv[i + 1].split(',') : null;

const { dir, states: tracked, tests } = fixtures();
const states = [...tracked, ...tests].filter((s) => !only || only.includes(s));
const origin = await serve({ ui: join(ROOT, 'ui'), fixtures: dir });

const total = { violations: 0, review: 0, keyboard: 'ok', structure: noStructure ? '-' : 'same', size: 'ok', modes: 'ok', markup: 'ok' };
const off = new Set();
let engine = 'offline';
let failed = 0;
let fixtureStates = 0;
let testItems = 0;
let testFailed = 0;

for (const state of states) {
  const snapshot = join(HERE, 'structure', state + '.txt');
  const h = await open({ origin, state, views: [VIEW] });
  const line = { violations: 0, review: 0, keyboard: 'ok', structure: noStructure ? '-' : 'same', size: 'ok', modes: 'ok', modes_off: '-', markup: 'ok' };
  const names = [];
  let ok = true;
  try {
    for (let n = 1; n <= 6; n++) {
      if (n === 1 && engine === 'none') continue;
      if (n === 3 && noStructure) continue;
      let r;
      try {
        r = await check(h, n, noStructure ? {} : { snapshot, write });
      } catch (err) {
        if (n === 1 && err && err.code === 'engine') {
          engine = 'none';
          console.error('engine none reason=' + err.reason);
          continue;
        }
        r = { ok: false, counts: n === 3 ? { structure: 'differ' } : {}, names: ['throws_' + ((err && err.code) || 'error')] };
      }
      Object.assign(line, r.counts);
      if (!r.ok) {
        ok = false;
        names.push(n + ':' + r.names.join(','));
      }
      if (n === 5 && r.counts.modes_off && r.counts.modes_off !== '-') for (const m of r.counts.modes_off.split(',')) off.add(m);
    }
  } finally {
    await h.close();
  }
  if (!ok) failed += 1;
  if (tests.includes(state)) {
    testItems += 1;
    if (!ok) testFailed += 1;
  } else {
    fixtureStates += 1;
    total.violations += line.violations;
    total.review += line.review;
    for (const k of ['keyboard', 'structure', 'size', 'modes', 'markup']) if (line[k] !== 'ok' && line[k] !== 'same') total[k] = line[k];
  }
  console.log(
    'page state=' + state + ' violations=' + line.violations + ' review=' + line.review + ' keyboard=' + line.keyboard +
      ' structure=' + line.structure + ' size=' + line.size + ' modes=' + line.modes + ' modes_off=' + line.modes_off +
      ' markup=' + line.markup + (names.length ? ' misses=' + names.join(';') : ''),
  );
}

console.log('page test_items=' + testItems + ' failed=' + testFailed);
console.log(
  'page states=' + fixtureStates + ' violations=' + total.violations + ' review=' + total.review + ' keyboard=' + total.keyboard +
    ' structure=' + total.structure + ' size=' + total.size + ' modes=' + total.modes + ' modes_off=' + (off.size ? [...off].sort().join(',') : '-') +
    ' markup=' + total.markup + ' engine=' + engine + (write ? ' written=yes' : ''),
);
process.exit(failed === 0 && states.length > 0 ? 0 : 1);
