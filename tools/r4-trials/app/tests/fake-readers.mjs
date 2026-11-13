// Readers of the system for tests: they read the state file that fake-app.mjs writes and whether
// its process and a child that it names live, in the shape of the readers of system/system.mjs.
import fs from 'node:fs';

function alive(pid) {
  try {
    process.kill(Number(pid), 0);
    return true;
  } catch {
    return false;
  }
}

/** The readers `{ window, tree, alive }` over the state file `stateFile`. */
export function fakeReaders(stateFile) {
  return {
    window(pid) {
      const [state, owner] = fs.readFileSync(stateFile, 'utf8').trim().split(' ');
      return { state: Number(owner) === pid && alive(pid) ? state : 'absent', by: 'title' };
    },
    tree(pid) {
      if (!alive(pid)) return { count: 0, webview: 0, procs: [] };
      const child = Number(fs.readFileSync(stateFile, 'utf8').trim().split(' ')[2]);
      const procs = [{ pid, start: '1' }, ...(child ? [{ pid: child, start: '1' }] : [])];
      return { count: procs.length + 1, webview: 1, procs };
    },
    alive(procs) {
      return procs.filter((p) => alive(p.pid)).length;
    },
  };
}
