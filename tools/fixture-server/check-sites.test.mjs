import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { scratchDir } from './servers.mjs';

const CHECKER = fileURLToPath(new URL('./check-sites.mjs', import.meta.url));
const SERVER = fileURLToPath(new URL('./server.mjs', import.meta.url));
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

/**
 * Runs `script` with `args` and, last, one argument that holds a byte that is no UTF-8: 0xFF through sh on Unix,
 * a lone surrogate on Windows, which reaches the child as U+FFFD.
 */
function runWithBadLast(script, args) {
  if (process.platform === 'win32') {
    return spawnSync(process.execPath, [script, ...args, 'bad\uD800.json'], { encoding: 'utf8', input: '' });
  }
  return spawnSync('/bin/sh', ['-c', 'exec "$@" "$(printf "bad\\377.json")"', 'sh', process.execPath, script, ...args], {
    encoding: 'utf8',
    input: '',
  });
}

test('arguments: the checker and the server refuse an argument that is no UTF-8 with usage, before they read a file', () => {
  const dir = mkdtempSync(join(ROOT.dir, 'utf8-'));
  const good = put(dir, 'acorn-news.json', news());
  const missing = join(dir, 'missing.json');
  for (const [script, args] of [
    [CHECKER, [missing]],
    [CHECKER, [good]],
    [CHECKER, [good, '--parameters']],
    [SERVER, [missing]],
    [SERVER, ['--port', '0', good]],
  ]) {
    const run = runWithBadLast(script, args);
    assert.equal(run.status, 2, `${run.stdout}${run.stderr}`);
    assert.equal(run.stdout, '');
    assert.match(run.stderr, /^REFUSED usage: [^\n]*\n$/);
  }
});

/** A shop whose first parameter holds `n` values, written compact. */
function wide(n) {
  const site = marked();
  site.values[`${MARKER}_item`] = Array.from({ length: n }, (_, i) => `v${i}`);
  return site;
}

test('checker: a site file of about 1,500,000 values is refused with too-large and no RangeError, and one under the bound is checked', () => {
  const dir = mkdtempSync(join(ROOT.dir, 'wide-'));
  const other = put(dir, 'a-news.json', news());
  const big = put(dir, 'b-shop.json', wide(1_500_000));
  const run = runChecker([other, big]);
  assert.equal(run.status, 2, run.stderr);
  assert.equal(run.stdout, '');
  assert.equal(run.stderr, 'REFUSED too-large: SITE_DATA 2\n');
  const under = put(dir, 'c-shop.json', wide(29_000));
  assert.ok(statSync(under).size <= 262_144 && statSync(under).size > 200_000, String(statSync(under).size));
  const checked = runChecker([under]);
  assert.equal(checked.stderr, '');
  assert.equal(checked.status, 0);
  assert.match(checked.stdout, /^password SITE_DATA 1 page 1$/m);
  const exact = wide(29_000);
  const list = exact.values[`${MARKER}_item`];
  let need = 262_144 - Buffer.byteLength(`${JSON.stringify(exact)}\n`);
  for (let j = list.length - 1; need > 0; j -= 1) {
    const add = Math.min(need, 64 - list[j].length);
    list[j] += 'x'.repeat(add);
    need -= add;
  }
  const atBound = put(dir, 'd-shop.json', exact);
  assert.equal(statSync(atBound).size, 262_144);
  const bound = runChecker([atBound]);
  assert.equal(bound.stderr, '');
  assert.equal(bound.status, 0);
  list[0] += 'x';
  const over = put(dir, 'e-shop.json', exact);
  assert.equal(statSync(over).size, 262_145);
  assert.equal(runChecker([over]).stderr, 'REFUSED too-large: SITE_DATA 1\n');
});

test('checker: over site files whose names, ids and parameters hold the marker, every line it prints holds numbers, fixed words and arguments alone', () => {
  const dir = mkdtempSync(join(ROOT.dir, 'lines-'));
  const shop = put(dir, `${MARKER}-shop.json`, marked());
  const twice = marked();
  twice.pages[3].page = twice.pages[2].page;
  const broken = put(dir, `${MARKER}-twice.json`, twice);
  const big = put(dir, `${MARKER}-big.json`, wide(40_000));
  const gone = join(dir, `${MARKER}-gone.json`);
  const rows = join(dir, 'rows.md');
  const runs = [
    [[shop], 0, null],
    [[shop, broken], 2, 'REFUSED site-data: SITE_DATA 2\n'],
    [[shop, big], 2, 'REFUSED too-large: SITE_DATA 1\n'],
    [[gone], 2, 'REFUSED site-data: SITE_DATA 1\n'],
    [['--parameters', rows, shop], 2, `REFUSED parameters: ${rows}\n`],
    [[], 2, null],
  ];
  const forms = [
    /^checked files=\d+$/,
    /^services [a-z_]+ \d+$/,
    /^(password|card) SITE_DATA \d+ page \d+$/,
    /^parameters names=\d+ missing=\d+$/,
    /^missing SITE_DATA \d+ [a-z_]+ parameter \d+$/,
  ];
  for (const [args, status, stderr] of runs) {
    const run = runChecker(args);
    assert.equal(run.status, status, `${args.length} ${run.stderr}`);
    assert.doesNotMatch(run.stdout + run.stderr, new RegExp(MARKER));
    if (stderr !== null) assert.equal(run.stderr, stderr);
    if (status === 0) {
      for (const line of run.stdout.trimEnd().split('\n')) {
        assert.ok(forms.some((form) => form.test(line)), line);
      }
    } else {
      assert.equal(run.stdout, '');
      assert.match(run.stderr, /^REFUSED [a-z-]+: [^\n]*\n$/);
    }
  }
});

test('checker: a site file that is no JSON, no UTF-8, a directory or unreadable, and a fault of its own, print no name and no text of the file', () => {
  const dir = mkdtempSync(join(ROOT.dir, 'broken-'));
  const good = put(dir, `${MARKER}-good.json`, news());
  const noJson = join(dir, `${MARKER}-nojson.json`);
  writeFileSync(noJson, `${MARKER} is no json\n`);
  const noUtf8 = join(dir, `${MARKER}-noutf8.json`);
  writeFileSync(noUtf8, Buffer.from([0x71, 0x7a, 0x6d, 0x71, 0xff, 0x0a]));
  const folder = join(dir, `${MARKER}-folder.json`);
  mkdirSync(folder);
  const cases = [[noJson, 2], [noUtf8, 2], [folder, 1]];
  if (process.platform !== 'win32') {
    const closed = put(dir, `${MARKER}-closed.json`, marked());
    chmodSync(closed, 0o000);
    cases.push([closed, 1]);
  }
  for (const [path, number] of cases) {
    const run = runChecker([good, path]);
    assert.equal(run.status, 2, path);
    assert.equal(run.stdout, '');
    assert.equal(run.stderr, `REFUSED site-data: SITE_DATA ${number}\n`, path);
  }
  const fault = `data:text/javascript,${encodeURIComponent(`Buffer.byteLength = () => { throw new TypeError('${MARKER} fault'); };`)}`;
  const run = spawnSync(process.execPath, [`--import=${fault}`, CHECKER, good], { encoding: 'utf8' });
  assert.equal(run.status, 2, run.stderr);
  assert.equal(run.stdout, '');
  assert.equal(run.stderr, 'REFUSED internal: the checker stopped at a fault of its own\n');
});
