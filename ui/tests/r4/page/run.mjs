import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { ACTS_FILE, FIXTURES_DIR, RUN_VIEWS, SNAPSHOTS_DIR, statesOf } from './files.mjs';

const NEEDS = RUN_VIEWS['needs.js'];
const AGENTS = RUN_VIEWS['agents.js'];
export const KEYS = ['keyboard', 'structure', 'size', 'modes', 'markup'];

/** The views whose status region the rule reads: the agents view's, and the needs view's once U3.6 has landed it. */
export const REGIONS = ['agents'];

function json(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

/** Whether a state's first item, in the reply's order, is a confirmation of the presenter page (contracts.md I9.3, ruling RD-22 (f)). */
export function pageConfirmFirst(snapshot) {
  const first = snapshot && Array.isArray(snapshot.items) ? snapshot.items[0] : null;
  return Boolean(first && first.kind === 'confirm' && first.presenter === 'page');
}

/** The runs of the page check: one per state, with both views where the first item is a page confirmation and the agents view alone elsewhere. */
export function plan(fixtures, states) {
  return states.map((state) => {
    const both = pageConfirmFirst(json(join(fixtures, 'state-' + state + '.json')).snapshot);
    return { state, label: both ? 'both' : 'agents', views: both ? [NEEDS, AGENTS] : [AGENTS] };
  });
}

async function regionsOf(page, ids) {
  return page.evaluate((list) => {
    const out = {};
    for (const id of list) {
      const heading = document.getElementById('view-' + id);
      const section = heading ? heading.closest('section') : null;
      if (!section) continue;
      const found = [...section.querySelectorAll('[role="status"]')];
      out[id] = {
        count: found.length,
        polite: found.every((e) => e.getAttribute('aria-live') === null || e.getAttribute('aria-live') === 'polite'),
        marked: found.filter((e) => e.r4pageMark === 'kept').length,
        text: found.length === 1 ? found[0].textContent : null,
      };
      for (const e of found) e.r4pageMark = 'kept';
    }
    return out;
  }, ids);
}

/** The status-region rule of ruling RD-29 (c) on one open page: one polite region per reading view, the same element after the next reply that the view applies. */
export async function regionRule(harness, h, views) {
  const ids = REGIONS.filter((id) => views.some((v) => v.endsWith('/' + id + '.js')));
  if (ids.length === 0) return { read: 0, misses: [] };
  const misses = [];
  const before = await regionsOf(h.page, ids);
  await h.page.evaluate(() => {
    window.Halvo.views.window('hidden');
    window.Halvo.views.window('shown');
  });
  await harness.settled(h);
  const after = await regionsOf(h.page, ids);
  for (const id of ids) {
    const b = before[id];
    const a = after[id];
    if (!b || b.count !== 1) misses.push(id + ':count');
    else if (!b.polite) misses.push(id + ':not_polite');
    else if (!a || a.count !== 1 || a.marked !== 1) misses.push(id + ':rebuilt');
  }
  return { read: ids.length, misses };
}

function flip(snapshot) {
  const row = snapshot.rows[0];
  const status = row.status === 'off' ? 'all_set' : 'off';
  return { ...snapshot, seq: snapshot.seq + 1, rows: snapshot.rows.map((r, i) => (i === 0 ? { ...r, status, say: 'The region holds this.' } : r)) };
}

/** A reply that changes the first row's status adds that row's say to the agents view's one region (contracts.md I9.2). */
export async function sayRule(harness, origin, fixtures, states, ui) {
  const state = states.find((s) => (json(join(fixtures, 'state-' + s + '.json')).snapshot.rows || []).length > 0);
  if (!state || !REGIONS.includes('agents')) return { read: 0, misses: [] };
  const first = json(join(fixtures, 'state-' + state + '.json')).snapshot;
  const h = await harness.open({ origin, replies: [{ state: first }, { state: flip(first) }], views: [AGENTS] });
  try {
    const r = await h.page.evaluate(() => {
      const heading = document.getElementById('view-agents');
      const found = heading ? [...heading.closest('section').querySelectorAll('[role="status"]')] : [];
      return { count: found.length, text: found.length === 1 ? found[0].textContent : null };
    });
    return { read: 1, misses: r.count === 1 && r.text === 'The region holds this.' ? [] : ['agents:say'] };
  } finally {
    await h.close();
  }
}

function blank() {
  return { violations: 0, review: 0, keyboard: 'ok', structure: 'same', size: 'ok', modes: 'ok', modes_off: '-', markup: 'ok' };
}

/** Runs the six checks of contracts.md I9.3 and the region rule on each run of the plan; returns one result per run. */
export async function runStates(harness, { origin, runs, snapshots, acts, write, log, checks = [1, 2, 3, 4, 5, 6], region: readRegion = true }) {
  const results = [];
  for (const run of runs) {
    const line = blank();
    const names = [];
    const h = await harness.open({ origin, state: run.state, views: run.views, acts: acts[run.state] || [] });
    let region = { read: 0, misses: [] };
    try {
      for (const n of checks) {
        let r;
        try {
          r = await harness.check(h, n, { snapshot: join(snapshots, run.state + '.txt'), write: write && n === 3 });
        } catch (err) {
          r = { ok: false, counts: n === 3 ? { structure: 'differ' } : {}, names: ['throws_' + ((err && err.code) || 'error')] };
        }
        Object.assign(line, r.counts);
        if (!r.ok) names.push(n + ':' + r.names.join(','));
      }
      if (readRegion) region = await regionRule(harness, h, run.views);
    } finally {
      await h.close();
    }
    const ok = names.length === 0 && region.misses.length === 0;
    log(
      'page state=' + run.state + ' views=' + run.label + ' violations=' + line.violations + ' review=' + line.review + ' keyboard=' + line.keyboard +
        ' structure=' + line.structure + ' size=' + line.size + ' modes=' + line.modes + ' modes_off=' + line.modes_off + ' markup=' + line.markup +
        ' region=' + (region.read === 0 ? '-' : region.misses.length ? 'miss' : 'ok') + (names.length ? ' misses=' + names.join(';') : '') +
        (region.misses.length ? ' region_misses=' + region.misses.join(',') : ''),
    );
    results.push({ ...run, line, names, region, ok });
  }
  return results;
}

/** Runs ui/tests/r4/needs/page.mjs, the needs view alone on every state and on its test items (ruling RD-25 (e)), and reads its last two lines. */
export function needsPage(repo, log) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [join(repo, 'ui', 'tests', 'r4', 'needs', 'page.mjs')], { cwd: repo, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    child.stdout.on('data', (d) => {
      out += d;
    });
    child.stderr.on('data', (d) => process.stderr.write(d));
    child.on('close', (code) => {
      const lines = out.split('\n').filter(Boolean);
      for (const l of lines) log('needs ' + l);
      const last = lines.length ? lines[lines.length - 1] : '';
      const items = lines.find((l) => l.startsWith('page test_items=')) || '';
      const m = /^page states=(\d+) violations=(\d+) review=(\d+) keyboard=(\S+) structure=(\S+) size=(\S+) modes=(\S+) modes_off=(\S+) markup=(\S+) engine=(\S+)$/.exec(last);
      const t = /^page test_items=(\d+) failed=(\d+)$/.exec(items);
      if (!m || !t) {
        done({ code, parsed: false });
        return;
      }
      done({
        code,
        parsed: true,
        states: Number(m[1]),
        line: { violations: Number(m[2]), review: Number(m[3]), keyboard: m[4], structure: m[5], size: m[6], modes: m[7], modes_off: m[8], markup: m[9] },
        engine: m[10],
        items: Number(t[1]),
        itemsFailed: Number(t[2]),
      });
    });
  });
}

/** Joins the results of the own runs and of page.mjs into the one line of contracts.md I9.3. */
export function joined(count, results, needs) {
  const total = blank();
  const off = new Set();
  const all = [...results.map((r) => r.line), ...(needs && needs.parsed ? [needs.line] : [])];
  for (const l of all) {
    total.violations += l.violations;
    total.review += l.review;
    for (const k of KEYS) if (l[k] !== 'ok' && l[k] !== 'same') total[k] = l[k];
    if (l.modes_off !== '-') for (const m of l.modes_off.split(',')) off.add(m);
  }
  total.modes_off = off.size ? [...off].sort().join(',') : '-';
  return (
    'page states=' + count + ' violations=' + total.violations + ' review=' + total.review + ' keyboard=' + total.keyboard + ' structure=' + total.structure +
    ' size=' + total.size + ' modes=' + total.modes + ' modes_off=' + total.modes_off + ' markup=' + total.markup
  );
}

export async function loadHarness(repo) {
  return import(pathToFileURL(join(repo, 'tools', 'r4-page', 'index.mjs')).href);
}

/** The whole page check on a repository: returns the first cause that holds, or null. */
export async function pageCheck({ repo, ui, fixtures, snapshots, write, needs: runNeeds, log }) {
  const harness = await loadHarness(repo);
  const fx = fixtures || join(repo, FIXTURES_DIR);
  const states = statesOf(fx);
  const acts = json(join(repo, ACTS_FILE));
  const origin = await harness.serve({ ui: ui || join(repo, 'ui'), fixtures: fx });
  const results = await runStates(harness, { origin, runs: plan(fx, states), snapshots: snapshots || join(repo, SNAPSHOTS_DIR), acts, write, log });
  const say = await sayRule(harness, origin, fx, states, ui);
  log('page regions read=' + results.reduce((a, r) => a + r.region.read, 0) + ' rebuilt=' + results.filter((r) => r.region.misses.length).length + ' say=' + (say.read === 0 ? '-' : say.misses.length ? 'miss' : 'ok'));
  const needs = runNeeds ? await needsPage(repo, log) : null;
  if (needs) log('page needs_alone states=' + (needs.parsed ? needs.states : '-') + ' test_items=' + (needs.parsed ? needs.items : '-') + ' failed=' + (needs.parsed ? needs.itemsFailed : '-') + ' exit=' + needs.code);
  log(joined(states.length, results, needs));
  return verdict(states, results, needs, say);
}

/** The first cause that holds for a whole run: a state that no run or page.mjs did not read, a check that missed, then a region that missed. */
export function verdict(states, results, needs, say) {
  if (states.length === 0 || results.length !== states.length) return 'states';
  const ran = results.map((r) => r.state).sort();
  if (ran.some((s, i) => s !== states[i])) return 'states';
  if (needs && (!needs.parsed || needs.states !== states.length)) return 'states';
  if (results.some((r) => r.names.length) || (needs && (needs.code !== 0 || needs.itemsFailed !== 0))) return 'page';
  if (results.some((r) => r.region.misses.length) || say.misses.length) return 'status';
  return null;
}
