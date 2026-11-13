import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

/** A PATH for a child that sets CI: a directory of this test whose only program is a stand-in
 * `cargo` that writes one line to `record` and exits 1, then /usr/bin and /bin. */
export function standInPath() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-floor-cargo-'));
  const record = path.join(dir, 'called.txt');
  const cargo = path.join(dir, 'cargo');
  fs.writeFileSync(cargo, `#!/bin/sh\necho "cargo $*" >> '${record}'\nexit 1\n`);
  fs.chmodSync(cargo, 0o755);
  return { dir, record, cargo, PATH: [dir, '/usr/bin', '/bin'].join(path.delimiter) };
}

/** The environment of a child that sets CI to `ci`, with the stand-in's PATH and no R4_GUEST. */
export function hostedEnv(ci, stand) {
  const copy = { ...process.env, CI: ci, PATH: stand.PATH };
  delete copy.R4_GUEST;
  return copy;
}
