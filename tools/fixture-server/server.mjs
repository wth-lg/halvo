import { createServer } from 'node:http';
import { compactPagesAndFlows, encodeValue, fill, isPort, loadSites, namesOf, readArgs } from './sites.mjs';

const DEFAULT_PORT = 18080;
const USAGE = 'usage: node server.mjs [--port N] SITE_DATA...';
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

function answer(hosts, req) {
  if (req.method !== 'GET') return null;
  const service = hosts.get(hostOf(req));
  if (service === undefined) return null;
  if (req.url === '/api/pages') return { type: 'application/json', body: service.api };
  const at = req.url.indexOf('?');
  const route = service.pages.get(at < 0 ? req.url : req.url.slice(0, at));
  if (route === undefined) return null;
  const values = new Map();
  if (route.names.length === 0) {
    if (at >= 0) return null;
  } else {
    if (at < 0) return null;
    const pairs = req.url.slice(at + 1).split('&');
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

const server = createServer({ requireHostHeader: false }, (req, res) => {
  const found = answer(hosts, req);
  if (found === null) {
    res.writeHead(404, { 'Content-Length': '0' });
    res.end();
    return;
  }
  res.writeHead(200, { 'Content-Type': found.type, 'Content-Length': String(found.body.length) });
  res.end(found.body);
});
server.on('connect', (_req, socket) => endWithNotFound(socket));
server.on('upgrade', (_req, socket) => endWithNotFound(socket));
server.on('clientError', (_error, socket) => endWithNotFound(socket));
server.once('error', () => refuse('port', String(port)));
server.listen({ host: '127.0.0.1', port, exclusive: true }, () => {
  process.stdout.write(`listening 127.0.0.1:${server.address().port}\n`);
});

function end() {
  server.close(() => process.exit(0));
  server.closeAllConnections();
}
process.on('SIGINT', end);
process.on('SIGTERM', end);
