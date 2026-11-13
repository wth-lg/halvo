import { createServer } from 'node:http';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node crates/halvo-web/rows/ws-listener.mjs --port N

The listener of the row ws_allowed of web-rows (CP-176 A2 (c)), from the web isolation trial's
listener (c15edbec, tools/r3-trials/web/ws-listener.mjs) without its two TLS listeners. Plain
HTTP on 127.0.0.1:N serves /page, which runs one probe in seven contexts (the page, a frame from
the listener, a dedicated worker and a shared worker whose scripts come from the listener, an
about:blank frame, a frame from a data: address and a worker from a blob: address). Each probe
opens a WebSocket to /ws, fetches /f, reports on /r whether data: and blob: fetches work, and
reports on /e when its WebSocket failed before it opened. The listener counts the handshakes on
/ws, the fetches, the reports and the failures of each context, and answers GET /count with them
as JSON. It prints one line "listening 127.0.0.1:<port>" and stops at the end of its standard
input, on TERM or on INT. Exit 0 when it stopped, 2 for a refusal: one line
REFUSED <usage|port>: <detail> on standard error.
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
if (argv.length !== 2 || argv[0] !== '--port' || !/^[0-9]+$/.test(argv[1])) refuse('usage', 'see --help');
const port = Number(argv[1]);

const H = 'utility.agent.test';
const counts = { ws: {}, f: {}, r: {}, e: {} };
const bump = (map, ctx) => { map[ctx] = (map[ctx] || 0) + 1; };
const ctxOf = (url) => new URL(url, 'http://x').searchParams.get('ctx') || '-';

function goSource() {
  return `const go = (ctx) => {
  const report = (path) => fetch('http://${H}:${port}' + path + '?ctx=' + ctx).catch(() => {});
  try {
    const ws = new WebSocket('ws://${H}:${port}/ws?ctx=' + ctx);
    let opened = false;
    ws.onopen = () => { opened = true; };
    ws.onerror = () => { if (!opened) report('/e'); };
  } catch (e) { report('/e'); }
  report('/f');
  Promise.all([
    fetch('data:,x').then(() => 'ok', () => 'no'),
    fetch(URL.createObjectURL(new Blob(['x']))).then(() => 'ok', () => 'no'),
  ]).then(([d, b]) => fetch('http://${H}:${port}/r?ctx=' + ctx + '&data=' + d + '&blob=' + b).catch(() => {}));
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
  const none = { 'Access-Control-Allow-Origin': '*', 'Cache-Control': 'no-store' };
  if (path === '/page') { res.writeHead(200, html); return res.end(page()); }
  if (path === '/frame') { res.writeHead(200, html); return res.end(`<!doctype html><script>${goSource()} go('frame');</script>`); }
  if (path === '/worker.js') { res.writeHead(200, js); return res.end(`${goSource()} go('worker');`); }
  if (path === '/shared.js') { res.writeHead(200, js); return res.end(`${goSource()} go('shared');`); }
  if (path === '/f') { bump(counts.f, ctxOf(req.url)); res.writeHead(204, none); return res.end(); }
  if (path === '/e') { bump(counts.e, ctxOf(req.url)); res.writeHead(204, none); return res.end(); }
  if (path === '/r') {
    const q = new URL(req.url, 'http://x').searchParams;
    if (q.get('data') === 'ok' && q.get('blob') === 'ok') bump(counts.r, ctxOf(req.url));
    res.writeHead(204, none);
    return res.end();
  }
  if (path === '/count') {
    res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
    return res.end(JSON.stringify(counts));
  }
  res.writeHead(404); res.end();
});
http.on('upgrade', (req, socket) => { bump(counts.ws, ctxOf(req.url)); socket.destroy(); });
http.on('error', (err) => refuse('port', `listen failed (${err.code || err.name})`));
await new Promise((r) => http.listen({ host: '127.0.0.1', port, exclusive: true }, r));
process.stdout.write(`listening 127.0.0.1:${http.address().port}\n`);

function stop() {
  http.close();
  http.closeAllConnections?.();
  setTimeout(() => process.exit(0), 500).unref();
}
createInterface({ input: process.stdin }).on('close', stop);
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
