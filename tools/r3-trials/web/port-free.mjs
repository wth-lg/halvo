import { createServer } from 'node:net';

const ports = process.argv.slice(2).map(Number);
let next = 0;

function probe() {
  if (next === ports.length) process.exit(0);
  const port = ports[next];
  next += 1;
  const server = createServer();
  server.once('error', () => {
    process.stdout.write(`${port}\n`);
    process.exit(1);
  });
  server.listen({ host: '127.0.0.1', port, exclusive: true }, () => server.close(probe));
}

probe();
