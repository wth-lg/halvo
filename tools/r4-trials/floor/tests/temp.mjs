import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const made = [];

/** A new directory under the temporary directory, named `r4-floor-<prefix>-` and a suffix, which
 * `cleanUp` removes. */
export function tempDir(prefix) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `r4-floor-${prefix}-`));
  made.push(dir);
  return dir;
}

/** Removes every directory that `tempDir` made in this process, and returns how many. */
export function cleanUp() {
  let n = 0;
  while (made.length > 0) {
    fs.rmSync(made.pop(), { recursive: true, force: true });
    n += 1;
  }
  return n;
}
