import { createServer } from 'node:https';
import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node tools/r3-trials/web/tls-listener.mjs --cert FILE --key FILE

The TLS listener of the row cert_error of contract A13.7. It serves HTTPS with the certificate and
the key in the two PEM files on 127.0.0.1 and a port that the system picks, prints one line
"listening 127.0.0.1:<port>", and answers every request with a short page. It counts the requests
that it receives, the TLS connections that completed and the ones that failed. Each line "count"
on its standard input gets one line "count requests=<n> handshakes=<n> tls_errors=<n>". It stops at
the end of its standard input, on TERM or on INT. It prints no byte of the certificate or the key.
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
const opts = {};
for (let i = 0; i < argv.length; i += 2) {
  const flag = argv[i];
  const value = argv[i + 1];
  if (flag !== '--cert' && flag !== '--key') refuse('usage', `unknown argument ${flag}; see --help`);
  if (value === undefined) refuse('usage', `${flag} needs a value`);
  opts[flag.slice(2)] = value;
}
if (!opts.cert || !opts.key) refuse('usage', '--cert and --key are required');

let cert;
let key;
try {
  cert = readFileSync(opts.cert);
  key = readFileSync(opts.key);
} catch (err) {
  refuse('files', `cannot read a PEM file (${err.code || err.name})`);
}

const counts = { requests: 0, handshakes: 0, tls_errors: 0 };
let server;
try {
  server = createServer({ cert, key }, (req, res) => {
    counts.requests += 1;
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end('<!doctype html><title>tls listener</title><p>tls listener</p>\n');
  });
} catch (err) {
  refuse('files', `the PEM files hold no usable certificate and key (${err.code || err.name})`);
}
server.on('secureConnection', () => {
  counts.handshakes += 1;
});
server.on('tlsClientError', () => {
  counts.tls_errors += 1;
});
server.on('error', (err) => refuse('port', `listen failed (${err.code || err.name})`));

function stop() {
  server.close(() => process.exit(0));
  server.closeAllConnections?.();
}

server.listen({ host: '127.0.0.1', port: 0, exclusive: true }, () => {
  const { address, port } = server.address();
  process.stdout.write(`listening ${address}:${port}\n`);
  const lines = createInterface({ input: process.stdin });
  lines.on('line', (line) => {
    if (line.trim() === 'count') {
      process.stdout.write(`count requests=${counts.requests} handshakes=${counts.handshakes} tls_errors=${counts.tls_errors}\n`);
    }
  });
  lines.on('close', stop);
});

process.on('SIGTERM', stop);
process.on('SIGINT', stop);
