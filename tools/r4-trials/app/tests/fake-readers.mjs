// Readers of the system for tests: they read the state file that fake-app.mjs writes and whether
// its process lives, in the shape of the readers of system/system.mjs.
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
      return { count: 2, webview: 1, procs: [{ pid, start: '1' }] };
    },
    alive(procs) {
      return procs.filter((p) => alive(p.pid)).length;
    },
  };
}
