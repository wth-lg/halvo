import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import vm from 'node:vm';
import {
  ROOT,
  VIEW,
  VIEW_FILE,
  act,
  callFaults,
  callsOf,
  commands,
  expectedControls,
  expectedUnits,
  firstDiff,
  fixtures,
  open,
  outDir,
  pageConfirms,
  read,
  serve,
  stateFile,
  words,
} from './lib.mjs';

const DENIED = [
  'setTimeout',
  'setInterval',
  'requestAnimationFrame',
  'requestIdleCallback',
  'localStorage',
  'sessionStorage',
  'indexedDB',
  'fetch',
  'XMLHttpRequest',
  'WebSocket',
  'EventSource',
  'sendBeacon',
  'innerHTML',
  'outerHTML',
  'insertAdjacentHTML',
  'setHTMLUnsafe',
  'parseHTMLUnsafe',
  'createContextualFragment',
  'DOMParser',
  'document.write',
  'attachShadow',
];
const TYPED = 'Flat 2';

const argv = process.argv.slice(2);
const pick = (flag) => {
  const i = argv.indexOf(flag);
  return i >= 0 && argv[i + 1] ? argv[i + 1].split(',') : null;
};
const onlyTests = pick('--tests');
const onlyStates = pick('--states');

const results = [];
const seen = { states: 0, items: 0, texts: 0, controls: 0, plans: 0, calls: 0 };
function report(name, faults, counted) {
  results.push({ name, ok: faults.length === 0 });
  const tail = counted ? ' ' + counted.map((k) => k + '=' + seen[k]).join(' ') : '';
  console.log((faults.length ? 'not ok ' : 'ok ') + name + tail + (faults.length ? ' ' + faults.join(' ') : ''));
}

function lint() {
  const faults = [];
  if (!existsSync(VIEW_FILE)) return ['missing'];
  const src = readFileSync(VIEW_FILE, 'utf8');
  for (const name of DENIED) {
    const re = new RegExp('(^|[^A-Za-z0-9_$])' + name.replace('.', '\\s*\\.\\s*') + '($|[^A-Za-z0-9_$])');
    if (re.test(src)) faults.push('denied=' + name);
  }
  if (/__playwright/.test(src)) faults.push('denied=__playwright');
  if (/__r4/.test(src)) faults.push('denied=__r4');
  if (src.includes('`')) faults.push('template');
  if (/^\s*(import|export)\b/m.test(src)) faults.push('module');
  if (!/^\s*heading: "[^"\\]*",$/m.test(src)) faults.push('heading');
  try {
    new vm.Script(src, { filename: 'needs.js' });
  } catch {
    faults.push('syntax');
  }
  return faults;
}

function itemsTrueToWords(tests, dir) {
  const table = Object.values(words());
  const sentence = (text) =>
    table.some((w) => w === text || ['1', '2', '3'].some((n) => w.split('{count}').join(n) === text));
  const faults = [];
  for (const state of tests) {
    for (const item of stateFile(dir, state).snapshot.items) {
      const texts = [item.title, ...item.fields.flatMap((f) => [f.label, f.from_text]), ...item.choices.map((c) => c.text)];
      if (item.text !== '') texts.push(item.text);
      texts.forEach((t, i) => {
        if (!sentence(t)) faults.push(state + '.text' + i);
      });
    }
  }
  return faults;
}

async function render(origin, dir, states) {
  const faults = [];
  for (const state of states) {
    const items = stateFile(dir, state).snapshot.items;
    const h = await open({ origin, state, views: [VIEW] });
    try {
      const r = await read(h);
      if (!r) {
        faults.push(state + ':no_section');
        continue;
      }
      const want = expectedUnits(items);
      const u = firstDiff(r.units, want);
      if (u >= 0) faults.push(state + ':text_at=' + u + ':got=' + r.units.length + ':want=' + want.length);
      seen.states += 1;
      seen.items += items.length;
      seen.texts += want.length;
      seen.controls += expectedControls(items).length;
      const c = firstDiff(r.controls, expectedControls(items));
      if (c >= 0) faults.push(state + ':control_at=' + c);
      const confirms = pageConfirms(items).length;
      if (r.dialogs.length !== confirms) faults.push(state + ':dialogs=' + r.dialogs.length);
      if (confirms > 0 && !(r.dialogs[0] && r.dialogs[0].modal && r.dialogs[0].rendered)) faults.push(state + ':dialog_not_modal');
    } finally {
      await h.close();
    }
  }
  return faults;
}

function plan(item, choice) {
  if (choice.choice === 'present') return { acts: [{ act: 'present', param: item.item }], calls: [{ call: 'agents_present', args: { item: item.item } }] };
  if (choice.choice === 'choose' && item.params.length > 0) {
    const acts = [];
    const calls = [];
    for (const p of item.params) {
      if (p.candidates.length > 0) {
        acts.push({ act: 'candidate', param: p.name, n: 1 });
        calls.push({ call: 'agents_answer', args: { item: item.item, choice: 'choose', param: p.name, n: 1 } });
      } else if (p.typed === 'yes') {
        acts.push({ act: 'typed', param: p.name, text: TYPED });
        calls.push({ call: 'agents_setup_typed', args: { item: item.item, param: p.name, text: TYPED } });
      }
    }
    acts.push({ act: 'choose', param: item.item });
    return { acts, calls };
  }
  return {
    acts: [{ act: choice.choice, param: item.item }],
    calls: [{ call: 'agents_answer', args: { item: item.item, choice: choice.choice } }],
  };
}

async function after(origin, state, acts) {
  const out = outDir();
  const h = await open({ origin, state, views: [VIEW], out });
  try {
    for (const a of acts) await act(h, a);
    return { calls: callsOf(out), page: await read(h) };
  } finally {
    await h.close();
  }
}

async function calls(origin, dir, states) {
  const list = commands();
  const faults = [];
  for (const state of states) {
    const items = stateFile(dir, state).snapshot.items;
    for (const item of items) {
      for (const choice of item.choices) {
        const p = plan(item, choice);
        const tag = state + ':' + choice.choice;
        const r = await after(origin, state, p.acts);
        seen.plans += 1;
        seen.calls += p.calls.length;
        const got = r.calls.map((c) => ({ call: c.call, args: c.args }));
        if (firstDiff(got, p.calls) >= 0) faults.push(tag + ':calls=' + got.length + ':want=' + p.calls.length);
        for (const c of r.calls) for (const f of callFaults(list, c)) faults.push(tag + ':' + f);
        if (item.kind === 'confirm' && item.presenter === 'page' && r.page.dialogs.some((d) => d.rendered)) faults.push(tag + ':not_closed');
      }
      if (item.params.length > 0) {
        const r = await after(origin, state, [{ act: 'choose', param: item.item }]);
        if (r.calls.length !== 0) faults.push(state + ':choose_with_no_answer=' + r.calls.length);
      }
    }
  }
  return faults;
}

function withId(item, id) {
  const copy = JSON.parse(JSON.stringify(item));
  copy.item = id;
  return copy;
}

async function nextState(origin, dir) {
  const native = withId(stateFile(dir, 'needs_confirm_native').snapshot.items[0], 'a'.repeat(32));
  const stuck = withId(stateFile(dir, 'needs_stuck').snapshot.items[0], 'b'.repeat(32));
  const ready = withId(stateFile(dir, 'needs_ready').snapshot.items[0], 'c'.repeat(32));
  const first = { seq: 1, rows: [], items: [native, stuck], note: null };
  const second = { seq: 2, rows: [], items: [native, stuck, ready], note: null };
  const replies = [
    { state: first },
    { call: 'agents_present', args: { item: native.item }, reply: { ok: 'yes' } },
    { state: second },
  ];
  const faults = [];
  const h = await open({ origin, replies, views: [VIEW] });
  try {
    const before = await read(h);
    if (firstDiff(before.units, expectedUnits(first.items)) >= 0) faults.push('first_state');
    await h.page.evaluate(() => {
      const section = document.getElementById('view-needs').parentNode;
      for (const el of section.querySelectorAll('*')) el.r4needsMark = true;
    });
    await act(h, { act: 'present', param: native.item });
    const later = await read(h);
    if (firstDiff(later.units, expectedUnits(second.items)) >= 0) faults.push('second_state');
    const kept = await h.page.evaluate((ids) => {
      const section = document.getElementById('view-needs').parentNode;
      const control = (id) => section.querySelector('[data-param="' + id + '"]');
      return {
        native: Boolean(control(ids[0]) && control(ids[0]).r4needsMark),
        stuck: Boolean(control(ids[1]) && control(ids[1]).r4needsMark),
        ready: Boolean(control(ids[2]) && control(ids[2]).r4needsMark),
        status: [...document.querySelectorAll('[role="status"]')].every((s) => s.r4needsMark === true),
      };
    }, [native.item, stuck.item, ready.item]);
    if (!kept.native) faults.push('native_rebuilt');
    if (!kept.stuck) faults.push('stuck_rebuilt');
    if (kept.ready) faults.push('ready_not_new');
    if (!kept.status) faults.push('status_rebuilt');
  } finally {
    await h.close();
  }
  return faults;
}

const want = (name) => !onlyTests || onlyTests.includes(name);

const { dir, states: tracked, tests } = fixtures();
const all = [...tracked, ...tests];
const states = onlyStates ? all.filter((s) => onlyStates.includes(s)) : all;
if (want('lint')) report('lint', lint());
if (want('words')) report('words', itemsTrueToWords(tests, dir));
if (want('render') || want('calls') || want('next')) {
  if (!existsSync(VIEW_FILE)) {
    for (const name of ['render', 'calls', 'next']) if (want(name)) report(name, ['no_view']);
  } else {
    const origin = await serve({ ui: join(ROOT, 'ui'), fixtures: dir });
    if (want('render')) report('render', await render(origin, dir, states), ['states', 'items', 'texts', 'controls']);
    if (want('calls')) report('calls', await calls(origin, dir, states), ['plans', 'calls']);
    if (want('next')) report('next', await nextState(origin, dir));
  }
}
const passed = results.filter((r) => r.ok).length;
console.log('needs tests=' + results.length + ' passed=' + passed + ' states=' + states.length);
process.exit(passed === results.length ? 0 : 1);
