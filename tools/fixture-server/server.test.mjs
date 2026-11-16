import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer, connect } from 'node:net';
import { networkInterfaces } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { SERVER, scratchDir, startServer, stopServer } from './servers.mjs';

const SHOP = {
  v: 1,
  service: 'acorn-shop',
  category: 'shop_order',
  kind: 'web',
  host: 'acorn-shop.test',
  app: null,
  parameters: ['item', 'size'],
  values: { item: ['blue cup', 'tea & honey', 'crème <b>'], size: ['small'] },
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

const SHEET = {
  v: 1,
  service: 'acorn-sheet',
  category: 'spreadsheet_fill',
  kind: 'native',
  host: null,
  app: 'org.libreoffice.script',
  parameters: [],
  values: {},
  pages: [{ page: 'book', title: 'budget.ods - LibreOffice Calc', text: 'Rows.', password_input: false, card_title: false }],
  flows: [
    { flow: 'fill', role: 'task', pages: ['book'] },
    { flow: 'read', role: 'near_miss', pages: ['book'] },
  ],
};

const NEWS = {
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

const ROOT = scratchDir('fixture-server-server-');
after(ROOT.remove);

function scratch() {
  return mkdtempSync(join(ROOT.dir, 'case-'));
}

function writeSites(dir, sites) {
  return sites.map((site, i) => {
    const file = join(dir, `${i}-${site.service}.json`);
    writeFileSync(file, `${JSON.stringify(site)}\n`);
    return file;
  });
}

function raw(port, text, address = '127.0.0.1') {
  return new Promise((resolve, reject) => {
    const socket = connect({ host: address, port });
    const chunks = [];
    socket.setTimeout(15000, () => socket.destroy());
    socket.on('data', (chunk) => chunks.push(chunk));
    socket.on('error', reject);
    socket.on('close', () => {
      const all = Buffer.concat(chunks);
      const split = all.indexOf('\r\n\r\n');
      const head = all.subarray(0, split < 0 ? all.length : split).toString('latin1');
      const lines = head.split('\r\n');
      const status = Number(/^HTTP\/1\.[01] (\d{3}) /.exec(lines[0])?.[1] ?? 0);
      const headers = new Map(lines.slice(1).map((line) => {
        const at = line.indexOf(':');
        return [line.slice(0, at).toLowerCase(), line.slice(at + 1).trim()];
      }));
      resolve({ status, headers, body: split < 0 ? Buffer.alloc(0) : all.subarray(split + 4) });
    });
    socket.write(text, 'latin1');
  });
}

function get(port, target, host, method = 'GET', extra = '') {
  return raw(port, `${method} ${target} HTTP/1.1\r\nHost: ${host}\r\n${extra}Connection: close\r\n\r\n`);
}

function refusal(args) {
  const run = spawnSync(process.execPath, [SERVER, ...args], { encoding: 'utf8', timeout: 20000 });
  return { status: run.status, stdout: run.stdout, stderr: run.stderr };
}

test('server: the page face and the API face answer 200, and every other request 404 with an empty body', async () => {
  const dir = scratch();
  const files = writeSites(dir, [SHOP, SHEET, NEWS]);
  const server = await startServer(['--port', '0', ...files]);
  const { port, line } = server;
  assert.equal(line, `listening 127.0.0.1:${port}\n`);
  const shop = `acorn-shop.test:${port}`;
  try {
    const pages = [
      ['page without parameters', '/sign-in', shop, 'Sign in - Acorn', true],
      ['page with two parameters', '/item?item=tea%20%26%20honey&size=small', shop, 'tea &amp; honey - Acorn', false],
      ['page with an encoded multi-byte value', '/item?item=cr%C3%A8me%20%3Cb%3E&size=small', shop, 'crème &lt;b&gt; - Acorn', false],
      ['card page', '/pay', shop, 'Pay 4242 4242 4242 4242 - Acorn', false],
      ['Host without a port', '/browse', 'acorn-shop.test', 'Browse - Acorn', false],
      ['page of a service of the category other', '/home', `acorn-news.test:${port}`, 'News - Acorn', false],
    ];
    for (const [name, target, host, title, password] of pages) {
      const res = await get(port, target, host);
      const body = res.body.toString('utf8');
      console.log(`case 200 ${name}: status=${res.status} type=${res.headers.get('content-type')}`);
      assert.equal(res.status, 200, name);
      assert.equal(res.headers.get('content-type'), 'text/html; charset=utf-8', name);
      assert.equal(Number(res.headers.get('content-length')), res.body.length, name);
      assert.equal((body.match(/<title>/g) ?? []).length, 1, name);
      assert.ok(body.includes(`<title>${title}</title>`), name);
      assert.equal(body.includes('<input type="password">'), password, name);
    }
    const item = (await get(port, '/item?item=blue%20cup&size=small', shop)).body.toString('utf8');
    assert.ok(item.includes('<p>One blue cup, size small.</p>'));

    const api = await get(port, '/api/pages', shop);
    console.log(`case 200 API face: status=${api.status} type=${api.headers.get('content-type')}`);
    assert.equal(api.status, 200);
    assert.equal(api.headers.get('content-type'), 'application/json');
    assert.equal(api.body.toString('utf8'), `${JSON.stringify({ pages: SHOP.pages, flows: SHOP.flows })}\n`);
    const newsApi = await get(port, '/api/pages', `acorn-news.test:${port}`);
    assert.equal(newsApi.body.toString('utf8'), `${JSON.stringify({ pages: NEWS.pages, flows: NEWS.flows })}\n`);

    const notFound = [
      ['Host that names no service', '/sign-in', `nobody.test:${port}`],
      ['Host of a native service', '/book', `acorn-sheet.test:${port}`],
      ['Host of a native service, API face', '/api/pages', `acorn-sheet.test:${port}`],
      ['Host in upper case', '/sign-in', `ACORN-SHOP.TEST:${port}`],
      ['Host with a final dot', '/sign-in', `acorn-shop.test.:${port}`],
      ['Host with an empty port', '/sign-in', 'acorn-shop.test:'],
      ['Host with a port that is no number', '/sign-in', 'acorn-shop.test:x1'],
      ['Host that is the address', '/sign-in', `127.0.0.1:${port}`],
      ['Host of the service name alone', '/sign-in', 'acorn-shop'],
      ['root path', '/', shop],
      ['empty query', '/sign-in?', shop],
      ['query on a page that uses no name', '/sign-in?item=blue%20cup', shop],
      ['final slash', '/sign-in/', shop],
      ['double slash', '//sign-in', shop],
      ['page id in another case', '/Sign-in', shop],
      ['page id percent-encoded', '/sign%2Din', shop],
      ['page of another service', '/home', shop],
      ['page of a native service on a web host', '/book', shop],
      ['query missing', '/item', shop],
      ['one name missing', '/item?item=blue%20cup', shop],
      ['names in another order', '/item?size=small&item=blue%20cup', shop],
      ['another name of the same length', '/item?itex=blue%20cup&size=small', shop],
      ['plus for a space', '/item?item=blue+cup&size=small', shop],
      ['lower-case hex digits', '/item?item=cr%c3%a8me%20%3cb%3e&size=small', shop],
      ['value not encoded', '/item?item=tea%20&%20honey&size=small', shop],
      ['value encoded twice', '/item?item=blue%2520cup&size=small', shop],
      ['unreserved byte encoded', '/item?item=blue%20cu%70&size=small', shop],
      ['value not listed', '/item?item=red%20cup&size=small', shop],
      ['extra pair', '/item?item=blue%20cup&size=small&x=1', shop],
      ['name repeated', '/item?item=blue%20cup&item=blue%20cup', shop],
      ['empty pair', '/item?item=blue%20cup&&size=small', shop],
      ['API face with a query', '/api/pages?x', shop],
      ['API face with a final slash', '/api/pages/', shop],
      ['API prefix alone', '/api', shop],
      ['API face in upper case', '/API/pages', shop],
      ['favicon', '/favicon.ico', shop],
      ['absolute form with another authority', `http://acorn-news.test:${port}/sign-in`, shop],
      ['absolute form of an unlisted path', `http://acorn-shop.test:${port}/nothing`, shop],
      ['absolute form with no path', `http://acorn-shop.test:${port}`, shop],
      ['absolute form of another scheme', `https://acorn-shop.test:${port}/sign-in`, shop],
      ['asterisk form', '*', shop],
    ];
    for (const [name, target, host] of notFound) {
      const res = await get(port, target, host);
      console.log(`case 404 ${name}: status=${res.status} body_bytes=${res.body.length}`);
      assert.equal(res.status, 404, name);
      assert.equal(res.body.length, 0, name);
    }
    for (const method of ['HEAD', 'POST', 'PUT', 'DELETE', 'OPTIONS', 'PATCH', 'TRACE']) {
      const res = await get(port, '/sign-in', shop, method, method === 'POST' ? 'Content-Length: 0\r\n' : '');
      console.log(`case 404 method ${method}: status=${res.status} body_bytes=${res.body.length}`);
      assert.equal(res.status, 404, method);
      assert.equal(res.body.length, 0, method);
    }
    const listedRaw = [
      ['absolute form', `GET http://acorn-shop.test:${port}/sign-in HTTP/1.1\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['absolute form with a query', `GET http://acorn-shop.test/item?item=blue%20cup&size=small HTTP/1.1\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['upgrade offer', `GET /sign-in HTTP/1.1\r\nHost: ${shop}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`],
      ['upgrade offer h2c, API face', `GET /api/pages HTTP/1.1\r\nHost: ${shop}\r\nConnection: Upgrade, HTTP2-Settings\r\nUpgrade: h2c\r\nHTTP2-Settings: AAMAAABkAARAAAAAAAIAAAAA\r\n\r\n`],
      ['expectation other than 100-continue', `GET /sign-in HTTP/1.1\r\nHost: ${shop}\r\nExpect: x-probe\r\nConnection: close\r\n\r\n`],
    ];
    for (const [name, text] of listedRaw) {
      const res = await raw(port, text);
      console.log(`case 200 ${name}: status=${res.status} type=${res.headers.get('content-type')}`);
      assert.equal(res.status, 200, name);
      assert.equal(Number(res.headers.get('content-length')), res.body.length, name);
      assert.ok(res.body.length > 0, name);
    }
    const rawCases = [
      ['no Host header', 'GET /sign-in HTTP/1.0\r\n\r\n'],
      ['no Host header, page of a native service', 'GET /book HTTP/1.0\r\n\r\n'],
      ['no Host header, API face', 'GET /api/pages HTTP/1.0\r\n\r\n'],
      ['two Host headers, page of a native service', `GET /book HTTP/1.1\r\nHost: ${shop}\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['two Host headers', `GET /sign-in HTTP/1.1\r\nHost: ${shop}\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['two Host headers, the first unknown', `GET /sign-in HTTP/1.1\r\nHost: nobody.test\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['method CONNECT', `CONNECT acorn-shop.test:${port} HTTP/1.1\r\nHost: ${shop}\r\n\r\n`],
      ['method in lower case', `get /sign-in HTTP/1.1\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['upgrade offer on an unlisted path', `GET /nothing HTTP/1.1\r\nHost: ${shop}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`],
      ['upgrade offer with another method', `POST /sign-in HTTP/1.1\r\nHost: ${shop}\r\nConnection: Upgrade\r\nUpgrade: h2c\r\nContent-Length: 0\r\n\r\n`],
      ['expectation on an unlisted path', `GET /nothing HTTP/1.1\r\nHost: ${shop}\r\nExpect: x-probe\r\nConnection: close\r\n\r\n`],
      ['request that is not HTTP', 'hello\r\n\r\n'],
    ];
    for (const [name, text] of rawCases) {
      const res = await raw(port, text);
      console.log(`case 404 ${name}: status=${res.status} body_bytes=${res.body.length}`);
      assert.equal(res.status, 404, name);
      assert.equal(res.body.length, 0, name);
    }
  } finally {
    await stopServer(server);
  }
});

test('server: binds 127.0.0.1 alone', async () => {
  const dir = scratch();
  const server = await startServer(['--port', '0', ...writeSites(dir, [NEWS])]);
  const { port } = server;
  try {
    const ok = await get(port, '/home', `acorn-news.test:${port}`);
    console.log(`bind 127.0.0.1: status=${ok.status}`);
    assert.equal(ok.status, 200);
    const others = ['::1', ...Object.values(networkInterfaces()).flat()
      .filter((entry) => !entry.internal && entry.family === 'IPv4').map((entry) => entry.address)];
    for (const address of others) {
      const reached = await new Promise((resolve) => {
        const socket = connect({ host: address, port });
        socket.setTimeout(3000, () => { socket.destroy(); resolve(false); });
        socket.on('connect', () => { socket.destroy(); resolve(true); });
        socket.on('error', () => resolve(false));
      });
      console.log(`bind other address ${address === '::1' ? 'ipv6-loopback' : 'interface'}: reached=${reached}`);
      assert.equal(reached, false);
    }
    console.log(`bind addresses tried=${others.length}`);
  } finally {
    await stopServer(server);
  }
});

test('server: ends on SIGTERM and on SIGINT, with status 0 where a process can handle the signal', async () => {
  const dir = scratch();
  const files = writeSites(dir, [NEWS]);
  for (const signal of ['SIGTERM', 'SIGINT']) {
    const server = await startServer(['--port', '0', ...files]);
    try {
      await get(server.port, '/home', `acorn-news.test:${server.port}`, 'GET', '');
      const ended = await stopServer(server, signal);
      console.log(`signal ${signal}: code=${ended.code} killed=${ended.killed}`);
      if (process.platform === 'win32') assert.deepEqual({ killed: ended.killed, gone: ended.gone }, { killed: false, gone: true });
      else assert.deepEqual(ended, { code: 0, signal: null, killed: false, gone: true });
    } finally {
      await stopServer(server);
    }
  }
});

test('server: refusals usage, site-data and port, each with nothing on standard output', async () => {
  const dir = scratch();
  const [good] = writeSites(dir, [NEWS]);
  const twin = join(dir, 'twin.json');
  writeFileSync(twin, readFileSync(good));
  const spaced = join(dir, 'spaced.json');
  writeFileSync(spaced, readFileSync(good, 'utf8').replace('"v":1,', '"v":1, '));
  const usage = [[], ['--port', '65536', good], ['--host', '0.0.0.0', good]];
  for (const args of usage) {
    const run = refusal(args);
    console.log(`refusal usage: status=${run.status}`);
    assert.equal(run.status, 2, args.join(' '));
    assert.equal(run.stdout, '');
    assert.match(run.stderr, /^REFUSED usage: [^\n]*\n$/);
  }
  for (const [args, bad] of [[[good, spaced], spaced], [[good, twin], twin]]) {
    const run = refusal(['--port', '0', ...args]);
    console.log(`refusal site-data: status=${run.status}`);
    assert.equal(run.status, 2);
    assert.equal(run.stdout, '');
    assert.equal(run.stderr, `REFUSED site-data: ${bad}\n`);
  }
  const held = createServer();
  await new Promise((resolve) => held.listen({ host: '127.0.0.1', port: 0, exclusive: true }, resolve));
  try {
    const port = held.address().port;
    const run = await new Promise((resolve) => {
      const child = spawn(process.execPath, [SERVER, '--port', String(port), good], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
      child.stdin.on('error', () => {});
      let stdout = '';
      let stderr = '';
      child.stdout.on('data', (chunk) => { stdout += chunk; });
      child.stderr.on('data', (chunk) => { stderr += chunk; });
      const deadline = setTimeout(() => child.kill('SIGKILL'), 20000);
      child.on('exit', (status) => {
        clearTimeout(deadline);
        resolve({ status, stdout, stderr });
      });
    });
    console.log(`refusal port: status=${run.status}`);
    assert.equal(run.status, 2);
    assert.equal(run.stdout, '');
    assert.equal(run.stderr, `REFUSED port: ${port}\n`);
  } finally {
    held.close();
  }
});

test('server: imports nothing from node_modules', () => {
  const seen = new Set();
  const pending = ['./server.mjs'];
  while (pending.length > 0) {
    const local = pending.pop();
    if (seen.has(local)) continue;
    seen.add(local);
    const source = readFileSync(fileURLToPath(new URL(local, import.meta.url)), 'utf8');
    assert.doesNotMatch(source, /\bimport\s*\(|\brequire\s*\(|createRequire/, local);
    const specifiers = [...source.matchAll(/^\s*(?:import|export)\b[^'"]*?\bfrom\s*['"]([^'"]+)['"]|^\s*import\s*['"]([^'"]+)['"]/gm)]
      .map((match) => match[1] ?? match[2]);
    for (const specifier of specifiers) {
      if (specifier.startsWith('./')) pending.push(specifier);
      else assert.match(specifier, /^node:[a-z_/]+$/, `${local} imports ${specifier}`);
    }
  }
  console.log(`imports files=${seen.size} node_modules=0`);
  assert.ok(seen.has('./sites.mjs'));
});

const MAIL = {
  v: 1,
  service: 'acorn-mail',
  category: 'email',
  kind: 'web',
  host: 'acorn-mail.test',
  app: null,
  parameters: ['box', 'tag'],
  values: { box: ['in', "a!b'c", '(x)*y'], tag: ['red'] },
  pages: [
    { page: 'sign-in', title: 'Sign in - Mail', text: 'Password.', password_input: true, card_title: false },
    { page: 'list', title: 'List {tag} {box}', text: 'Messages.', password_input: false, card_title: false },
    { page: 'read', title: 'Read {box}', text: 'One message.', password_input: false, card_title: false },
    { page: 'write', title: 'Write', text: 'A new message.', password_input: false, card_title: false },
    { page: 'sent', title: 'Sent', text: 'Gone.', password_input: false, card_title: false },
    { page: 'prefs', title: 'Prefs', text: 'Settings.', password_input: false, card_title: false },
  ],
  flows: [
    { flow: 'answer', role: 'task', pages: ['sign-in', 'list', 'read', 'write', 'sent'] },
    { flow: 'tune', role: 'near_miss', pages: ['sign-in', 'prefs'] },
  ],
};

const MAIL_API = '{"pages":[{"page":"sign-in","title":"Sign in - Mail","text":"Password.","password_input":true,"card_title":false},'
  + '{"page":"list","title":"List {tag} {box}","text":"Messages.","password_input":false,"card_title":false},'
  + '{"page":"read","title":"Read {box}","text":"One message.","password_input":false,"card_title":false},'
  + '{"page":"write","title":"Write","text":"A new message.","password_input":false,"card_title":false},'
  + '{"page":"sent","title":"Sent","text":"Gone.","password_input":false,"card_title":false},'
  + '{"page":"prefs","title":"Prefs","text":"Settings.","password_input":false,"card_title":false}],'
  + '"flows":[{"flow":"answer","role":"task","pages":["sign-in","list","read","write","sent"]},'
  + '{"flow":"tune","role":"near_miss","pages":["sign-in","prefs"]}]}\n';

const MAIL_PAGES = [
  ['/sign-in', 'Sign in - Mail', 'Password.'],
  ['/list?box=in&tag=red', 'List red in', 'Messages.'],
  ['/list?box=a%21b%27c&tag=red', "List red a!b'c", 'Messages.'],
  ['/list?box=%28x%29%2Ay&tag=red', 'List red (x)*y', 'Messages.'],
  ['/read?box=in', 'Read in', 'One message.'],
  ['/read?box=a%21b%27c', "Read a!b'c", 'One message.'],
  ['/read?box=%28x%29%2Ay', 'Read (x)*y', 'One message.'],
  ['/write', 'Write', 'A new message.'],
  ['/sent', 'Sent', 'Gone.'],
  ['/prefs', 'Prefs', 'Settings.'],
];

function rawBytes(port, text) {
  return new Promise((resolve, reject) => {
    const socket = connect({ host: '127.0.0.1', port });
    const chunks = [];
    socket.setTimeout(15000, () => socket.destroy());
    socket.on('data', (chunk) => chunks.push(chunk));
    socket.on('error', reject);
    socket.on('close', () => resolve(Buffer.concat(chunks).toString('latin1')));
    socket.write(text, 'latin1');
  });
}

test('server: literal answers for the rules of Host, header case, parameter order, encoding, the API body and Expect', async () => {
  const dir = scratch();
  const server = await startServer(['--port', '0', ...writeSites(dir, [MAIL])]);
  const { port } = server;
  const host = `acorn-mail.test:${port}`;
  try {
    for (const [path, title, text] of MAIL_PAGES) {
      const res = await get(port, path, host);
      const body = res.body.toString('utf8');
      console.log(`literal 200 ${path.split('?')[0]}: status=${res.status} bytes=${res.body.length}`);
      assert.equal(res.status, 200, path);
      assert.equal(body, '<!DOCTYPE html>\n<html><head><meta charset="utf-8"><title>' + title + '</title></head><body><p>' + text
        + '</p>' + (path === '/sign-in' ? '<input type="password">' : '') + '</body></html>\n', path);
    }
    const api = await get(port, '/api/pages', host);
    console.log(`literal API body: status=${api.status} bytes=${api.body.length}`);
    assert.equal(api.body.toString('utf8'), MAIL_API);
    assert.equal(api.body.length, 756);
    const notFound = [
      ['the reversed query', '/list?tag=red&box=in'],
      ['a value outside the list', '/read?box=out'],
      ['a value encoded with a raw unsafe byte', "/read?box=a!b'c"],
    ];
    for (const [name, path] of notFound) {
      const res = await get(port, path, host);
      console.log(`literal 404 ${name}: status=${res.status} bytes=${res.body.length}`);
      assert.equal(res.status, 404, name);
      assert.equal(res.body.length, 0, name);
    }
    const noHost = await rawBytes(port, 'GET /sign-in HTTP/1.1\r\nConnection: close\r\n\r\n');
    console.log(`literal 404 HTTP/1.1 with no Host: bytes=${noHost.length}`);
    assert.match(noHost, /^HTTP\/1\.1 404 Not Found\r\n/);
    assert.ok(noHost.endsWith('\r\n\r\n'));
    assert.doesNotMatch(noHost, /content-type/i);
    const lower = await rawBytes(port, `GET /sign-in HTTP/1.1\r\nhost: ${host}\r\nconnection: close\r\n\r\n`);
    console.log(`literal 200 a header name in lower case: ${lower.slice(0, 12)}`);
    assert.match(lower, /^HTTP\/1\.1 200 OK\r\n/);
    const twoCases = await rawBytes(port, `GET /sign-in HTTP/1.1\r\nHost: ${host}\r\nHOST: ${host}\r\nConnection: close\r\n\r\n`);
    console.log(`literal 404 two Host headers of different case: ${twoCases.slice(0, 12)}`);
    assert.match(twoCases, /^HTTP\/1\.1 404 Not Found\r\n/);
    assert.ok(twoCases.endsWith('\r\n\r\n'));
    for (const [path, status] of [['/sign-in', '200 OK'], ['/nothing', '404 Not Found']]) {
      const answer = await rawBytes(port, `GET ${path} HTTP/1.1\r\nHost: ${host}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n`);
      const lines = answer.match(/HTTP\/1\.1 \d{3} /g) ?? [];
      console.log(`literal Expect 100-continue ${path}: status_lines=${lines.length}`);
      assert.equal(lines.length, 1, path);
      assert.ok(answer.startsWith(`HTTP/1.1 ${status}\r\n`), path);
    }
  } finally {
    await stopServer(server);
  }
});
