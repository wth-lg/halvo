import { createServer } from 'node:http';
import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { appendFile, mkdir, open as openFile, readFile, writeFile } from 'node:fs/promises';
import { extname, join, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { HERE, Refusal, browsersDir, installed } from './machine.mjs';

const LICENCES = ['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'Unicode-3.0'];
const TEXT_KEYS = new Set(['name', 'word', 'line', 'say', 'level', 'next', 'day', 'text', 'title', 'label', 'value', 'from_text', 'note']);
const MARKUP = '<b data-x="1">x</b>&amp;';
const VIEWPORT = { width: 1024, height: 768 };
const SETTLE_MS = 5000;
const QUIET_MS = 80;
const FRAME = '/__r4/frame/index.html';
const ENGINE = '/__r4/engine/ace.js';
const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
};
const MODES = [
  ['reduced_motion', { reducedMotion: 'reduce' }, () => matchMedia('(prefers-reduced-motion: reduce)').matches],
  ['forced_colors', { forcedColors: 'active' }, () => matchMedia('(forced-colors: active)').matches],
  ['width_320', { viewport: { width: 320, height: 768 } }, () => innerWidth === 320],
];

class EngineNone extends Error {
  constructor(reason) {
    super('the rule engine cannot run: ' + reason);
    this.code = 'engine';
    this.reason = reason;
  }
}

function engineDir() {
  return join(browsersDir(), 'node_modules', 'accessibility-checker-engine');
}

function engineInfo() {
  let pkg;
  try {
    pkg = JSON.parse(readFileSync(join(engineDir(), 'package.json'), 'utf8'));
  } catch {
    throw new EngineNone('package');
  }
  if (!LICENCES.includes(pkg.license)) throw new EngineNone('licence');
  if (!existsSync(join(engineDir(), 'ace.js'))) throw new EngineNone('file');
  return { version: pkg.version, license: pkg.license };
}

function locate(path, routes) {
  for (const [prefix, root, exact] of routes) {
    if (exact) {
      if (path === prefix) return existsSync(root) ? root : null;
      continue;
    }
    if (!path.startsWith(prefix)) continue;
    const file = resolve(root, path.slice(prefix.length).replace(/^\/+/, ''));
    if (file !== root && !file.startsWith(root + sep)) return null;
    let real;
    let top;
    try {
      real = realpathSync(file);
      top = realpathSync(root);
    } catch {
      return null;
    }
    if (!real.startsWith(top + sep)) return null;
    return statSync(real).isFile() ? real : null;
  }
  return null;
}

export async function serve({ ui, fixtures } = {}) {
  if (typeof ui !== 'string' || typeof fixtures !== 'string') throw new Refusal('usage', 'serve({ui, fixtures}) takes two directories');
  const routes = [
    [ENGINE, join(engineDir(), 'ace.js'), true],
    ['/__r4/frame/', join(HERE, 'frame')],
    ['/__r4/sample/', join(HERE, 'sample')],
    ['/__fixtures/', resolve(fixtures)],
    ['/', resolve(ui)],
  ];
  let own = '';
  const server = createServer(async (req, res) => {
    const plain = { 'x-content-type-options': 'nosniff', 'cache-control': 'no-store' };
    if (req.headers.host !== own) {
      res.writeHead(403, plain).end();
      return;
    }
    let path;
    try {
      path = decodeURIComponent(new URL(req.url, 'http://127.0.0.1').pathname);
    } catch {
      res.writeHead(400, plain).end();
      return;
    }
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      res.writeHead(405, plain).end();
      return;
    }
    const file = locate(path, routes);
    if (!file) {
      res.writeHead(404, plain).end();
      return;
    }
    let handle;
    try {
      handle = await openFile(file, 'r');
    } catch {
      res.writeHead(500, plain).end();
      return;
    }
    res.writeHead(200, { ...plain, 'content-type': TYPES[extname(file)] || 'application/octet-stream' });
    if (req.method === 'HEAD') {
      await handle.close();
      res.end();
      return;
    }
    const stream = handle.createReadStream();
    stream.on('error', () => res.destroy());
    stream.pipe(res);
  });
  server.on('connection', (socket) => socket.unref());
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  server.unref();
  own = '127.0.0.1:' + server.address().port;
  return 'http://' + own;
}

function own(url, origin) {
  try {
    return new URL(url).host === new URL(origin).host;
  } catch {
    return false;
  }
}

async function pass(route, hit) {
  if (new URL(route.request().url()).pathname === ENGINE) return route.continue();
  let res;
  try {
    res = await route.fetch();
  } catch {
    return route.abort().catch(() => {});
  }
  const body = await res.body();
  if (/shadowrootmode/i.test(body.toString('latin1'))) hit();
  return route.fulfill({ response: res, body });
}

async function guard(ctx, origin, count, hit) {
  await ctx.route('**/*', (route) => {
    if (own(route.request().url(), origin)) return pass(route, hit);
    count();
    return route.abort();
  });
  await ctx.routeWebSocket(
    () => true,
    (ws) => {
      if (own(ws.url(), origin)) {
        ws.connectToServer();
        return;
      }
      count();
      ws.close();
    },
  );
}

let shared = null;
let announced = false;

async function launch() {
  const build = installed();
  const pw = await import(pathToFileURL(build.pkg).href);
  const browser = await pw[build.browser].launch({ headless: true, executablePath: build.exe });
  if (!announced) {
    announced = true;
    process.stdout.write('browser name=' + build.browser + ' version=' + browser.version() + '\n');
  }
  return { browser, build };
}

async function acquire() {
  if (!shared) shared = { users: 0, ready: launch() };
  const mine = shared;
  mine.users++;
  try {
    return await mine.ready;
  } catch (err) {
    mine.users--;
    if (shared === mine && mine.users === 0) shared = null;
    throw err;
  }
}

async function release() {
  const mine = shared;
  if (!mine) return;
  mine.users--;
  if (mine.users > 0) return;
  shared = null;
  const { browser } = await mine.ready;
  await browser.close();
}

const INIT = () => {
  let opaque = false;
  const attach = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (init) {
    opaque = true;
    return attach.call(this, init);
  };
  const wrap = (owner, key, count) => {
    const f = owner && owner[key];
    if (typeof f !== 'function') return;
    owner[key] = function (...args) {
      const given = args.slice(0, count);
      const text = given.map((a) => `${a}`);
      if (given.some((a) => typeof a !== 'string') || text.some((t) => /shadowrootmode/i.test(t))) opaque = true;
      return f.apply(this, [...text, ...args.slice(count)]);
    };
  };
  wrap(Element.prototype, 'setHTMLUnsafe', 1);
  wrap(typeof ShadowRoot === 'undefined' ? null : ShadowRoot.prototype, 'setHTMLUnsafe', 1);
  wrap(Document, 'parseHTMLUnsafe', 1);
  wrap(Document.prototype, 'write', Infinity);
  wrap(Document.prototype, 'writeln', Infinity);
  Object.defineProperty(window, '__r4opaque', { get: () => opaque, configurable: false });
  let tampered = false;
  const apply = Reflect.apply;
  for (const name of ['__playwright__binding__', '__playwright__binding__controller__']) {
    const value = window[name];
    if (value === undefined) continue;
    try {
      Object.defineProperty(window, name, { get: () => value, set: () => (tampered = true), configurable: false });
    } catch {
      tampered = true;
    }
  }
  const controller = window.__playwright__binding__controller__;
  if (controller && typeof controller.deliverBindingResult === 'function' && controller._bindings instanceof Map) {
    const real = controller.deliverBindingResult;
    const bindings = controller._bindings;
    const get = Map.prototype.get;
    const has = Map.prototype.has;
    const deliver = function (arg) {
      const data = arg ? apply(get, bindings, [arg.name]) : undefined;
      if (!data || !apply(has, data.callbacks, [arg.seq])) {
        tampered = true;
        return undefined;
      }
      return apply(real, this, [arg]);
    };
    try {
      Object.defineProperty(controller, 'deliverBindingResult', { value: deliver, writable: false, configurable: false });
    } catch {
      tampered = true;
    }
  }
  Object.defineProperty(window, '__r4tampered', { get: () => tampered, configurable: false });
  const binding = window.__r4call;
  delete window.__r4call;
  if (Object.getOwnPropertyDescriptor(window, '__r4call')) Object.defineProperty(window, '__r4call', { value: undefined, writable: false, configurable: false });
  const stub = Object.freeze({ call: (name, args) => binding(name, args === undefined ? null : args) });
  const halvo = {};
  Object.defineProperty(halvo, 'ipc', { get: () => stub, set: () => {}, enumerable: true });
  Object.defineProperty(window, 'Halvo', {
    get: () => halvo,
    set: (value) => {
      if (value && typeof value === 'object') for (const k of Object.keys(value)) if (k !== 'ipc') halvo[k] = value[k];
    },
  });
};

const QUIET = ([quiet, left]) =>
  new Promise((done) => {
    let timer = 0;
    let changed = false;
    const limit = setTimeout(() => {
      observer.disconnect();
      clearTimeout(timer);
      done('limit');
    }, left);
    const end = () => {
      observer.disconnect();
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          clearTimeout(limit);
          observer.disconnect();
          done(changed ? 'again' : 'quiet');
        }),
      );
      observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
    };
    const observer = new MutationObserver(() => {
      if (timer === -1) {
        changed = true;
        return;
      }
      clearTimeout(timer);
      timer = setTimeout(() => {
        timer = -1;
        end();
      }, quiet);
    });
    observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
    timer = setTimeout(() => {
      timer = -1;
      end();
    }, quiet);
  });

const FINISH = () => {
  let moving = 0;
  for (const a of document.getAnimations()) {
    if (a.playState !== 'running') continue;
    moving++;
    const timing = a.effect ? a.effect.getComputedTiming() : null;
    if (!timing || timing.iterations === Infinity || timing.endTime === Infinity) {
      a.pause();
      continue;
    }
    try {
      a.finish();
    } catch {
      a.pause();
    }
  }
  return moving;
};

function sleep(ms) {
  return new Promise((done) => setTimeout(done, ms).unref());
}

async function tamper(h) {
  if (await h.page.evaluate(() => window.__r4tampered === true).catch(() => false)) h.flags.add('stub_tampered');
}

async function settle(h) {
  await wait(h);
  await tamper(h);
}

async function wait(h) {
  await h.page.waitForFunction(() => document.readyState === 'complete' && (!window.__r4frame || window.__r4frame.ready), null, { timeout: 15000 });
  const end = Date.now() + SETTLE_MS;
  for (;;) {
    const left = end - Date.now();
    if (left <= 0) break;
    if (h.inflight > 0 || h.writing > 0) {
      await sleep(10);
      continue;
    }
    const calls = h.started;
    const moving = await h.page.evaluate(FINISH);
    const quiet = await h.page.evaluate(QUIET, [QUIET_MS, Math.max(1, end - Date.now())]);
    if (quiet === 'limit') break;
    if (quiet === 'quiet' && moving === 0 && h.inflight === 0 && h.writing === 0 && h.started === calls && (await h.page.evaluate(FINISH)) === 0) return;
  }
  h.flags.add('unsettled');
}

function markup(value, key) {
  if (Array.isArray(value)) return value.map((x) => (typeof x === 'string' ? (TEXT_KEYS.has(key) && x !== '' ? MARKUP : x) : markup(x, key)));
  if (value && typeof value === 'object') {
    const out = {};
    for (const k of Object.keys(value)) {
      const v = value[k];
      out[k] = typeof v === 'string' ? (TEXT_KEYS.has(k) && v !== '' ? MARKUP : v) : markup(v, k);
    }
    return out;
  }
  return value;
}

function note(h, line) {
  h.trail.push(line);
}

function why(err) {
  const c = err && (err.code || (err.cause && err.cause.code));
  return String(c || (err && err.name) || 'unknown').replace(/\s+/g, '_');
}

/** Waits until the page has settled as a check reads it, the stub's own writes of calls.jsonl among what it waits for. */
export async function settled(h) {
  await settle(h);
}

/** The stub's lines for one page: each call that it rejected, refused or could not answer, and each fixture that it could not read. */
export function trail(h) {
  return h && Array.isArray(h.trail) ? h.trail.slice() : [];
}

async function fixture(h, name) {
  if (h.cache.has(name)) return h.cache.get(name);
  let res;
  try {
    res = await fetch(h.origin + '/__fixtures/' + name);
  } catch (err) {
    note(h, 'stub fixture=' + name + ' cause=fetch_' + why(err));
    throw new Refusal('fixture', 'the fixtures directory could not be read at ' + name);
  }
  if (!res.ok) {
    note(h, 'stub fixture=' + name + ' cause=status_' + res.status);
    throw new Refusal('fixture', 'no fixture ' + name);
  }
  let value;
  try {
    value = JSON.parse(await res.text());
  } catch (err) {
    note(h, 'stub fixture=' + name + ' cause=json_' + why(err));
    throw new Refusal('fixture', name + ' is no JSON');
  }
  h.cache.set(name, value);
  return value;
}

function same(a, b) {
  return JSON.stringify(sorted(a)) === JSON.stringify(sorted(b));
}

function sorted(v) {
  if (Array.isArray(v)) return v.map(sorted);
  if (v && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v).sort()) o[k] = sorted(v[k]);
    return o;
  }
  return v;
}

function flush(h) {
  const t = h.transcript;
  const line = t.lines[t.i];
  if (t.waiting && t.i > 0 && line && 'state' in line) {
    const wake = t.waiting;
    t.waiting = null;
    t.i++;
    h.current = h.transform ? h.transform(line.state) : line.state;
    wake(h.current);
  }
}

const REJECT = Symbol('reject');

function fromTranscript(h, name, args) {
  const t = h.transcript;
  const line = t.lines[t.i];
  if (name === 'agents_snapshot') {
    if (t.i === 0 && line && 'state' in line) {
      t.i++;
      h.current = h.transform ? h.transform(line.state) : line.state;
      return h.current;
    }
    h.mismatch.push(name);
    return { error: 'malformed' };
  }
  if (name === 'agents_next') {
    return new Promise((wake) => {
      t.waiting = wake;
      flush(h);
    });
  }
  if (!line || !('call' in line) || line.call !== name || !same(line.args || {}, args)) {
    h.mismatch.push(name);
    return { error: 'malformed' };
  }
  t.i++;
  setImmediate(() => flush(h));
  if (line.reject === true) return REJECT;
  if (line.hold === true) return new Promise((wake) => h.held.push(wake));
  return h.transform ? h.transform(line.reply) : line.reply;
}

async function record(h, name, a) {
  h.calls++;
  if (!h.out) return;
  h.writing++;
  try {
    await appendFile(join(h.out, 'calls.jsonl'), JSON.stringify({ n: h.calls, call: name, args: a }) + '\n');
  } finally {
    h.writing--;
  }
}

function unheld(h, fn) {
  if (h.mismatch.length) throw new Error(fn + ': the page made a call that the transcript does not hold next: ' + h.mismatch.join(','));
}

async function stub(h, name, args) {
  const a = args && typeof args === 'object' && !Array.isArray(args) ? args : {};
  if (typeof name !== 'string') return { error: 'malformed' };
  h.started++;
  const logged = name !== 'agents_snapshot' && name !== 'agents_next';
  if (h.transcript) {
    const wrong = h.mismatch.length;
    const reply = fromTranscript(h, name, a);
    if (h.mismatch.length !== wrong) note(h, 'stub call=' + name + ' refused cause=not_next_in_transcript');
    try {
      if (logged && h.mismatch.length === wrong) await record(h, name, a);
    } catch (err) {
      note(h, 'stub call=' + name + ' unanswered cause=record_' + why(err));
      throw err;
    }
    if (reply === REJECT) {
      note(h, 'stub call=' + name + ' rejected by=transcript');
      throw new Error('the transcript rejects this call');
    }
    return reply;
  }
  try {
    if (logged) await record(h, name, a);
  } catch (err) {
    note(h, 'stub call=' + name + ' unanswered cause=record_' + why(err));
    throw err;
  }
  if (name === 'agents_next') {
    h.nexts++;
    if (h.nexts > 1 || h.closed) return h.closed ? { closed: 'yes' } : new Promise((wake) => h.held.push(wake));
  }
  h.inflight++;
  try {
    if (name === 'agents_snapshot') {
      h.current = h.state.snapshot;
      return h.current;
    }
    if (name === 'agents_card') return h.state.card;
    if (name === 'agents_next') {
      const reply = await fixture(h, 'reply-next-same.json');
      if (reply && typeof reply === 'object' && !(h.current && reply.seq <= h.current.seq)) h.current = reply;
      return reply;
    }
    return await fixture(h, 'reply-ok.json');
  } catch (err) {
    note(h, 'stub call=' + name + ' unanswered cause=' + why(err));
    throw err;
  } finally {
    h.inflight--;
  }
}

function lines(replies) {
  return replies.map((line) => {
    const v = typeof line === 'string' ? JSON.parse(line) : line;
    if (!v || typeof v !== 'object' || !('state' in v || 'call' in v)) throw new Refusal('fixture', 'a transcript line holds neither state nor call');
    return v;
  });
}

async function openWith(options, context, transform, inner) {
  const o = options || {};
  if (typeof o.origin !== 'string' || (typeof o.state !== 'string' && !Array.isArray(o.replies))) {
    throw new Refusal('usage', 'open({origin, state, views, frame, replies, acts, out}) needs origin and a state or replies');
  }
  if (o.acts !== undefined && !(Array.isArray(o.acts) && o.acts.every((a) => a && typeof a === 'object' && typeof a.act === 'string'))) {
    throw new Refusal('usage', 'open({acts}) takes a list of act arguments, each {act, param, n, text}');
  }
  const { browser, build } = await acquire();
  let ctx = null;
  try {
    ctx = await browser.newContext({ viewport: VIEWPORT, ...context });
    const h = {
      origin: o.origin,
      options: o,
      context,
      transform,
      build,
      ctx,
      page: null,
      out: o.out ? resolve(o.out) : null,
      cache: new Map(),
      transcript: Array.isArray(o.replies) ? { i: 0, waiting: null, lines: lines(o.replies) } : null,
      state: null,
      current: null,
      blocked: 0,
      inflight: 0,
      started: 0,
      nexts: 0,
      held: [],
      flags: new Set(),
      kids: [],
      calls: 0,
      actions: 0,
      errors: 0,
      mismatch: [],
      trail: [],
      writing: 0,
      closed: false,
      modes: null,
      word: false,
    };
    h.close = () => closeHandle(h);
    if (typeof o.state === 'string' && !h.transcript) {
      const raw = await fixture(h, 'state-' + o.state + '.json');
      if (!raw || typeof raw !== 'object' || !('snapshot' in raw)) throw new Refusal('fixture', 'state-' + o.state + '.json holds no snapshot');
      h.state = transform ? transform(raw) : raw;
    }
    if (h.out) await mkdir(h.out, { recursive: true });
    let text = '';
    try {
      text = await (await fetch(new URL(o.frame || FRAME, o.origin).href)).text();
    } catch {
      text = '';
    }
    if (/shadowrootmode/i.test(text)) throw new Refusal('fixture', 'the frame file holds the word shadowrootmode');
    await guard(
      ctx,
      o.origin,
      () => {
        h.blocked++;
      },
      () => {
        h.word = true;
      },
    );
    await ctx.exposeBinding('__r4call', (_source, name, args) => stub(h, name, args));
    await ctx.addInitScript(INIT);
    h.page = await ctx.newPage();
    h.page.on('pageerror', () => {
      h.errors++;
      h.flags.add('page_error');
    });
    const url = new URL(o.frame || FRAME, o.origin);
    if (Array.isArray(o.views) && o.views.length) url.searchParams.set('views', o.views.join(','));
    await h.page.goto(url.href);
    await settle(h);
    if (await h.page.evaluate(() => Boolean(window.__r4frame && window.__r4frame.failed))) throw new Refusal('fixture', 'a view script of views did not load');
    unheld(h, 'open');
    if (h.errors > 0 && !inner) throw new Error('open: the page reported an uncaught error');
    for (const a of o.acts || []) await act(h, a);
    if (h.errors > 0 && !inner) throw new Error('open: the page reported an uncaught error');
    return h;
  } catch (err) {
    if (ctx) await ctx.close().catch(() => {});
    await release();
    throw err;
  }
}

async function closeHandle(h) {
  if (h.closed) return;
  h.closed = true;
  for (const wake of h.held.splice(0)) wake({ closed: 'yes' });
  await h.ctx.close().catch(() => {});
  await release();
}

export async function open(options) {
  return openWith(options, {}, null);
}

async function again(h, context, transform) {
  const o = { ...h.options, out: undefined };
  const kid = await openWith(o, context === undefined ? h.context : context, transform === undefined ? h.transform : transform, true);
  h.kids.push(kid);
  return kid;
}

function cssq(s) {
  return String(s).replace(/["\\]/g, '\\$&');
}

export async function act(h, { act: name, param, n, text } = {}) {
  if (!h || !h.page || typeof name !== 'string') throw new Refusal('usage', 'act(page, {act, param, n, text}) needs a page and a data-act name');
  let sel = '[data-act="' + cssq(name) + '"]';
  if (param !== undefined && param !== null) sel += '[data-param="' + cssq(param) + '"]';
  if (n !== undefined && n !== null) sel += '[data-n="' + cssq(n) + '"]';
  const loc = h.page.locator(sel);
  const count = await loc.count();
  if (count !== 1) throw new Error('act: ' + count + ' controls match data-act=' + name);
  const kind = await loc.evaluate((el) => (el.tagName === 'INPUT' ? el.type : el.tagName.toLowerCase()));
  let type;
  if (kind === 'radio') {
    await loc.check();
    type = 'choose';
  } else if (name === 'typed') {
    await loc.pressSequentially(String(text === undefined || text === null ? '' : text));
    type = 'type';
  } else if (await loc.evaluate((el) => Boolean(el.closest('[role="alertdialog"]') && el.closest('[role="alertdialog"]').matches(':modal')))) {
    const on = () => loc.evaluate((el) => el === document.activeElement);
    for (let i = 0; i < 40 && !(await on()); i++) await h.page.keyboard.press(h.build.walkKey);
    if (!(await on())) throw new Error('act: the walk key does not reach data-act=' + name + ' in its confirmation');
    await h.page.keyboard.press('Enter');
    type = 'key';
  } else {
    await loc.click();
    type = 'click';
  }
  await settle(h);
  unheld(h, 'act');
  h.actions++;
  if (h.out) await appendFile(join(h.out, 'actions.txt'), 'action i=' + h.actions + ' type=' + type + ' act=' + name + '\n');
}

const ENGINE_RUN = async () => {
  for (const s of document.querySelectorAll('script[src$="/__r4/engine/ace.js"]')) s.remove();
  try {
    if (typeof ace === 'undefined' || !ace.Checker) return { error: 'file' };
    const checker = new ace.Checker();
    const ids = typeof checker.getGuidelineIds === 'function' ? checker.getGuidelineIds() : checker.rulesetIds || [];
    if (!ids.includes('WCAG_2_2')) return { error: 'ruleset' };
    const report = await checker.check(document, ['WCAG_2_2']);
    let violations = 0;
    let review = 0;
    const rules = new Set();
    for (const r of report.results) {
      const v = r.value || [];
      if (v[1] === 'PASS') continue;
      if (v[0] === 'VIOLATION' && v[1] === 'FAIL') {
        violations++;
        rules.add(r.ruleId);
      } else {
        review++;
      }
    }
    return { executed: report.numExecuted, violations, review, rules: [...rules].sort() };
  } catch {
    return { error: 'throws' };
  }
};

let engineAlone = null;

function alone(h) {
  if (!engineAlone) {
    engineAlone = (async () => {
      let count = 0;
      const ctx = await h.ctx.browser().newContext({ viewport: VIEWPORT });
      try {
        await guard(
          ctx,
          h.origin,
          () => {
            count++;
          },
          () => {},
        );
        const page = await ctx.newPage();
        await page.goto(new URL(FRAME, h.origin).href);
        let r;
        try {
          await page.addScriptTag({ url: h.origin + ENGINE });
          r = await page.evaluate(ENGINE_RUN);
        } catch {
          r = { error: 'file' };
        }
        return { count, error: r.error || (r.executed > 0 ? null : 'no_rule') };
      } finally {
        await ctx.close().catch(() => {});
      }
    })();
    engineAlone.catch(() => {
      engineAlone = null;
    });
  }
  return engineAlone;
}

async function machineWide(h, err) {
  if (err.reason === 'package' || err.reason === 'licence') return true;
  const a = await alone(h);
  return a.count > 0 || a.error !== null;
}

async function engineCheck(h) {
  engineInfo();
  try {
    await h.page.addScriptTag({ url: h.origin + ENGINE });
  } catch {
    throw new EngineNone('file');
  }
  const r = await h.page.evaluate(ENGINE_RUN);
  if (r.error) throw new EngineNone(r.error);
  if (!(r.executed > 0)) throw new EngineNone('no_rule');
  const outside = h.blocked > 0;
  if (outside && (await alone(h)).count > 0) throw new EngineNone('request');
  const names = outside ? [...r.rules, 'outside_request'].sort() : r.rules;
  return { ok: r.violations === 0 && !outside, counts: { violations: r.violations, review: r.review }, names };
}

const HELPER = () => {
  if (window.__r4w) return;
  const NATIVE = 'button, input:not([type="hidden"]), select, textarea, a[href], area[href], summary, audio[controls], video[controls]';
  const ROLES = new Set(['button', 'link', 'radio', 'checkbox', 'switch', 'tab', 'menuitem', 'menuitemradio', 'menuitemcheckbox', 'option', 'textbox', 'combobox', 'slider', 'spinbutton', 'searchbox', 'scrollbar', 'treeitem', 'gridcell']);
  const ARIA = new Set(['alert', 'alertdialog', 'application', 'article', 'banner', 'blockquote', 'button', 'caption', 'cell', 'checkbox', 'code', 'columnheader', 'combobox', 'complementary', 'contentinfo', 'definition', 'deletion', 'dialog', 'directory', 'document', 'emphasis', 'feed', 'figure', 'form', 'generic', 'grid', 'gridcell', 'group', 'heading', 'img', 'image', 'insertion', 'link', 'list', 'listbox', 'listitem', 'log', 'main', 'mark', 'marquee', 'math', 'menu', 'menubar', 'menuitem', 'menuitemcheckbox', 'menuitemradio', 'meter', 'navigation', 'none', 'note', 'option', 'paragraph', 'presentation', 'progressbar', 'radio', 'radiogroup', 'region', 'row', 'rowgroup', 'rowheader', 'scrollbar', 'search', 'searchbox', 'separator', 'slider', 'spinbutton', 'status', 'strong', 'subscript', 'superscript', 'switch', 'tab', 'table', 'tablist', 'tabpanel', 'term', 'textbox', 'time', 'timer', 'toolbar', 'tooltip', 'tree', 'treegrid', 'treeitem']);
  const role = (el) => {
    for (const token of (el.getAttribute('role') || '').trim().toLowerCase().split(/\s+/)) if (ARIA.has(token)) return token;
    return '';
  };
  const PROPS = ['outline-style', 'outline-width', 'outline-color', 'outline-offset'];
  for (const side of ['top', 'right', 'bottom', 'left']) for (const p of ['style', 'width', 'color']) PROPS.push('border-' + side + '-' + p);
  const ids = new WeakMap();
  const byId = new Map();
  let next = 1;
  const idOf = (el) => {
    if (!ids.has(el)) ids.set(el, next++);
    const id = ids.get(el);
    byId.set(id, el);
    return id;
  };
  const rendered = (el) => {
    if (!el.isConnected) return false;
    if (el.tagName === 'AREA') return true;
    const s = getComputedStyle(el);
    if (s.display === 'none' || s.visibility === 'hidden' || s.visibility === 'collapse') return false;
    return el.getClientRects().length > 0;
  };
  const host = (el) => el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable);
  const isControl = (el) => el.matches(NATIVE) || host(el) || el.hasAttribute('data-act') || (el.hasAttribute('tabindex') && el.tabIndex >= 0) || ROLES.has(role(el));
  const dialog = () => {
    const all = [...document.querySelectorAll('[role]')].filter((el) => role(el) === 'alertdialog' && rendered(el));
    return all.length ? all[all.length - 1] : null;
  };
  const controls = () => [...document.querySelectorAll('*')].filter((el) => isControl(el) && rendered(el) && !el.disabled);
  const style = (el) => {
    const s = getComputedStyle(el);
    return PROPS.map((p) => s.getPropertyValue(p));
  };
  const name = (el) => el.getAttribute('data-act') || el.tagName.toLowerCase();
  const rgba = (text) => {
    const m = /^rgba?\(([^)]+)\)$/.exec(String(text).trim());
    if (!m) return null;
    const p = m[1].split(/[\s,/]+/).filter(Boolean).map(parseFloat);
    if (p.length < 3 || p.some((v) => Number.isNaN(v))) return null;
    return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
  };
  const over = (top, bottom) => [0, 1, 2].map((i) => top[i] * top[3] + bottom[i] * (1 - top[3])).concat(1);
  const backdrop = (start) => {
    const layers = [];
    for (let e = start; e; e = e.parentElement) {
      const s = getComputedStyle(e);
      if (s.backgroundImage && s.backgroundImage !== 'none') return null;
      const c = rgba(s.backgroundColor);
      if (!c) return null;
      if (c[3] > 0) layers.push(c);
      if (c[3] >= 1) break;
    }
    let base = [255, 255, 255, 1];
    for (let i = layers.length - 1; i >= 0; i--) base = over(layers[i], base);
    return base;
  };
  const lum = (c) => {
    const f = (v) => {
      const x = v / 255;
      return x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
  };
  const contrast = (colour, start) => {
    const c = rgba(colour);
    const bg = backdrop(start);
    if (!c || !bg) return null;
    const a = lum(over(c, bg));
    const b = lum(bg);
    return Math.round(((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)) * 100) / 100;
  };
  const ring = (el) => {
    const s = getComputedStyle(el);
    const inside = (parseFloat(s.outlineOffset) || 0) < 0;
    return {
      outline: contrast(s.outlineColor, inside ? el : el.parentElement),
      borders: ['top', 'right', 'bottom', 'left'].map((side) => contrast(s.getPropertyValue('border-' + side + '-color'), el.parentElement)),
    };
  };
  const textOf = (ids) =>
    (ids || '')
      .trim()
      .split(/\s+/)
      .filter(Boolean)
      .map((id) => document.getElementById(id))
      .filter(Boolean)
      .map((e) => e.textContent.trim())
      .join(' ')
      .trim();
  const isModal = (el) => {
    try {
      return el.matches(':modal');
    } catch {
      return false;
    }
  };
  const SKIP = ['SCRIPT', 'STYLE', 'TEMPLATE', 'LINK', 'META', 'NOSCRIPT'];
  const restInert = (d) => {
    for (let e = d; e && e !== document.documentElement; e = e.parentElement) if (isModal(e)) return true;
    for (let e = d; e && e.parentElement && e !== document.body; e = e.parentElement) {
      for (const sib of e.parentElement.children) {
        if (sib === e || SKIP.includes(sib.tagName) || sib.closest('[inert]') || !rendered(sib)) continue;
        return false;
      }
    }
    return true;
  };
  const props = (d) => ({
    modal: d.getAttribute('aria-modal') === 'true',
    label: Boolean((d.getAttribute('aria-label') || '').trim() || textOf(d.getAttribute('aria-labelledby'))),
    description: Boolean((d.getAttribute('aria-description') || '').trim() || textOf(d.getAttribute('aria-describedby'))),
    inert: restInert(d),
  });
  const dialogs = () => [...document.querySelectorAll('[role]')].filter((el) => role(el) === 'alertdialog' && rendered(el));
  window.__r4w = {
    begin() {
      const d = dialog();
      const list = controls().filter((el) => (d ? d.contains(el) : !el.closest('[inert]')));
      const a = document.activeElement;
      const active = a && a !== document.body && a !== document.documentElement ? idOf(a) : 0;
      const own = d ? d.querySelector('[data-param]') : null;
      return {
        dialog: Boolean(d),
        dialogId: d ? idOf(d) : 0,
        dialogParam: own ? own.getAttribute('data-param') : null,
        dialogs: dialogs().map(props),
        active,
        controls: list.map((el) => ({ id: idOf(el), group: el.tagName === 'INPUT' && el.type === 'radio' && el.name ? 'radio:' + (el.form ? idOf(el.form) : 0) + ':' + el.name : null, name: name(el) })),
      };
    },
    step() {
      const a = document.activeElement;
      if (!a || a === document.body || a === document.documentElement) return { id: 0 };
      const d = dialog();
      return { id: idOf(a), outside: d ? !d.contains(a) : false, self: a === d, style: style(a), ring: ring(a) };
    },
    rest(list) {
      if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
      return list.map((id) => (byId.has(id) && byId.get(id).isConnected ? style(byId.get(id)) : null));
    },
    element(id) {
      return byId.get(id) || null;
    },
    focus(id) {
      const el = byId.get(id);
      if (el) el.focus();
      return Boolean(el) && document.activeElement === el;
    },
    chosen(ids) {
      const on = ids.find((id) => byId.has(id) && byId.get(id).checked);
      return on === undefined ? 0 : on;
    },
    mark() {
      const d = dialog();
      return d ? [d, ...d.querySelectorAll('*')].map(idOf) : [];
    },
    closed(ids, more) {
      const gone = ids.every((id) => {
        const el = byId.get(id);
        return !el || !el.isConnected || !rendered(el);
      });
      let modals = [];
      try {
        modals = [...document.querySelectorAll(':modal')];
      } catch {
        modals = [document.body];
      }
      const next = modals.every((m) => more && role(m) === 'alertdialog');
      return gone && next && document.querySelector('[inert]') === null;
    },
    sizes() {
      const small = [];
      const CUT = ['hidden', 'clip'];
      const clip = (el) => {
        const box = { left: -Infinity, top: -Infinity, right: Infinity, bottom: Infinity };
        let mode = getComputedStyle(el).position;
        for (let p = el.parentElement; p && p !== document.body && p !== document.documentElement; p = p.parentElement) {
          const s = getComputedStyle(p);
          if (mode === 'fixed' && s.transform === 'none') continue;
          if (mode === 'absolute' && s.position === 'static' && s.transform === 'none') continue;
          mode = s.position;
          const x = CUT.includes(s.overflowX);
          const y = CUT.includes(s.overflowY);
          if (!x && !y) continue;
          const r = p.getBoundingClientRect();
          const left = r.left + p.clientLeft;
          const top = r.top + p.clientTop;
          if (x) {
            box.left = Math.max(box.left, left);
            box.right = Math.min(box.right, left + p.clientWidth);
          }
          if (y) {
            box.top = Math.max(box.top, top);
            box.bottom = Math.min(box.bottom, top + p.clientHeight);
          }
        }
        return box;
      };
      const kind = (el) => {
        const act = el.getAttribute('data-act');
        if (act === null) return true;
        if (el.tagName === 'BUTTON') return true;
        if (el.tagName === 'INPUT' && el.type === 'radio') return true;
        return el.tagName === 'INPUT' && el.type === 'text' && act === 'typed';
      };
      const round = (v) => Math.round(v * 100) / 100;
      for (const el of controls()) {
        if (el.getAttribute('data-act') === null) small.push('no_act');
        let inDialog = false;
        for (let p = el.parentElement; p; p = p.parentElement) if (role(p) === 'alertdialog') inDialog = true;
        const min = inDialog ? 44 : 24;
        const box = clip(el);
        const rects = [...el.getClientRects()].map((r) => ({
          width: Math.max(0, Math.min(r.right, box.right) - Math.max(r.left, box.left)),
          height: Math.max(0, Math.min(r.bottom, box.bottom) - Math.max(r.top, box.top)),
        }));
        if (!kind(el) || !rects.length || rects.some((r) => round(r.width) < min || round(r.height) < min)) small.push(name(el));
      }
      return small;
    },
  };
};

function seen(color) {
  return !(color === 'transparent' || /^rgba\(.*,\s*0\)$/.test(color));
}

function border(style, side) {
  const [kind, width, color] = style.slice(4 + side * 3, 7 + side * 3);
  if (kind === 'none' || kind === 'hidden' || !(parseFloat(width) > 0) || !seen(color)) return 'off';
  return kind + ' ' + width + ' ' + color;
}

function shownFocus(stop, rest) {
  if (!stop || !rest) return 'focus_unseen';
  const focused = stop.style;
  const shown = [];
  const outlineOn = focused[0] !== 'none' && (focused[0] === 'auto' || parseFloat(focused[1]) > 0) && seen(focused[2]);
  if (outlineOn && [0, 1, 2, 3].some((i) => focused[i] !== rest[i])) shown.push(stop.ring.outline);
  for (const side of [0, 1, 2, 3]) {
    if (border(focused, side) === border(rest, side)) continue;
    shown.push(border(focused, side) === 'off' ? null : stop.ring.borders[side]);
  }
  if (!shown.length) return 'focus_unseen';
  return shown.some((c) => c !== null && c >= 3) ? null : 'focus_contrast';
}

function confirmItems(h) {
  const s = h.current;
  return s && Array.isArray(s.items) ? s.items.filter((i) => i && i.kind === 'confirm' && i.presenter === 'page') : [];
}

function safeChoice(item) {
  if (!item) return null;
  const c = (Array.isArray(item.choices) ? item.choices : []).find((x) => x && x.safe === 'yes');
  return c && typeof c.choice === 'string' ? c.choice : '';
}

async function walkCheck(h) {
  const page = h.page;
  const key = h.build.walkKey;
  await page.evaluate(HELPER);
  const b = await page.evaluate(() => window.__r4w.begin());
  const misses = new Set();
  const confirms = confirmItems(h);
  if (confirms.length > 0 && b.dialogs.length === 0) misses.add('dialog_missing');
  for (const p of b.dialogs) {
    if (!p.modal) misses.add('dialog_not_modal');
    if (!p.label) misses.add('dialog_no_label');
    if (!p.description) misses.add('dialog_no_description');
    if (!p.inert) misses.add('dialog_not_inert');
  }
  const safeAct = safeChoice(confirms.find((i) => i.item === b.dialogParam) || confirms[0]);
  let safeId = null;
  if (b.dialog) {
    const idx = safeAct === null ? 0 : b.controls.findIndex((c) => c.name === safeAct);
    if (idx !== 0) misses.add('dialog_safe_not_first');
    if (idx >= 0) safeId = b.controls[idx].id;
    if (b.active !== b.dialogId) misses.add('dialog_not_taken');
  }
  const stops = new Map();
  const first = b.active;
  let start = b.dialog && first === b.dialogId ? 0 : first;
  let firstStop = null;
  let back = false;
  let body = false;
  const max = b.controls.length * 2 + 8;
  for (let i = 0; i < max; i++) {
    await page.keyboard.press(key);
    await page.evaluate(FINISH);
    const s = await page.evaluate(() => window.__r4w.step());
    if (s.id === 0) {
      if (b.dialog && body) misses.add('dialog_focus_lost');
      body = true;
      continue;
    }
    if (b.dialog && s.self && body) continue;
    if (s.outside) misses.add('dialog_focus_lost');
    if (b.dialog && body && b.controls.length && s.id !== b.controls[0].id) misses.add('dialog_focus_lost');
    body = false;
    if (firstStop === null) {
      firstStop = s.id;
      if (b.dialog && s.id !== safeId) misses.add('dialog_safe_not_first');
    }
    if (!stops.has(s.id)) stops.set(s.id, { style: s.style, ring: s.ring });
    if (start === 0) {
      start = s.id;
      continue;
    }
    if (s.id === start) {
      back = true;
      break;
    }
  }
  const groups = new Map();
  for (const c of b.controls) {
    if (c.group) {
      groups.set(c.group, (groups.get(c.group) || false) || stops.has(c.id));
    } else if (!stops.has(c.id)) {
      misses.add('unreached');
    }
  }
  for (const reached of groups.values()) if (!reached) misses.add('unreached');
  if (b.controls.some((c, i) => c.group && b.controls.findIndex((d) => d.group === c.group) !== i)) {
    const fresh = await again(h);
    try {
      await fresh.page.evaluate(HELPER);
      const fb = await fresh.page.evaluate(() => window.__r4w.begin());
      const members = new Map();
      for (const c of fb.controls) if (c.group) members.set(c.group, [...(members.get(c.group) || []), c.id]);
      for (const ids of members.values()) {
        if (ids.length < 2) continue;
        const got = new Set();
        if (!(await fresh.page.evaluate((id) => window.__r4w.focus(id), ids[0]))) {
          misses.add('radio_arrows');
          continue;
        }
        for (const arrow of ['ArrowDown', 'ArrowUp']) {
          for (let i = 1; i < ids.length; i++) {
            await fresh.page.keyboard.press(arrow);
            got.add(await fresh.page.evaluate((list) => window.__r4w.chosen(list), ids));
          }
        }
        if (!ids.every((id) => got.has(id))) misses.add('radio_arrows');
      }
    } finally {
      await fresh.close();
    }
  }
  const ids = [...stops.keys()];
  await page.evaluate(() => document.activeElement && document.activeElement.blur && document.activeElement.blur());
  await page.evaluate(FINISH);
  const rest = await page.evaluate((list) => window.__r4w.rest(list), ids);
  ids.forEach((id, i) => {
    const word = shownFocus(stops.get(id), rest[i]);
    if (word) misses.add(word);
  });
  if (!back && b.controls.length > 0 && stops.size === 0) misses.add('unreached');
  if (b.dialog && body) misses.add('dialog_focus_lost');
  if (b.dialog) {
    const fresh = await again(h);
    try {
      await fresh.page.evaluate(HELPER);
      const fb = await fresh.page.evaluate(() => window.__r4w.begin());
      const target = safeAct === null ? fb.controls[0] : fb.controls.find((c) => c.name === safeAct);
      if (fb.dialog) await fresh.page.keyboard.press(key);
      const reached = fb.dialog && target ? await fresh.page.evaluate((id) => document.activeElement === window.__r4w.element(id), target.id) : false;
      if (!reached) {
        misses.add('dialog_not_closed');
      } else {
        const marks = await fresh.page.evaluate(() => window.__r4w.mark());
        await fresh.page.keyboard.press('Enter');
        await settle(fresh);
        if (!(await fresh.page.evaluate((args) => window.__r4w.closed(args[0], args[1]), [marks, confirms.length > 1]))) misses.add('dialog_not_closed');
      }
    } finally {
      await fresh.close();
    }
  }
  return { ok: misses.size === 0, counts: { keyboard: misses.size ? 'miss' : 'ok' }, names: [...misses].sort() };
}

function structure(nodes, depth = 0, lines = []) {
  for (const node of nodes || []) {
    const own = {};
    for (const k of Object.keys(node).sort()) if (k !== 'children') own[k] = node[k];
    lines.push('  '.repeat(depth) + '- ' + JSON.stringify(own));
    structure(node.children, depth + 1, lines);
  }
  return depth === 0 ? lines.join('\n') + '\n' : lines;
}

async function structureCheck(h, { snapshot, write } = {}) {
  if (typeof snapshot !== 'string') throw new Refusal('usage', 'check 3 needs {snapshot: <path of the structure file>}');
  const text = structure(await h.page.locator('body').ariaSnapshotJSON());
  if (write) {
    await writeFile(snapshot, text);
    return { ok: true, counts: { structure: 'same' }, names: [] };
  }
  let want;
  try {
    want = await readFile(snapshot, 'utf8');
  } catch {
    throw new Refusal('fixture', 'no structure file at the path of snapshot');
  }
  const ok = want === text;
  return { ok, counts: { structure: ok ? 'same' : 'differ' }, names: [] };
}

async function sizeCheck(h) {
  await h.page.evaluate(HELPER);
  const small = await h.page.evaluate(() => window.__r4w.sizes());
  const ok = small.length === 0;
  return { ok, counts: { size: ok ? 'ok' : 'miss' }, names: [...new Set(small)].sort() };
}

async function modesCheck(h, { snapshot } = {}) {
  const off = [];
  const modes = {};
  for (const [name, context, probe] of MODES) {
    const m = await again(h, { ...h.context, ...context });
    try {
      if (!(await m.page.evaluate(probe))) {
        off.push(name);
        modes[name] = 'off';
        continue;
      }
      const results = [];
      try {
        results.push(await engineCheck(m));
      } catch (err) {
        if (!(err instanceof EngineNone)) throw err;
        if (!(await machineWide(m, err))) results.push({ ok: false });
      }
      results.push(await walkCheck(m), await sizeCheck(m));
      if (typeof snapshot === 'string') results.push(await structureCheck(m, { snapshot }));
      const scroll = await m.page.evaluate((every) => {
        const main = document.scrollingElement || document.documentElement;
        if (main.scrollWidth > main.clientWidth) return true;
        const list = every ? document.querySelectorAll('*') : [document.documentElement, document.body];
        return [...list].some((e) => e && ['auto', 'scroll'].includes(getComputedStyle(e).overflowX) && e.scrollWidth > e.clientWidth);
      }, name === 'width_320');
      modes[name] = scroll || m.flags.size > 0 || results.some((r) => !r.ok) ? 'miss' : 'ok';
    } finally {
      await m.close();
    }
  }
  h.modes = modes;
  const ok = !Object.values(modes).includes('miss');
  return { ok, counts: { modes: ok ? 'ok' : 'miss', modes_off: off.length ? off.join(',') : '-' }, names: off };
}

const COUNT = () => ({ elements: document.getElementsByTagName('*').length, injected: document.querySelectorAll('[data-x]').length });

async function markupCheck(h) {
  const plain = await again(h, h.context, null);
  let a;
  try {
    a = await plain.page.evaluate(COUNT);
  } finally {
    await plain.close();
  }
  const marked = await again(h, h.context, (state) => markup(state, ''));
  let b;
  try {
    b = await marked.page.evaluate(COUNT);
  } finally {
    await marked.close();
  }
  const ok = a.elements === b.elements && b.injected === 0;
  return { ok, counts: { markup: ok ? 'ok' : 'miss' }, names: ok ? [] : ['elements'] };
}

const OPAQUE = () => window.__r4opaque === true || [...document.querySelectorAll('*')].some((e) => e.shadowRoot) || document.querySelector('iframe, frame, object, embed, fencedframe, portal') !== null;

const UNSEEN = {
  2: { keyboard: 'miss' },
  3: { structure: 'differ' },
  4: { size: 'miss' },
  5: { modes: 'miss', modes_off: MODES.map(([name]) => name).join(',') },
  6: { markup: 'miss' },
};

const MISSWORD = { 2: { keyboard: 'miss' }, 3: { structure: 'differ' }, 4: { size: 'miss' }, 5: { modes: 'miss' }, 6: { markup: 'miss' } };

function taint(h, n, r) {
  const flags = new Set(h.flags);
  for (const kid of h.kids) for (const f of kid.flags) flags.add(f);
  if (!flags.size) return r;
  return { ok: false, counts: { ...r.counts, ...(MISSWORD[n] || {}) }, names: [...new Set([...r.names, ...flags])].sort() };
}

export async function check(h, n, options = {}) {
  if (!h || !h.page || h.closed) throw new Refusal('usage', 'check(page, n, {snapshot, write}) needs an open page');
  if (!Number.isInteger(n) || n < 1 || n > 6) throw new Refusal('usage', 'check(page, n, {snapshot, write}) takes n from 1 to 6');
  unheld(h, 'check');
  await tamper(h);
  h.kids = [];
  if (UNSEEN[n] && !(options.write && n === 3)) {
    if (h.flags.has('unsettled')) return { ok: false, counts: { ...UNSEEN[n] }, names: ['unsettled'] };
    if (h.word || (await h.page.evaluate(OPAQUE))) return { ok: false, counts: { ...UNSEEN[n] }, names: ['opaque'] };
  }
  const result = taint(h, n, await run(h, n, options));
  unheld(h, 'check');
  return result;
}

async function run(h, n, options) {
  switch (n) {
    case 1:
      return engineCheck(h);
    case 2:
      return walkCheck(h);
    case 3:
      return structureCheck(h, options);
    case 4:
      return sizeCheck(h);
    case 5:
      return modesCheck(h, options);
    case 6:
      return markupCheck(h);
    default:
      throw new Refusal('usage', 'check(page, n, {snapshot, write}) takes n from 1 to 6');
  }
}
