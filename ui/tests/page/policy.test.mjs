// The app's page under the app's own policy: the csp of src-tauri/tauri.conf.json, sent as the
// page's Content-Security-Policy header, in the headless browser of tools/r4-page. A person sees
// the needs view, the agents view and the question of confirm.js with their own styles: the
// status region .hn-say of the needs view is hidden from sight, a state dot of the agents view has
// its size, and the question that the delete control of a row opens has its border and buttons.
// The list "What Halvo noticed" reads again every 15 s while the window shows and shows a new row,
// reads nothing while the window is hidden (a hiding played by visibilityState, as no window exists),
// with the page's clock paused so that only the test's own steps move it,
// and a read that brings the list that is drawn leaves an open Delete question open.
// Run: R4_PAGE_BROWSERS=<the fetched browsers> node ui/tests/page/policy.test.mjs
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { dirname, extname, join, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..', '..', '..');
const APP = join(ROOT, 'ui', 'app');
const FIXTURES = join(ROOT, 'ui', 'tests', 'fixtures');
const policy = JSON.parse(readFileSync(join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).app.security.csp;
const { installed } = await import(pathToFileURL(join(ROOT, 'tools', 'r4-page', 'machine.mjs')).href);

const fixture = (dir, name) => JSON.parse(readFileSync(join(FIXTURES, dir, name), 'utf8'));
const replies = {
  capture_status: fixture('capture', 'capture_status-running.json'),
  inventory: fixture('capture', 'inventory-empty.json'),
  onboarding_state: fixture('capture', 'onboarding_state-none.json'),
  last_delete: fixture('capture', 'last_delete-none.json'),
  'plugin:app|version': '0.0.0',
  agents_snapshot: fixture('coord', 'state-ten.json').snapshot,
  agents_next: { closed: 'yes' },
};
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8' };

const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://127.0.0.1').pathname;
  const file = resolve(APP, '.' + path);
  let body = null;
  try {
    body = file.startsWith(APP + sep) ? readFileSync(file) : null;
  } catch {
    body = null;
  }
  if (body === null) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, {
    'content-type': TYPES[extname(file)] || 'application/octet-stream',
    'content-security-policy': policy,
    'cache-control': 'no-store',
  });
  res.end(body);
});
await new Promise((done) => server.listen(0, '127.0.0.1', done));
const origin = 'http://127.0.0.1:' + server.address().port;

const build = installed();
const pw = await import(pathToFileURL(build.pkg).href);
const browser = await pw[build.browser].launch({ headless: true, executablePath: build.exe });
let failed = 0;
let ran = 0;
const result = (name, ok, detail) => {
  ran++;
  if (!ok) failed++;
  console.log('policy test=' + name + ' ' + (ok ? 'ok' : 'fail') + ' detail=' + JSON.stringify(detail));
};
try {
  const page = await (await browser.newContext()).newPage();
  await page.addInitScript((given) => {
    window.__TAURI_INTERNALS__ = {
      invoke: (cmd) => (cmd in given ? Promise.resolve(JSON.parse(JSON.stringify(given[cmd]))) : Promise.reject('no reply for ' + cmd)),
    };
  }, replies);
  await page.goto(origin + '/index.html');
  await page.waitForSelector('.hn-say', { state: 'attached' });
  await page.waitForSelector('.r4a-icon', { state: 'attached' });
  const seen = await page.evaluate(() => {
    const box = (el) => {
      const r = el.getBoundingClientRect();
      return { width: r.width, height: r.height, position: getComputedStyle(el).position };
    };
    return { say: box(document.querySelector('.hn-say')), dot: box(document.querySelector('.r4a-icon')) };
  });
  result('hn-say-hidden', seen.say.position === 'absolute' && seen.say.width <= 1 && seen.say.height <= 1, seen.say);
  result('state-dot-size', seen.dot.width >= 10 && seen.dot.height >= 10, seen.dot);
  await page.click('button[data-act="delete"]');
  await page.waitForSelector('dialog.hc-dialog[open]');
  const asked = await page.evaluate(() => {
    const dialog = document.querySelector('dialog.hc-dialog[open]');
    const button = dialog.querySelector('.hc-button');
    return { border: getComputedStyle(dialog).borderTopWidth, minHeight: button ? getComputedStyle(button).minHeight : null };
  });
  result('question-styles', asked.border === '2px' && asked.minHeight === '44px', asked);

  const listPage = await (await browser.newContext()).newPage();
  const listStart = Date.UTC(2026, 10, 26, 12, 0, 0);
  await listPage.clock.install({ time: listStart });
  await listPage.clock.pauseAt(listStart + 1000);
  await listPage.addInitScript((given) => {
    let reads = 0;
    let hidden = false;
    const kept = { state: 'running', proposals: [], stored: [given.stored] };
    const grown = { state: 'running', proposals: [given.proposal], stored: [given.stored] };
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (hidden ? 'hidden' : 'visible') });
    window.__list = {
      reads: () => reads,
      window: (state) => {
        hidden = state === 'hidden';
        document.dispatchEvent(new Event('visibilitychange'));
      },
    };
    window.__TAURI_INTERNALS__ = {
      invoke: (cmd) => {
        if (cmd === 'workflows') {
          reads += 1;
          return Promise.resolve(JSON.parse(JSON.stringify(reads < given.grownAt ? kept : grown)));
        }
        return cmd in given.replies ? Promise.resolve(JSON.parse(JSON.stringify(given.replies[cmd]))) : Promise.reject('no reply for ' + cmd);
      },
    };
  }, {
    replies,
    grownAt: 3,
    stored: { id: 'fedcba9876543210fedcba9876543210', category: 'email' },
    proposal: { id: '0123456789abcdef0123456789abcdef', category: 'email', app: 'Mail', site: 'example.com', count: 5 },
  });
  await listPage.goto(origin + '/index.html');
  await listPage.waitForSelector('#stored-workflows .stored-workflow', { state: 'attached' });
  const listNow = () =>
    listPage.evaluate(() => {
      const question = document.querySelector('#stored-workflows .confirm-row');
      return {
        reads: window.__list.reads(),
        rows: document.querySelectorAll('#proposals-list .proposal').length,
        open: Boolean(question) && !question.classList.contains('hidden'),
      };
    });
  const steps = { first: await listNow() };
  await listPage.evaluate(() => document.querySelector('#stored-workflows .stored-workflow button').click());
  await listPage.clock.runFor(15000);
  steps.same = await listNow();
  await listPage.clock.runFor(15000);
  steps.grown = await listNow();
  await listPage.evaluate(() => window.__list.window('hidden'));
  await listPage.clock.runFor(60000);
  steps.hidden = await listNow();
  const listOk =
    steps.first.reads === 1 && steps.first.rows === 0 &&
    steps.same.reads === 2 && steps.same.open &&
    steps.grown.reads === 3 && steps.grown.rows === 1 &&
    steps.hidden.reads === 3;
  result('list-reads-again', listOk, steps);
} finally {
  await browser.close();
  server.close();
}
console.log('policy tests passed=' + (ran - failed) + ' failed=' + failed + ' browser=' + build.browser);
process.exitCode = failed === 0 && ran === 4 ? 0 : 1;
