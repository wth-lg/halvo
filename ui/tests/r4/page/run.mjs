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

/** Where a run stands: the state and the step, which a run that stops names. */
export const stage = { state: '-', step: '-' };

/** The child processes that a run started and that have not ended: page.mjs. */
export const children = new Set();

/** Hands TERM, INT and HUP on to every child that a run started, waits for them for at most 5 s, and ends this process with 128 and the signal's number. */
export function forwardSignals() {
  for (const [signal, code] of [['SIGTERM', 143], ['SIGINT', 130], ['SIGHUP', 129]]) {
    process.on(signal, () => {
      const left = [...children];
      for (const c of left) c.kill(signal);
      const done = () => process.exit(code);
      if (left.length === 0) done();
      let open = left.length;
      for (const c of left) c.once('close', () => --open === 0 && done());
      setTimeout(done, 5000);
    });
  }
}

/** The milliseconds that the say rule's press waits for its control before the driver gives up. */
export const PRESS_MS = 10000;

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
        shown: found.every((e) => e.checkVisibility({ visibilityProperty: true }) && !e.closest('[aria-hidden="true"]')),
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
    else if (!b.shown || !a || !a.shown) misses.push(id + ':hidden');
    else if (!a || a.count !== 1 || a.marked !== 1) misses.push(id + ':rebuilt');
  }
  return { read: ids.length, misses };
}

export const MARKUP = '<b data-x="1">x</b>&amp;';
const ACTING = ['run_now', 'stop', 'switch_off', 'switch_on'];

function flip(snapshot, agent) {
  return {
    ...snapshot,
    seq: snapshot.seq + 1,
    rows: snapshot.rows.map((r) => (r.agent === agent ? { ...r, status: r.status === 'off' ? 'all_set' : 'off', say: MARKUP } : r)),
  };
}

function regionNow(id) {
  const heading = document.getElementById('view-' + id);
  const found = heading ? [...heading.closest('section').querySelectorAll('[role="status"]')] : [];
  const e = found.length === 1 ? found[0] : null;
  return {
    count: found.length,
    marked: found.filter((x) => x.r4pageMark === 'kept').length,
    text: e ? e.textContent : null,
    shown: Boolean(e && e.checkVisibility({ visibilityProperty: true }) && !e.closest('[aria-hidden="true"]')),
    markup: document.querySelectorAll('[data-x]').length,
  };
}

/** A reply that changes the first row's status adds that row's say to the agents view's one region (contracts.md I9.2). */
export async function sayRule(harness, origin, fixtures, states) {
  if (!REGIONS.includes('agents')) return { read: 0, misses: [] };
  let first = null;
  let row = null;
  for (const s of states) {
    const snap = json(join(fixtures, 'state-' + s + '.json')).snapshot;
    row = (snap.rows || []).find((r) => (r.controls || []).some((c) => ACTING.includes(c.control)));
    if (row) {
      first = snap;
      break;
    }
  }
  if (!first) return { read: 0, misses: ['agents:no_state'] };
  const control = row.controls.find((c) => ACTING.includes(c.control)).control;
  const replies = [{ state: first }, { call: 'agents_control', args: { agent: row.agent, action: control }, reply: { ok: 'yes' } }, { state: flip(first, row.agent) }];
  stage.state = 'say';
  stage.step = 'open';
  const h = await harness.open({ origin, replies, views: [AGENTS] });
  try {
    await h.page.evaluate(() => {
      const heading = document.getElementById('view-agents');
      for (const e of heading ? heading.closest('section').querySelectorAll('[role="status"]') : []) e.r4pageMark = 'kept';
    });
    try {
      stage.step = 'press';
      h.page.setDefaultTimeout(PRESS_MS);
      await harness.act(h, { act: control, param: row.agent });
      await harness.settled(h);
    } catch {
      return { read: 1, misses: ['agents:press'] };
    }
    const r = await h.page.evaluate(regionNow, 'agents');
    const misses = [];
    if (r.count !== 1) misses.push('agents:count');
    else {
      if (r.marked !== 1) misses.push('agents:rebuilt');
      if (!r.shown) misses.push('agents:hidden');
      if (r.text !== MARKUP) misses.push('agents:say');
      if (r.markup !== 0) misses.push('agents:markup');
    }
    return { read: 1, misses };
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
    stage.state = run.state;
    stage.step = 'open';
    const h = await harness.open({ origin, state: run.state, views: run.views, acts: acts[run.state] || [] });
    let region = { read: 0, misses: [] };
    try {
      for (const n of checks) {
        stage.step = 'check ' + n;
        let r;
        try {
          r = await harness.check(h, n, { snapshot: join(snapshots, run.state + '.txt'), write: write && n === 3 });
        } catch (err) {
          r = { ok: false, counts: n === 3 ? { structure: 'differ' } : {}, names: ['throws_' + ((err && err.code) || 'error')] };
        }
        Object.assign(line, r.counts);
        if (!r.ok) names.push(n + ':' + r.names.join(','));
      }
      stage.step = 'region';
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

/** Runs ui/tests/r4/needs/page.mjs, the needs view alone on every state and on its test items (ruling RD-25 (e)), and reads its lines. */
export function needsPage(repo, log, only) {
  stage.state = 'needs';
  stage.step = 'page.mjs';
  return new Promise((done) => {
    const args = [join(repo, 'ui', 'tests', 'r4', 'needs', 'page.mjs'), ...(only ? ['--states', only.join(',')] : [])];
    const child = spawn(process.execPath, args, { cwd: repo, stdio: ['ignore', 'pipe', 'pipe'] });
    children.add(child);
    let out = '';
    child.stdout.on('data', (d) => {
      out += d;
    });
    child.stderr.on('data', (d) => process.stderr.write(d));
    child.on('close', (code) => {
      children.delete(child);
      const lines = out.split('\n').filter(Boolean);
      for (const l of lines) log('needs ' + l);
      const last = lines.length ? lines[lines.length - 1] : '';
      const items = lines.find((l) => l.startsWith('page test_items=')) || '';
      const m = /^page states=(\d+) violations=(\d+) review=(\d+) keyboard=(\S+) structure=(\S+) size=(\S+) modes=(\S+) modes_off=(\S+) markup=(\S+) engine=(\S+)$/.exec(last);
      const t = /^page test_items=(\d+) failed=(\d+)$/.exec(items);
      const reviews = new Map();
      for (const l of lines) {
        const e = /^page state=(\S+) violations=\d+ review=(\d+) /.exec(l);
        if (e && !reviews.has(e[1])) reviews.set(e[1], Number(e[2]));
      }
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
        reviews,
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
export async function pageCheck({ repo, write, needs: runNeeds, only, needsOnly, log }) {
  const harness = await loadHarness(repo);
  const fx = join(repo, FIXTURES_DIR);
  const all = statesOf(fx);
  const states = only || all;
  const acts = json(join(repo, ACTS_FILE));
  const origin = await harness.serve({ ui: join(repo, 'ui'), fixtures: fx });
  const results = await runStates(harness, { origin, runs: plan(fx, states), snapshots: join(repo, SNAPSHOTS_DIR), acts, write, log });
  const say = await sayRule(harness, origin, fx, all);
  log('page regions read=' + results.reduce((a, r) => a + r.region.read, 0) + ' rebuilt=' + results.filter((r) => r.region.misses.length).length + ' say=' + (say.read === 0 ? '-' : say.misses.length ? 'miss' : 'ok'));
  const needs = runNeeds && (!needsOnly || needsOnly.length) ? await needsPage(repo, log, needsOnly) : null;
  if (needs) log('page needs_alone states=' + (needs.parsed ? needs.states : '-') + ' test_items=' + (needs.parsed ? needs.items : '-') + ' failed=' + (needs.parsed ? needs.itemsFailed : '-') + ' exit=' + needs.code);
  const needsStates = needsOnly ? needsOnly.filter((s) => all.includes(s)).length : all.length;
  const cause = verdict(states, results, runNeeds && (!needsOnly || needsOnly.length) ? needs || { parsed: false } : null, say, needsStates);
  return { all, states, results, needs, say, cause, line: joined(all.length, results, needs) };
}

/** The first cause that holds for a whole run: a state that a run or page.mjs left unread, then a check that missed, then a region that missed. */
export function verdict(states, results, needs, say, needsStates = states.length) {
  if (states.length === 0 || results.length !== states.length) return 'states';
  const ran = results.map((r) => r.state).sort();
  const want = states.slice().sort();
  if (ran.some((s, i) => s !== want[i])) return 'states';
  if (needs && (!needs.parsed || needs.states !== needsStates)) return 'states';
  if (results.some((r) => r.names.length) || (needs && (needs.code !== 0 || needs.itemsFailed !== 0))) return 'page';
  if (results.some((r) => r.region.misses.length) || say.misses.length) return 'status';
  return null;
}
