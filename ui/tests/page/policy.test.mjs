// The app's page under the app's own policy: the csp of src-tauri/tauri.conf.json, sent as the
// page's Content-Security-Policy header, in the headless browser of tools/r4-page. A person sees
// the needs view, the agents view and the question of confirm.js with their own styles: the
// status region .hn-say of the needs view is hidden from sight, a state dot of the agents view has
// its size, and the question that the delete control of a row opens has its border and buttons.
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
} finally {
  await browser.close();
  server.close();
}
console.log('policy tests: ' + ran + ', failed: ' + failed + ', browser: ' + build.browser);
process.exitCode = failed === 0 && ran === 3 ? 0 : 1;
