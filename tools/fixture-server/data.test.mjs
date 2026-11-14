import assert from 'node:assert/strict';
import { readdirSync } from 'node:fs';
import { connect } from 'node:net';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { startServer, stopServer } from './servers.mjs';
import { CATEGORIES, compactPagesAndFlows, loadSites, requestsOf } from './sites.mjs';

const DIR = new URL('./sites/', import.meta.url);
const NAMES = readdirSync(DIR).filter((name) => name.endsWith('.json') && !name.startsWith('.')).sort();
const FILES = NAMES.map((name) => fileURLToPath(new URL(name, DIR)));
const ALLOWED = new Set(['!DOCTYPE html', 'html', '/html', 'head', '/head', 'meta charset="utf-8"', 'title', '/title', 'body', '/body', 'p', '/p']);
const PASSWORD = 'input type="password"';

function sites() {
  const loaded = loadSites(FILES);
  assert.equal(loaded.refused, undefined, `refused: ${loaded.refused}`);
  return loaded.sites;
}

function tokens(text) {
  return text.toLowerCase().split(/[^a-z0-9]+/).filter((token) => token !== '');
}

function siteMasked(site, title) {
  const own = new Set(site.host === null ? [] : tokens(site.service));
  return tokens(title).map((token) => (own.has(token) ? '*' : token)).join(' ');
}

function escapeHtml(text) {
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

test('development services: every file is of C11.1 and is named <service>.json', () => {
  assert.ok(NAMES.length > 0);
  sites().forEach((site, i) => assert.equal(NAMES[i], `${site.service}.json`));
  console.log(`site files=${NAMES.length}`);
});

test('development services: at least 4 for each seed category and 6 of the category other', () => {
  const all = sites();
  for (const category of CATEGORIES) {
    const count = all.filter((site) => site.category === category).length;
    console.log(`services ${category} ${count}`);
    assert.ok(count >= (category === 'other' ? 6 : 4), category);
  }
});

test('development services: each flow holds a page whose site-masked titles no other service shows', () => {
  const all = sites();
  const masked = all.map((site) => new Set(requestsOf(site).map((r) => siteMasked(site, r.title))));
  let flows = 0;
  all.forEach((site, i) => {
    const others = new Set(masked.flatMap((set, j) => (j === i ? [] : [...set])));
    const own = requestsOf(site);
    for (const flow of site.flows) {
      const distinct = flow.pages.some((id) => own.filter((r) => r.page.page === id).every((r) => !others.has(siteMasked(site, r.title))));
      assert.ok(distinct, `file ${i + 1} flow ${flow.flow}`);
      flows += 1;
    }
  });
  console.log(`flows checked=${flows} without_distinct_title=0`);
});

test('development services: no title of a service is a title of another service', () => {
  const all = sites();
  const owner = new Map();
  all.forEach((site, i) => {
    for (const r of requestsOf(site)) {
      assert.ok(!owner.has(r.title) || owner.get(r.title) === i, `file ${i + 1} page ${r.page.page}`);
      owner.set(r.title, i);
    }
  });
  console.log(`titles distinct=${owner.size}`);
});

function request(port, host, target) {
  return new Promise((resolve, reject) => {
    const socket = connect({ host: '127.0.0.1', port });
    const chunks = [];
    socket.setTimeout(15000, () => socket.destroy());
    socket.on('data', (chunk) => chunks.push(chunk));
    socket.on('error', reject);
    socket.on('close', () => {
      const all = Buffer.concat(chunks);
      const split = all.indexOf('\r\n\r\n');
      const lines = all.subarray(0, split).toString('latin1').split('\r\n');
      const headers = lines.slice(1).map((line) => [line.slice(0, line.indexOf(':')).toLowerCase(), line.slice(line.indexOf(':') + 1).trim()]);
      const body = all.subarray(split + 4);
      resolve({
        status: Number(lines[0].slice(9, 12)),
        type: headers.find(([name]) => name === 'content-type')?.[1],
        headers,
        bytes: body,
        body: body.toString('utf8'),
      });
    });
    socket.write(`GET ${target} HTTP/1.1\r\nHost: ${host}:${port}\r\nConnection: close\r\n\r\n`, 'latin1');
  });
}

function badHeaders(res) {
  let count = res.headers.filter(([name]) => name === 'location' || name === 'set-cookie').length;
  const lengths = res.headers.filter(([name]) => name === 'content-length');
  if (lengths.length !== 1 || lengths[0][1] !== String(res.bytes.length)) count += 1;
  return count;
}

function forbidden(html, password) {
  let count = 0;
  let inputs = 0;
  const tags = [...html.matchAll(/<([^<>]*)>/g)].map((match) => match[1]);
  for (const tag of tags) {
    if (tag === PASSWORD) inputs += 1;
    else if (!ALLOWED.has(tag)) count += 1;
  }
  const titles = tags.filter((tag) => tag === 'title').length;
  if (titles !== 1) count += Math.max(titles - 1, 1);
  const outside = html.replace(/<[^<>]*>/g, '');
  if (outside.includes('<') || outside.includes('>')) count += 1;
  if (inputs !== (password ? 1 : 0)) count += Math.max(inputs, 1);
  return count;
}

test('development services: every listed page of every web service holds no link, script, form or other input', async () => {
  const all = sites();
  const server = await startServer(['--port', '0', ...FILES]);
  const { port } = server;
  try {
    let total = 0;
    for (const [i, site] of all.entries()) {
      if (site.kind !== 'web') {
        const res = await request(port, `${site.service}.test`, `/${site.pages[0].page}`);
        assert.equal(res.status, 404);
        console.log(`page scan file=${i + 1} kind=native status=${res.status}`);
        continue;
      }
      const api = await request(port, site.host, '/api/pages');
      assert.equal(api.status, 200);
      assert.equal(api.body, `${compactPagesAndFlows(site)}\n`);
      let count = 0;
      const requests = requestsOf(site);
      for (const r of requests) {
        const res = await request(port, site.host, r.path);
        assert.equal(res.status, 200, `file ${i + 1} ${r.path}`);
        assert.equal(res.type, 'text/html; charset=utf-8');
        assert.ok(res.body.includes(`<title>${escapeHtml(r.title)}</title>`), `file ${i + 1} ${r.path}`);
        count += forbidden(res.body, r.page.password_input) + badHeaders(res);
      }
      console.log(`page scan file=${i + 1} kind=web requests=${requests.length} forbidden=${count}`);
      assert.equal(count, 0, `file ${i + 1}`);
      total += requests.length;
    }
    console.log(`page scan requests=${total} forbidden=0`);
  } finally {
    const ended = await stopServer(server);
    assert.equal(ended.killed, false);
  }
});

test('page scan: the scan counts a Location, a Set-Cookie and a Content-Length that is not the body length', () => {
  const answer = (headers, body) => ({ headers, bytes: Buffer.from(body, 'utf8') });
  assert.equal(badHeaders(answer([['content-type', 'text/html; charset=utf-8'], ['content-length', '2']], 'ab')), 0);
  assert.equal(badHeaders(answer([['content-length', '2'], ['location', '/x']], 'ab')), 1);
  assert.equal(badHeaders(answer([['content-length', '2'], ['set-cookie', 'a=b']], 'ab')), 1);
  assert.equal(badHeaders(answer([['content-length', '3']], 'ab')), 1);
  assert.equal(badHeaders(answer([], 'ab')), 1);
  assert.equal(badHeaders(answer([['content-length', '2'], ['content-length', '2']], 'ab')), 1);
});

test('page scan: the scan counts a link, a script, a form, a second input, a second title and a stray tag', () => {
  const page = (body) => `<!DOCTYPE html>\n<html><head><meta charset="utf-8"><title>t</title></head><body><p>x</p>${body}</body></html>\n`;
  assert.equal(forbidden(page(''), false), 0);
  assert.equal(forbidden(page('<title>u</title>'), false), 1);
  assert.equal(forbidden(page('').replace('<title>t</title>', ''), false), 1);
  assert.equal(forbidden(page('<input type="password">'), true), 0);
  assert.equal(forbidden(page(''), true), 1);
  assert.equal(forbidden(page('<input type="password">'), false), 1);
  assert.equal(forbidden(page('<a href="/x">x</a>'), false), 2);
  assert.equal(forbidden(page('<script>1</script>'), false), 2);
  assert.equal(forbidden(page('<form action="/x"></form>'), false), 2);
  assert.equal(forbidden(page('<input type="password"><input type="text">'), true), 1);
  assert.equal(forbidden(page('<input type="password"><input type="password">'), true), 2);
  assert.equal(forbidden(page('<meta http-equiv="refresh" content="0;url=/x">'), false), 1);
  assert.equal(forbidden(page('<p onclick="x">y</p>'), false), 1);
  assert.equal(forbidden(page('a > b'), false), 1);
});
