// usage: R4_PAGE_BROWSERS=DIR node tools/r4-trials/floor/headless.mjs [--browser webkit|chromium] [--list FILE] [--no-policy] [--without NAME]
// Loads probe.html in the headless browser of U0.1's harness, fetched into DIR by
// tools/r4-page/fetch.sh, under the Content-Security-Policy of the trial app's scheme (POLICY,
// the text of policy() in tools/r4-trials/app/src/pages.rs) and with a stand-in for call.js, and
// prints the probe's line. With --list the probe is made from FILE instead of probe.js; with
// --no-policy the responses carry no policy, the control of a reading; with --without NAME a script
// of this tool, served after the stand-in for call.js and before probe.js, deletes the global NAME,
// so that the probe runs in a page that lacks that interface. No window is shown.
// Exit: 0 when the probe reported, 1 when it did not, 2 on a refusal.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { installed, refuse, Refusal } from '../../r4-page/machine.mjs';
import { parseList, probeSource } from './list.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ORIGIN = 'http://floor.localhost';
export const POLICY = `default-src 'none'; script-src 'self' ${ORIGIN}; connect-src ipc://localhost; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`;
const STAND_IN = "window.__floorLines = []; window.r4app = { report: function (line) { window.__floorLines.push(String(line)); } };";

/** The stand-in for call.js, followed by the deletion of the global `without` where one is named. */
export function standIn(without) {
  return without ? `${STAND_IN}\ndelete window.${without};\n` : STAND_IN;
}

function parseArgs(argv) {
  const options = { browser: 'webkit', list: null, policy: true, without: null };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--browser' && (argv[i + 1] === 'webkit' || argv[i + 1] === 'chromium')) options.browser = argv[++i];
    else if (argv[i] === '--list' && argv[i + 1]) options.list = argv[++i];
    else if (argv[i] === '--no-policy' && options.policy) options.policy = false;
    else if (argv[i] === '--without' && /^[A-Za-z][A-Za-z0-9]*$/.test(argv[i + 1] ?? '') && !options.without) options.without = argv[++i];
    else throw new Refusal('usage', 'node tools/r4-trials/floor/headless.mjs [--browser webkit|chromium] [--list FILE] [--no-policy] [--without NAME]');
  }
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  let probe = fs.readFileSync(path.join(HERE, 'probe.js'), 'utf8');
  if (options.list) {
    const { features, problems } = parseList(fs.readFileSync(options.list, 'utf8'));
    if (problems.length) throw new Refusal('usage', 'the list breaks its form');
    probe = probeSource(features);
  }
  const build = installed(options.browser);
  const pw = await import(pathToFileURL(build.pkg).href);
  const browser = await pw[build.browser].launch({ headless: true, executablePath: build.exe });
  try {
    const page = await (await browser.newContext()).newPage();
    const bodies = {
      '/probe.html': ['text/html; charset=utf-8', fs.readFileSync(path.join(HERE, 'probe.html'))],
      '/probe.js': ['text/javascript; charset=utf-8', probe],
      '/__app/call.js': ['text/javascript; charset=utf-8', standIn(options.without)],
    };
    const policy = options.policy ? { 'Content-Security-Policy': POLICY } : {};
    await page.route('**/*', (route) => {
      const url = new URL(route.request().url());
      const body = url.origin === ORIGIN ? bodies[url.pathname] : undefined;
      if (!body) return route.fulfill({ status: 404, headers: policy, body: '' });
      return route.fulfill({ status: 200, headers: { 'Content-Type': body[0], ...policy }, body: body[1] });
    });
    await page.goto(`${ORIGIN}/probe.html`);
    const lines = await page.evaluate(() => window.__floorLines);
    const line = lines.find((l) => l.startsWith('probe ')) ?? null;
    process.stdout.write(`headless browser=${build.browser} version=${browser.version()} policy=${options.policy ? 'app' : 'none'} without=${options.without ?? '-'} ${line ?? 'probe=none'}\n`);
    process.exitCode = line ? 0 : 1;
  } finally {
    await browser.close();
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    if (refuse(err)) return;
    process.stdout.write(`headless failed error=${String(err && err.name)}\n`);
    process.exitCode = 1;
  });
}
