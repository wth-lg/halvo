import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer, connect } from 'node:net';
import { networkInterfaces, tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const SERVER = fileURLToPath(new URL('./server.mjs', import.meta.url));

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

function scratch() {
  return mkdtempSync(join(tmpdir(), 'fixture-server-server-'));
}

function writeSites(dir, sites) {
  return sites.map((site, i) => {
    const file = join(dir, `${i}-${site.service}.json`);
    writeFileSync(file, `${JSON.stringify(site)}\n`);
    return file;
  });
}

function start(args) {
  const child = spawn(process.execPath, [SERVER, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
  return new Promise((resolve, reject) => {
    let out = '';
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      out += chunk;
      const line = /^listening 127\.0\.0\.1:(\d+)\n/.exec(out);
      if (line) resolve({ child, port: Number(line[1]), line: line[0] });
    });
    child.on('exit', (code) => reject(new Error(`server exited ${code} before it listened`)));
    child.on('error', reject);
  });
}

function stop(child, signal = 'SIGTERM') {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolve({ code: child.exitCode, signal: child.signalCode });
      return;
    }
    child.once('exit', (code, sig) => resolve({ code, signal: sig }));
    child.kill(signal);
  });
}

function raw(port, text, address = '127.0.0.1') {
  return new Promise((resolve, reject) => {
    const socket = connect({ host: address, port });
    const chunks = [];
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
  const { child, port, line } = await start(['--port', '0', ...files]);
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
      ['absolute form', `http://acorn-shop.test:${port}/sign-in`, shop],
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
    const rawCases = [
      ['no Host header', 'GET /sign-in HTTP/1.0\r\n\r\n'],
      ['two Host headers', `GET /sign-in HTTP/1.1\r\nHost: ${shop}\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['two Host headers, the first unknown', `GET /sign-in HTTP/1.1\r\nHost: nobody.test\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['method CONNECT', `CONNECT acorn-shop.test:${port} HTTP/1.1\r\nHost: ${shop}\r\n\r\n`],
      ['method in lower case', `get /sign-in HTTP/1.1\r\nHost: ${shop}\r\nConnection: close\r\n\r\n`],
      ['upgrade request', `GET /sign-in HTTP/1.1\r\nHost: ${shop}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`],
      ['request that is not HTTP', 'hello\r\n\r\n'],
    ];
    for (const [name, text] of rawCases) {
      const res = await raw(port, text);
      console.log(`case 404 ${name}: status=${res.status} body_bytes=${res.body.length}`);
      assert.equal(res.status, 404, name);
      assert.equal(res.body.length, 0, name);
    }
  } finally {
    await stop(child);
  }
});

test('server: binds 127.0.0.1 alone', async () => {
  const dir = scratch();
  const { child, port } = await start(['--port', '0', ...writeSites(dir, [NEWS])]);
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
    await stop(child);
  }
});

test('server: ends with status 0 on SIGTERM and on SIGINT', { skip: process.platform === 'win32' && 'Windows has no signal that a process can handle (contracts.md C11.7)' }, async () => {
  const dir = scratch();
  const files = writeSites(dir, [NEWS]);
  for (const signal of ['SIGTERM', 'SIGINT']) {
    const { child, port } = await start(['--port', '0', ...files]);
    await get(port, '/home', `acorn-news.test:${port}`, 'GET', '');
    const ended = await stop(child, signal);
    console.log(`signal ${signal}: code=${ended.code}`);
    assert.deepEqual(ended, { code: 0, signal: null });
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
      const child = spawn(process.execPath, [SERVER, '--port', String(port), good], { stdio: ['ignore', 'pipe', 'pipe'] });
      let stdout = '';
      let stderr = '';
      child.stdout.on('data', (chunk) => { stdout += chunk; });
      child.stderr.on('data', (chunk) => { stderr += chunk; });
      child.on('exit', (status) => resolve({ status, stdout, stderr }));
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
