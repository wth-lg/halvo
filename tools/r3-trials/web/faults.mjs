import { request, createServer } from 'node:http';

const FAULTS = {
  signin_500: { status: { method: 'POST', host: 'utility', path: '/signin' } },
  history_changed: { rewrite: { host: 'utility', path: '/bill', from: '<td>Paid</td>', to: '<td>Late</td>' } },
  prefs_500: { status: { method: 'POST', host: 'utility', path: '/prefs' } },
  frame_hangs: { hang: { host: 'pay', path: '/frame' } },
  upload_input_gone: { rewrite: { host: 'utility', path: '/upload', from: /<input[^>]*type="file"[^>]*>/, to: '' } },
  pay_now_renamed: { rewrite: { host: 'pay', path: '/frame', from: '>Pay now</button>', to: '>Pay</button>' } },
  prefs_get_form: { rewrite: { host: 'utility', path: '/prefs', from: 'method="post" action="/prefs"', to: 'method="get" action="/prefs"' } },
  password_plain: {
    rewrite: { host: 'utility', path: '/signin', from: / type="password"([^>]*) autocomplete="current-password"/, to: ' type="text"$1' },
  },
};

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    const [flag, value] = [argv[i], argv[i + 1]];
    if (value === undefined) refuse('usage', `${flag} needs a value`);
    if (flag === '--listen') out.listen = Number(value);
    else if (flag === '--upstream') out.upstream = Number(value);
    else if (flag === '--fault') out.fault = value;
    else refuse('usage', 'node faults.mjs --listen N --upstream N --fault NAME');
  }
  if (!out.listen || !out.upstream || !FAULTS[out.fault]) {
    refuse('usage', `--fault takes one of ${Object.keys(FAULTS).join(',')}`);
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const fault = FAULTS[args.fault];
const held = [];

function hostOf(req) {
  return String(req.headers.host || '').split('.')[0].toLowerCase();
}

function pathOf(req) {
  return req.url.split('?')[0];
}

function matches(rule, req) {
  return rule && hostOf(req) === rule.host && pathOf(req) === rule.path && (!rule.method || rule.method === req.method);
}

function relocate(text) {
  return text.split(`.agent.test:${args.upstream}`).join(`.agent.test:${args.listen}`);
}

const server = createServer((req, res) => {
  if (matches(fault.status, req)) {
    req.resume();
    res.writeHead(500, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('fault\n');
    return;
  }
  if (matches(fault.hang, req)) {
    req.resume();
    held.push(res);
    return;
  }
  const upstream = request(
    { host: '127.0.0.1', port: args.upstream, method: req.method, path: req.url, headers: req.headers },
    (up) => {
      const headers = { ...up.headers };
      if (headers.location) headers.location = relocate(headers.location);
      const html = String(headers['content-type'] || '').startsWith('text/html');
      if (!html) {
        res.writeHead(up.statusCode, headers);
        up.pipe(res);
        return;
      }
      const chunks = [];
      up.on('data', (c) => chunks.push(c));
      up.on('end', () => {
        let body = relocate(Buffer.concat(chunks).toString('utf8'));
        if (matches(fault.rewrite, req) && req.method === 'GET') body = body.replace(fault.rewrite.from, fault.rewrite.to);
        delete headers['content-length'];
        delete headers['transfer-encoding'];
        headers['content-length'] = String(Buffer.byteLength(body));
        res.writeHead(up.statusCode, headers);
        res.end(body);
      });
    },
  );
  upstream.on('error', () => {
    if (!res.headersSent) res.writeHead(502);
    res.end();
  });
  req.pipe(upstream);
});

server.on('error', (err) => refuse('port', `127.0.0.1:${args.listen} cannot be bound (${err.code || err.name})`));
server.listen({ host: '127.0.0.1', port: args.listen, exclusive: true }, () => {
  process.stdout.write(`listening 127.0.0.1:${server.address().port}\n`);
});

function stop() {
  for (const res of held) res.destroy();
  server.close(() => process.exit(0));
  server.closeAllConnections?.();
}

process.on('SIGTERM', stop);
process.on('SIGINT', stop);
