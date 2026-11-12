import { createServer } from 'node:http';
import { Socket } from 'node:net';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { VARIANTS, createFixture, loadSeeds, startDayOf } from './fixture.mjs';

const DEFAULT_PORT = 18090;

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const out = { port: DEFAULT_PORT, seed: 1, variants: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (flag === '--port' || flag === '--seed' || flag === '--variant') {
      if (value === undefined) refuse('usage', `${flag} needs a value`);
      i += 1;
    }
    if (flag === '--port') {
      if (!/^\d{1,5}$/.test(value) || Number(value) > 65535) refuse('usage', '--port takes an integer from 0 to 65535');
      out.port = Number(value);
    } else if (flag === '--seed') {
      if (!['1', '2', '3'].includes(value)) refuse('usage', '--seed takes 1, 2 or 3');
      out.seed = Number(value);
    } else if (flag === '--variant') {
      if (!VARIANTS.includes(value)) refuse('usage', `--variant takes one of ${VARIANTS.join(',')}`);
      out.variants.push(value);
    } else {
      refuse('usage', `unknown argument; usage: node server.mjs [--port N] [--seed 1|2|3] [--variant NAME]...`);
    }
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
let seeds;
try {
  seeds = loadSeeds(join(dirname(fileURLToPath(import.meta.url)), 'data'));
} catch (err) {
  refuse('data', err.message);
}

const fixture = createFixture({
  seeds,
  seed: args.seed,
  variants: args.variants,
  startDay: startDayOf(Date.now()),
  port: args.port,
});

const server = createServer((req, res) => {
  fixture.handle(req, res).catch(() => {
    if (!res.headersSent) res.writeHead(500, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('fixture error\n');
  });
});

server.on('error', (err) => {
  if (err.code === 'EADDRINUSE' || err.code === 'EACCES') refuse('port', `127.0.0.1:${args.port} cannot be bound (${err.code})`);
  refuse('port', `listen failed (${err.code || err.name})`);
});

server.listen({ host: '127.0.0.1', port: args.port, exclusive: true }, () => {
  const { address, port } = server.address();
  fixture.setPort(port);
  process.stdout.write(`listening ${address}:${port}\n`);
});

function stop() {
  server.close(() => process.exit(0));
  server.closeAllConnections?.();
}

process.on('SIGTERM', stop);
process.on('SIGINT', stop);

if (process.stdin instanceof Socket && !process.stdin.isTTY) {
  process.stdin.on('end', stop);
  process.stdin.on('error', stop);
  process.stdin.resume();
}
