import { createServer } from 'node:http';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node tools/r3-trials/web/chain-listener.mjs

The listener of the rows list_hops and list_close of U0.7b (contract A13.7). It serves plain HTTP on
127.0.0.1 and a port that the system picks, prints one line "listening 127.0.0.1:<port>", and routes
by the Host header without its port: /start on hop1.agent.test answers 302 to /next on
hop2.agent.test, /next on hop2.agent.test answers 302 to /end on hop3.agent.test, /end answers a
short page, and /f on any host answers 204 and keeps the connection open. It counts the requests of
each host. Each line "count" on its standard input gets one line "count <host>=<n> ..." with the
hosts in order, or "count -" when none came. It stops at the end of its standard input, on TERM or
on INT. Exit 0 when it stopped, 2 for a refusal: one line REFUSED <usage|port>: <detail> on
standard error.
`);
}

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

const argv = process.argv.slice(2);
if (argv.includes('--help')) {
  usage();
  process.exit(0);
}
if (argv.length !== 0) refuse('usage', 'no argument is taken; see --help');

const counts = new Map();
let port = 0;
const server = createServer({ keepAliveTimeout: 30000 }, (req, res) => {
  const host = String(req.headers.host || '').replace(/:\d+$/, '').toLowerCase();
  counts.set(host, (counts.get(host) || 0) + 1);
  const path = new URL(req.url, 'http://x').pathname;
  if (path === '/start' && host === 'hop1.agent.test') {
    res.writeHead(302, { Location: `http://hop2.agent.test:${port}/next` });
    return res.end();
  }
  if (path === '/next' && host === 'hop2.agent.test') {
    res.writeHead(302, { Location: `http://hop3.agent.test:${port}/end` });
    return res.end();
  }
  if (path === '/end') {
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    return res.end('<!doctype html><title>end of the chain</title><p>end of the chain</p>\n');
  }
  if (path === '/f') {
    res.writeHead(204, { 'Access-Control-Allow-Origin': '*', Connection: 'keep-alive' });
    return res.end();
  }
  res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
  res.end('not found\n');
});
server.on('error', (err) => refuse('port', `listen failed (${err.code || err.name})`));

function stop() {
  server.close(() => process.exit(0));
  server.closeAllConnections?.();
  setTimeout(() => process.exit(0), 1000).unref();
}

server.listen({ host: '127.0.0.1', port: 0, exclusive: true }, () => {
  port = server.address().port;
  process.stdout.write(`listening 127.0.0.1:${port}\n`);
  const lines = createInterface({ input: process.stdin });
  lines.on('line', (line) => {
    if (line.trim() !== 'count') return;
    const hosts = [...counts.keys()].sort();
    const text = hosts.length ? hosts.map((h) => `${h}=${counts.get(h)}`).join(' ') : '-';
    process.stdout.write(`count ${text}\n`);
  });
  lines.on('close', stop);
});

process.on('SIGTERM', stop);
process.on('SIGINT', stop);
