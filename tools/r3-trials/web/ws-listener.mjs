import { createServer } from 'node:http';
import { createServer as createTls } from 'node:https';
import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node tools/r3-trials/web/ws-listener.mjs --cert FILE --key FILE

The listener of the row ws_allowed of U0.7b (contract A13.7). Plain HTTP on 127.0.0.1 and a port
that the system picks serves /page, which runs one probe in seven contexts (the page, a frame from
the listener, a dedicated worker and a shared worker whose scripts come from the listener, an
about:blank frame, a frame from a data: address and a worker from a blob: address), and counts the
WebSocket handshakes on /ws, the fetches of /f and the reports on /r of each context. Two TLS
listeners with the certificate in FILE count the connections for wss and for https fetches; the
browser does not trust the certificate, so each counts connections only. It prints one line
"listening 127.0.0.1:<port>". Each line "count" on its standard input gets one line "count <json>"
with the ports and the counts. It stops at the end of its standard input, on TERM or on INT.
Exit 0 when it stopped, 2 for a refusal: one line REFUSED <usage|files|port>: <detail> on standard
error.
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
if (argv.length !== 4 || argv[0] !== '--cert' || argv[2] !== '--key') refuse('usage', 'see --help');
let cert;
let key;
try {
  cert = readFileSync(argv[1]);
  key = readFileSync(argv[3]);
} catch (err) {
  refuse('files', `cannot read a PEM file (${err.code || err.name})`);
}

const H = 'utility.agent.test';
const ports = { http: 0, wss: 0, https: 0 };
const counts = { ws: {}, f: {}, r: {}, wss_connections: 0, https_connections: 0 };
const bump = (map, ctx, value = 1) => { map[ctx] = (map[ctx] || 0) + value; };
const ctxOf = (url) => new URL(url, 'http://x').searchParams.get('ctx') || '-';

function goSource() {
  return `const go = (ctx) => {
  try { new WebSocket('ws://${H}:${ports.http}/ws?ctx=' + ctx); } catch (e) {}
  try { new WebSocket('wss://${H}:${ports.wss}/ws?ctx=' + ctx); } catch (e) {}
  fetch('http://${H}:${ports.http}/f?ctx=' + ctx).catch(() => {});
  fetch('https://${H}:${ports.https}/f?ctx=' + ctx).catch(() => {});
  Promise.all([
    fetch('data:,x').then(() => 'ok', () => 'no'),
    fetch(URL.createObjectURL(new Blob(['x']))).then(() => 'ok', () => 'no'),
  ]).then(([d, b]) => fetch('http://${H}:${ports.http}/r?ctx=' + ctx + '&data=' + d + '&blob=' + b).catch(() => {}));
};`;
}

function page() {
  const go = goSource();
  return `<!doctype html><title>ws allowed</title><body><script>
${go}
go('page');
const frame = document.createElement('iframe'); frame.src = '/frame'; document.body.appendChild(frame);
new Worker('/worker.js');
new SharedWorker('/shared.js');
const blank = document.createElement('iframe'); document.body.appendChild(blank);
blank.contentWindow.eval(${JSON.stringify(go)} + "; go('blank');");
const data = document.createElement('iframe');
data.src = 'data:text/html,' + encodeURIComponent('<script>' + ${JSON.stringify(go)} + "go('dataframe')<\\/script>");
document.body.appendChild(data);
new Worker(URL.createObjectURL(new Blob([${JSON.stringify(go)} + "go('blobworker');"], { type: 'text/javascript' })));
</script>`;
}

const http = createServer((req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  const html = { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' };
  const js = { 'Content-Type': 'text/javascript', 'Cache-Control': 'no-store' };
  if (path === '/page') { res.writeHead(200, html); return res.end(page()); }
  if (path === '/frame') { res.writeHead(200, html); return res.end(`<!doctype html><script>${goSource()} go('frame');</script>`); }
  if (path === '/worker.js') { res.writeHead(200, js); return res.end(`${goSource()} go('worker');`); }
  if (path === '/shared.js') { res.writeHead(200, js); return res.end(`${goSource()} go('shared');`); }
  if (path === '/f') { bump(counts.f, ctxOf(req.url)); res.writeHead(204, { 'Access-Control-Allow-Origin': '*' }); return res.end(); }
  if (path === '/r') {
    const q = new URL(req.url, 'http://x').searchParams;
    if (q.get('data') === 'ok' && q.get('blob') === 'ok') bump(counts.r, ctxOf(req.url));
    res.writeHead(204, { 'Access-Control-Allow-Origin': '*' });
    return res.end();
  }
  res.writeHead(404); res.end();
});
http.on('upgrade', (req, socket) => { bump(counts.ws, ctxOf(req.url)); socket.destroy(); });
const wss = createTls({ cert, key }, (req, res) => { res.writeHead(404); res.end(); });
wss.on('connection', () => { counts.wss_connections += 1; });
wss.on('tlsClientError', () => {});
const https = createTls({ cert, key }, (req, res) => { res.writeHead(204); res.end(); });
https.on('connection', () => { counts.https_connections += 1; });
https.on('tlsClientError', () => {});
for (const s of [http, wss, https]) s.on('error', (err) => refuse('port', `listen failed (${err.code || err.name})`));

const listen = (s) => new Promise((r) => s.listen({ host: '127.0.0.1', port: 0, exclusive: true }, r));
await listen(wss);
await listen(https);
await listen(http);
ports.wss = wss.address().port;
ports.https = https.address().port;
ports.http = http.address().port;
process.stdout.write(`listening 127.0.0.1:${ports.http}\n`);

function stop() {
  for (const s of [http, wss, https]) { s.close(); s.closeAllConnections?.(); }
  setTimeout(() => process.exit(0), 500).unref();
}
const lines = createInterface({ input: process.stdin });
lines.on('line', (line) => {
  if (line.trim() === 'count') process.stdout.write(`count ${JSON.stringify({ ports, ...counts })}\n`);
});
lines.on('close', stop);
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
