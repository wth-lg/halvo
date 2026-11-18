import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { scratchDir } from './servers.mjs';

const CHECKER = fileURLToPath(new URL('./check-sites.mjs', import.meta.url));
const MARKER = 'qzmq';
const ROOT = scratchDir('fixture-server-check-sites-');
after(ROOT.remove);

function marked() {
  return {
    v: 1,
    service: `${MARKER}-shop`,
    category: 'shop_order',
    kind: 'web',
    host: `${MARKER}-shop.test`,
    app: null,
    parameters: [`${MARKER}_item`, `${MARKER}_size`],
    values: { [`${MARKER}_item`]: ['blue cup'], [`${MARKER}_size`]: ['small'] },
    pages: [
      { page: `${MARKER}-sign-in`, title: 'Sign in - Acorn', text: 'Enter your password.', password_input: true, card_title: false },
      { page: `${MARKER}-item`, title: `{${MARKER}_item} - Acorn`, text: `One {${MARKER}_item}, size {${MARKER}_size}.`, password_input: false, card_title: false },
      { page: `${MARKER}-pay`, title: 'Pay 4242 4242 4242 4242 - Acorn', text: 'Paying.', password_input: false, card_title: true },
      { page: `${MARKER}-browse`, title: 'Browse - Acorn', text: 'All items.', password_input: false, card_title: false },
    ],
    flows: [
      { flow: `${MARKER}-buy`, role: 'task', pages: [`${MARKER}-sign-in`, `${MARKER}-item`, `${MARKER}-pay`] },
      { flow: `${MARKER}-look`, role: 'near_miss', pages: [`${MARKER}-sign-in`, `${MARKER}-browse`] },
    ],
  };
}

function news() {
  return {
    v: 1,
    service: `${MARKER}-news`,
    category: 'other',
    kind: 'web',
    host: `${MARKER}-news.test`,
    app: null,
    parameters: [],
    values: {},
    pages: [{ page: `${MARKER}-home`, title: 'News - Acorn', text: 'Stories.', password_input: false, card_title: false }],
    flows: [{ flow: `${MARKER}-read`, role: 'other', pages: [`${MARKER}-home`] }],
  };
}

function put(dir, name, value) {
  const path = join(dir, name);
  writeFileSync(path, `${JSON.stringify(value)}\n`);
  return path;
}

function runChecker(args) {
  return spawnSync(process.execPath, [CHECKER, ...args], { encoding: 'utf8' });
}

test('checker: the password, card and missing lines name a site file, a page and a parameter by number, never by name', () => {
  const dir = mkdtempSync(join(ROOT.dir, `${MARKER}-`));
  const shop = put(dir, `${MARKER}-shop.json`, marked());
  const other = put(dir, `${MARKER}-news.json`, news());
  const rows = join(dir, 'rows.md');
  writeFileSync(rows, `| category | parameter | source |\n|---|---|---|\n| shop_order | ${MARKER}_item | open |\n`);
  const run = runChecker(['--parameters', rows, shop, other]);
  assert.equal(run.stderr, '');
  assert.equal(run.status, 1);
  assert.doesNotMatch(run.stdout, new RegExp(MARKER));
  const named = run.stdout.split('\n').filter((line) => /^(password|card|missing) /.test(line));
  assert.deepEqual(named, [
    'password SITE_DATA 2 page 1',
    'card SITE_DATA 2 page 3',
    'missing SITE_DATA 2 shop_order parameter 2',
  ]);
  assert.match(run.stdout, /^parameters names=2 missing=1$/m);
});
