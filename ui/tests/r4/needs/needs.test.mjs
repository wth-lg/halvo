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

async function bound(origin, dir) {
  const faults = [];
  const item = stateFile(dir, 'setup_three').snapshot.items[0];
  const p = item.params.find((x) => x.typed === 'yes');
  const cases = [
    ['bytes_1024', 'é'.repeat(512), 'é'.repeat(512)],
    ['bytes_1025', 'é'.repeat(512) + 'a', 'é'.repeat(512)],
    ['three_byte_edge', 'a'.repeat(1023) + '€', 'a'.repeat(1023)],
    ['four_byte_letters', '𝄞'.repeat(257), '𝄞'.repeat(256)],
    ['combining_accent', 'a'.repeat(1022) + 'é', 'a'.repeat(1022)],
    ['joined_emoji', 'a'.repeat(1016) + '\u{1F469}‍\u{1F4BB}', 'a'.repeat(1016)],
    ['flag', 'a'.repeat(1020) + '\u{1F1EB}\u{1F1F7}', 'a'.repeat(1020)],
    ['emoji_with_its_variation', 'a'.repeat(1021) + '❤️', 'a'.repeat(1021)],
  ];
  for (const [tag, typed, sent] of cases) {
    const out = outDir();
    const h = await open({ origin, state: 'setup_three', views: [VIEW], out });
    try {
      await h.page.locator('[data-act="typed"][data-param="' + p.name + '"]').fill(typed);
      const held = await h.page.locator('[data-act="typed"][data-param="' + p.name + '"]').inputValue();
      if (held !== sent) faults.push(tag + ':input_holds=' + Buffer.byteLength(held));
      await act(h, { act: 'choose', param: item.item });
      const c = callsOf(out).find((x) => x.call === 'agents_setup_typed');
      if (!c || c.args.text !== sent) faults.push(tag + ':sent=' + (c ? Buffer.byteLength(c.args.text) : 'none'));
    } finally {
      await h.close();
    }
  }
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
const PAGE_TESTS = ['render', 'calls', 'next', 'hidden', 'escape', 'once', 'hold', 'replies', 'setups', 'reorder', 'confirms', 'crossed', 'bound', 'features'];
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
  }
}
const passed = results.filter((r) => r.ok).length;
console.log('needs tests=' + results.length + ' passed=' + passed + ' states=' + states.length);
process.exit(passed === results.length ? 0 : 1);
