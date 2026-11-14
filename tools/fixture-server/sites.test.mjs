import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { scratchDir } from './servers.mjs';
import { encodeValue, fill, isPort, loadSites as loadSitesOf, parseSiteData as parseSiteBytes, readArgs, requestsOf } from './sites.mjs';

const CHECKER = fileURLToPath(new URL('./check-sites.mjs', import.meta.url));
const CASES = fileURLToPath(new URL('./site-cases.jsonl', import.meta.url));
const fixtures = new Map();
let listed = null;

function caseKey(files) {
  const hash = createHash('sha256');
  for (const file of files) {
    hash.update(`${file.length}\n`);
    hash.update(file);
  }
  return hash.digest('hex');
}

function sharedCases() {
  const bases = new Map();
  const cases = [];
  const text = readFileSync(CASES, 'utf8');
  assert.ok(text.endsWith('\n'));
  for (const line of text.slice(0, -1).split('\n')) {
    const value = JSON.parse(line);
    if (value.text !== undefined) {
      bases.set(value.base, Buffer.from(`${value.text}\n`, 'utf8'));
    } else {
      const files = value.files.map((f) => {
        const base = f.base === null ? Buffer.alloc(0) : bases.get(f.base);
        assert.ok(base !== undefined && f.head + f.tail <= base.length, value.case);
        return Buffer.concat([base.subarray(0, f.head), Buffer.from(f.put, 'hex'), base.subarray(base.length - f.tail)]);
      });
      cases.push({ name: value.case, taken: value.taken, rule: value.rule, files });
    }
  }
  return cases;
}

function check(files, taken, rule) {
  const key = caseKey(files);
  fixtures.set(key, { files, taken, rule });
  if (listed === null) listed = new Map(sharedCases().map((c) => [caseKey(c.files), c]));
  const c = listed.get(key);
  assert.ok(
    c !== undefined && c.taken === taken && (taken || files.length > 1 || c.rule === rule),
    'a fixture that tools/fixture-server/site-cases.jsonl lacks or holds with another verdict or rule',
  );
}

function readAll(paths) {
  try {
    return paths.map((path) => readFileSync(path));
  } catch {
    return null;
  }
}

function parseSiteData(buffer) {
  let site;
  try {
    site = parseSiteBytes(buffer);
  } catch (error) {
    if (error.name === 'SiteDataError') check([buffer], false, error.rule);
    throw error;
  }
  check([buffer], true, null);
  return site;
}

function loadSites(paths) {
  const result = loadSitesOf(paths);
  const files = readAll(paths);
  if (files !== null) check(files, result.refused === undefined, null);
  return result;
}

function shop() {
  return {
    v: 1,
    service: 'acorn-shop',
    category: 'shop_order',
    kind: 'web',
    host: 'acorn-shop.test',
    app: null,
    parameters: ['item', 'size'],
    values: { item: ['blue cup', 'tea & honey'], size: ['small'] },
    pages: [
      { page: 'sign-in', title: 'Sign in - Acorn', text: 'Enter your password.', password_input: true, card_title: false },
      { page: 'item', title: '{item} - Acorn', text: 'One {item}, size {size}.', password_input: false, card_title: false },
      { page: 'pay', title: 'Pay 4242 4242 4242 4242 - Acorn', text: 'Paying.', password_input: false, card_title: true },
      { page: 'browse', title: 'Browse - Acorn', text: 'All items.', password_input: false, card_title: false },
    ],
    flows: [
      { flow: 'buy', role: 'task', pages: ['sign-in', 'item', 'pay'] },
      { flow: 'look', role: 'near_miss', pages: ['sign-in', 'browse'] },
    ],
  };
}

function native() {
  return {
    v: 1,
    service: 'acorn-sheet',
    category: 'spreadsheet_fill',
    kind: 'native',
    host: null,
    app: 'org.libreoffice.script',
    parameters: ['sheet'],
    values: { sheet: ['budget'] },
    pages: [{ page: 'book', title: '{sheet}.ods - LibreOffice Calc', text: 'Rows.', password_input: false, card_title: false }],
    flows: [
      { flow: 'fill', role: 'task', pages: ['book'] },
      { flow: 'read', role: 'near_miss', pages: ['book'] },
    ],
  };
}

function other() {
  return {
    v: 1,
    service: 'acorn-news',
    category: 'other',
    kind: 'web',
    host: 'acorn-news.test',
    app: null,
    parameters: [],
    values: {},
    pages: [{ page: 'home', title: 'News - Acorn', text: 'Stories.', password_input: false, card_title: false }],
    flows: [{ flow: 'read', role: 'other', pages: ['home'] }],
  };
}

function bytes(value) {
  return Buffer.from(`${JSON.stringify(value)}\n`, 'utf8');
}

function refused(buffer, rule) {
  assert.throws(() => parseSiteData(buffer), (error) => {
    assert.equal(error.name, 'SiteDataError');
    assert.equal(error.rule, rule);
    return true;
  });
}

function changed(make, edit) {
  const value = make();
  edit(value);
  return bytes(value);
}

test('site data: the three fixtures are of C11.1', () => {
  for (const make of [shop, native, other]) {
    const value = make();
    assert.deepEqual(parseSiteData(bytes(value)), value);
  }
});

test('site data: a space between tokens is refused (form)', () => {
  const text = bytes(shop()).toString('utf8').replace('"v":1,', '"v":1, ');
  refused(Buffer.from(text, 'utf8'), 'form');
});

test('site data: pretty printing is refused (form)', () => {
  refused(Buffer.from(`${JSON.stringify(shop(), null, 2)}\n`, 'utf8'), 'form');
});

test('site data: a key that stands twice is refused (form)', () => {
  const text = bytes(shop()).toString('utf8').replace('"v":1,', '"v":1,"v":1,');
  refused(Buffer.from(text, 'utf8'), 'form');
});

test('site data: a missing final line feed, a carriage return and a second line are refused (form)', () => {
  const text = bytes(shop()).toString('utf8');
  refused(Buffer.from(text.slice(0, -1), 'utf8'), 'form');
  refused(Buffer.from(`${text.slice(0, -1)}\r\n`, 'utf8'), 'form');
  refused(Buffer.from(`${text}\n`, 'utf8'), 'form');
});

test('site data: a byte-order mark is refused (form)', () => {
  refused(Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), bytes(shop())]), 'form');
});

test('site data: an escape that the compact rewriting does not write is refused (form)', () => {
  const text = bytes(shop()).toString('utf8').replace('"Browse - Acorn"', '"Br\\u006fwse - Acorn"');
  refused(Buffer.from(text, 'utf8'), 'form');
  const slash = bytes(shop()).toString('utf8').replace('"All items."', '"All\\/items."');
  refused(Buffer.from(slash, 'utf8'), 'form');
});

test('site data: a number written another way is refused (form)', () => {
  const text = bytes(shop()).toString('utf8').replace('"v":1,', '"v":1.0,');
  refused(Buffer.from(text, 'utf8'), 'form');
});

test('site data: bytes that are not UTF-8 are refused (utf8)', () => {
  const good = bytes(shop());
  const at = good.indexOf('Browse');
  const bad = Buffer.from(good);
  bad[at] = 0xff;
  refused(bad, 'utf8');
});

test('site data: a lone surrogate is refused (string)', () => {
  const text = bytes(shop()).toString('utf8').replace('"Browse - Acorn"', '"Browse \\ud800 - Acorn"');
  refused(Buffer.from(text, 'utf8'), 'string');
});

test('site data: text that is not JSON, and JSON that is not an object, are refused (json, type)', () => {
  refused(Buffer.from('{"v":1,\n', 'utf8'), 'json');
  refused(Buffer.from('[]\n', 'utf8'), 'type');
  refused(Buffer.from('null\n', 'utf8'), 'type');
});

test('site data: two keys swapped are refused (D1)', () => {
  const { v, service, ...rest } = shop();
  refused(bytes({ service, v, ...rest }), 'D1');
});

test('site data: a missing key, an extra key and a page or flow key out of order are refused (D1)', () => {
  refused(changed(shop, (s) => delete s.app), 'D1');
  refused(changed(shop, (s) => { s.extra = 1; }), 'D1');
  refused(changed(shop, (s) => {
    const { page, title, ...rest } = s.pages[3];
    s.pages[3] = { title, page, ...rest };
  }), 'D1');
  refused(changed(shop, (s) => {
    const { flow, role, pages } = s.flows[1];
    s.flows[1] = { flow, pages, role };
  }), 'D1');
  refused(changed(shop, (s) => { s.pages[0].extra = true; }), 'D1');
});

test('site data: v other than 1 is refused (D1)', () => {
  refused(changed(shop, (s) => { s.v = 2; }), 'D1');
  refused(changed(shop, (s) => { s.v = '1'; }), 'D1');
});

test('site data: a service name outside its form is refused (service)', () => {
  for (const name of ['', 'Acorn-shop', '-acorn', 'acorn-', 'acorn_shop', 'acorn.shop', 'a'.repeat(33)]) {
    refused(changed(other, (s) => { s.service = name; s.host = `${name}.test`; }), 'service');
  }
  assert.equal(parseSiteData(changed(other, (s) => { s.service = 'a'.repeat(32); s.host = `${'a'.repeat(32)}.test`; })).service.length, 32);
});

test('site data: a category outside the closed list is refused (category)', () => {
  refused(changed(other, (s) => { s.category = 'Other'; }), 'category');
  refused(changed(other, (s) => { s.category = 'travel'; }), 'category');
});

test('site data: kind, host and app that do not agree are refused (kind)', () => {
  refused(changed(other, (s) => { s.kind = 'app'; }), 'kind');
  refused(changed(other, (s) => { s.host = 'other.test'; }), 'kind');
  refused(changed(other, (s) => { s.host = 'acorn-news.test.'; }), 'kind');
  refused(changed(other, (s) => { s.host = null; }), 'kind');
  refused(changed(other, (s) => { s.app = 'org.example'; }), 'kind');
  refused(changed(native, (s) => { s.host = 'acorn-sheet.test'; }), 'kind');
  refused(changed(native, (s) => { s.app = null; }), 'kind');
});

test('site data: an app id outside the form of C1.5 is refused (app)', () => {
  for (const app of ['', '.', '..', 'a/b', 'a\\b', 'a\u0007b', 'a\u007fb', 'a\u009bb', 'x'.repeat(256)]) {
    refused(changed(native, (s) => { s.app = app; }), 'app');
  }
  assert.equal(parseSiteData(changed(native, (s) => { s.app = 'soffice.bin'; })).app, 'soffice.bin');
});

test('site data: a parameter name outside the form of C8.2 is refused (parameter)', () => {
  for (const name of ['', 'Item', '1item', 'it-em', 'i'.repeat(33)]) {
    refused(changed(shop, (s) => {
      s.parameters[0] = name;
      s.values = { [name]: ['blue cup'], size: ['small'] };
      s.pages[1].title = `{${name}} - Acorn`;
    }), 'parameter');
  }
});

test('site data: a parameter name that stands twice is refused (D2)', () => {
  refused(changed(shop, (s) => { s.parameters = ['item', 'item']; }), 'D2');
});

test('site data: values whose keys are not the parameters in order are refused (D2)', () => {
  refused(changed(shop, (s) => { s.values = { size: ['small'], item: ['blue cup'] }; }), 'D2');
  refused(changed(shop, (s) => { s.values = { item: ['blue cup'] }; }), 'D2');
  refused(changed(shop, (s) => { s.values = { item: ['blue cup'], size: ['small'], extra: ['x'] }; }), 'D2');
});

test('site data: an empty value list and a value that stands twice are refused (D2)', () => {
  refused(changed(shop, (s) => { s.values.size = []; }), 'D2');
  refused(changed(shop, (s) => { s.values.size = ['small', 'small']; }), 'D2');
});

test('site data: a value outside its form is refused (value)', () => {
  for (const value of ['', 'x'.repeat(65), 'a\tb', 'a\u001fb', 'a\u007fb', 7]) {
    refused(changed(shop, (s) => { s.values.size = [value]; }), 'value');
  }
  assert.equal(parseSiteData(changed(shop, (s) => { s.values.size = ['é'.repeat(32)]; })).values.size[0].length, 32);
  refused(changed(shop, (s) => { s.values.size = ['é'.repeat(32) + 'x']; }), 'value');
});

test('site data: a value written as the escape of a lone surrogate, and a value that holds U+0000, are refused (value)', () => {
  const text = bytes(shop()).toString('utf8');
  assert.ok(text.includes('"size":["small"]'));
  refused(Buffer.from(text.replace('"size":["small"]', '"size":["\\ud800"]'), 'utf8'), 'value');
  refused(Buffer.from(text.replace('"size":["small"]', '"size":["a\\u0000b"]'), 'utf8'), 'value');
  refused(Buffer.from(text.replace('"size":["small"]', '"size":["\\udc00x"]'), 'utf8'), 'value');
});

test('site data: an app that is a number is refused (app)', () => {
  refused(Buffer.from(bytes(native()).toString('utf8').replace('"app":"org.libreoffice.script"', '"app":5'), 'utf8'), 'app');
});

test('site data: a card page whose number stands in its text alone is refused (D7)', () => {
  refused(changed(shop, (s) => { s.pages[2].title = 'Pay - Acorn'; s.pages[2].text = 'Card 4242 4242 4242 4242.'; }), 'D7');
});

test('site data: a file with no page, a page id twice and a flow id twice are refused (D3)', () => {
  refused(changed(other, (s) => { s.pages = []; s.flows = []; }), 'D3');
  refused(changed(shop, (s) => { s.pages[3].page = 'item'; }), 'D3');
  refused(changed(shop, (s) => { s.flows[1].flow = 'buy'; }), 'D3');
});

test('site data: a page or flow id outside its form, and the page id api, are refused (id)', () => {
  for (const id of ['', 'Browse', 'browse_all', 'b'.repeat(33)]) {
    refused(changed(shop, (s) => { s.pages[3].page = id; s.flows[1].pages[1] = id; }), 'id');
    refused(changed(shop, (s) => { s.flows[1].flow = id; }), 'id');
  }
  refused(changed(shop, (s) => { s.pages[3].page = 'api'; s.flows[1].pages[1] = 'api'; }), 'id');
});

test('site data: a reference to a name that is not a parameter is refused (D4)', () => {
  refused(changed(shop, (s) => { s.pages[3].title = 'Browse {colour} - Acorn'; }), 'D4');
  refused(changed(shop, (s) => { s.pages[3].text = 'All {colour} items.'; }), 'D4');
});

test('site data: braces that do not hold a name of C8.2 form are plain text (D4)', () => {
  for (const title of ['Browse {} - Acorn', 'Browse {Item} - Acorn', 'Browse { item } - Acorn', `Browse {${'a'.repeat(33)}} - Acorn`]) {
    assert.equal(parseSiteData(changed(shop, (s) => { s.pages[3].title = title; })).pages[3].title, title);
  }
});

test('site data: a flow with no page and a flow that names no page of the file are refused (D5)', () => {
  refused(changed(shop, (s) => { s.flows[1].pages = []; }), 'D5');
  refused(changed(shop, (s) => { s.flows[1].pages = ['sign-in', 'cart']; }), 'D5');
});

test('site data: a page may stand twice in a flow and in two flows (D5)', () => {
  parseSiteData(changed(shop, (s) => { s.flows[1].pages = ['sign-in', 'browse', 'browse', 'item']; }));
});

test('site data: a role outside the three is refused (role)', () => {
  refused(changed(shop, (s) => { s.flows[1].role = 'near-miss'; }), 'role');
});

test('site data: a seed service without one task flow and one near-miss flow, or with an other flow, is refused (D6)', () => {
  refused(changed(shop, (s) => { s.flows[1].role = 'task'; }), 'D6');
  refused(changed(shop, (s) => { s.flows[0].role = 'near_miss'; }), 'D6');
  refused(changed(shop, (s) => { s.flows.push({ flow: 'idle', role: 'other', pages: ['browse'] }); }), 'D6');
  refused(changed(native, (s) => { s.flows.pop(); }), 'D6');
});

test('site data: a service of the category other with a flow that is not other is refused (D6)', () => {
  refused(changed(other, (s) => { s.flows[0].role = 'task'; }), 'D6');
  refused(changed(other, (s) => { s.flows = []; }), 'D6');
});

test('site data: a seed web service without exactly one password page, first in its task flow, is refused (D7)', () => {
  refused(changed(shop, (s) => { s.pages[0].password_input = false; }), 'D7');
  refused(changed(shop, (s) => { s.pages[3].password_input = true; }), 'D7');
  refused(changed(shop, (s) => { s.flows[0].pages = ['item', 'sign-in', 'pay']; }), 'D7');
});

test('site data: a native seed service needs no password page (D7)', () => {
  parseSiteData(bytes(native()));
});

test('site data: a shop service without exactly one card page in its task flow is refused (D7)', () => {
  refused(changed(shop, (s) => { s.pages[2].card_title = false; }), 'D7');
  refused(changed(shop, (s) => { s.pages[3].card_title = true; s.pages[3].title = 'Card 4242 4242 4242 4242'; }), 'D7');
  refused(changed(shop, (s) => { s.flows[0].pages = ['sign-in', 'item']; s.flows[1].pages = ['sign-in', 'pay']; }), 'D7');
});

test('site data: a card page whose title lacks the test number is refused (D7)', () => {
  refused(changed(shop, (s) => { s.pages[2].title = 'Pay 4242424242424242 - Acorn'; }), 'D7');
  refused(changed(other, (s) => { s.pages[0].card_title = true; }), 'D7');
  parseSiteData(changed(other, (s) => { s.pages[0].card_title = true; s.pages[0].title = 'Card 4242 4242 4242 4242'; }));
});

test('site data: a parameter named as a key of C8.4 is refused (D9)', () => {
  for (const name of ['category', 'site', 'trigger', 'recurrence']) {
    refused(changed(shop, (s) => {
      s.parameters[1] = name;
      s.values = { item: s.values.item, [name]: ['small'] };
      s.pages[1].text = `One {item}, ${name} {${name}}.`;
    }), 'D9');
  }
});

test('site data: a service of the category other with parameters is refused (D9)', () => {
  refused(changed(other, (s) => {
    s.parameters = ['topic'];
    s.values = { topic: ['local'] };
    s.pages[0].title = 'News {topic} - Acorn';
  }), 'D9');
});

test('site data: a title or a value with a space at an end or two spaces in a row is refused (D10)', () => {
  for (const title of [' Browse - Acorn', 'Browse - Acorn ', 'Browse  - Acorn']) {
    refused(changed(shop, (s) => { s.pages[3].title = title; }), 'D10');
  }
  for (const value of [' small', 'small ', 'very  small']) {
    refused(changed(shop, (s) => { s.values.size = [value]; }), 'D10');
  }
  parseSiteData(changed(shop, (s) => { s.pages[3].text = ' two  spaces in text are allowed '; }));
});

test('site data: a parameter that no template uses is refused (unused)', () => {
  refused(changed(shop, (s) => {
    s.parameters.push('colour');
    s.values.colour = ['red'];
  }), 'unused');
});

test('site data: fields of the wrong type are refused', () => {
  refused(changed(shop, (s) => { s.pages[0].password_input = 1; }), 'type');
  refused(changed(shop, (s) => { s.pages[0].title = null; }), 'type');
  refused(changed(shop, (s) => { s.parameters = 'item'; }), 'type');
  refused(changed(shop, (s) => { s.values = []; }), 'type');
  refused(changed(shop, (s) => { s.pages = {}; }), 'type');
  refused(changed(shop, (s) => { s.flows[0].pages = 'sign-in'; }), 'type');
  refused(changed(shop, (s) => { s.pages[0] = 'sign-in'; }), 'type');
});

test('encoding: every byte outside the unreserved set is percent-encoded with upper-case digits', () => {
  assert.equal(encodeValue('blue cup'), 'blue%20cup');
  assert.equal(encodeValue('tea & honey'), 'tea%20%26%20honey');
  assert.equal(encodeValue('A-z_0.9~'), 'A-z_0.9~');
  assert.equal(encodeValue('a+b=c/d?e#f%'), 'a%2Bb%3Dc%2Fd%3Fe%23f%25');
  assert.equal(encodeValue('é'), '%C3%A9');
});

test('fill: each reference takes its value once and a value is not read again', () => {
  assert.equal(fill('{a} and {b} and {a}', new Map([['a', '{b}'], ['b', 'x']])), '{b} and x and {b}');
  assert.equal(fill('{{a}} {A} {}', new Map([['a', 'v']])), '{v} {A} {}');
});

test('requests: one path per choice of the values of the names a page uses, in the order of parameters', () => {
  const site = parseSiteData(bytes(shop()));
  const paths = requestsOf(site).map((r) => `${r.page.page} ${r.path} ${r.title}`);
  assert.deepEqual(paths, [
    'sign-in /sign-in Sign in - Acorn',
    'item /item?item=blue%20cup&size=small blue cup - Acorn',
    'item /item?item=tea%20%26%20honey&size=small tea & honey - Acorn',
    'pay /pay Pay 4242 4242 4242 4242 - Acorn',
    'browse /browse Browse - Acorn',
  ]);
});

const ROOT = scratchDir('fixture-server-sites-');
after(ROOT.remove);

test('requests: every value of a list of three, and two names that a template uses out of the order of parameters', () => {
  const site = parseSiteData(changed(shop, (s) => {
    s.values.item = ['blue cup', "a!b'c", '(x)*y'];
    s.pages[1].title = '{size} {item} - Acorn';
  }));
  assert.deepEqual(requestsOf(site).filter((r) => r.page.page === 'item').map((r) => `${r.path} ${r.title}`), [
    '/item?item=blue%20cup&size=small small blue cup - Acorn',
    "/item?item=a%21b%27c&size=small small a!b'c - Acorn",
    '/item?item=%28x%29%2Ay&size=small small (x)*y - Acorn',
  ]);
});

function scratch() {
  return mkdtempSync(join(ROOT.dir, 'case-'));
}

function runChecker(args) {
  const run = spawnSync(process.execPath, [CHECKER, ...args], { encoding: 'utf8' });
  const files = readAll(args.filter((arg, i) => !arg.startsWith('--') && args[i - 1] !== '--parameters'));
  if (files !== null && files.length > 0 && (run.status !== 2 || run.stderr.startsWith('REFUSED site-data: '))) {
    check(files, run.status !== 2, null);
  }
  return run;
}

test('checker: counts per category, the password and card pages, and the parameter rows', () => {
  const dir = scratch();
  const files = [shop(), native(), other()].map((value) => {
    const file = join(dir, `${value.service}.json`);
    writeFileSync(file, bytes(value));
    return file;
  });
  const rows = join(dir, 'rows.md');
  writeFileSync(rows, '| category | parameter | source |\n|---|---|---|\n| shop_order | item | open |\n| shop_order | size | open |\n| spreadsheet_fill | sheet | open |\n');
  const run = runChecker(['--parameters', rows, ...files]);
  assert.equal(run.stderr, '');
  assert.equal(run.status, 0);
  const lines = run.stdout.trimEnd().split('\n');
  assert.deepEqual(lines, [
    'checked files=3',
    'services utility_bill_payment 0',
    'services tenant_rent 0',
    'services lease_renewal_or_change 0',
    'services tenant_application_review 0',
    'services email 0',
    'services spreadsheet_fill 1',
    'services shop_order 1',
    'services calendar_cleanup 0',
    'services ride_order 0',
    'services daily_chart_check 0',
    'services other 1',
    `password ${files[0]} sign-in`,
    `card ${files[0]} pay`,
    'parameters names=3 missing=0',
  ]);
});

test('checker: a name with no row is printed and makes the status 1', () => {
  const dir = scratch();
  const file = join(dir, 'acorn-shop.json');
  writeFileSync(file, bytes(shop()));
  const rows = join(dir, 'rows.md');
  writeFileSync(rows, '| category | parameter | source |\n|---|---|---|\n| shop_order | item | open |\n| email | size | open |\n');
  const run = runChecker(['--parameters', rows, file]);
  assert.equal(run.status, 1);
  assert.match(run.stdout, /^parameters names=2 missing=1$/m);
  assert.match(run.stdout, new RegExp(`^missing ${file.replace(/[\\^$.*+?()[\]{}|]/g, '\\$&')} shop_order size$`, 'm'));
});

test('checker: without --parameters it compares no rows and prints no parameters line', () => {
  const dir = scratch();
  const file = join(dir, 'acorn-shop.json');
  writeFileSync(file, bytes(shop()));
  const run = runChecker([file]);
  assert.equal(run.status, 0);
  assert.doesNotMatch(run.stdout, /parameters/);
});

test('checker: a file not of C11.1 is refused with site-data naming the file, and nothing on standard output', () => {
  const dir = scratch();
  const good = join(dir, 'acorn-news.json');
  writeFileSync(good, bytes(other()));
  const spaced = join(dir, 'spaced.json');
  writeFileSync(spaced, Buffer.from(bytes(shop()).toString('utf8').replace('"v":1,', '"v":1, '), 'utf8'));
  const { v, service, ...rest } = shop();
  const swapped = join(dir, 'swapped.json');
  writeFileSync(swapped, bytes({ service, v, ...rest }));
  const numberApp = join(dir, 'number-app.json');
  writeFileSync(numberApp, Buffer.from('{"v":1,"service":"acorn-sheet","category":"spreadsheet_fill","kind":"native","host":null,"app":5,"parameters":[],"values":{},"pages":[{"page":"book","title":"Book - Acorn","text":"Rows.","password_input":false,"card_title":false}],"flows":[{"flow":"fill","role":"task","pages":["book"]},{"flow":"read","role":"near_miss","pages":["book"]}]}\n', 'utf8'));
  for (const bad of [spaced, swapped, numberApp]) {
    const run = runChecker([good, bad]);
    assert.equal(run.status, 2);
    assert.equal(run.stdout, '');
    assert.equal(run.stderr, `REFUSED site-data: ${bad}\n`);
  }
});

test('arguments: one optional flag with a value, then one or more files', () => {
  const any = () => true;
  assert.deepEqual(readArgs(['a.json'], '--parameters', any), { value: null, files: ['a.json'] });
  assert.deepEqual(readArgs(['a.json', '--parameters', 'r.md', 'b.json'], '--parameters', any), { value: 'r.md', files: ['a.json', 'b.json'] });
  for (const argv of [[], ['--parameters'], ['--parameters', 'r.md'], ['--port', '1', 'a.json'], ['--parameters', 'r.md', '--parameters', 'r.md', 'a.json'], ['-', '--x']]) {
    assert.equal(readArgs(argv, '--parameters', any), null, argv.join(' '));
  }
  assert.deepEqual(readArgs(['--port', '0', 'a.json'], '--port', isPort), { value: '0', files: ['a.json'] });
  assert.equal(readArgs(['--port', 'x', 'a.json'], '--port', isPort), null);
  for (const port of ['0', '1', '18080', '65535', '00080']) assert.equal(isPort(port), true, port);
  for (const port of ['', '-1', '65536', '100000', ' 80', '80 ', '1e3', '0x50', '8.0']) assert.equal(isPort(port), false, port);
});

test('loading: a missing file, a directory and two files that name one service are refused by path (D8)', () => {
  const dir = scratch();
  const one = join(dir, 'one.json');
  const two = join(dir, 'two.json');
  writeFileSync(one, bytes(shop()));
  writeFileSync(two, bytes(shop()));
  assert.equal(loadSites([one]).sites.length, 1);
  assert.deepEqual(loadSites([one, join(dir, 'missing.json')]), { refused: join(dir, 'missing.json') });
  assert.deepEqual(loadSites([dir, one]), { refused: dir });
  assert.deepEqual(loadSites([one, two]), { refused: two });
  assert.deepEqual(loadSites([one, one]), { refused: one });
});

test('checker: usage and parameters refusals', () => {
  const dir = scratch();
  const file = join(dir, 'acorn-shop.json');
  writeFileSync(file, bytes(shop()));
  const usage = runChecker(['--parameters', file]);
  assert.equal(usage.status, 2);
  assert.equal(usage.stdout, '');
  assert.match(usage.stderr, /^REFUSED usage: [^\n]*\n$/);
  const run = runChecker(['--parameters', join(dir, 'rows.md'), file]);
  assert.equal(run.status, 2);
  assert.equal(run.stdout, '');
  assert.equal(run.stderr, `REFUSED parameters: ${join(dir, 'rows.md')}\n`);
});

test('site data: every case of the shared list of crates/halvo-sample gets its verdict and its rule here', () => {
  const dir = scratch();
  const cases = sharedCases();
  assert.equal(new Set(cases.map((c) => c.name)).size, cases.length);
  assert.equal(new Set(cases.map((c) => caseKey(c.files))).size, cases.length);
  for (const [k, c] of cases.entries()) {
    if (c.files.length === 1) {
      let rule = null;
      try {
        parseSiteData(c.files[0]);
      } catch (error) {
        assert.equal(error.name, 'SiteDataError', c.name);
        rule = error.rule;
      }
      assert.equal(rule === null, c.taken, c.name);
      assert.equal(rule, c.rule, c.name);
    } else {
      const paths = c.files.map((file, i) => {
        const path = join(dir, `case-${k}-${i}.json`);
        writeFileSync(path, file);
        return path;
      });
      assert.equal(loadSites(paths).refused === undefined, c.taken, c.name);
    }
  }
});

test('site data: every fixture of this file stands in the shared list with its verdict and rule', () => {
  assert.ok(fixtures.size > 0);
  for (const f of fixtures.values()) check(f.files, f.taken, f.rule);
});
