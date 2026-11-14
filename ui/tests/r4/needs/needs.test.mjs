import { existsSync, readFileSync, writeFileSync } from 'node:fs';
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
  settled,
  trail,
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
const seen = { states: 0, items: 0, texts: 0, controls: 0, plans: 0, calls: 0, escapes: 0, presses: 0, clicks: '-', features: 0, touches: 0 };
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
  const table = words();
  const patterns = Object.values(table).map(
    (w) => new RegExp('^' + w.replace(/[.*+?^$()|[\]\\]/g, '\\$&').replace(/\{[a-z]+\}/g, '.+') + '$', 'u'),
  );
  const sentence = (text) => patterns.some((p) => p.test(text));
  const faults = [];
  for (const state of tests) {
    for (const item of stateFile(dir, state).snapshot.items) {
      const texts = [item.title, ...item.fields.flatMap((f) => [f.label, f.from_text]), ...item.choices.map((c) => c.text)];
      const row = stateFile(dir, state).snapshot.rows.find((r) => r.agent === item.agent);
      const name = row ? row.name : null;
      if (name === null) faults.push(state + '.' + item.item.slice(-2) + '.no_row_names_its_agent');
      if (item.say_new !== table['say.item.new'].replace('{name}', name)) faults.push(state + '.' + item.item.slice(-2) + '.say_new');
      if (item.say_gone !== table['say.item.gone'].replace('{name}', name)) faults.push(state + '.' + item.item.slice(-2) + '.say_gone');
      for (const p of item.params) if (p.typed === 'yes') texts.push(p.full);
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
      const earlier = item.kind === 'confirm' && item.presenter === 'page' ? pageConfirms(items).slice(0, pageConfirms(items).indexOf(item)) : [];
      for (const choice of item.choices) {
        const own = plan(item, choice);
        const first = earlier.map((e) => plan(e, e.choices.find((c) => c.safe === 'yes')));
        const p = { acts: [...first.flatMap((f) => f.acts), ...own.acts], calls: [...first.flatMap((f) => f.calls), ...own.calls] };
        const tag = state + ':' + item.item.slice(-2) + ':' + choice.choice;
        const r = await after(origin, state, p.acts);
        seen.plans += 1;
        seen.calls += p.calls.length;
        const got = r.calls.map((c) => ({ call: c.call, args: c.args }));
        if (firstDiff(got, p.calls) >= 0) faults.push(tag + ':calls=' + got.length + ':want=' + p.calls.length);
        for (const c of r.calls) for (const f of callFaults(list, c)) faults.push(tag + ':' + f);
        if (item.kind === 'confirm' && item.presenter === 'page' && r.page.dialogs.some((d) => d.rendered && d.item === item.item)) faults.push(tag + ':not_closed');
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
      };
    }, [native.item, stuck.item, ready.item]);
    if (!kept.native) faults.push('native_rebuilt');
    if (!kept.stuck) faults.push('stuck_rebuilt');
    if (kept.ready) faults.push('ready_not_new');
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
  await settled(h);
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
  if (how === 'slow_held_enter') {
    await locator.focus();
    await h.page.keyboard.down('Enter');
    await frames(h);
    await frames(h);
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.up('Enter');
  } else if (how === 'slow_double_click') {
    const box = await locator.boundingBox();
    const x = box.x + box.width / 2;
    const y = box.y + box.height / 2;
    await h.page.mouse.click(x, y);
    await frames(h);
    await frames(h);
    await h.page.mouse.down({ clickCount: 2 });
    await h.page.mouse.up({ clickCount: 2 });
  } else if (how === 'held_enter') {
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

async function waitCalls(h, out, count) {
  for (let i = 0; i < 200 && callsOf(out).length < count; i++) await frames(h);
  for (let i = 0; i < 4; i++) await frames(h);
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
    for (const how of ['held_enter', 'double_click', 'slow_held_enter', 'slow_double_click']) {
      const out = outDir();
      const h = await open({ origin, state, views: [VIEW], out });
      try {
        for (const a of prepare(item)) await act(h, a);
        await presses(h, how, h.page.locator(sel(choice, item.item)));
        await waitCalls(h, out, want.length);
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
  const answered = outDir();
  h = await open({ origin, replies: [{ state: first }, { ...done, reply: { ok: 'yes' } }, { ...done, reply: { error: 'state' } }], views: [VIEW], out: answered });
  try {
    await act(h, { act: 'done', param: item.item });
    await act(h, { act: 'done', param: item.item });
    if (callsOf(answered).length !== 2) faults.push('not_given_back_by_the_answer:calls=' + callsOf(answered).length);
  } catch {
    faults.push('not_given_back_by_the_answer');
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
  for (const cause of ['state', 'early', 'busy', 'no_service', 'stale', 'presenter']) await back({ ...reject, reply: { error: cause } }, 'refused_' + cause + '_comes_back');
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
  const dup = withId(stateFile(dir, 'needs_stuck').snapshot.items[0], '9'.repeat(32));
  const out = outDir();
  const h = await open({ origin, replies: [{ state: snap(1, [one, two, dup, dup, odd]) }, { state: snap(2, [one, two, dup, dup, odd]) }], views: [VIEW], out });
  try {
    await shows(h, [one, two, dup, dup, odd], 'two_setups', faults);
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

function writeState(dir, name, items) {
  writeFileSync(join(dir, 'state-' + name + '.json'), JSON.stringify({ snapshot: snap(1, items), card: null }) + '\n');
}

function said(out) {
  return callsOf(out).map((c) => c.call + ':' + (c.args.choice || '-') + ':' + c.args.item.slice(0, 1));
}

async function openDialog(h) {
  return h.page.evaluate(() => {
    const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
    if (!d) return null;
    return { item: d.querySelector('[data-param]').getAttribute('data-param').slice(0, 1), focused: document.activeElement === d };
  });
}

function engineOf(h) {
  return h.page.context().browser().browserType().name();
}

async function transcript(h, tags) {
  const facts = await h.page
    .evaluate(() => ({
      active: document.activeElement ? document.activeElement.tagName + ':' + (document.activeElement.getAttribute('data-act') || document.activeElement.getAttribute('role') || '-') : '-',
      dialogs: [...document.querySelectorAll('[role="alertdialog"]')].map((d) => (d.matches(':modal') ? 'modal' : d.open ? 'open' : 'closed') + ':' + (d.isConnected ? 'in' : 'out')),
    }))
    .catch(() => null);
  const head = 'transcript case=' + tags.join(';');
  console.log(head + ' stub_lines=' + trail(h).length + ' calls=' + h.calls + ' started=' + h.started + ' errors=' + h.errors + ' flags=' + ([...h.flags].join(',') || '-'));
  for (const line of trail(h)) console.log(head + ' ' + line);
  console.log(head + ' page ' + JSON.stringify(facts));
}

async function confirms(origin, dir) {
  const faults = [];
  let shown = 0;
  const keep = async (h) => {
    if (faults.length > shown) await transcript(h, faults.slice(shown));
    shown = faults.length;
  };
  const base = stateFile(dir, 'needs_confirm_page').snapshot.items[0];
  const a = withId(base, 'a'.repeat(32));
  const b = withId(base, 'b'.repeat(32));
  writeState(dir, 'test_two_page', [a, b]);
  const keyed = async (tag, steps, want) => {
    const out = outDir();
    const h = await open({ origin, state: 'test_two_page', views: [VIEW], out });
    try {
      const first = await openDialog(h);
      if (!first || first.item !== 'a' || !first.focused) faults.push(tag + ':first_not_taken');
      for (const s of steps) {
        if (s === 'walk') await h.page.keyboard.press(h.build.walkKey);
        else if (s === 'down') await h.page.keyboard.down('Enter');
        else if (s === 'up') await h.page.keyboard.up('Enter');
        else if (s === 'enter') await h.page.keyboard.press('Enter');
        await frames(h);
      }
      const got = said(out).join(',');
      if (got !== want) faults.push(tag + ':calls=' + (got || '-'));
    } finally {
      await keep(h);
      await keep(h);
    await h.close();
    }
  };
  await keyed('held_enter', ['walk', 'down', 'down', 'down', 'up'], 'agents_answer:reject:a');
  await keyed('two_quick_enters', ['walk', 'enter', 'enter'], 'agents_answer:reject:a');
  await keyed('tab_enter_twice', ['walk', 'enter', 'walk', 'enter'], 'agents_answer:reject:a,agents_answer:reject:b');
  await keyed('keys_on_the_confirmation', ['enter', 'down', 'up'], '');
  let out = outDir();
  let h = await open({ origin, state: 'test_two_page', views: [VIEW], out });
  try {
    await h.page.evaluate(() => {
      window.r4needsClicks = [];
      window.addEventListener('click', (e) => window.r4needsClicks.push(e.detail), true);
    });
    await h.page.locator(sel('allow', a.item)).dblclick();
    await frames(h);
    seen.clicks = (await h.page.evaluate(() => window.r4needsClicks.join('+'))) || '-';
    const got = said(out).join(',');
    if (got !== 'agents_answer:allow:a') faults.push('double_click_unsafe:calls=' + (got || '-'));
    const next = await openDialog(h);
    if (!next || next.item !== 'b' || !next.focused) faults.push('double_click_unsafe:second_not_open');
  } finally {
    await keep(h);
    await h.close();
  }
  const sign = stateFile(dir, 'needs_sign_in').snapshot.items[0];
  out = outDir();
  h = await open({
    origin,
    replies: [
      { state: snap(1, [sign]) },
      { call: 'agents_answer', args: { item: sign.item, choice: 'done' }, reply: { ok: 'yes' } },
      { state: snap(2, [sign, a]) },
      { call: 'agents_answer', args: { item: a.item, choice: 'reject' }, reply: { ok: 'yes' } },
    ],
    views: [VIEW],
    out,
  });
  try {
    await h.page.locator(sel('done', sign.item)).focus();
    await h.page.keyboard.down('Enter');
    await frames(h);
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.up('Enter');
    await frames(h);
    const d = await openDialog(h);
    if (!d || d.item !== 'a' || !d.focused) faults.push('held_key_reply:not_taken');
    await act(h, { act: 'reject', param: a.item });
    if (said(out).join(',') !== 'agents_answer:done:0,agents_answer:reject:a') faults.push('held_key_reply:calls=' + said(out).join(','));
  } catch (err) {
    faults.push('held_key_reply:throws');
  } finally {
    await keep(h);
    await h.close();
  }
  const typed = stateFile(dir, 'setup_three').snapshot.items[0];
  const stuck = withId(stateFile(dir, 'needs_stuck').snapshot.items[0], 'c'.repeat(32));
  out = outDir();
  h = await open({
    origin,
    replies: [
      { state: snap(1, [typed, stuck]) },
      { call: 'agents_answer', args: { item: stuck.item, choice: 'done' }, reply: { ok: 'yes' } },
      { state: snap(2, [typed, stuck, a]) },
      { call: 'agents_answer', args: { item: a.item, choice: 'reject' }, reply: { ok: 'yes' } },
    ],
    views: [VIEW],
    out,
  });
  try {
    await h.page.locator('[data-act="typed"]').first().focus();
    await h.page.keyboard.down('Enter');
    await h.page.evaluate((id) => document.querySelector('[data-act="done"][data-param="' + id + '"]').click(), stuck.item);
    await frames(h);
    await h.page.keyboard.down('Enter');
    await h.page.keyboard.up('Enter');
    await h.page.keyboard.press(' ');
    await h.page.keyboard.press('x');
    await frames(h);
    const d = await openDialog(h);
    if (!d || d.item !== 'a' || !d.focused) faults.push('typing_reply:not_taken');
    await act(h, { act: 'reject', param: a.item });
    if (said(out).join(',') !== 'agents_answer:done:c,agents_answer:reject:a') faults.push('typing_reply:calls=' + said(out).join(','));
  } catch (err) {
    faults.push('typing_reply:throws');
  } finally {
    await keep(h);
    await h.close();
  }
  out = outDir();
  h = await open({ origin, state: 'needs_confirm_page', views: [VIEW], out });
  try {
    await h.page.evaluate((id) => document.querySelector('[data-act="reject"][data-param="' + id + '"]').click(), base.item);
    await frames(h);
    const r = await read(h);
    if (callsOf(out).length !== 1 || r.dialogs.some((x) => x.rendered)) faults.push('click_with_no_press:calls=' + callsOf(out).length);
  } finally {
    await keep(h);
    await h.close();
  }
  out = outDir();
  h = await open({ origin, state: 'needs_confirm_page', views: [VIEW], out });
  try {
    await h.page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await h.page.locator(sel('reject', base.item)).click();
    await frames(h);
    if (callsOf(out).length !== 0) faults.push('raising_press_answered');
    if ((await h.page.locator(sel('reject', base.item)).count()) === 1) {
      await h.page.locator(sel('reject', base.item)).click();
      await frames(h);
    }
    if (callsOf(out).length !== 1) faults.push('press_after_raise:calls=' + callsOf(out).length);
  } finally {
    await keep(h);
    await h.close();
  }
  out = outDir();
  h = await open({ origin, state: 'needs_confirm_page', views: [VIEW], out });
  try {
    const box = await h.page.locator(sel('allow', base.item)).boundingBox();
    await h.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await h.page.mouse.down();
    await h.page.evaluate(() => document.querySelector('[role="alertdialog"]').close());
    await frames(h);
    const again = await openDialog(h);
    if (!again) faults.push('pressed_then_reopened:not_reopened');
    await h.page.mouse.up();
    await frames(h);
    if (callsOf(out).length !== 0) faults.push('pressed_then_reopened:calls=' + said(out).join(','));
  } finally {
    await keep(h);
    await h.close();
  }
  const allowA = { call: 'agents_answer', args: { item: a.item, choice: 'allow' }, reply: { ok: 'yes' } };
  const doneSign = { call: 'agents_answer', args: { item: sign.item, choice: 'done' }, reply: { ok: 'yes' } };
  let point = null;
  h = await open({ origin, replies: [{ state: snap(1, [sign, a]) }], views: [VIEW] });
  try {
    const box = await h.page.locator(sel('allow', a.item)).boundingBox();
    point = { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) };
  } finally {
    await h.close();
  }
  const opens = (k) => k.page.waitForFunction(() => Boolean(document.querySelector('[role="alertdialog"]:modal')), null, { timeout: 10000 }).then(() => true, () => false);
  out = outDir();
  h = await open({ origin, replies: [{ state: snap(1, [sign]) }, doneSign, { state: snap(2, [sign, a]) }, allowA], views: [VIEW], out });
  try {
    await h.page.mouse.move(point.x, point.y);
    await h.page.mouse.down();
    await h.page.evaluate((id) => document.querySelector('[data-act="done"][data-param="' + id + '"]').click(), sign.item);
    if (!(await opens(h))) faults.push('mouse_before_open:not_opened');
    await h.page.mouse.up();
    await frames(h);
    await frames(h);
    if (said(out).join(',') !== 'agents_answer:done:0') faults.push('mouse_before_open:calls=' + said(out).join(','));
  } finally {
    await keep(h);
    await h.close();
  }
  if (engineOf(h) !== 'chromium') {
    console.log('confirms touch: not run on ' + engineOf(h) + ', which takes no touch input through the browser\'s protocol in this harness');
  } else {
    const touch = async (k, type) => {
      const cdp = await k.page.context().newCDPSession(k.page);
      await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 });
      return (kind) => cdp.send('Input.dispatchTouchEvent', kind === 'start' ? { type: 'touchStart', touchPoints: [point] } : { type: 'touchEnd', touchPoints: [] });
    };
    out = outDir();
    h = await open({ origin, replies: [{ state: snap(1, [sign]) }, doneSign, { state: snap(2, [sign, a]) }, allowA], views: [VIEW], out });
    try {
      const send = await touch(h);
      await send('start');
      await h.page.evaluate((id) => document.querySelector('[data-act="done"][data-param="' + id + '"]').click(), sign.item);
      if (!(await opens(h))) faults.push('touch_before_open:not_opened');
      await send('end');
      await frames(h);
      await frames(h);
      seen.touches += 1;
      if (said(out).join(',') !== 'agents_answer:done:0') faults.push('touch_before_open:calls=' + said(out).join(','));
    } finally {
      await keep(h);
      await h.close();
    }
    out = outDir();
    h = await open({ origin, replies: [{ state: snap(1, [sign]) }, doneSign, { state: snap(2, [sign, a]) }, allowA], views: [VIEW], out });
    try {
      const cdp = await h.page.context().newCDPSession(h.page);
      const pen = (type) => cdp.send('Input.dispatchMouseEvent', { type, x: point.x, y: point.y, button: 'left', clickCount: 1, pointerType: 'pen' });
      await pen('mousePressed');
      await h.page.evaluate((id) => document.querySelector('[data-act="done"][data-param="' + id + '"]').click(), sign.item);
      if (!(await opens(h))) faults.push('pen_before_open:not_opened');
      await pen('mouseReleased');
      await frames(h);
      await frames(h);
      seen.touches += 1;
      if (said(out).join(',') !== 'agents_answer:done:0') faults.push('pen_before_open:calls=' + said(out).join(','));
    } finally {
      await keep(h);
      await h.close();
    }
    out = outDir();
    h = await open({ origin, replies: [{ state: snap(1, [sign, a]) }, allowA], views: [VIEW], out });
    try {
      const send = await touch(h);
      await send('start');
      await send('end');
      await frames(h);
      await frames(h);
      seen.touches += 1;
      if (said(out).join(',') !== 'agents_answer:allow:a') faults.push('touch_after_open:calls=' + (said(out).join(',') || '-'));
    } finally {
      await keep(h);
      await h.close();
    }
  }
  const long = withId(base, 'f'.repeat(32));
  const body = long.fields.find((f) => f.value.join('') !== '');
  long.fields = Array.from({ length: 40 }, () => ({ ...body, value: ['Thank you. Your rent for October came in and the receipt is attached for your records.'] }));
  writeState(dir, 'test_long_confirm', [long]);
  h = await open({ origin, state: 'test_long_confirm', views: [VIEW] });
  try {
    const top = await h.page.evaluate(() => {
      const d = document.querySelector('[role="alertdialog"]');
      const t = d.querySelector('h3');
      const r = d.getBoundingClientRect();
      const q = t.getBoundingClientRect();
      return { taller: d.scrollHeight > d.clientHeight, scroll: d.scrollTop, inView: r.top >= 0 && q.top >= r.top && q.bottom <= window.innerHeight, focused: document.activeElement === d };
    });
    if (!top.taller) faults.push('long_confirm_not_long');
    if (top.scroll !== 0 || !top.inView || !top.focused) faults.push('long_confirm_top_out_of_view');
  } finally {
    await keep(h);
    await h.close();
  }
  return faults;
}

async function crossed(origin, dir) {
  const faults = [];
  const base = stateFile(dir, 'needs_sign_in').snapshot.items[0];
  const a = withId(base, 'a'.repeat(32));
  const b = { ...withId(base, 'b'.repeat(32)), agent: 'f'.repeat(32) };
  writeState(dir, 'test_two_sign_in', [a, b]);
  const sel = (act, id) => '[data-act="' + act + '"][data-param="' + id + '"]';
  const gate = { call: 'agents_card', args: { agent: 'e'.repeat(32) }, reply: { error: 'busy' } };
  for (const choice of ['decline', 'done']) {
    const answer = { call: 'agents_answer', args: { item: b.item, choice }, reply: { ok: 'yes' } };
    let out = outDir();
    let h = await open({ origin, state: 'test_two_sign_in', views: [VIEW], out });
    try {
      const pointerClick = (from, to) =>
        h.page.evaluate(
          ([f, t]) => {
            document.querySelector(f).dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, isPrimary: true }));
            document.querySelector(t).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
          },
          [from, to],
        );
      await pointerClick(sel(choice, a.item), sel(choice, b.item));
      await frames(h);
      const elsewhere = said(out).join(',');
      if (elsewhere !== '') faults.push('pointer_begun_on_another_items_' + choice + ':calls=' + elsewhere);
      await pointerClick(sel(choice, b.item), sel(choice, b.item));
      await frames(h);
      const own = said(out).join(',');
      if (own !== 'agents_answer:' + choice + ':b') faults.push('pointer_begun_on_its_own_' + choice + ':calls=' + (own || '-'));
    } finally {
      await h.close();
    }
    if (engineOf(h) !== 'chromium') continue;
    out = outDir();
    h = await open({ origin, replies: [{ state: snap(1, [a, b]) }, gate, { state: snap(2, [b]) }, answer, answer], views: [VIEW], out });
    try {
      const box = await h.page.locator(sel(choice, a.item)).boundingBox();
      const point = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
      const cdp = await h.page.context().newCDPSession(h.page);
      await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 });
      const touch = (kind) => cdp.send('Input.dispatchTouchEvent', kind === 'start' ? { type: 'touchStart', touchPoints: [point] } : { type: 'touchEnd', touchPoints: [] });
      await touch('start');
      await h.page.evaluate((agent) => window.Halvo.ipc.call('agents_card', { agent }), gate.args.agent);
      await h.page.waitForFunction((s) => document.querySelector(s) === null, sel(choice, a.item), { timeout: 3000 });
      await frames(h);
      const under = await h.page.evaluate(([x, y]) => {
        const e = document.elementFromPoint(x, y);
        return e && e.tagName === 'BUTTON' ? e.getAttribute('data-act') + ':' + e.getAttribute('data-param').slice(0, 1) : '-';
      }, [point.x, point.y]);
      if (under !== choice + ':b') faults.push('touch_' + choice + ':not_under_the_finger=' + under);
      await touch('end');
      await frames(h);
      const lifted = callsOf(out).filter((c) => c.call === 'agents_answer').map((c) => c.call + ':' + c.args.choice + ':' + c.args.item.slice(0, 1)).join(',');
      if (lifted !== '') faults.push('touch_begun_on_another_items_' + choice + ':calls=' + lifted);
      await h.page.waitForTimeout(800);
      await touch('start');
      await touch('end');
      await frames(h);
      const fresh = callsOf(out).filter((c) => c.call === 'agents_answer').map((c) => c.call + ':' + c.args.choice + ':' + c.args.item.slice(0, 1)).join(',');
      if (fresh !== 'agents_answer:' + choice + ':b') faults.push('touch_fresh_' + choice + ':calls=' + (fresh || '-'));
    } finally {
      await h.close();
    }
  }
  return faults;
}

function told(item, tag) {
  return { ...item, say_new: tag + ' came.', say_gone: tag + ' went.' };
}

async function region(origin, dir) {
  const faults = [];
  const stuck = told(withId(stateFile(dir, 'needs_stuck').snapshot.items[0], 'b'.repeat(32)), 'Stuck');
  const ready = told(withId(stateFile(dir, 'needs_ready').snapshot.items[0], 'c'.repeat(32)), 'Ready');
  const confirm = told(withId(stateFile(dir, 'needs_confirm_page').snapshot.items[0], 'a'.repeat(32)), 'Confirm');
  const gate = { call: 'agents_card', args: { agent: 'e'.repeat(32) }, reply: { error: 'busy' } };
  const facts = (h) =>
    h.page.evaluate(() => {
      const section = document.getElementById('view-needs').parentNode;
      const all = [...section.querySelectorAll('[role="status"]')];
      const r = all[0] || null;
      const a = document.activeElement;
      return {
        count: all.length,
        everywhere: document.querySelectorAll('[role="status"]').length,
        live: r ? r.getAttribute('aria-live') : null,
        atomic: r ? r.getAttribute('aria-atomic') : null,
        top: section.querySelector('ul').getBoundingClientRect().top,
        place: Boolean(r) && r.nextElementSibling === section.querySelector('ul') && Boolean(document.getElementById('view-needs').compareDocumentPosition(r) & Node.DOCUMENT_POSITION_FOLLOWING),
        kept: Boolean(r && r.r4needsMark),
        says: r ? [...r.childNodes].map((n) => n.textContent) : [],
        focus: a ? (a.getAttribute('data-act') || a.getAttribute('role') || a.tagName) + ':' + (a.getAttribute('data-param') || '-').slice(0, 1) : '-',
      };
    });
  const mark = (h) =>
    h.page.evaluate(() => {
      const r = document.getElementById('view-needs').parentNode.querySelector('[role="status"]');
      if (r) r.r4needsMark = true;
    });
  const step = async (tag, first, focusOn, second, wanted, wantFocus) => {
    const h = await open({ origin, replies: [{ state: snap(1, first) }, gate, { state: snap(2, second) }], views: [VIEW] });
    try {
      const at = await facts(h);
      if (at.count !== 1 || at.live !== 'polite' || at.says.length !== 0) faults.push(tag + ':before=' + JSON.stringify(at));
      if (!at.place) faults.push(tag + ':region_not_between_the_heading_and_the_list');
      if (at.atomic !== 'true') faults.push(tag + ':aria_atomic=' + at.atomic);
      await mark(h);
      if (focusOn) await h.page.locator(focusOn).focus();
      const ids = second.map((i) => i.item).join(',');
      await h.page.evaluate((agent) => window.Halvo.ipc.call('agents_card', { agent }), gate.args.agent);
      await h.page.waitForFunction(
        (want) => [...new Set([...document.querySelectorAll('#views [data-param]')].map((e) => e.getAttribute('data-param')))].filter((p) => /^[0-9a-f]{32}$/.test(p)).join(',') === want,
        ids,
        { timeout: 5000 },
      );
      await frames(h);
      const got = await facts(h);
      if (got.count !== 1 || got.everywhere !== 1 || got.live !== 'polite' || !got.kept) faults.push(tag + ':region=' + JSON.stringify(got));
      if (got.top !== at.top) faults.push(tag + ':cards_moved=' + (got.top - at.top));
      if (JSON.stringify(got.says) !== JSON.stringify(wanted)) faults.push(tag + ':says=' + (got.says.join('|') || '-'));
      if (wantFocus && got.focus !== wantFocus) faults.push(tag + ':focus=' + got.focus);
    } catch (err) {
      faults.push(tag + ':throws');
    } finally {
      await h.close();
    }
  };
  await step('arrives_while_the_focus_is_elsewhere', [stuck], sel('done', stuck.item), [stuck, ready], ['Ready came.'], 'done:b');
  await step('goes_while_the_focus_is_elsewhere', [stuck, ready], sel('done', stuck.item), [stuck], ['Ready went.'], 'done:b');
  await step('arrives_while_the_focus_is_on_the_page', [stuck], null, [stuck, ready], ['Ready came.'], null);
  await step('goes_while_the_focus_is_in_it', [stuck, ready], sel('start', ready.item), [stuck], ['Ready went.'], null);
  await step('a_confirmation_arrives_and_holds_the_focus', [stuck], sel('done', stuck.item), [stuck, confirm], [], 'alertdialog:-');
  await step('one_goes_and_one_arrives', [stuck], null, [ready], ['Stuck went.', 'Ready came.'], null);
  await step('one_goes_with_the_focus_in_it_and_one_arrives', [stuck], sel('done', stuck.item), [ready], ['Stuck went.', 'Ready came.'], null);
  await step('an_item_with_no_sentence_adds_nothing', [stuck], null, [stuck, { ...ready, say_new: '' }], [], null);
  const h = await open({ origin, state: 'test_many', views: [VIEW] });
  try {
    await mark(h);
    await h.page.evaluate(() => Halvo.views.window('hidden'));
    await h.page.evaluate(() => Halvo.views.window('shown'));
    await settled(h);
    await frames(h);
    const got = await facts(h);
    if (got.count !== 1 || !got.kept || got.says.length !== 0) faults.push('the_first_reply_says_nothing_and_a_show_keeps_the_region=' + JSON.stringify(got));
  } catch {
    faults.push('the_first_reply_says_nothing_and_a_show_keeps_the_region:throws');
  } finally {
    await h.close();
  }
  writeState(dir, 'test_region_hidden', [stuck]);
  const hid = 'an_item_that_comes_while_the_view_is_hidden_is_said_at_the_show';
  const k = await open({ origin, state: 'test_region_hidden', views: [VIEW] });
  try {
    await mark(k);
    await k.page.evaluate(() => Halvo.views.window('hidden'));
    k.state = { snapshot: snap(2, [stuck, ready]), card: null };
    await k.page.evaluate(() => Halvo.views.window('shown'));
    await k.page.waitForFunction((id) => document.querySelector('[data-param="' + id + '"]') !== null, ready.item, { timeout: 5000 });
    await settled(k);
    await frames(k);
    const got = await facts(k);
    if (got.count !== 1 || !got.kept || JSON.stringify(got.says) !== JSON.stringify(['Ready came.'])) faults.push(hid + ':says=' + (got.says.join('|') || '-') + ':count=' + got.count);
  } catch {
    faults.push(hid + ':throws');
  } finally {
    await k.close();
  }
  const second = told(withId(stateFile(dir, 'needs_confirm_page').snapshot.items[0], 'd'.repeat(32)), 'Second');
  const other = told(withId(stateFile(dir, 'needs_show').snapshot.items[0], '7'.repeat(32)), 'Other');
  const answerLine = (c) => ({ call: 'agents_answer', args: { item: c.item, choice: 'reject' }, reply: { ok: 'yes' } });
  const says = (h) => facts(h).then((f) => f.says);
  const waitIds = (h, items) =>
    h.page.waitForFunction(
      (want) => [...new Set([...document.querySelectorAll('#views [data-param]')].map((e) => e.getAttribute('data-param')))].filter((p) => /^[0-9a-f]{32}$/.test(p)).sort().join(',') === want,
      items.map((i) => i.item).sort().join(','),
      { timeout: 5000 },
    );
  const pass = async (h, items) => {
    await h.page.evaluate((agent) => window.Halvo.ipc.call('agents_card', { agent }), gate.args.agent);
    await waitIds(h, items);
    await frames(h);
  };
  const answerOpen = (h) =>
    h.page.evaluate(() => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="reject"]').click();
    });
  const afterClose = async (h) => {
    await frames(h);
    await settled(h);
    await frames(h);
  };
  const flow = async (tag, replies, fn) => {
    const h = await open({ origin, replies, views: [VIEW] });
    try {
      await fn(h);
    } catch (err) {
      faults.push(tag + ':throws');
    } finally {
      await h.close();
    }
  };
  const expect = (tag, got, want) => {
    if (JSON.stringify(got) !== JSON.stringify(want)) faults.push(tag + ':says=' + (got.join('|') || '-'));
  };
  const open1 = 'arrives_while_a_confirmation_holds_the_focus';
  await flow(open1, [{ state: snap(1, [confirm]) }, gate, { state: snap(2, [confirm, ready]) }, answerLine(confirm)], async (h) => {
    await pass(h, [confirm, ready]);
    expect(open1 + '_while_open', await says(h), []);
    await answerOpen(h);
    await afterClose(h);
    expect(open1 + '_after_the_answer', await says(h), ['Ready came.']);
  });
  const both = 'a_reply_with_a_confirmation_and_another_item';
  await flow(both, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, other]) }, gate, { state: snap(3, [stuck, other, confirm, ready]) }, answerLine(confirm)], async (h) => {
    await pass(h, [stuck, other]);
    expect(both + '_before', await says(h), ['Other came.']);
    await pass(h, [stuck, other, confirm, ready]);
    expect(both + '_while_open', await says(h), []);
    await answerOpen(h);
    await afterClose(h);
    expect(both + '_after_the_close', await says(h), ['Ready came.']);
  });
  const waits = 'a_waiting_confirmation_is_never_said';
  await flow(waits, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm, second]) }, answerLine(confirm), answerLine(second)], async (h) => {
    const seen = [];
    await pass(h, [stuck, confirm, second]);
    seen.push(...(await says(h)));
    await answerOpen(h);
    await afterClose(h);
    seen.push(...(await says(h)));
    await answerOpen(h);
    await afterClose(h);
    const end = await says(h);
    seen.push(...end);
    if (seen.includes('Second came.') || seen.includes('Confirm came.')) faults.push(waits + ':said=' + seen.join('|'));
    expect(waits + '_after', end, []);
  });
  const between = 'an_item_that_came_and_went_while_a_confirmation_was_open';
  await flow(between, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm]) }, gate, { state: snap(3, [stuck, confirm, ready]) }, gate, { state: snap(4, [stuck, confirm]) }, answerLine(confirm)], async (h) => {
    await pass(h, [stuck, confirm]);
    await pass(h, [stuck, confirm, ready]);
    await pass(h, [stuck, confirm]);
    await answerOpen(h);
    await afterClose(h);
    expect(between, await says(h), []);
  });
  const dropped = 'a_confirmation_dropped_without_an_answer_is_said_gone_after';
  await flow(dropped, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm]) }, gate, { state: snap(3, [stuck]) }], async (h) => {
    await pass(h, [stuck, confirm]);
    await pass(h, [stuck]);
    await afterClose(h);
    expect(dropped, await says(h), ['Confirm went.']);
  });
  const answeredOne = 'an_answered_confirmation_is_not_said_gone';
  await flow(answeredOne, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm]) }, answerLine(confirm), { state: snap(3, [stuck]) }], async (h) => {
    await pass(h, [stuck, confirm]);
    await answerOpen(h);
    await afterClose(h);
    await waitIds(h, [stuck]);
    await afterClose(h);
    expect(answeredOne, await says(h), []);
  });
  const many = 'the_element_holds_the_last_additions_sentences_alone';
  const silent = { ...told(withId(stateFile(dir, 'needs_check').snapshot.items[0], '6'.repeat(32)), 'Silent'), say_new: '', say_gone: '' };
  const changes = [snap(1, [stuck])];
  for (let i = 1; i <= 80; i++) changes.push(snap(i + 1, i % 2 ? [stuck, ready] : [stuck]));
  changes.push(snap(82, [stuck]));
  await flow(many, [...changes.map((s) => ({ state: s })), gate, { state: snap(83, [stuck, silent]) }, gate, { state: snap(84, [stuck, silent, ready]) }], async (h) => {
    await h.page.waitForFunction(() => document.querySelectorAll('#views [data-act]').length > 0, null, { timeout: 5000 });
    await settled(h);
    await frames(h);
    expect(many + '_after_80_replies_and_one_with_the_same_items', await says(h), ['Ready went.']);
    await pass(h, [stuck, silent]);
    expect(many + '_after_a_reply_that_adds_nothing', await says(h), []);
    await pass(h, [stuck, silent, ready]);
    expect(many + '_after_one_more', await says(h), ['Ready came.']);
    await h.page.evaluate(() => Halvo.views.window('hidden'));
    expect(many + '_after_a_hide', await says(h), []);
  });
  const ax = 'the_tree_holds_the_element_after_the_close';
  await flow(ax, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm, ready]) }, answerLine(confirm)], async (h) => {
    if (engineOf(h) !== 'chromium') {
      console.log('region tree: not read on ' + engineOf(h) + ', whose tree this harness does not read');
      return;
    }
    await pass(h, [stuck, confirm, ready]);
    await answerOpen(h);
    await afterClose(h);
    const cdp = await h.page.context().newCDPSession(h.page);
    await cdp.send('DOM.enable');
    await cdp.send('Accessibility.enable');
    const { root } = await cdp.send('DOM.getDocument', { depth: 0 });
    const { nodeId } = await cdp.send('DOM.querySelector', { nodeId: root.nodeId, selector: '[role="status"]' });
    const { nodes } = await cdp.send('Accessibility.getPartialAXTree', { nodeId, fetchRelatives: false });
    const node = nodes.find((n) => n.role && n.role.value === 'status');
    if (!node || node.ignored) faults.push(ax + ':status_node=' + (node ? 'ignored' : 'missing'));
    const full = await cdp.send('Accessibility.getFullAXTree');
    const byId = new Map(full.nodes.map((n) => [n.nodeId, n]));
    const status = full.nodes.find((n) => n.role && n.role.value === 'status' && !n.ignored);
    const texts = [];
    const walk = (n) => {
      if (!n) return;
      if (n.role && n.role.value === 'StaticText' && n.name) texts.push(n.name.value);
      (n.childIds || []).forEach((c) => walk(byId.get(c)));
    };
    walk(status);
    if (!texts.join(' ').includes('Ready came.')) faults.push(ax + ':tree_text=' + (texts.join('|') || '-'));
  });
  const task = 'the_release_is_not_made_in_the_task_that_closes';
  await flow(task, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm, ready]) }, answerLine(confirm)], async (h) => {
    await pass(h, [stuck, confirm, ready]);
    const inTask = await h.page.evaluate(() => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="reject"]').click();
      return [...document.querySelector('[role="status"]').childNodes].map((n) => n.textContent);
    });
    expect(task + '_in_the_closing_task', inTask, []);
    await afterClose(h);
    expect(task + '_after_the_close', await says(h), ['Ready came.']);
  });
  return faults;
}

const cp = (...codes) => String.fromCodePoint(...codes);
const xs = (n) => 'x'.repeat(n);

async function typedFacts(h, name) {
  return h.page.evaluate((n) => {
    const t = document.querySelector('[data-act="typed"][data-param="' + n + '"]');
    const id = t.getAttribute('aria-describedby');
    const note = id ? document.getElementById(id) : null;
    const after = t.nextSibling;
    return {
      value: t.value,
      caret: [t.selectionStart, t.selectionEnd],
      described: id !== null,
      note: note ? note.textContent : null,
      noteNodes: note ? [...note.childNodes].filter((c) => c.nodeType === 3).length : 0,
      noteAfterLine: Boolean(note && after === note),
      notes: t.parentNode.querySelectorAll('.hn-full').length,
    };
  }, name);
}

async function bound(origin, dir) {
  const faults = [];
  const item = stateFile(dir, 'setup_three').snapshot.items[0];
  const p = item.params.find((x) => x.typed === 'yes');
  const line = '[data-act="typed"][data-param="' + p.name + '"]';
  const sentOf = async (h, out) => {
    await act(h, { act: 'choose', param: item.item });
    const c = callsOf(out).find((x) => x.call === 'agents_setup_typed');
    return c ? c.args.text : null;
  };
  const shows = (tag, f, want) => {
    const text = want === 'too_long' ? p.too_long : p.full;
    const ok = want
      ? f.described && f.note === text && f.noteNodes === 1 && f.noteAfterLine && f.notes === 1
      : !f.described && f.note === null && f.notes === 0;
    if (!ok) faults.push(tag + ':sentence=' + JSON.stringify({ described: f.described, note: f.note !== null, notes: f.notes }));
  };
  const once = [
    ['one_input_of_1024_bytes', cp(0xe9).repeat(512), true],
    ['one_input_of_1025_bytes', cp(0xe9).repeat(512) + 'a', false],
    ['one_input_ending_in_a_three_byte_letter', 'a'.repeat(1023) + cp(0x20ac), false],
    ['one_input_of_four_byte_letters', cp(0x1d11e).repeat(257), false],
    ['one_input_ending_in_a_combining_accent', 'a'.repeat(1022) + 'e' + cp(0x301), false],
    ['one_input_ending_in_a_joined_emoji', 'a'.repeat(1016) + cp(0x1f469, 0x200d, 0x1f4bb), false],
    ['one_input_ending_in_a_flag', 'a'.repeat(1020) + cp(0x1f1eb, 0x1f1f7), false],
    ['one_input_ending_in_an_emoji_with_its_variation', 'a'.repeat(1021) + cp(0x2764, 0xfe0f), false],
  ];
  for (const [tag, text, fits] of once) {
    const out = outDir();
    const h = await open({ origin, state: 'setup_three', views: [VIEW], out });
    try {
      await h.page.locator(line).fill(text);
      const f = await typedFacts(h, p.name);
      const want = fits ? text : '';
      if (f.value !== want) faults.push(tag + ':line_holds=' + Buffer.byteLength(f.value));
      shows(tag, f, fits ? false : 'too_long');
      const sent = await sentOf(h, out);
      if (fits ? sent !== text : sent !== null) faults.push(tag + ':sent=' + (sent === null ? 'none' : Buffer.byteLength(sent)));
    } finally {
      await h.close();
    }
  }
  const keyed = [
    ['hole_a_three_byte_letter_before_two_letters', xs(1022), cp(0x20ac) + 'ab', xs(1022)],
    ['hole_a_flag_before_four_digits', xs(1020), cp(0x1f1eb, 0x1f1f7) + '1234', xs(1020) + cp(0x1f1eb)],
    ['hole_a_thai_letter_with_its_vowel_and_tone_marks', xs(1022), cp(0xe01, 0xe34, 0xe48), xs(1022)],
    ['hole_a_hangul_syllable_as_three_jamo', xs(1020), cp(0x1112, 0x1161, 0x11ab), xs(1020) + cp(0x1112)],
    ['a_two_byte_letter_across_the_bound_counts_two_bytes', xs(1023), cp(0xe9) + 'a', xs(1023)],
    ['a_two_byte_letter_up_to_the_bound_fits', xs(1022), cp(0xe9), xs(1022) + cp(0xe9)],
  ];
  for (const [tag, start, typed, want] of keyed) {
    const out = outDir();
    const h = await open({ origin, state: 'setup_three', views: [VIEW], out });
    try {
      await h.page.locator(line).fill(start);
      await h.page.locator(line).pressSequentially(typed);
      await settled(h);
      const f = await typedFacts(h, p.name);
      if (f.value !== want) faults.push(tag + ':line_ends_in=' + [...f.value.slice(-4)].map((c) => c.codePointAt(0).toString(16)).join('.'));
      if (!(start + typed).startsWith(f.value)) faults.push(tag + ':not_a_start_of_what_was_typed');
      shows(tag, f, want !== start + typed);
      const sent = await sentOf(h, out);
      if (sent !== want) faults.push(tag + ':sent=' + (sent === null ? 'none' : Buffer.byteLength(sent)));
    } finally {
      await h.close();
    }
  }
  const same = await open({ origin, state: 'setup_three', views: [VIEW], out: outDir() });
  try {
    const t = 'the_same_letter_over_a_selection_at_the_bound_keeps_the_bound';
    await same.page.locator(line).fill(xs(1022));
    await same.page.locator(line).pressSequentially(cp(0x20ac));
    await same.page.locator(line).evaluate((x) => x.setSelectionRange(1021, 1022));
    await same.page.locator(line).pressSequentially('x');
    await same.page.locator(line).pressSequentially('a');
    const f = await typedFacts(same, p.name);
    if (f.value !== xs(1022)) faults.push(t + ':line_ends_in=' + f.value.slice(-2));
    shows(t, f, true);
  } finally {
    await same.close();
  }
  const out = outDir();
  const h = await open({ origin, state: 'setup_three', views: [VIEW], out });
  const tag = 'typed_key_by_key_to_the_bound';
  try {
    await act(h, { act: 'typed', param: p.name, text: xs(1022) + cp(0x20ac) + 'ab' });
    const f = await typedFacts(h, p.name);
    if (f.value !== xs(1022)) faults.push(tag + ':line_holds=' + Buffer.byteLength(f.value));
    shows(tag, f, true);
  } finally {
    await h.close();
  }
  const caretCases = [
    ['a_replacement_on_a_full_line_is_refused', 'a'.repeat(500) + 'b'.repeat(524), [600, 601], 'z', [600, 601]],
    ['a_refused_letter_among_repeated_letters_leaves_the_caret', xs(1024), [10, 10], 'x', [10, 10]],
    ['a_refused_emoji_before_an_emoji_leaves_the_caret', xs(1020) + cp(0x1f600), [1020, 1020], cp(0x1f601), [1020, 1020]],
  ];
  for (const [t, start, at, typed, caret] of caretCases) {
    const k = await open({ origin, state: 'setup_three', views: [VIEW], out: outDir() });
    try {
      await k.page.locator(line).fill(start);
      await k.page.locator(line).evaluate((x, a) => x.setSelectionRange(a[0], a[1]), at);
      await k.page.locator(line).pressSequentially(typed);
      const f = await typedFacts(k, p.name);
      if (f.value !== start) faults.push(t + ':line_changed');
      if (JSON.stringify(f.caret) !== JSON.stringify(caret)) faults.push(t + ':caret=' + f.caret.join('-'));
      shows(t, f, true);
    } finally {
      await k.close();
    }
  }
  const m = await open({ origin, state: 'setup_three', views: [VIEW], out: outDir() });
  const mid = 'a_letter_put_into_the_middle_of_a_full_line';
  try {
    const full = 'a'.repeat(500) + 'b'.repeat(524);
    await m.page.locator(line).fill(full);
    await m.page.locator(line).evaluate((t) => t.setSelectionRange(500, 500));
    await m.page.locator(line).pressSequentially('z');
    let f = await typedFacts(m, p.name);
    if (f.value !== full) faults.push(mid + ':line_ends_in=' + f.value.slice(-2) + ':bytes=' + Buffer.byteLength(f.value));
    if (JSON.stringify(f.caret) !== '[500,500]') faults.push(mid + ':caret=' + f.caret.join('-'));
    shows(mid, f, true);
    await m.page.locator(line).evaluate((t) => t.setSelectionRange(500, 510));
    await m.page.locator(line).pressSequentially('z');
    f = await typedFacts(m, p.name);
    if (f.value !== full) faults.push(mid + ':a_letter_over_a_selection_at_the_bound');
    if (JSON.stringify(f.caret) !== '[500,510]') faults.push(mid + ':selection=' + f.caret.join('-'));
    await m.page.keyboard.press('Backspace');
    f = await typedFacts(m, p.name);
    if (f.value !== 'a'.repeat(500) + 'b'.repeat(514)) faults.push(mid + ':removal_not_taken');
    shows(mid + '_after_a_removal', f, false);
    await m.page.locator(line).pressSequentially('zz');
    f = await typedFacts(m, p.name);
    if (f.value !== 'a'.repeat(500) + 'zz' + 'b'.repeat(514)) faults.push(mid + ':a_letter_after_a_removal_not_taken');
    shows(mid + '_after_a_letter', f, false);
  } finally {
    await m.close();
  }
  const one = async (t, fn) => {
    const k = await open({ origin, state: 'setup_three', views: [VIEW], out: outDir() });
    try {
      await fn(k, k.page.locator(line));
    } catch (err) {
      faults.push(t + ':throws');
    } finally {
      await k.close();
    }
  };
  const big = 'y'.repeat(2000);
  const digits = Array.from({ length: 1024 }, (_, i) => String(i % 10)).join('');
  const second = 'a_second_refusal_after_a_removal_shows_the_sentence_again';
  await one(second, async (k, l) => {
    await l.fill(xs(1000));
    await k.page.keyboard.insertText('y'.repeat(30));
    shows(second + '_first', await typedFacts(k, p.name), 'too_long');
    await k.page.keyboard.press('Backspace');
    shows(second + '_after_the_removal', await typedFacts(k, p.name), false);
    await k.page.keyboard.insertText('y'.repeat(30));
    const f = await typedFacts(k, p.name);
    if (f.value !== xs(999)) faults.push(second + ':line_holds=' + Buffer.byteLength(f.value));
    shows(second + '_second', f, 'too_long');
  });
  const empties = [
    ['an_empty_line_after_a_refusal_takes_the_next_letter', async (k, l) => l.focus()],
    [
      'a_line_emptied_after_a_refusal_takes_the_next_letter',
      async (k, l) => {
        await l.fill('abc');
        await k.page.keyboard.insertText(big);
        await l.evaluate((x) => x.setSelectionRange(0, 3));
        await k.page.keyboard.press('Backspace');
      },
    ],
  ];
  for (const [t, prep] of empties) {
    await one(t, async (k, l) => {
      await prep(k, l);
      await k.page.keyboard.insertText(big);
      const f = await typedFacts(k, p.name);
      if (f.value !== '') faults.push(t + ':line_after_the_refusal=' + Buffer.byteLength(f.value));
      shows(t + '_refused', f, 'too_long');
      await k.page.keyboard.type('a');
      const g = await typedFacts(k, p.name);
      if (g.value !== 'a') faults.push(t + ':line_after_a_letter=' + Buffer.byteLength(g.value));
      shows(t + '_taken(the_sentence_stands_until_a_removal)', g, 'too_long');
    });
  }
  const inside = 'a_removal_inside_a_full_line_is_taken';
  await one(inside, async (k, l) => {
    await l.fill(digits);
    await l.evaluate((x) => x.setSelectionRange(700, 700));
    await k.page.keyboard.press('Backspace');
    const afterBack = digits.slice(0, 699) + digits.slice(700);
    let f = await typedFacts(k, p.name);
    if (f.value !== afterBack) faults.push(inside + ':backspace=' + f.value.length);
    await k.page.keyboard.press('Delete');
    const afterDelete = afterBack.slice(0, 699) + afterBack.slice(700);
    f = await typedFacts(k, p.name);
    if (f.value !== afterDelete) faults.push(inside + ':delete=' + f.value.length);
    await k.page.keyboard.type('z');
    f = await typedFacts(k, p.name);
    if (f.value !== afterDelete.slice(0, 699) + 'z' + afterDelete.slice(699)) faults.push(inside + ':letter=' + f.value.length);
    shows(inside, f, false);
  });
  const script = 'an_input_with_no_beforeinput_over_a_selection_at_the_bound';
  await one(script, async (k, l) => {
    await l.fill(digits);
    await k.page.keyboard.type('z');
    await l.evaluate((x) => x.setSelectionRange(600, 610));
    await l.evaluate((x) => {
      x.value = x.value.slice(0, 600) + 'z' + x.value.slice(610);
      x.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const f = await typedFacts(k, p.name);
    if (f.value !== digits) faults.push(script + ':line_changed');
    if (JSON.stringify(f.caret) !== '[600,610]') faults.push(script + ':selection=' + f.caret.join('-'));
    shows(script, f, true);
  });
  const saidNow = (k) => k.page.evaluate(() => [...document.querySelector('[role="status"]').childNodes].map((n) => n.textContent));
  for (const [n, letterTaken] of [
    [1020, true],
    [1021, false],
  ]) {
    const t = 'the_bound_after_a_refusal_on_' + n + '_bytes';
    await one(t, async (k, l) => {
      await l.fill(xs(n));
      await k.page.keyboard.insertText(big);
      shows(t + '_refused', await typedFacts(k, p.name), letterTaken ? 'too_long' : 'full');
      await k.page.keyboard.type('a');
      const f = await typedFacts(k, p.name);
      if (f.value !== (letterTaken ? xs(n) + 'a' : xs(n))) faults.push(t + ':letter_' + (letterTaken ? 'not_taken' : 'taken'));
    });
  }
  const below = 'a_refusal_below_the_bound_refuses_only_itself';
  await one(below, async (k, l) => {
    await l.fill(xs(10));
    await k.page.keyboard.insertText(big);
    let f = await typedFacts(k, p.name);
    if (f.value !== xs(10)) faults.push(below + ':line_after_the_refusal=' + Buffer.byteLength(f.value));
    shows(below + '_refused', f, 'too_long');
    const said = await saidNow(k);
    if (JSON.stringify(said) !== JSON.stringify([p.too_long])) faults.push(below + ':status_element_holds=' + said.length);
    await k.page.keyboard.type('a');
    f = await typedFacts(k, p.name);
    if (f.value !== xs(10) + 'a') faults.push(below + ':letter_not_taken');
    shows(below + '_after_a_taken_letter', f, 'too_long');
    await k.page.keyboard.press('Backspace');
    f = await typedFacts(k, p.name);
    if (f.value !== xs(10)) faults.push(below + ':removal_not_taken');
    shows(below + '_after_a_removal', f, false);
  });
  const flag = 'a_flag_in_one_input_on_1018_bytes_then_four_digits(the_limit_of_1_6_a,pinned_as_ruled)';
  {
    const out = outDir();
    const h = await open({ origin, state: 'setup_three', views: [VIEW], out });
    try {
      await h.page.locator(line).fill(xs(1018));
      await h.page.keyboard.insertText(cp(0x1f1eb, 0x1f1f7));
      await h.page.keyboard.type('1234');
      const f = await typedFacts(h, p.name);
      if (f.value !== xs(1018) + '1234') faults.push(flag + ':line_ends_in=' + f.value.slice(-4));
      shows(flag, f, 'too_long');
      const sent = await sentOf(h, out);
      if (sent !== xs(1018) + '1234') faults.push(flag + ':sent=' + (sent === null ? 'none' : Buffer.byteLength(sent)));
    } catch {
      faults.push(flag + ':throws');
    } finally {
      await h.close();
    }
  }
  const atb = 'at_the_bound_only_a_removal_changes_the_line';
  await one(atb, async (k, l) => {
    await l.fill(xs(1024));
    await k.page.keyboard.type('a');
    shows(atb + '_full', await typedFacts(k, p.name), 'full');
    await l.evaluate((x) => x.setSelectionRange(0, 1024));
    await k.page.keyboard.insertText('short');
    let f = await typedFacts(k, p.name);
    if (f.value !== xs(1024)) faults.push(atb + ':whole_line_replaced');
    await l.evaluate((x) => x.setSelectionRange(1024, 1024));
    await k.page.keyboard.press('Backspace');
    shows(atb + '_after_a_removal', await typedFacts(k, p.name), false);
    await k.page.keyboard.type('b');
    f = await typedFacts(k, p.name);
    if (f.value !== xs(1023) + 'b') faults.push(atb + ':letter_after_the_removal_not_taken');
  });
  const reach = 'a_line_with_too_long_reaches_the_bound_and_shows_full';
  await one(reach, async (k, l) => {
    await l.fill(xs(10));
    await k.page.keyboard.insertText(big);
    shows(reach + '_too_long', await typedFacts(k, p.name), 'too_long');
    await k.page.keyboard.insertText(xs(1014));
    let f = await typedFacts(k, p.name);
    if (f.value !== xs(1024)) faults.push(reach + ':fill_not_taken=' + Buffer.byteLength(f.value));
    shows(reach + '_still_too_long', f, 'too_long');
    await k.page.keyboard.type('a');
    f = await typedFacts(k, p.name);
    shows(reach + '_full', f, 'full');
    const said = await saidNow(k);
    if (JSON.stringify(said) !== JSON.stringify([p.full])) faults.push(reach + ':status_element_holds=' + said.length);
  });
  return faults;
}

async function compose(origin, dir) {
  const faults = [];
  const item = stateFile(dir, 'setup_three').snapshot.items[0];
  const p = item.params.find((x) => x.typed === 'yes');
  const line = '[data-act="typed"][data-param="' + p.name + '"]';
  const choose = '[data-act="choose"][data-param="' + item.item + '"]';
  const sentence = (f) => (f.note === p.full ? 'full' : f.note === p.too_long ? 'too_long' : f.note === null ? 'none' : 'other');
  const run = async (tag, fn) => {
    const out = outDir();
    const h = await open({ origin, state: 'setup_three', views: [VIEW], out });
    try {
      await fn(h, out);
    } catch (err) {
      faults.push(tag + ':throws');
    } finally {
      await h.close();
    }
  };
  const typedCalls = (out) => callsOf(out).filter((c) => c.call === 'agents_setup_typed').map((c) => c.args.text);
  const scripted = (h, plan) =>
    h.page.evaluate(
      ([sel, plan, order]) => {
        const t = document.querySelector(sel);
        t.focus();
        if (plan.caret !== undefined) t.setSelectionRange(plan.caret, plan.caret);
        const at = t.selectionStart;
        const base = t.value;
        const put = (s) => {
          t.value = base.slice(0, at) + s + base.slice(at);
          t.setSelectionRange(at + s.length, at + s.length);
        };
        t.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }));
        for (const s of plan.updates) {
          put(s);
          t.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: s }));
          t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: s, isComposing: true }));
        }
        const during = t.value;
        if (plan.open) return { during };
        if (plan.press) {
          document.querySelector(plan.press).dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, isPrimary: true }));
        }
        if (plan.cancel) {
          t.value = base;
          t.setSelectionRange(at, at);
        } else put(plan.commit);
        const end = () => t.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: plan.cancel ? '' : plan.commit }));
        const last = () => t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: plan.cancel ? '' : plan.commit, isComposing: order === 'chromium' }));
        if (order === 'chromium') {
          last();
          end();
        } else {
          end();
          last();
        }
        if (plan.press) {
          document.querySelector(plan.press).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
        }
        return { during };
      },
      [line, plan, engineOf(h)],
    );
  const facts = (h) => typedFacts(h, p.name);
  const kana = (...c) => cp(...c);
  const over = { caret: 500, updates: [kana(0x304b), kana(0x304b, 0x306a), kana(0x304b, 0x306a, 0x304c), kana(0x304b, 0x306a, 0x304c, 0x308f)], commit: kana(0x304b, 0x306a, 0x304c, 0x308f) };
  const spellings = [0x304b, 0x306a, 0x304c, 0x308f, 0x3051, 0x3093];
  const spelling = {
    updates: spellings.map((_, i) => cp(...spellings.slice(0, i + 1))),
    commit: cp(0x795e, 0x5948, 0x5ddd, 0x770c),
  };
  await run('a_commit_that_would_end_above_the_bound_is_refused', async (h, out) => {
    const t = 'a_commit_that_would_end_above_the_bound_is_refused';
    await h.page.locator(line).fill(xs(1018));
    await scripted(h, over);
    const f = await facts(h);
    if (f.value !== xs(1018)) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
    if (JSON.stringify(f.caret) !== '[500,500]') faults.push(t + ':selection=' + f.caret.join('-'));
    if (sentence(f) !== 'too_long') faults.push(t + ':sentence=' + sentence(f));
    await act(h, { act: 'choose', param: item.item });
    if (typedCalls(out).length !== 0) faults.push(t + ':the_press_after_the_end_sent=' + typedCalls(out).length);
    const back = await facts(h);
    if (!(await h.page.evaluate((s) => document.activeElement === document.querySelector(s), line))) faults.push(t + ':focus_not_on_the_line');
    if (JSON.stringify(back.caret) !== '[500,500]') faults.push(t + ':selection_after_the_stop=' + back.caret.join('-'));
    await act(h, { act: 'choose', param: item.item });
    const sent = typedCalls(out);
    if (sent.length !== 1 || sent[0] !== xs(1018)) faults.push(t + ':sent=' + sent.map((s) => Buffer.byteLength(s)).join(','));
  });
  await run('a_word_that_fits_is_taken_although_its_spelling_passed_the_bound', async (h) => {
    const t = 'a_word_that_fits_is_taken_although_its_spelling_passed_the_bound';
    await h.page.locator(line).fill(xs(1010));
    const r = await scripted(h, { ...spelling, open: true });
    if (r.during !== xs(1010) + spelling.updates[5]) faults.push(t + ':line_written_during_the_composition=' + Buffer.byteLength(r.during));
    const g = await facts(h);
    if (g.value !== xs(1010) + spelling.updates[5] || sentence(g) !== 'none') faults.push(t + ':changed_during_the_composition=' + Buffer.byteLength(g.value) + ':' + sentence(g));
    await h.page.evaluate(([sel, commit, order]) => {
      const x = document.querySelector(sel);
      x.value = 'x'.repeat(1010) + commit;
      x.setSelectionRange(x.value.length, x.value.length);
      const end = () => x.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: commit }));
      const last = () => x.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: commit, isComposing: order === 'chromium' }));
      if (order === 'chromium') {
        last();
        end();
      } else {
        end();
        last();
      }
    }, [line, spelling.commit, engineOf(h)]);
    const f = await facts(h);
    if (f.value !== xs(1010) + spelling.commit) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
    if (sentence(f) !== 'none') faults.push(t + ':sentence=' + sentence(f));
  });
  await run('a_cancelled_composition_leaves_the_line', async (h) => {
    const t = 'a_cancelled_composition_leaves_the_line';
    await h.page.locator(line).fill(xs(1018));
    await scripted(h, { updates: [kana(0x304b)], cancel: true });
    const f = await facts(h);
    if (f.value !== xs(1018) || sentence(f) !== 'none') faults.push(t + ':line=' + Buffer.byteLength(f.value) + ':' + sentence(f));
  });
  for (const [n, taken] of [
    [1022, true],
    [1023, false],
  ]) {
    const t = 'a_dead_key_on_' + n + '_bytes';
    await run(t, async (h) => {
      await h.page.locator(line).fill(xs(n));
      await scripted(h, { updates: [cp(0xb4)], commit: cp(0xe9) });
      const f = await facts(h);
      if (f.value !== (taken ? xs(n) + cp(0xe9) : xs(n))) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
      if (sentence(f) !== (taken ? 'none' : 'full')) faults.push(t + ':sentence=' + sentence(f));
      if (!taken) {
        await h.page.keyboard.type('a');
        const g = await facts(h);
        if (g.value !== xs(n)) faults.push(t + ':no_mark');
      }
    });
  }
  await run('a_commit_in_two_parts_keeps_the_first', async (h) => {
    const t = 'a_commit_in_two_parts_keeps_the_first';
    await h.page.locator(line).fill(xs(1015));
    await scripted(h, { updates: [kana(0x304b), kana(0x304b, 0x306a)], commit: kana(0x304b, 0x306a) });
    await scripted(h, { updates: [kana(0x304c), kana(0x304c, 0x308f)], commit: kana(0x304c, 0x308f) });
    const f = await facts(h);
    if (f.value !== xs(1015) + kana(0x304b, 0x306a)) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
    if (sentence(f) !== 'full') faults.push(t + ':sentence=' + sentence(f));
  });
  await run('a_refused_commit_on_1015_bytes_then_a_commit_of_3(the_limit_of_1_6_a)', async (h) => {
    const t = 'a_refused_commit_on_1015_bytes_then_a_commit_of_3(the_limit_of_1_6_a)';
    await h.page.locator(line).fill(xs(1015));
    await scripted(h, { updates: spelling.updates.slice(0, 4), commit: spelling.commit });
    await scripted(h, { updates: [kana(0x304b)], commit: kana(0x304b) });
    const f = await facts(h);
    if (f.value !== xs(1015) + kana(0x304b)) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
    if (sentence(f) !== 'too_long') faults.push(t + ':sentence=' + sentence(f));
  });
  await run('a_press_with_a_composition_open_sends_nothing', async (h, out) => {
    const t = 'a_press_with_a_composition_open_sends_nothing';
    await h.page.locator(line).fill('abc');
    await scripted(h, { caret: 3, updates: [kana(0x304b)], open: true });
    await h.page.evaluate((sel) => document.querySelector(sel).click(), choose);
    await settled(h);
    if (typedCalls(out).length !== 0) faults.push(t + ':calls=' + typedCalls(out).length);
    if (!(await h.page.evaluate((s) => document.activeElement === document.querySelector(s), line))) faults.push(t + ':focus_not_on_the_line');
  });
  await run('a_press_in_which_a_composition_end_was_refused_sends_nothing', async (h, out) => {
    const t = 'a_press_in_which_a_composition_end_was_refused_sends_nothing';
    await h.page.locator(line).fill(xs(1018));
    await scripted(h, { ...over, press: choose });
    await settled(h);
    if (typedCalls(out).length !== 0) faults.push(t + ':calls=' + typedCalls(out).length);
    if (!(await h.page.evaluate((s) => document.activeElement === document.querySelector(s), line))) faults.push(t + ':focus_not_on_the_line');
    await act(h, { act: 'choose', param: item.item });
    const sent = typedCalls(out);
    if (sent.length !== 1 || sent[0] !== xs(1018)) faults.push(t + ':the_next_press=' + sent.length);
  });
  await run('the_send_takes_the_kept_line_and_never_the_field', async (h, out) => {
    const t = 'the_send_takes_the_kept_line_and_never_the_field';
    await h.page.locator(line).fill('abc');
    await h.page.evaluate((sel) => {
      document.querySelector(sel).value = 'abcdef';
    }, line);
    await act(h, { act: 'choose', param: item.item });
    const sent = typedCalls(out);
    if (sent.length !== 1 || sent[0] !== 'abc') faults.push(t + ':sent=' + JSON.stringify(sent));
  });
  const ime = 'by_the_protocol';
  await run(ime, async (h, out) => {
    if (engineOf(h) !== 'chromium') {
      console.log('compose protocol: not run on ' + engineOf(h) + ', which takes no composition through the browser\'s protocol in this harness');
      return;
    }
    const cdp = await h.page.context().newCDPSession(h.page);
    const send = async (plan) => {
      for (const s of plan.updates) await cdp.send('Input.imeSetComposition', { text: s, selectionStart: s.length, selectionEnd: s.length });
      await cdp.send('Input.insertText', { text: plan.commit });
    };
    await h.page.locator(line).fill(xs(1018));
    await h.page.locator(line).evaluate((x) => x.setSelectionRange(500, 500));
    await send(over);
    let f = await facts(h);
    if (f.value !== xs(1018)) faults.push(ime + '_over:line_holds=' + Buffer.byteLength(f.value));
    if (JSON.stringify(f.caret) !== '[500,500]') faults.push(ime + '_over:selection=' + f.caret.join('-'));
    if (sentence(f) !== 'too_long') faults.push(ime + '_over:sentence=' + sentence(f));
    await act(h, { act: 'choose', param: item.item });
    if (typedCalls(out).length !== 0) faults.push(ime + '_over:the_press_after_the_end_sent=' + typedCalls(out).length);
    if (!(await h.page.evaluate((s) => document.activeElement === document.querySelector(s), line))) faults.push(ime + '_over:focus_not_on_the_line');
    await act(h, { act: 'choose', param: item.item });
    const sent = typedCalls(out);
    if (sent.length !== 1 || sent[0] !== xs(1018)) faults.push(ime + '_over:sent=' + sent.length);
    await h.page.locator(line).fill(xs(1010));
    await h.page.locator(line).evaluate((x) => x.setSelectionRange(1010, 1010));
    await send(spelling);
    f = await facts(h);
    if (f.value !== xs(1010) + spelling.commit) faults.push(ime + '_spelling:line_holds=' + Buffer.byteLength(f.value));
  });
  return faults;
}

const KANA2 = String.fromCodePoint(0x304b, 0x306a);
const KANA4 = String.fromCodePoint(0x304b, 0x306a, 0x304c, 0x308f);

/** Whether a case of press or holds is left out: R4_CASES, when set, names the prefixes of the cases to run. */
function skipCase(tag) {
  const only = process.env.R4_CASES;
  return Boolean(only) && !only.split(',').some((x) => tag.startsWith(x));
}

/** Page helpers of the press and hold cases: a composition, its end and a press, made by a script. */
function pageKit(h, engine) {
  const run = (fn, arg) => h.page.evaluate(fn, arg);
  return {
    begin: (sel, updates) =>
      run(([s, ups]) => {
        const t = document.querySelector(s);
        t.focus();
        t.r4at = t.selectionStart;
        t.r4base = t.value;
        t.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }));
        for (const u of ups) {
          t.value = t.r4base.slice(0, t.r4at) + u + t.r4base.slice(t.r4at);
          t.setSelectionRange(t.r4at + u.length, t.r4at + u.length);
          t.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: u }));
          t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: u, isComposing: true }));
        }
      }, [sel, updates]),
    end: (sel, commit) =>
      run(([s, c, order]) => {
        const t = document.querySelector(s);
        const text = c === null ? '' : c;
        t.value = t.r4base.slice(0, t.r4at) + text + t.r4base.slice(t.r4at);
        t.setSelectionRange(t.r4at + text.length, t.r4at + text.length);
        const end = () => t.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: text }));
        const last = () => t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: text, isComposing: order === 'chromium' }));
        if (order === 'chromium') {
          last();
          end();
        } else {
          end();
          last();
        }
      }, [sel, commit, engine]),
    endOnBlur: (sel, commit) =>
      run(([s, c, order]) => {
        const t = document.querySelector(s);
        t.addEventListener(
          'blur',
          () => {
            t.value = t.r4base.slice(0, t.r4at) + c + t.r4base.slice(t.r4at);
            const end = () => t.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: c }));
            const last = () => t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: c, isComposing: order === 'chromium' }));
            if (order === 'chromium') {
              last();
              end();
            } else {
              end();
              last();
            }
          },
          { once: true },
        );
      }, [sel, commit, engine]),
    input: (sel, value) =>
      run(([s, v]) => {
        const t = document.querySelector(s);
        t.value = v;
        t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: 'x' }));
      }, [sel, value]),
    event: (type, init) =>
      run(([ty, i]) => {
        const e = ty.startsWith('pointer') ? new PointerEvent(ty, { bubbles: true, isPrimary: true, ...i }) : new KeyboardEvent(ty, { bubbles: true, ...i });
        document.body.dispatchEvent(e);
      }, [type, init || {}]),
    pointerDown: (sel) => run((s) => document.querySelector(s).dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, isPrimary: true })), sel),
    clickDetail1: (sel) => run((s) => document.querySelector(s).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 })), sel),
    click0: (sel) => run((s) => document.querySelector(s).click(), sel),
    focus: (sel) => run((s) => document.querySelector(s).focus(), sel),
    activeIs: (sel) => run((s) => document.activeElement === document.querySelector(s), sel),
    line: (sel) =>
      run((s) => {
        const t = document.querySelector(s);
        const id = t.getAttribute('aria-describedby');
        const note = id ? document.getElementById(id) : null;
        return { value: t.value, caret: [t.selectionStart, t.selectionEnd], note: note ? note.textContent : null };
      }, sel),
    says: () => run(() => [...document.querySelector('[role="status"]').childNodes].map((n) => n.textContent)),
  };
}

async function press(origin, dir) {
  const faults = [];
  const item = stateFile(dir, 'setup_three').snapshot.items[0];
  const p = item.params.find((x) => x.typed === 'yes');
  const line = '[data-act="typed"][data-param="' + p.name + '"]';
  const choose = '[data-act="choose"][data-param="' + item.item + '"]';
  const typedSent = (out) => callsOf(out).filter((c) => c.call === 'agents_setup_typed').map((c) => c.args.text);
  const bytesOf = (list) => list.map((s) => Buffer.byteLength(s)).join(',') || '-';
  const run = async (tag, fn, opts) => {
    if (skipCase(tag)) return;
    const out = outDir();
    const h = await open({ origin, views: [VIEW], out, ...(opts || { state: 'setup_three' }) });
    try {
      await fn(h, out, pageKit(h, engineOf(h)));
    } catch (err) {
      faults.push(tag + ':throws=' + String((err && err.message) || err).split('\n')[0].slice(0, 80));
    } finally {
      await h.close();
    }
  };
  const expectSent = (tag, out, want) => {
    const got = typedSent(out);
    if (JSON.stringify(got.map((s) => Buffer.byteLength(s))) !== JSON.stringify(want)) faults.push(tag + ':sent=' + bytesOf(got));
  };
  const mousePress = async (h, sel) => {
    const box = await h.page.locator(sel).boundingBox();
    await h.page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await settled(h);
  };
  const protocolOnly = (h, tag) => {
    if (engineOf(h) === 'chromium') return false;
    console.log('press ' + tag + ': not run on ' + engineOf(h) + ', which takes no composition through the browser\'s protocol in this harness');
    return true;
  };

  await run('p1_a_pointer_press_through_the_protocol_on_an_open_composition', async (h, out) => {
    const t = 'p1_a_pointer_press_through_the_protocol_on_an_open_composition';
    if (protocolOnly(h, t)) return;
    await h.page.locator(line).fill(xs(1000));
    await h.page.evaluate((s) => {
      window.r4order = [];
      const t = document.querySelector(s);
      t.addEventListener('compositionend', () => window.r4order.push('compositionend'), true);
      for (const ty of ['pointerdown', 'pointerup', 'click']) window.addEventListener(ty, () => window.r4order.push(ty), true);
    }, line);
    const cdp = await h.page.context().newCDPSession(h.page);
    for (const s of [KANA2.slice(0, 1), KANA2]) await cdp.send('Input.imeSetComposition', { text: s, selectionStart: s.length, selectionEnd: s.length });
    await mousePress(h, choose);
    const order = await h.page.evaluate(() => window.r4order.join(','));
    console.log('press p1 order=' + order);
    const kit = pageKit(h, engineOf(h));
    const f = await kit.line(line);
    expectSent(t + '_first', out, []);
    if (Buffer.byteLength(f.value) !== 1006) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
    if (!(await kit.activeIs(line))) faults.push(t + ':focus_not_on_the_line');
    await mousePress(h, choose);
    expectSent(t + '_second', out, [1006]);
  });
  await run('p2_a_press_through_the_protocol_on_a_composition_that_does_not_fit(pin)', async (h, out) => {
    const t = 'p2_a_press_through_the_protocol_on_a_composition_that_does_not_fit(pin)';
    if (protocolOnly(h, t)) return;
    await h.page.locator(line).fill(xs(1018));
    const cdp = await h.page.context().newCDPSession(h.page);
    for (let i = 1; i <= 4; i++) await cdp.send('Input.imeSetComposition', { text: KANA4.slice(0, i), selectionStart: i, selectionEnd: i });
    await mousePress(h, choose);
    expectSent(t + '_first', out, []);
    const f = await pageKit(h, engineOf(h)).line(line);
    if (f.note !== p.too_long) faults.push(t + ':sentence=' + (f.note === null ? 'none' : 'other'));
    await mousePress(h, choose);
    expectSent(t + '_second', out, [1018]);
  });
  await run('p3_a_pointer_press_through_the_protocol_ends_a_script_composition', async (h, out, k) => {
    const t = 'p3_a_pointer_press_through_the_protocol_ends_a_script_composition';
    await h.page.locator(line).fill(xs(1000));
    await k.begin(line, [KANA2.slice(0, 1), KANA2]);
    await k.endOnBlur(line, KANA2);
    await mousePress(h, choose);
    expectSent(t + '_first', out, []);
    const f = await k.line(line);
    if (f.value !== xs(1000) + KANA2) faults.push(t + ':line_holds=' + Buffer.byteLength(f.value));
    if (JSON.stringify(f.caret) !== '[1002,1002]') faults.push(t + ':selection=' + f.caret.join('-'));
    if (!(await k.activeIs(line))) faults.push(t + ':focus_not_on_the_line');
    await mousePress(h, choose);
    expectSent(t + '_second', out, [1006]);
  });
  await run('p3_the_same_order_by_a_script', async (h, out, k) => {
    const t = 'p3_the_same_order_by_a_script';
    await h.page.locator(line).fill(xs(1000));
    await k.begin(line, [KANA2]);
    await k.pointerDown(choose);
    await k.focus(choose);
    await k.end(line, KANA2);
    await k.clickDetail1(choose);
    await settled(h);
    expectSent(t + '_first', out, []);
    if (!(await k.activeIs(line))) faults.push(t + ':focus_not_on_the_line');
    await k.pointerDown(choose);
    await k.clickDetail1(choose);
    await settled(h);
    expectSent(t + '_second', out, [1006]);
  });
  for (const [n, commit, sent] of [
    [1000, KANA2, 1006],
    [1018, KANA4, 1018],
  ]) {
    const t = 'p4_the_end_first_then_a_pointer_press_on_' + n + '_bytes';
    await run(t, async (h, out, k) => {
      await h.page.locator(line).fill(xs(n));
      await k.begin(line, [commit]);
      await k.end(line, commit);
      await k.pointerDown(choose);
      await k.clickDetail1(choose);
      await settled(h);
      expectSent(t + '_first', out, []);
      if (n === 1018) {
        const f = await k.line(line);
        if (f.note !== p.too_long) faults.push(t + ':too_long_gone_before_the_send');
      }
      await k.pointerDown(choose);
      await k.clickDetail1(choose);
      await settled(h);
      expectSent(t + '_second', out, [sent]);
      if (n === 1018 && (await k.line(line)).note !== null) faults.push(t + ':too_long_stands_after_the_send');
    });
  }
  await run('p5_the_order_of_an_assistive_press', async (h, out, k) => {
    const t = 'p5_the_order_of_an_assistive_press';
    await h.page.locator(line).fill(xs(1000));
    await k.begin(line, [KANA2]);
    await k.focus(choose);
    await k.end(line, KANA2);
    await k.click0(choose);
    await settled(h);
    expectSent(t + '_first', out, []);
    if (!(await k.activeIs(line))) faults.push(t + ':focus_not_on_the_line');
    await k.click0(choose);
    await settled(h);
    expectSent(t + '_second', out, [1006]);
  });
  await run('p6_a_refused_end_then_two_clicks_with_no_press', async (h, out, k) => {
    const t = 'p6_a_refused_end_then_two_clicks_with_no_press';
    await h.page.locator(line).fill(xs(1018));
    await k.begin(line, [KANA4]);
    await k.end(line, KANA4);
    await k.click0(choose);
    await settled(h);
    expectSent(t + '_first', out, []);
    await k.click0(choose);
    await settled(h);
    expectSent(t + '_second', out, [1018]);
  });
  await run('p7_a_cancelled_composition_then_two_clicks_with_no_press', async (h, out, k) => {
    const t = 'p7_a_cancelled_composition_then_two_clicks_with_no_press';
    await h.page.locator(line).fill('abc');
    await k.begin(line, [KANA2]);
    await k.end(line, null);
    await k.click0(choose);
    await settled(h);
    expectSent(t + '_first', out, []);
    await k.click0(choose);
    await settled(h);
    expectSent(t + '_second', out, [3]);
  });
  for (const [n, commit, sent, mark] of [
    [1018, KANA4, 1018, 'after_a_refused_end'],
    [1000, KANA2, 1006, 'after_a_taken_end(pin)'],
  ]) {
    const t = 'p8_a_commit_by_a_key_' + mark;
    await run(t, async (h, out, k) => {
      await h.page.locator(line).fill(xs(n));
      await k.begin(line, [commit]);
      await k.event('keydown', { key: 'Enter' });
      await k.end(line, commit);
      await k.event('keyup', { key: 'Enter' });
      await k.click0(choose);
      await settled(h);
      expectSent(t, out, [sent]);
    });
  }
  for (const [what, type, init, sends] of [
    ['a_pointerup(pin)', 'pointerup', {}, true],
    ['a_pointercancel(pin)', 'pointercancel', {}, true],
    ['a_keydown_that_is_a_repeat', 'keydown', { key: 'a', repeat: true }, false],
  ]) {
    const t = 'p9_after_a_taken_end_' + what;
    await run(t, async (h, out, k) => {
      await h.page.locator(line).fill(xs(1000));
      await k.begin(line, [KANA2]);
      await k.end(line, KANA2);
      await k.event(type, init);
      await k.click0(choose);
      await settled(h);
      expectSent(t, out, sends ? [1006] : []);
    });
  }
  await run('p10_the_check_at_the_click_stays(pin)', async (h, out, k) => {
    const t = 'p10_the_check_at_the_click_stays(pin)';
    await h.page.locator(line).fill('abc');
    await k.pointerDown(choose);
    await k.begin(line, [KANA2]);
    await k.clickDetail1(choose);
    await settled(h);
    expectSent(t, out, []);
  });
  const two = withId(item, 'e'.repeat(32));
  two.agent = 'f'.repeat(32);
  writeState(dir, 'test_two_setups', [item, two]);
  const lineIn = (id) => 'li:has([data-param="' + id + '"]) ' + line;
  const chooseOf = (id) => '[data-act="choose"][data-param="' + id + '"]';
  await run(
    'p11_two_cards',
    async (h, out, k) => {
      const t = 'p11_two_cards';
      await h.page.locator(lineIn(item.item)).fill(xs(1018));
      await h.page.locator(lineIn(two.item)).fill('abc');
      await k.begin(lineIn(item.item), [KANA4]);
      await k.end(lineIn(item.item), KANA4);
      await k.click0(chooseOf(two.item));
      await settled(h);
      const calls = () => callsOf(out).filter((c) => c.call === 'agents_setup_typed').map((c) => c.args.item.slice(0, 1) + Buffer.byteLength(c.args.text));
      if (calls().join(',') !== 'e3') faults.push(t + ':after_the_second_card=' + calls().join(','));
      await k.click0(chooseOf(item.item));
      await settled(h);
      if (calls().join(',') !== 'e3') faults.push(t + ':first_card_first_click=' + calls().join(','));
      await k.click0(chooseOf(item.item));
      await settled(h);
      if (calls().join(',') !== 'e3,01018') faults.push(t + ':first_card_second_click=' + calls().join(','));
    },
    { state: 'test_two_setups' },
  );
  await run(
    'p11_a_stopped_click_on_one_card_leaves_the_other',
    async (h, out, k) => {
      const t = 'p11_a_stopped_click_on_one_card_leaves_the_other';
      for (const id of [item.item, two.item]) {
        await h.page.locator(lineIn(id)).fill(xs(1018));
        await k.begin(lineIn(id), [KANA4]);
        await k.end(lineIn(id), KANA4);
      }
      await k.click0(chooseOf(item.item));
      await settled(h);
      await k.click0(chooseOf(two.item));
      await settled(h);
      if (typedSent(out).length !== 0) faults.push(t + ':the_other_card_sent=' + typedSent(out).length);
      await k.click0(chooseOf(two.item));
      await settled(h);
      if (typedSent(out).length !== 1) faults.push(t + ':the_other_card_at_its_second_click=' + typedSent(out).length);
    },
    { state: 'test_two_setups' },
  );
  const gate = { call: 'agents_card', args: { agent: 'e'.repeat(32) }, reply: { error: 'busy' } };
  const typedLine = (text, extra) => ({ call: 'agents_setup_typed', args: { item: item.item, param: p.name, text }, ...extra });
  await run(
    'p12_a_card_that_waits_for_a_reply',
    async (h, out, k) => {
      const t = 'p12_a_card_that_waits_for_a_reply';
      await h.page.locator(line).fill(xs(1018));
      await k.click0(choose);
      await settled(h);
      await k.begin(line, [KANA4]);
      await k.end(line, KANA4);
      await k.click0(choose);
      await h.page.evaluate((agent) => window.Halvo.ipc.call('agents_card', { agent }), gate.args.agent);
      await settled(h);
      await frames(h);
      const before = typedSent(out).length;
      await k.click0(choose);
      await settled(h);
      if (typedSent(out).length !== before) faults.push(t + ':click_after_the_reply_sent');
      await k.click0(choose);
      await settled(h);
      if (typedSent(out).length !== before + 1) faults.push(t + ':no_call_at_the_next_click=' + typedSent(out).length);
    },
    { replies: [{ state: snap(1, [item]) }, typedLine(xs(1018), { hold: true }), gate, { state: snap(2, [item]) }, typedLine(xs(1018), { reply: { ok: 'yes' } })] },
  );
  await run('p13_a_commit_by_the_protocol_insert_text_then_a_pointer_press(limit_of_4_5,pinned_as_ruled)', async (h, out) => {
    const t = 'p13_a_commit_by_the_protocol_insert_text_then_a_pointer_press(limit_of_4_5,pinned_as_ruled)';
    if (protocolOnly(h, t)) return;
    await h.page.locator(line).fill(xs(1000));
    const cdp = await h.page.context().newCDPSession(h.page);
    await cdp.send('Input.imeSetComposition', { text: KANA2, selectionStart: 2, selectionEnd: 2 });
    await cdp.send('Input.insertText', { text: KANA2 });
    await mousePress(h, choose);
    expectSent(t + '_first', out, []);
    await mousePress(h, choose);
    expectSent(t + '_second', out, [1006]);
  });
  return faults;
}

async function holds(origin, dir) {
  const faults = [];
  const setup = stateFile(dir, 'setup_three').snapshot.items[0];
  const p = setup.params.find((x) => x.typed === 'yes');
  const line = '[data-act="typed"][data-param="' + p.name + '"]';
  const stuck = told(withId(stateFile(dir, 'needs_stuck').snapshot.items[0], 'b'.repeat(32)), 'Stuck');
  const ready = told(withId(stateFile(dir, 'needs_ready').snapshot.items[0], 'c'.repeat(32)), 'Ready');
  const confirm = told(withId(stateFile(dir, 'needs_confirm_page').snapshot.items[0], 'a'.repeat(32)), 'Confirm');
  const second = told(withId(stateFile(dir, 'needs_confirm_page').snapshot.items[0], 'd'.repeat(32)), 'Second');
  const other = told(withId(stateFile(dir, 'needs_show').snapshot.items[0], '7'.repeat(32)), 'Other');
  const gate = { call: 'agents_card', args: { agent: 'e'.repeat(32) }, reply: { error: 'busy' } };
  const answerLine = (c, extra) => ({ call: 'agents_answer', args: { item: c.item, choice: 'reject' }, ...(extra || { reply: { ok: 'yes' } }) });
  const B = p.too_long;
  const waitIds = (h, items) =>
    h.page.waitForFunction(
      (want) => [...new Set([...document.querySelectorAll('#views [data-param]')].map((e) => e.getAttribute('data-param')))].filter((x) => /^[0-9a-f]{32}$/.test(x)).sort().join(',') === want,
      items.map((i) => i.item).sort().join(','),
      { timeout: 5000 },
    );
  const pass = async (h, items) => {
    await h.page.evaluate((agent) => window.Halvo.ipc.call('agents_card', { agent }), gate.args.agent);
    await waitIds(h, items);
    await frames(h);
  };
  const answerOpen = (h) =>
    h.page.evaluate(() => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="reject"]').click();
    });
  const afterClose = async (h) => {
    await frames(h);
    await settled(h);
    await frames(h);
  };
  const flow = async (tag, replies, fn) => {
    if (skipCase(tag)) return;
    const h = await open({ origin, replies, views: [VIEW] });
    try {
      await fn(h, pageKit(h, engineOf(h)));
    } catch (err) {
      faults.push(tag + ':throws=' + String((err && err.message) || err).split('\n')[0].slice(0, 80));
    } finally {
      await h.close();
    }
    if (process.env.R4_CASES) console.log('holds case ran ' + tag);
  };
  const expect = (tag, got, want) => {
    if (JSON.stringify(got) !== JSON.stringify(want)) faults.push(tag + ':says=' + (got.map((s) => (s === B ? 'B' : s === p.full ? 'A' : s)).join('|') || '-'));
  };
  const refuse = async (k) => {
    await k.begin(line, [KANA4]);
    await k.end(line, KANA4);
  };

  const h1 = 'h1_a_refusal_while_a_confirmation_is_open';
  await flow(h1, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, answerLine(confirm), { state: snap(3, [setup]) }, gate, { state: snap(4, [setup, second]) }, answerLine(second)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm]);
    await refuse(k);
    expect(h1 + '_while_open', await k.says(), []);
    const inTask = await h.page.evaluate(() => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="reject"]').click();
      return [...document.querySelector('[role="status"]').childNodes].map((n) => n.textContent);
    });
    expect(h1 + '_in_the_closing_task', inTask, []);
    await afterClose(h);
    await waitIds(h, [setup]);
    await afterClose(h);
    expect(h1 + '_after_the_close_and_the_reply', await k.says(), [B]);
    await pass(h, [setup, second]);
    await answerOpen(h);
    await afterClose(h);
    if ((await k.says()).includes(B)) faults.push(h1 + ':a_second_confirmation_adds_the_line_again');
  });
  const h2 = 'h2_a_refusal_and_items_that_came_and_went';
  await flow(h2, [{ state: snap(1, [setup, stuck]) }, gate, { state: snap(2, [setup, stuck, confirm]) }, gate, { state: snap(3, [setup, confirm, ready]) }, answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, stuck, confirm]);
    await refuse(k);
    await pass(h, [setup, confirm, ready]);
    await answerOpen(h);
    await afterClose(h);
    expect(h2, await k.says(), [B, 'Stuck went.', 'Ready came.']);
  });
  const h3 = 'h3_a_confirmation_dropped_with_no_answer';
  await flow(h3, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, gate, { state: snap(3, [setup]) }], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm]);
    await refuse(k);
    await pass(h, [setup]);
    await afterClose(h);
    expect(h3, await k.says(), [B, 'Confirm went.']);
  });
  const h4 = 'h4_a_card_built_again_during_the_hold(pin)';
  const setupChanged = { ...setup, text: setup.text + ' Again.' };
  await flow(h4, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, gate, { state: snap(3, [setupChanged, confirm]) }, answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm]);
    await refuse(k);
    await pass(h, [setupChanged, confirm]);
    await answerOpen(h);
    await afterClose(h);
    if ((await k.says()).includes(B)) faults.push(h4 + ':the_old_line_is_added');
  });
  const h5 = 'h5_a_removal_during_the_hold(pin)';
  await flow(h5, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm]);
    await refuse(k);
    await k.input(line, xs(1017));
    await answerOpen(h);
    await afterClose(h);
    if ((await k.says()).includes(B)) faults.push(h5 + ':the_removed_sentence_is_added');
  });
  const h6 = 'h6_the_confirmation_shown_again';
  await flow(h6, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, answerLine(confirm, { hold: true }), answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm]);
    await answerOpen(h);
    await afterClose(h);
    h.held.shift()({ error: 'busy' });
    await h.page.waitForFunction(() => Boolean([...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'))), null, { timeout: 5000 });
    await frames(h);
    await refuse(k);
    expect(h6 + '_while_shown_again', await k.says(), []);
    await answerOpen(h);
    await afterClose(h);
    expect(h6 + '_after_the_second_close(the_first_close_is_delivered_before_the_error,by_a_held_call)', await k.says(), [B]);
  });
  const h7 = 'h7_a_refusal_in_the_task_that_closes';
  await flow(h7, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm, ready]) }, answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm, ready]);
    await h.page.evaluate(([s, big]) => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="reject"]').click();
      const t = document.querySelector(s);
      t.value = t.value + big;
      t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste' }));
    }, [line, 'y'.repeat(30)]);
    await afterClose(h);
    expect(h7, await k.says(), [B, 'Ready came.']);
  });
  const two = stateFile(dir, 'test_setup').snapshot.items[0];
  const typed = two.params.filter((x) => x.typed === 'yes').map((x) => '[data-act="typed"][data-param="' + x.name + '"]');
  for (const [mark, firstFill, firstRefusal, want] of [
    ['h8_the_same_words_once', 'abc', 'abc' + 'y'.repeat(2000), [B]],
    ['h8_full_then_too_long_in_the_order_of_the_list', xs(1024), xs(1024) + 'a', [p.full, B]],
  ]) {
    await flow(mark, [{ state: snap(1, [two]) }, gate, { state: snap(2, [two, confirm]) }, answerLine(confirm)], async (h, k) => {
      await h.page.locator(typed[0]).fill(firstFill);
      await h.page.locator(typed[1]).fill('abc');
      await pass(h, [two, confirm]);
      await k.input(typed[0], firstRefusal);
      await k.input(typed[1], 'abc' + 'y'.repeat(2000));
      await answerOpen(h);
      await afterClose(h);
      expect(mark, await k.says(), want);
    });
  }
  const h9 = 'h9_an_item_that_came_with_a_confirmation';
  await flow(h9, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm, ready]) }, answerLine(confirm), gate, { state: snap(3, [stuck, ready]) }], async (h, k) => {
    await pass(h, [stuck, confirm, ready]);
    await answerOpen(h);
    await afterClose(h);
    await pass(h, [stuck, ready]);
    await afterClose(h);
    expect(h9 + '_after_the_reply_without_the_answered_confirmation', await k.says(), ['Ready came.']);
  });
  const h9b = 'h9_a_reply_without_the_answered_confirmation_and_with_another_item';
  await flow(h9b, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm]) }, answerLine(confirm), gate, { state: snap(3, [stuck, other]) }], async (h, k) => {
    await pass(h, [stuck, confirm]);
    await answerOpen(h);
    await afterClose(h);
    await pass(h, [stuck, other]);
    expect(h9b, await k.says(), ['Other came.']);
  });
  const h10 = 'h10_a_composition_through_the_protocol_when_a_confirmation_opens';
  await flow(h10, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, answerLine(confirm)], async (h, k) => {
    if (engineOf(h) !== 'chromium') {
      console.log('hold h10: not run on ' + engineOf(h) + ', which takes no composition through the browser\'s protocol in this harness');
      return;
    }
    await h.page.locator(line).fill(xs(1018));
    const cdp = await h.page.context().newCDPSession(h.page);
    for (let i = 1; i <= 4; i++) await cdp.send('Input.imeSetComposition', { text: KANA4.slice(0, i), selectionStart: i, selectionEnd: i });
    await pass(h, [setup, confirm]);
    await h.page.keyboard.press(h.build.walkKey);
    await h.page.keyboard.press('Enter');
    await afterClose(h);
    expect(h10, await k.says(), [B]);
    const f = await k.line(line);
    if (Buffer.byteLength(f.value) !== 1018) faults.push(h10 + ':line_holds=' + Buffer.byteLength(f.value));
  });
  const h11 = 'h11_a_sentence_from_before_the_hold(pin)';
  await flow(h11, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await refuse(k);
    expect(h11 + '_before', await k.says(), [B]);
    await pass(h, [setup, confirm]);
    await answerOpen(h);
    await afterClose(h);
    if ((await k.says()).includes(B)) faults.push(h11 + ':added_at_the_release');
  });
  const h12 = 'h12_a_reply_that_drops_an_answered_confirmation_and_another_item';
  await flow(h12, [{ state: snap(1, [stuck, ready]) }, gate, { state: snap(2, [stuck, ready, confirm]) }, answerLine(confirm), gate, { state: snap(3, [stuck]) }], async (h, k) => {
    await pass(h, [stuck, ready, confirm]);
    await answerOpen(h);
    await afterClose(h);
    await pass(h, [stuck]);
    await afterClose(h);
    expect(h12, await k.says(), ['Ready went.']);
  });
  const h13 = 'h13_a_refusal_in_the_task_that_closes_a_confirmation_shown_again';
  await flow(h13, [{ state: snap(1, [setup]) }, gate, { state: snap(2, [setup, confirm]) }, answerLine(confirm, { hold: true }), answerLine(confirm)], async (h, k) => {
    await h.page.locator(line).fill(xs(1018));
    await pass(h, [setup, confirm]);
    await answerOpen(h);
    await afterClose(h);
    h.held.shift()({ error: 'busy' });
    await h.page.waitForFunction(() => Boolean([...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'))), null, { timeout: 5000 });
    await frames(h);
    const inTask = await h.page.evaluate(([s, more]) => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="reject"]').click();
      const t = document.querySelector(s);
      t.value = t.value + more;
      t.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste' }));
      return [...document.querySelector('[role="status"]').childNodes].map((n) => n.textContent);
    }, [line, 'y'.repeat(30)]);
    expect(h13 + '_in_the_closing_task', inTask, []);
    await afterClose(h);
    expect(h13 + '_after_the_close', await k.says(), [B]);
  });
  const h14 = 'h14_a_reply_that_drops_the_waiting_confirmation_before_the_first_close';
  await flow(h14, [{ state: snap(1, [stuck]) }, gate, { state: snap(2, [stuck, confirm, second]) }, answerLine(confirm), gate, { state: snap(3, [stuck]) }], async (h, k) => {
    await pass(h, [stuck, confirm, second]);
    await h.page.evaluate(() => {
      window.__r4closes = [];
      for (const d of document.querySelectorAll('[role="alertdialog"]')) {
        d.closest('li').addEventListener(
          'close',
          (e) => {
            if (!e.isTrusted || e.target !== d) return;
            e.stopImmediatePropagation();
            window.__r4closes.push(d);
          },
          true,
        );
      }
    });
    const heldIds = () => h.page.evaluate(() => window.__r4closes.map((d) => d.querySelector('[data-param]').getAttribute('data-param')));
    await answerOpen(h);
    await h.page.waitForFunction(() => window.__r4closes.length === 1, null, { timeout: 5000 });
    expect(h14 + '_the_first_close_held', await heldIds(), [confirm.item]);
    await pass(h, [stuck]);
    await h.page.waitForFunction(() => window.__r4closes.length === 2, null, { timeout: 5000 });
    expect(h14 + '_the_second_close_held', await heldIds(), [confirm.item, second.item]);
    const steps = await h.page.evaluate(() => {
      const read = () => [...document.querySelector('[role="status"]').childNodes].map((n) => n.textContent);
      const out = [read()];
      window.__r4closes.forEach((d) => {
        d.dispatchEvent(new Event('close'));
        out.push(read());
      });
      return out;
    });
    expect(h14 + '_before_the_closes', steps[0], []);
    expect(h14 + '_after_the_first_close', steps[1], ['Second went.']);
    expect(h14 + '_after_the_second_close', steps[2], ['Second went.']);
    await afterClose(h);
    expect(h14 + '_after_the_turn', await k.says(), ['Second went.']);
  });
  return faults;
}

const FEATURES = join(ROOT, 'docs', 'r4', 'page-features.txt');

async function features(origin) {
  if (!existsSync(FEATURES)) return ['no_list'];
  const faults = [];
  const lines = readFileSync(FEATURES, 'utf8').split('\n').filter((l) => l !== '');
  const allow = [];
  lines.forEach((line, i) => {
    if (line.startsWith('#')) return;
    const cells = line.split('\t');
    const [kind, name, version, test] = cells;
    if (cells.length !== 4 || !['allow', 'deny'].includes(kind) || !/^[a-z0-9_]{1,32}$/.test(name) || !/^\d+(\.\d+)*$/.test(version)) faults.push('form_line_' + (i + 1));
    else if (!lines[i - 1] || !lines[i - 1].startsWith('# ')) faults.push('no_comment_line_' + (i + 1));
    else if (kind === 'allow' && parseFloat(version) > 17) faults.push('above_17_' + name);
    else if (kind === 'allow') allow.push([name, test]);
  });
  const h = await open({ origin, state: 'empty', views: [VIEW] });
  try {
    for (const [name, test] of allow) {
      const ok = await h.page.evaluate(test).catch(() => false);
      seen.features += 1;
      if (ok !== true) faults.push('missing_' + name);
    }
  } finally {
    await h.close();
  }
  if (allow.length === 0) faults.push('no_allow_line');
  return faults;
}

const want = (name) => !onlyTests || onlyTests.includes(name);

const { dir, states: tracked, tests } = fixtures();
const all = [...tracked, ...tests];
const states = onlyStates ? all.filter((s) => onlyStates.includes(s)) : all;
if (want('lint')) report('lint', lint());
if (want('words')) report('words', itemsTrueToWords(tests, dir));
const PAGE_TESTS = ['render', 'calls', 'next', 'hidden', 'escape', 'once', 'hold', 'replies', 'setups', 'reorder', 'confirms', 'crossed', 'bound', 'features', 'region', 'compose', 'press', 'holds'];
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
    if (want('confirms')) report('confirms', await confirms(origin, dir), ['clicks', 'touches']);
    if (want('crossed')) report('crossed', await crossed(origin, dir));
    if (want('bound')) report('bound', await bound(origin, dir));
    if (want('features')) report('features', await features(origin), ['features']);
    if (want('region')) report('region', await region(origin, dir));
    if (want('compose')) report('compose', await compose(origin, dir));
    if (want('press')) report('press', await press(origin, dir));
    if (want('holds')) report('holds', await holds(origin, dir));
  }
}
const passed = results.filter((r) => r.ok).length;
console.log('needs tests=' + results.length + ' passed=' + passed + ' states=' + states.length);
process.exit(passed === results.length ? 0 : 1);
