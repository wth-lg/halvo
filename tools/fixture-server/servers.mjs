import { spawn } from 'node:child_process';
import { appendFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The server that the tests start. */
export const SERVER = fileURLToPath(new URL('./server.mjs', import.meta.url));
/** How long a test waits for a server to end after it asked, in milliseconds. */
export const END_MS = 5000;
const START_MS = 20000;

/** Whether a process with this id is alive. */
export function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === 'EPERM';
  }
}

/** Resolves to `{ code, signal }` when the child exits within `ms`, else to null. */
export function exitWithin(child, ms) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  }
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), ms);
    child.once('exit', (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal });
    });
  });
}

/** Resolves to true when the process is gone by `deadline` (a Date.now() value), polling every 100 ms. */
export async function goneBy(pid, deadline) {
  while (alive(pid)) {
    if (Date.now() > deadline) return false;
    await new Promise((resolve) => { setTimeout(resolve, 100); });
  }
  return true;
}

/**
 * Starts server.mjs with these arguments and resolves to `{ child, pid, port, line }` once it prints its
 * `listening` line. With `input: 'pipe'` (the default) the server's standard input is a pipe whose write end this
 * process holds, so the server ends when this process ends, however it ends; the child and its streams are then
 * unreferenced, so a test process that fails before its cleanup still exits. With `input: 'ignore'` the input is
 * the null device. A server that prints no `listening` line within 20 s is killed, and the promise rejects.
 * Where the environment names a file in FIXTURE_SERVER_PIDS, the server's process id is appended to it.
 */
export function startServer(args, { input = 'pipe' } = {}) {
  const child = spawn(process.execPath, [SERVER, ...args], {
    stdio: [input, 'pipe', 'pipe'],
    windowsHide: true,
  });
  child.stdin?.on('error', () => {});
  if (process.env.FIXTURE_SERVER_PIDS) appendFileSync(process.env.FIXTURE_SERVER_PIDS, `${child.pid}\n`);
  return new Promise((resolve, reject) => {
    let out = '';
    const deadline = setTimeout(() => child.kill('SIGKILL'), START_MS);
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      out += chunk;
      const line = /^listening 127\.0\.0\.1:(\d+)\n/.exec(out);
      if (line === null) return;
      clearTimeout(deadline);
      child.unref();
      for (const stream of [child.stdin, child.stdout, child.stderr]) stream?.unref?.();
      resolve({ child, pid: child.pid, port: Number(line[1]), line: line[0] });
    });
    child.on('exit', (code, signal) => {
      clearTimeout(deadline);
      reject(new Error(`server exited ${code ?? signal} before it listened`));
    });
    child.on('error', reject);
  });
}

/**
 * Ends a server that `startServer` started: sends `signal` (none when it is null), waits up to END_MS for its exit,
 * then sends SIGKILL and waits for that exit. Resolves to `{ code, signal, killed, gone }`: `killed` is true when
 * SIGKILL was needed, `gone` is whether the server's process id is no longer alive at the end.
 */
export async function stopServer(server, signal = 'SIGTERM') {
  const { child } = server;
  if (child.exitCode !== null || child.signalCode !== null) {
    return { code: child.exitCode, signal: child.signalCode, killed: false, gone: !alive(child.pid) };
  }
  if (signal !== null) child.kill(signal);
  const ended = await exitWithin(child, END_MS);
  if (ended !== null) return { ...ended, killed: false, gone: !alive(child.pid) };
  child.kill('SIGKILL');
  const killed = await exitWithin(child, END_MS);
  return { code: killed?.code ?? null, signal: killed?.signal ?? null, killed: true, gone: !alive(child.pid) };
}

/** A new directory under the system's temporary directory, removed by the returned function. */
export function scratchDir(prefix) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  return { dir, remove: () => rmSync(dir, { recursive: true, force: true }) };
}
