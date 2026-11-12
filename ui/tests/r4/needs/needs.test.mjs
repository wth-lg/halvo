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
  expectedNodes,
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
const seen = { states: 0, items: 0, texts: 0, controls: 0, plans: 0, calls: 0, escapes: 0, presses: 0 };
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
  const patterns = Object.values(words()).map(
    (w) => new RegExp('^' + w.replace(/[.*+?^$()|[\]\\]/g, '\\$&').replace(/\{[a-z]+\}/g, '.+') + '$', 'u'),
  );
  const sentence = (text) => patterns.some((p) => p.test(text));
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
      const wantNodes = expectedNodes(items);
      const t = firstDiff(r.nodes, wantNodes);
      if (t >= 0) faults.push(state + ':node_at=' + t + ':got=' + r.nodes.length + ':want=' + wantNodes.length);
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
        const n = p.candidates[p.candidates.length - 1].n;
        acts.push({ act: 'candidate', param: p.name, n });
        calls.push({ call: 'agents_answer', args: { item: item.item, choice: 'choose', param: p.name, n } });
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

async function hidden(origin, dir) {
  const native = withId(stateFile(dir, 'needs_confirm_native').snapshot.items[0], 'a'.repeat(32));
  const ready = withId(stateFile(dir, 'needs_ready').snapshot.items[0], 'c'.repeat(32));
  const first = { seq: 1, rows: [], items: [native], note: null };
  const second = { seq: 2, rows: [], items: [native, ready], note: null };
  const replies = [
    { state: first },
    { call: 'agents_present', args: { item: native.item }, reply: { ok: 'yes' } },
    { state: second },
  ];
  const faults = [];
  const h = await open({ origin, replies, views: [VIEW] });
  try {
    await h.page.evaluate(() => Halvo.views.window('hidden'));
    await act(h, { act: 'present', param: native.item });
    const later = await read(h);
    if (firstDiff(later.units, expectedUnits(first.items)) >= 0) faults.push('applied_while_hidden');
  } finally {
    await h.close();
  }
  return faults;
}

async function frames(h) {
  await h.page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
}

async function escape(origin, dir, states) {
  const faults = [];
  for (const state of states) {
    if (pageConfirms(stateFile(dir, state).snapshot.items).length === 0) continue;
    const out = outDir();
    const h = await open({ origin, state, views: [VIEW], out });
    try {
      for (let i = 0; i < 3; i++) {
        await h.page.keyboard.press('Escape');
        await frames(h);
      }
      const r = await read(h);
      seen.escapes += 1;
      if (!(r.dialogs[0] && r.dialogs[0].modal && r.dialogs[0].rendered)) faults.push(state + ':closed_by_escape');
      if (callsOf(out).length !== 0) faults.push(state + ':call_on_escape');
    } finally {
      await h.close();
    }
  }
  return faults;
}

function sel(act, param) {
  return '[data-act="' + act + '"][data-param="' + param + '"]';
}

async function presses(h, how, locator) {
  if (how === 'held_enter') {
    await locator.focus();
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.up('Enter');
  } else {
    await locator.dblclick();
  }
  await frames(h);
  await frames(h);
}

function prepare(item) {
  return item.params
    .map((p) =>
      p.candidates.length > 0
        ? { act: 'candidate', param: p.name, n: p.candidates[p.candidates.length - 1].n }
        : p.typed === 'yes'
          ? { act: 'typed', param: p.name, text: TYPED }
          : null,
    )
    .filter(Boolean);
}

async function once(origin, dir) {
  const faults = [];
  const cases = [
    ['needs_confirm_native', 'present'],
    ['needs_sign_in', 'done'],
    ['needs_setup', 'choose'],
    ['test_setup', 'choose'],
  ];
  for (const [state, choice] of cases) {
    const item = stateFile(dir, state).snapshot.items[0];
    const want = plan(item, item.choices.find((c) => c.choice === choice)).calls;
    for (const how of ['held_enter', 'double_click']) {
      const out = outDir();
      const h = await open({ origin, state, views: [VIEW], out });
      try {
        for (const a of prepare(item)) await act(h, a);
        await presses(h, how, h.page.locator(sel(choice, item.item)));
        const got = callsOf(out).map((c) => ({ call: c.call, args: c.args }));
        seen.presses += 1;
        if (firstDiff(got, want) >= 0) faults.push(state + ':' + choice + ':' + how + ':calls=' + got.length + ':want=' + want.length);
      } finally {
        await h.close();
      }
    }
  }
  const setup = stateFile(dir, 'needs_setup').snapshot.items[0];
  const chain = plan(setup, setup.choices.find((c) => c.choice === 'choose')).calls;
  const out = outDir();
  const h = await open({ origin, state: 'needs_setup', views: [VIEW], out });
  try {
    await act(h, { act: 'choose', param: setup.item });
    if (callsOf(out).length !== 0) faults.push('choose_with_nothing:calls=' + callsOf(out).length);
    for (const a of prepare(setup)) await act(h, a);
    await act(h, { act: 'choose', param: setup.item });
    const got = callsOf(out).map((c) => ({ call: c.call, args: c.args }));
    if (firstDiff(got, chain) >= 0) faults.push('choose_after_nothing:calls=' + got.length + ':want=' + chain.length);
  } finally {
    await h.close();
  }
  return faults;
}

async function hold(origin, dir) {
  const faults = [];
  const item = stateFile(dir, 'needs_sign_in').snapshot.items[0];
  const first = { seq: 1, rows: [], items: [item], note: null };
  const same = { seq: 2, rows: [], items: [item], note: null };
  const done = { call: 'agents_answer', args: { item: item.item, choice: 'done' } };
  let h = await open({ origin, replies: [{ state: first }, { ...done, hold: true }], views: [VIEW] });
  try {
    await act(h, { act: 'done', param: item.item });
    await act(h, { act: 'done', param: item.item });
  } catch {
    faults.push('second_call_while_the_first_waits');
  } finally {
    await h.close();
  }
  const out = outDir();
  h = await open({ origin, replies: [{ state: first }, { ...done, hold: true }, { state: same }, { ...done, reply: { ok: 'yes' } }], views: [VIEW], out });
  try {
    await act(h, { act: 'done', param: item.item });
    await act(h, { act: 'done', param: item.item });
    if (callsOf(out).length !== 2) faults.push('not_given_back_by_a_newer_reply:calls=' + callsOf(out).length);
  } catch {
    faults.push('not_given_back_by_a_newer_reply');
  } finally {
    await h.close();
  }
  return faults;
}

function snap(seq, items) {
  return { seq, rows: [], items, note: null };
}

async function shows(h, items, tag, faults) {
  const r = await read(h);
  if (firstDiff(r.units, expectedUnits(items)) >= 0) faults.push(tag + ':texts');
  if (firstDiff(r.controls, expectedControls(items)) >= 0) faults.push(tag + ':controls');
  return r;
}

async function replies(origin, dir) {
  const faults = [];
  const sign = stateFile(dir, 'needs_sign_in').snapshot.items[0];
  const check = withId(stateFile(dir, 'needs_check').snapshot.items[0], 'd'.repeat(32));
  const step2 = JSON.parse(JSON.stringify(sign));
  step2.text = stateFile(dir, 'needs_stuck').snapshot.items[0].text;
  step2.choices = [
    { choice: 'decline', text: sign.choices[0].text, safe: 'yes' },
    { choice: 'allow', text: sign.choices[1].text, safe: 'no' },
  ];
  const check2 = JSON.parse(JSON.stringify(check));
  check2.fields[0].value = ['Water Board'];
  let h = await open({ origin, replies: [{ state: snap(1, [sign, check]) }, { state: snap(2, [step2, check2]) }], views: [VIEW] });
  try {
    await shows(h, [step2, check2], 'changed_in_place', faults);
  } finally {
    await h.close();
  }
  const confirm = stateFile(dir, 'needs_confirm_page').snapshot.items[0];
  const reject = { call: 'agents_answer', args: { item: confirm.item, choice: 'reject' } };
  const back = async (line, tag) => {
    const out = outDir();
    const k = await open({ origin, replies: [{ state: snap(1, [confirm]) }, line], views: [VIEW], out });
    try {
      await act(k, { act: 'reject', param: confirm.item });
      const r = await shows(k, [confirm], tag, faults);
      if (!(r.dialogs[0] && r.dialogs[0].modal && r.dialogs[0].rendered)) faults.push(tag + ':not_back');
    } catch {
      faults.push(tag + ':throws');
    } finally {
      await k.close();
    }
  };
  await back({ ...reject, reply: { error: 'state' } }, 'refused_comes_back');
  await back({ ...reject, reject: true }, 'rejected_comes_back');
  const out = outDir();
  h = await open({ origin, replies: [{ state: snap(1, [confirm]) }, { ...reject, reply: { ok: 'yes' } }, { state: snap(2, [confirm]) }], views: [VIEW], out });
  try {
    await act(h, { act: 'reject', param: confirm.item });
    const r = await read(h);
    if (r.dialogs.some((d) => d.rendered)) faults.push('answered_reopened');
    await h.page.keyboard.press('Enter');
    await frames(h);
    if (callsOf(out).length !== 1) faults.push('answered_called_again=' + callsOf(out).length);
  } catch {
    faults.push('answered_throws');
  } finally {
    await h.close();
  }
  return faults;
}

async function setups(origin, dir) {
  const faults = [];
  const one = stateFile(dir, 'needs_setup').snapshot.items[0];
  const two = withId(one, 'e'.repeat(32));
  two.agent = 'f'.repeat(32);
  const odd = withId(stateFile(dir, 'needs_confirm_page').snapshot.items[0], 'odd id x');
  const out = outDir();
  const h = await open({ origin, replies: [{ state: snap(1, [one, two, odd]) }], views: [VIEW], out });
  try {
    await shows(h, [one, two, odd], 'two_setups', faults);
    const facts = await h.page.evaluate((words) => {
      const section = document.getElementById('view-needs').parentNode;
      const named = [...section.querySelectorAll('[id],[name]')].map((e) => (e.id || '') + ' ' + (e.getAttribute('name') || ''));
      const leaks = named.filter((n) => words.some((w) => n.includes(w))).length;
      const d = section.querySelector('[role="alertdialog"]');
      const label = d ? document.getElementById(d.getAttribute('aria-labelledby')) : null;
      return { leaks, own: Boolean(label && d.contains(label)) };
    }, [one.item, two.item, 'odd id x', ...one.params.map((p) => p.name)]);
    if (facts.leaks !== 0) faults.push('ids_hold_reply_names=' + facts.leaks);
    if (!facts.own) faults.push('dialog_label_not_its_own');
    await h.page.evaluate(() => {
      const d = document.querySelector('dialog[open]');
      if (d) d.close();
    });
  } finally {
    await h.close();
  }
  const k = await open({ origin, replies: [{ state: snap(1, [one, two]) }, { call: 'agents_answer', args: { item: one.item, choice: 'choose', param: one.params[0].name, n: 1 }, reply: { ok: 'yes' } }], views: [VIEW], out: outDir() });
  try {
    const radios = k.page.locator('[data-act="candidate"][data-param="' + one.params[0].name + '"]');
    await radios.nth(0).check();
    await radios.nth(1).check();
    const both = await radios.evaluateAll((list) => list.map((r) => r.checked));
    if (!(both[0] && both[1])) faults.push('groups_joined');
    await act(k, { act: 'choose', param: one.item });
  } catch {
    faults.push('two_setups_choose_throws');
  } finally {
    await k.close();
  }
  return faults;
}

async function reorder(origin, dir) {
  const faults = [];
  const a = stateFile(dir, 'needs_sign_in').snapshot.items[0];
  const b = withId(stateFile(dir, 'needs_stuck').snapshot.items[0], 'b'.repeat(32));
  const c = withId(stateFile(dir, 'needs_ready').snapshot.items[0], 'c'.repeat(32));
  const h = await open({ origin, replies: [{ state: snap(1, [a, b, c]) }, { state: snap(2, [c, a, b]) }], views: [VIEW] });
  try {
    await shows(h, [c, a, b], 'reordered', faults);
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
const PAGE_TESTS = ['render', 'calls', 'next', 'hidden', 'escape', 'once', 'hold', 'replies', 'setups', 'reorder'];
if (PAGE_TESTS.some(want)) {
  if (!existsSync(VIEW_FILE)) {
    for (const name of PAGE_TESTS) if (want(name)) report(name, ['no_view']);
  } else {
    const origin = await serve({ ui: join(ROOT, 'ui'), fixtures: dir });
    if (want('render')) report('render', await render(origin, dir, states), ['states', 'items', 'texts', 'controls']);
    if (want('calls')) report('calls', await calls(origin, dir, states), ['plans', 'calls']);
    if (want('next')) report('next', await nextState(origin, dir));
    if (want('hidden')) report('hidden', await hidden(origin, dir));
    if (want('escape')) report('escape', await escape(origin, dir, states), ['escapes']);
    if (want('once')) report('once', await once(origin, dir), ['presses']);
    if (want('hold')) report('hold', await hold(origin, dir));
    if (want('replies')) report('replies', await replies(origin, dir));
    if (want('setups')) report('setups', await setups(origin, dir));
    if (want('reorder')) report('reorder', await reorder(origin, dir));
  }
}
const passed = results.filter((r) => r.ok).length;
console.log('needs tests=' + results.length + ' passed=' + passed + ' states=' + states.length);
process.exit(passed === results.length ? 0 : 1);
