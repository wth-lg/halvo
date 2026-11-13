/**
 * The fixture server of contracts.md C11.3 and C11.7: `node server.mjs [--port N] SITE_DATA...`.
 * It ends when a piped standard input ends or fails, so a caller that gives it a pipe holds the write end; a null
 * device, a file or a terminal is not read, and the server then serves until SIGINT or SIGTERM.
 * On SIGINT or SIGTERM it closes its listener and every open connection and exits 0.
 * It closes a connection after a request that it cannot parse, a header block above 16 KiB among them, after its 404.
 * A 404 has no Content-Type and no body. A request with an Expect header gets one answer and no interim 100.
 * After the answer to an upgrade offer the connection closes, and a second request in the same write gets no answer.
 */
import { createServer } from 'node:http';
import { Socket } from 'node:net';
import { compactPagesAndFlows, encodeValue, fill, isPort, loadSites, namesOf, readArgs } from './sites.mjs';

const DEFAULT_PORT = 18080;
const USAGE = 'usage: node server.mjs [--port N] SITE_DATA... (ends when a piped standard input ends)';
const NOT_FOUND = 'HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n';

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

function escapeHtml(text) {
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

function pageHtml(title, text, password) {
  const input = password ? '<input type="password">' : '';
  return `<!DOCTYPE html>\n<html><head><meta charset="utf-8"><title>${escapeHtml(title)}</title></head>`
    + `<body><p>${escapeHtml(text)}</p>${input}</body></html>\n`;
}

function routes(sites) {
  const hosts = new Map();
  for (const site of sites) {
    if (site.kind !== 'web') continue;
    const pages = new Map();
    for (const page of site.pages) {
      const names = namesOf(site, page).map((name) => ({
        name,
        values: new Map(site.values[name].map((value) => [encodeValue(value), value])),
      }));
      pages.set(`/${page.page}`, { page, names });
    }
    hosts.set(site.host, { api: Buffer.from(`${compactPagesAndFlows(site)}\n`, 'utf8'), pages });
  }
  return hosts;
}

function hostOf(req) {
  const values = [];
  for (let i = 0; i < req.rawHeaders.length; i += 2) {
    if (req.rawHeaders[i].toLowerCase() === 'host') values.push(req.rawHeaders[i + 1]);
  }
  if (values.length !== 1) return null;
  const match = /^([^:]+)(?::[0-9]+)?$/.exec(values[0]);
  return match ? match[1] : null;
}

function target(req, host) {
  if (req.url.startsWith('/')) return req.url;
  const absolute = /^http:\/\/([^/?#]*)(\/.*)$/.exec(req.url);
  if (absolute === null) return null;
  const authority = /^([^:]+)(?::[0-9]+)?$/.exec(absolute[1]);
  return authority !== null && authority[1] === host ? absolute[2] : null;
}

function answer(hosts, req) {
  if (req.method !== 'GET') return null;
  const host = hostOf(req);
  const service = hosts.get(host);
  if (service === undefined) return null;
  const url = target(req, host);
  if (url === null) return null;
  if (url === '/api/pages') return { type: 'application/json', body: service.api };
  const at = url.indexOf('?');
  const route = service.pages.get(at < 0 ? url : url.slice(0, at));
  if (route === undefined) return null;
  const values = new Map();
  if (route.names.length === 0) {
    if (at >= 0) return null;
  } else {
    if (at < 0) return null;
    const pairs = url.slice(at + 1).split('&');
    if (pairs.length !== route.names.length) return null;
    for (let i = 0; i < pairs.length; i += 1) {
      const { name, values: listed } = route.names[i];
      if (!pairs[i].startsWith(`${name}=`)) return null;
      const value = listed.get(pairs[i].slice(name.length + 1));
      if (value === undefined) return null;
      values.set(name, value);
    }
  }
  const { page } = route;
  const html = pageHtml(fill(page.title, values), fill(page.text, values), page.password_input);
  return { type: 'text/html; charset=utf-8', body: Buffer.from(html, 'utf8') };
}

function endWithNotFound(socket) {
  if (socket.writable) socket.end(NOT_FOUND);
  else socket.destroy();
}

const args = readArgs(process.argv.slice(2), '--port', isPort);
if (args === null) refuse('usage', USAGE);
const port = args.value === null ? DEFAULT_PORT : Number(args.value);
const loaded = loadSites(args.files);
if (loaded.refused !== undefined) refuse('site-data', loaded.refused);
const hosts = routes(loaded.sites);

function respond(req, res) {
  const found = answer(hosts, req);
  if (found === null) {
    res.writeHead(404, { 'Content-Length': '0' });
    res.end();
    return;
  }
  res.writeHead(200, { 'Content-Type': found.type, 'Content-Length': String(found.body.length) });
  res.end(found.body);
}

function respondWithoutUpgrade(req, socket) {
  const found = answer(hosts, req);
  if (found === null || !socket.writable) {
    endWithNotFound(socket);
    return;
  }
  const head = `HTTP/1.1 200 OK\r\nContent-Type: ${found.type}\r\nContent-Length: ${found.body.length}\r\nConnection: close\r\n\r\n`;
  socket.end(Buffer.concat([Buffer.from(head, 'latin1'), found.body]));
}

const server = createServer({ requireHostHeader: false }, respond);
const sockets = new Set();
server.on('connection', (socket) => {
  sockets.add(socket);
  socket.on('close', () => sockets.delete(socket));
});
server.on('checkContinue', respond);
server.on('checkExpectation', respond);
server.on('connect', (_req, socket) => endWithNotFound(socket));
server.on('upgrade', respondWithoutUpgrade);
server.on('clientError', (_error, socket) => endWithNotFound(socket));
server.once('error', () => refuse('port', String(port)));
let ending = false;
function end() {
  if (ending) return;
  ending = true;
  server.close(() => process.exit(0));
  for (const socket of sockets) socket.destroy();
}
process.on('SIGINT', end);
process.on('SIGTERM', end);
server.listen({ host: '127.0.0.1', port, exclusive: true }, () => {
  process.stdout.write(`listening 127.0.0.1:${server.address().port}\n`);
  if (process.stdin instanceof Socket && !process.stdin.isTTY) {
    process.stdin.on('end', end);
    process.stdin.on('error', end);
    process.stdin.resume();
  }
});
