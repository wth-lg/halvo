import { spawn } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [input, ...rest] = process.argv.slice(2);
if (input !== 'pipe' && input !== 'null') {
  process.stderr.write('REFUSED usage: node parent.mjs pipe|null [server argument]...\n');
  process.exit(2);
}
const server = spawn(process.execPath, [join(dirname(fileURLToPath(import.meta.url)), 'server.mjs'), ...rest], {
  stdio: [input === 'pipe' ? 'pipe' : 'ignore', 'pipe', 'ignore'],
  detached: input === 'pipe',
  windowsHide: true,
});
server.stdin?.on('error', () => {});
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
const leave = () => {
  if (server.exitCode === null && server.signalCode === null) server.kill();
  else process.exit(0);
};
process.stdin.on('end', leave);
process.stdin.on('error', leave);
process.stdin.resume();
