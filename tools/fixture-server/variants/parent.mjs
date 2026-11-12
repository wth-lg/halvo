import { spawn } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const server = spawn(process.execPath, [join(dirname(fileURLToPath(import.meta.url)), 'server.mjs'), ...process.argv.slice(2)], {
  stdio: ['pipe', 'pipe', 'ignore'],
  detached: true,
  windowsHide: true,
});
server.stdin.on('error', () => {});
let out = '';
let told = false;
server.stdout.on('data', (d) => {
  if (told) return;
  out += d;
  const m = /^listening 127\.0\.0\.1:(\d+)\n/.exec(out);
  if (m) {
    told = true;
    process.stdout.write(`server ${server.pid} ${m[1]}\n`);
  }
});
server.on('exit', () => process.exit(1));
process.stdin.on('end', () => process.exit(0));
process.stdin.on('error', () => process.exit(0));
process.stdin.resume();
