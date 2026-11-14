import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { tempDir } from './temp.mjs';

const CARGO = ['cargo', 'cargo.exe', 'cargo.cmd'];

/** Whether a directory holds a program named cargo. */
export function holdsCargo(dir) {
  return CARGO.some((name) => fs.existsSync(path.join(dir, name)));
}

/** A PATH for a child of the tests: a directory of this test whose only program is a stand-in
 * `cargo` that writes one line to `record` and exits 1, then every directory of this process's
 * PATH that holds no cargo, so that the child finds sh and git and no other cargo. */
export function standInPath() {
  const dir = tempDir('cargo');
  const record = path.join(dir, 'called.txt');
  const cargo = path.join(dir, 'cargo');
  fs.writeFileSync(cargo, `#!/bin/sh\necho "cargo $*" >> '${record.replace(/\\/g, '/')}'\nexit 1\n`);
  fs.chmodSync(cargo, 0o755);
  const kept = (process.env.PATH ?? '').split(path.delimiter).filter((d) => d !== '' && !holdsCargo(d));
  return { dir, record, cargo, PATH: [dir, ...kept].join(path.delimiter) };
}

/** A copy of this process's environment with `extra`, whose PATH, however it is spelled, is `stand.PATH`. */
export function withPath(stand, extra = {}) {
  const copy = { ...process.env };
  for (const key of Object.keys(copy)) if (key.toUpperCase() === 'PATH') delete copy[key];
  return { ...copy, PATH: stand.PATH, ...extra };
}

/** The environment of a child that sets CI to `ci`, with the stand-in's PATH and no R4_GUEST. */
export function hostedEnv(ci, stand) {
  const copy = withPath(stand, { CI: ci });
  delete copy.R4_GUEST;
  return copy;
}
