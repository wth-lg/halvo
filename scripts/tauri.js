// Runs the Tauri CLI with ~/.cargo/bin on PATH, so `npm run tauri build`
// works in a shell that has not re-read its profile since rustup was installed.
"use strict";
const { spawnSync } = require("node:child_process");
const os = require("node:os");
const path = require("node:path");

const cargoBin = path.join(process.env.CARGO_HOME || path.join(os.homedir(), ".cargo"), "bin");
const env = { ...process.env, PATH: `${cargoBin}${path.delimiter}${process.env.PATH || ""}` };
const cli = require.resolve("@tauri-apps/cli/tauri.js");

const result = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], { stdio: "inherit", env });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
