import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const here = new URL('.', import.meta.url);
const callJs = readFileSync(new URL('../src/call.js', here), 'utf8');
const pageJs = readFileSync(new URL('../pages/page.js', here), 'utf8');

function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

function makePage(replies, { withPage = true } = {}) {
  const calls = [];
  const callbacks = new Map();
  const frames = new Map();
  const intervals = new Map();
  let nextId = 1;
  const elements = {
    act: { listeners: [], addEventListener(type, fn) { this.listeners.push(fn); } },
    count: { textContent: '0' }
  };
  const loaded = [];
  const visibility = [];
  const documentListeners = new Map();
  const target = new EventTarget();
  const ctx = {
    CustomEvent,
    Map,
    addEventListener: target.addEventListener.bind(target),
    dispatchEvent: target.dispatchEvent.bind(target),
    document: {
      visibilityState: 'visible',
      getElementById: (id) => elements[id],
      addEventListener: (type, fn) => {
        if (type === 'DOMContentLoaded') loaded.push(fn);
        if (type === 'visibilitychange') visibility.push(fn);
        documentListeners.set(type, [...(documentListeners.get(type) ?? []), fn]);
      }
    },
    requestAnimationFrame: (fn) => { const id = nextId++; frames.set(id, fn); return id; },
    cancelAnimationFrame: (id) => { frames.delete(id); },
    setInterval: (fn, ms) => { const id = nextId++; intervals.set(id, { fn, ms }); return id; },
    clearInterval: (id) => { intervals.delete(id); },
    __TAURI_INTERNALS__: {
      invoke(command, args) {
        calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        const reply = replies[command];
        return typeof reply === 'function' ? reply(args) : Promise.resolve(reply ?? null);
      },
      transformCallback(fn) { const id = nextId++; callbacks.set(id, fn); return id; }
    }
  };
  ctx.window = ctx;
  vm.createContext(ctx);
  vm.runInContext(callJs, ctx);
  if (withPage) vm.runInContext(pageJs, ctx);
  return {
    ctx, calls, callbacks, frames, intervals, elements,
    load: () => loaded.forEach((fn) => fn()),
    documentEvent: (type, event) => (documentListeners.get(type) ?? []).forEach((fn) => fn(event)),
    setVisibility(state) { ctx.document.visibilityState = state; visibility.forEach((fn) => fn()); },
    flushFrames() { const due = [...frames.values()]; frames.clear(); due.forEach((fn) => fn()); },
    tick(n) { for (let i = 0; i < n; i++) [...intervals.values()].forEach((t) => t.fn()); },
    reports: () => calls.filter((c) => c.command === 'report').map((c) => c.args.line),
    named: (name) => calls.filter((c) => c.command === name)
  };
}

test('call.js gives the frozen helpers and the window event', () => {
  const page = makePage({});
  assert.ok(Object.isFrozen(page.ctx.r4app));
  const seen = [];
  page.ctx.addEventListener('r4-window', (e) => seen.push(e.detail));
  page.ctx.__r4Window('hide');
  page.ctx.__r4Window('show');
  assert.deepEqual(seen, ['hide', 'show']);
});

test('a frame request reports the first animation frame with its generation', async () => {
  const page = makePage({});
  page.ctx.__r4AppFrame(7);
  assert.equal(page.named('frame').length, 0);
  page.flushFrames();
  await settle();
  assert.deepEqual(page.named('frame').map((c) => c.args), [{ gen: 7 }]);
});

test('the page counts its interval and its frames, and counts are reported on request', async () => {
  const page = makePage({ config: {} });
  page.load();
  await settle();
  assert.equal([...page.intervals.values()][0].ms, 100);
  page.tick(3);
  page.flushFrames();
  page.flushFrames();
  page.ctx.__r4AppCounts(5);
  assert.deepEqual(page.named('counts').at(-1).args, { token: 5, ticks: 3, frames: 2 });
});

test('a page that loads call.js alone gives no counts', () => {
  const page = makePage({}, { withPage: false });
  page.ctx.__r4AppCounts(4);
  assert.equal(page.named('counts').length, 0);
});

test('a policy violation is reported with its directive and the blocked scheme', () => {
  const page = makePage({}, { withPage: false });
  page.documentEvent('securitypolicyviolation', {
    effectiveDirective: 'connect-src',
    blockedURI: 'https://example.com/'
  });
  page.documentEvent('securitypolicyviolation', { effectiveDirective: 'script-src', blockedURI: 'inline' });
  assert.deepEqual(page.reports(), [
    'policy violated directive=connect-src blocked=https',
    'policy violated directive=script-src blocked=inline'
  ]);
});

test('the window event stops and starts the interval and the loop', async () => {
  const page = makePage({ config: {} });
  page.load();
  await settle();
  assert.equal(page.intervals.size, 1);
  assert.equal(page.frames.size, 1);
  page.ctx.__r4Window('hide');
  assert.equal(page.intervals.size, 0);
  assert.equal(page.frames.size, 0);
  page.ctx.__r4Window('show');
  assert.equal(page.intervals.size, 1);
  assert.equal(page.frames.size, 1);
  assert.deepEqual(page.reports(), ['window shown after_hide=0 hidden_ticks=0 hide_seen=yes']);
});

test('the button calls bump and shows its count', async () => {
  const page = makePage({ config: {}, bump: 1 });
  page.load();
  page.elements.act.listeners.forEach((fn) => fn());
  await settle();
  assert.equal(page.elements.count.textContent, '1');
  assert.equal(page.named('bump').length, 1);
});

test('a channel delivers in the order of its index and serialises as a channel id', async () => {
  const page = makePage({});
  const got = [];
  const channel = page.ctx.r4app.channel((m) => got.push(m));
  const id = Number(channel.toJSON().slice('__CHANNEL__:'.length));
  assert.equal(JSON.stringify({ channel }), JSON.stringify({ channel: '__CHANNEL__:' + id }));
  const fn = page.callbacks.get(id);
  fn({ message: 'b', index: 1 });
  assert.deepEqual(got, []);
  fn({ message: 'a', index: 0 });
  fn({ message: 'c', index: 2 });
  fn({ end: true, index: 3 });
  assert.deepEqual(got, ['a', 'b', 'c']);
});

test('listen registers with the event plugin and hands over the payload', async () => {
  const page = makePage({});
  const got = [];
  page.ctx.r4app.listen('r4-change', (p) => got.push(p));
  const call = page.named('plugin:event|listen')[0];
  assert.equal(call.args.event, 'r4-change');
  assert.deepEqual(call.args.target, { kind: 'Any' });
  page.callbacks.get(call.args.handler)({ event: 'r4-change', payload: { seq: 1 }, id: 1 });
  assert.deepEqual(got, [{ seq: 1 }]);
});

test('the get way waits, stops at closed and calls again after the show', async () => {
  const pending = [];
  const page = makePage({
    config: { sync: 'get' },
    next: () => new Promise((resolve) => pending.push(resolve))
  });
  page.load();
  await settle();
  assert.deepEqual(page.named('next').map((c) => c.args.seq), [0]);
  pending.shift()({ seq: 10000, of: 30000, payload: 'x' });
  await settle();
  assert.deepEqual(page.named('next').map((c) => c.args.seq), [0, 10000]);
  page.ctx.__r4Window('hide');
  pending.shift()({ closed: 'yes' });
  await settle();
  assert.equal(page.named('next').length, 2);
  page.ctx.__r4Window('show');
  await settle();
  assert.deepEqual(page.named('next').map((c) => c.args.seq), [0, 10000, 10000]);
  pending.shift()({ seq: 30000, of: 30000, payload: 'x' });
  await settle();
  assert.deepEqual(page.reports(), [
    'sync mark=10000',
    'sync closed last=10000',
    'window shown after_hide=0 hidden_ticks=0 hide_seen=yes',
    'sync mark=30000',
    'sync end last=30000 order=ok after_hide=0 hidden_ticks=0'
  ]);
});

test('a reply in flight at the hide is followed by one more call, whose closed reply is reported', async () => {
  const pending = [];
  const page = makePage({
    config: { sync: 'get' },
    next: () => new Promise((resolve) => pending.push(resolve))
  });
  page.load();
  await settle();
  page.ctx.__r4Window('hide');
  pending.shift()({ seq: 5, of: 9, payload: 'x', hidden: false });
  await settle();
  assert.deepEqual(page.named('next').map((c) => c.args.seq), [0, 5]);
  pending.shift()({ closed: 'yes' });
  await settle();
  assert.equal(page.named('next').length, 2);
  assert.deepEqual(page.reports(), ['sync closed last=5']);
});

test('a hidden page whose call never gets closed prints no closed line', async () => {
  const pending = [];
  const page = makePage({
    config: { sync: 'get' },
    next: () => new Promise((resolve) => pending.push(resolve))
  });
  page.load();
  await settle();
  page.ctx.__r4Window('hide');
  pending.shift()({ seq: 1, of: 3, payload: 'x', hidden: true });
  await settle();
  pending.shift()({ seq: 2, of: 3, payload: 'x', hidden: true });
  await settle();
  page.ctx.__r4Window('show');
  await settle();
  pending.shift()({ seq: 3, of: 3, payload: 'x', hidden: false });
  await settle();
  assert.deepEqual(page.reports(), [
    'window shown after_hide=2 hidden_ticks=0 hide_seen=yes',
    'sync end last=3 order=ok after_hide=2 hidden_ticks=0'
  ]);
});

test('a page hidden by its own visibility alone makes no call', async () => {
  const pending = [];
  const page = makePage({
    config: { sync: 'get' },
    next: () => new Promise((resolve) => pending.push(resolve))
  });
  page.load();
  await settle();
  page.setVisibility('hidden');
  pending.shift()({ seq: 1, of: 3, payload: 'x', hidden: false });
  await settle();
  assert.equal(page.named('next').length, 1);
});

test('the channel way counts what came while hidden and a step back as a miss', async () => {
  const page = makePage({ config: { sync: 'channel' }, subscribe: null });
  page.load();
  await settle();
  const sub = page.named('subscribe')[0];
  const id = Number(sub.args.channel.slice('__CHANNEL__:'.length));
  const fn = page.callbacks.get(id);
  fn({ message: { seq: 1, of: 4, hidden: false }, index: 0 });
  page.ctx.__r4Window('hide');
  fn({ message: { seq: 2, of: 4, hidden: true }, index: 1 });
  page.ctx.__r4Window('show');
  fn({ message: { seq: 2, of: 4 }, index: 2 });
  fn({ message: { seq: 4, of: 4 }, index: 3 });
  await settle();
  assert.deepEqual(page.reports(), [
    'window shown after_hide=1 hidden_ticks=0 hide_seen=yes',
    'sync order_miss seq=2 last=2',
    'sync end last=4 order=miss after_hide=1 hidden_ticks=0'
  ]);
});

test('a reply that the core sent before the hide and that resolves after it is not counted', async () => {
  const pending = [];
  const page = makePage({
    config: { sync: 'get' },
    next: () => new Promise((resolve) => pending.push(resolve))
  });
  page.load();
  await settle();
  assert.deepEqual(page.named('next').map((c) => c.args.seq), [0]);
  page.ctx.__r4Window('hide');
  pending.shift()({ seq: 5, of: 5, payload: 'x', hidden: false });
  await settle();
  assert.deepEqual(page.reports(), ['sync end last=5 order=ok after_hide=0 hidden_ticks=0']);
});

test('a change that the core sent after the hide is counted when it arrives before the show', async () => {
  const page = makePage({ config: { sync: 'channel' }, subscribe: null });
  page.load();
  await settle();
  const id = Number(page.named('subscribe')[0].args.channel.slice('__CHANNEL__:'.length));
  const fn = page.callbacks.get(id);
  fn({ message: { seq: 1, of: 3, hidden: false }, index: 0 });
  page.ctx.__r4Window('hide');
  fn({ message: { seq: 2, of: 3, hidden: true }, index: 1 });
  page.ctx.__r4Window('show');
  fn({ message: { seq: 3, of: 3, hidden: false }, index: 2 });
  await settle();
  assert.deepEqual(page.reports(), [
    'window shown after_hide=1 hidden_ticks=0 hide_seen=yes',
    'sync end last=3 order=ok after_hide=1 hidden_ticks=0'
  ]);
});

test('the events way listens to r4-change', async () => {
  const page = makePage({ config: { sync: 'events' } });
  page.load();
  await settle();
  const call = page.named('plugin:event|listen')[0];
  page.callbacks.get(call.args.handler)({ payload: { seq: 3, of: 3 } });
  await settle();
  assert.deepEqual(page.reports(), ['sync end last=3 order=ok after_hide=0 hidden_ticks=0']);
});

test('ticks after the page became hidden count as hidden ticks before the window event', async () => {
  const page = makePage({ config: { sync: 'channel' }, subscribe: null });
  page.load();
  await settle();
  const id = Number(page.named('subscribe')[0].args.channel.slice('__CHANNEL__:'.length));
  page.tick(2);
  page.setVisibility('hidden');
  page.tick(3);
  page.callbacks.get(id)({ message: { seq: 1, of: 2, hidden: true }, index: 0 });
  page.ctx.__r4Window('hide');
  page.tick(4);
  page.setVisibility('visible');
  page.ctx.__r4Window('show');
  page.callbacks.get(id)({ message: { seq: 2, of: 2 }, index: 1 });
  await settle();
  assert.deepEqual(page.reports(), [
    'window shown after_hide=1 hidden_ticks=3 hide_seen=yes',
    'sync end last=2 order=ok after_hide=1 hidden_ticks=3'
  ]);
});

test('a page hidden without the window event counts its ticks as hidden', async () => {
  const page = makePage({ config: {} });
  page.load();
  await settle();
  page.setVisibility('hidden');
  page.tick(5);
  page.setVisibility('visible');
  page.tick(1);
  page.ctx.__r4AppCounts(1);
  assert.deepEqual(page.named('counts').at(-1).args, { token: 1, ticks: 6, frames: 0 });
  page.ctx.__r4Window('show');
  assert.deepEqual(page.reports(), ['window shown after_hide=0 hidden_ticks=5 hide_seen=no']);
});

test('a change after the end line is reported with the order', async () => {
  const page = makePage({ config: { sync: 'events' } });
  page.load();
  await settle();
  const handler = page.callbacks.get(page.named('plugin:event|listen')[0].args.handler);
  handler({ payload: { seq: 2, of: 2 } });
  handler({ payload: { seq: 1, of: 2 } });
  await settle();
  assert.deepEqual(page.reports(), [
    'sync end last=2 order=ok after_hide=0 hidden_ticks=0',
    'sync order_miss seq=1 last=2',
    'sync after_end seq=1 last=2 order=miss'
  ]);
});

test('a way that gets no change for 150 ticks of a shown page reports where it stands, once per stall', async () => {
  const page = makePage({ config: { sync: 'channel' }, subscribe: null });
  page.load();
  await settle();
  const fn = page.callbacks.get(Number(page.named('subscribe')[0].args.channel.slice('__CHANNEL__:'.length)));
  fn({ message: { seq: 1, of: 4, hidden: false }, index: 0 });
  page.tick(149);
  await settle();
  assert.deepEqual(page.reports(), []);
  page.tick(1);
  await settle();
  assert.deepEqual(page.reports(), ['sync stalled last=1 order=ok after_hide=0 hidden_ticks=0']);
  page.tick(300);
  fn({ message: { seq: 3, of: 4, hidden: false }, index: 1 });
  page.tick(150);
  await settle();
  assert.deepEqual(page.reports().slice(1), ['sync stalled last=3 order=ok after_hide=0 hidden_ticks=0']);
  fn({ message: { seq: 4, of: 4, hidden: false }, index: 2 });
  page.tick(400);
  await settle();
  assert.deepEqual(page.reports().slice(2), ['sync end last=4 order=ok after_hide=0 hidden_ticks=0']);
});

test('a page without a sync way reports no stall', async () => {
  const page = makePage({ config: { sync: null } });
  page.load();
  await settle();
  page.tick(400);
  await settle();
  assert.deepEqual(page.reports(), []);
});

test('the show line says whether the page saw the hide', async () => {
  const page = makePage({ config: { sync: 'channel' }, subscribe: null });
  page.load();
  await settle();
  page.ctx.__r4Window('show');
  page.ctx.__r4Window('hide');
  page.ctx.__r4Window('show');
  await settle();
  assert.deepEqual(page.reports(), [
    'window shown after_hide=0 hidden_ticks=0 hide_seen=no',
    'window shown after_hide=0 hidden_ticks=0 hide_seen=yes'
  ]);
});
