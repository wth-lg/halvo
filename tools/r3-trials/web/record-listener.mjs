import { createServer } from 'node:net';
import { createInterface } from 'node:readline';

function usage() {
  process.stdout.write(`Usage: node tools/r3-trials/web/record-listener.mjs --path PATH

The recording listener of the row driver_killed_port (unit U0.7, ruling CP-22). It listens with
plain TCP on 127.0.0.1 and a port that the system picks, prints one line
"listening 127.0.0.1:<port>", and records every connection and every byte that it receives, and
how many request lines name PATH. It answers a request with an empty 204 once it has its headers.
Each line "count" on its standard input gets one line
"count connections=<n> bytes=<n> path_requests=<n>". It stops at the end of its standard input,
on TERM or on INT. Exit 0 when it stopped, 2 for a refusal: one line REFUSED <usage|port>: <detail>
on standard error.
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
if (argv.length !== 2 || argv[0] !== '--path' || !argv[1].startsWith('/')) {
  refuse('usage', '--path takes one absolute path; see --help');
}
const path = argv[1];
const counts = { connections: 0, bytes: 0, path_requests: 0 };

const server = createServer((socket) => {
  counts.connections += 1;
  let head = '';
  let answered = false;
  socket.on('data', (chunk) => {
    counts.bytes += chunk.length;
    if (answered) return;
    head += chunk.toString('latin1');
    const end = head.indexOf('\r\n\r\n');
    if (end < 0) return;
    const line = head.slice(0, head.indexOf('\r\n'));
    if (line.split(' ')[1] === path) counts.path_requests += 1;
    answered = true;
    socket.end('HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n');
  });
  socket.on('error', () => {});
});
server.on('error', (err) => refuse('port', `listen failed (${err.code || err.name})`));

function stop() {
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(0), 1000).unref();
}

server.listen({ host: '127.0.0.1', port: 0, exclusive: true }, () => {
  process.stdout.write(`listening 127.0.0.1:${server.address().port}\n`);
  const lines = createInterface({ input: process.stdin });
  lines.on('line', (line) => {
    if (line.trim() === 'count') {
      process.stdout.write(`count connections=${counts.connections} bytes=${counts.bytes} path_requests=${counts.path_requests}\n`);
    }
  });
  lines.on('close', stop);
});

process.on('SIGTERM', stop);
process.on('SIGINT', stop);
