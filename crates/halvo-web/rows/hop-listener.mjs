import { createServer } from 'node:http';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node crates/halvo-web/rows/hop-listener.mjs --port N

The listener of the hop case of the row hold_release of web-rows (CP-186 T4 (b)) and of the row
block_popup (CP-186 N2). Plain HTTP on 127.0.0.1:N serves /page, a form whose button "Send" posts
to /act. The first POST /act since the last reset is answered with 307 and a Location line that
names /act itself, the same address; every later one with 200. It serves /popup, a form with the
target _blank whose button "Save preferences" posts to /prefs, so that the POST leaves from a new
window, as the row block_popup of A13.7 has the trial's script set it on the fixture's /prefs.
The listener counts each arrival of POST /act and of POST /prefs and answers GET /count with
{"act": <n>, "prefs": <n>} and GET /reset with {} after it sets both counts to 0. It prints one line
"listening 127.0.0.1:<port>" and stops at the end of its standard input, on TERM or on INT.
Exit 0 when it stopped, 2 for a refusal: one line REFUSED <usage|port>: <detail> on standard
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
if (argv.length !== 2 || argv[0] !== '--port' || !/^[0-9]+$/.test(argv[1])) refuse('usage', 'see --help');
const port = Number(argv[1]);

let act = 0;
let prefs = 0;
const json = { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' };

const http = createServer((req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  if (req.method === 'GET' && path === '/page') {
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
    return res.end('<!doctype html><title>hop</title><form method="post" action="/act"><input type="hidden" name="n" value="1"><button>Send</button></form>');
  }
  if (req.method === 'GET' && path === '/popup') {
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
    return res.end('<!doctype html><title>popup</title><form method="post" action="/prefs" target="_blank"><label>Paperless billing <input type="checkbox" name="paperless" value="on"></label><button>Save preferences</button></form>');
  }
  if (req.method === 'POST' && path === '/prefs') {
    req.resume();
    req.on('end', () => {
      prefs += 1;
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
      return res.end('<!doctype html><title>saved</title><p>saved</p>');
    });
    return undefined;
  }
  if (req.method === 'POST' && path === '/act') {
    req.resume();
    req.on('end', () => {
      act += 1;
      if (act === 1) {
        res.writeHead(307, { Location: '/act', 'Cache-Control': 'no-store' });
        return res.end();
      }
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
      return res.end('<!doctype html><title>done</title><p>done</p>');
    });
    return undefined;
  }
  if (req.method === 'GET' && path === '/count') {
    res.writeHead(200, json);
    return res.end(JSON.stringify({ act, prefs }));
  }
  if (req.method === 'GET' && path === '/reset') {
    act = 0;
    prefs = 0;
    res.writeHead(200, json);
    return res.end('{}');
  }
  res.writeHead(404);
  return res.end();
});
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
