// Runs the Tauri CLI with ~/.cargo/bin on PATH, so `npm run tauri build`
// works in a shell that has not re-read its profile since rustup was installed.
// For `build` and `bundle` it first runs scripts/r1/ci.d/54-tag-needs-store4.sh,
// then builds and stages each helper of packaging/helpers.d and gives the CLI
// their paths as bundle.externalBin for that run alone (packaging/README.md).
// On a Mac it also gives the CLI bundle.macOS.signingIdentity "-", the ad hoc seal,
// for that run alone, and starts the CLI with CI=true, so that the script of the
// disk image drives no Finder window; when that CLI call fails on hdiutil's busy line,
// it runs the CLI once more.
// It takes the signing variables out of the environment of everything it starts.
"use strict";
const { spawn, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const cargoBin = path.join(process.env.CARGO_HOME || path.join(os.homedir(), ".cargo"), "bin");
const pathKey = Object.keys(process.env).find((key) => key.toUpperCase() === "PATH") || "PATH";
const env = { ...process.env, [pathKey]: `${cargoBin}${path.delimiter}${process.env[pathKey] || ""}` };
for (const key of Object.keys(env)) {
  const upper = key.toUpperCase();
  if (upper.startsWith("APPLE_") || upper.startsWith("TAURI_SIGNING_") || upper === "HALVO_WINDOWS_SIGN_COMMAND") {
    delete env[key];
  }
}
const cli = require.resolve("@tauri-apps/cli/tauri.js");
const exe = process.platform === "win32" ? ".exe" : "";

function fail(message) {
  console.error(`scripts/tauri.js: ${message}`);
  process.exit(1);
}

function run(file, args, options) {
  const result = spawnSync(file, args, { stdio: "inherit", env, ...options });
  if (result.error) fail(`${file}: ${result.error.message}`);
  return result.status === null ? 1 : result.status;
}

function read(file, args) {
  const result = spawnSync(file, args, {
    cwd: root,
    env,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (result.error) fail(`${file}: ${result.error.message}`);
  if (result.status !== 0) fail(`${file} ${args[0]} exited ${result.status}`);
  return result.stdout;
}

// Each file of packaging/helpers.d is one helper; its name is the installed name.
// A name that starts with a dot, such as Finder's .DS_Store, is no helper.
// Beside its three lines a file may hold the one line version=none, for a helper
// with no version argument: scripts/r1/ci.d/62-build.sh --files does not start it.
function helpers() {
  const directory = path.join(root, "packaging", "helpers.d");
  return fs
    .readdirSync(directory)
    .filter((name) => !name.startsWith("."))
    .sort()
    .map((name) => {
      const where = `packaging/helpers.d/${name}`;
      if (!/^[a-z0-9][a-z0-9-]*$/.test(name)) fail(`${where}: a helper's name is lower-case letters, digits and -`);
      const fields = {};
      for (const line of fs.readFileSync(path.join(directory, name), "utf8").split(/\r?\n/)) {
        if (line === "") continue;
        const match = /^(package|bin|manifest)=([^\s=]+)$/.exec(line) || /^(version)=(none)$/.exec(line);
        if (!match || match[1] in fields) fail(`${where}: the line "${line}" is not one of package=, bin=, manifest=, version=none`);
        fields[match[1]] = match[2];
      }
      for (const key of ["package", "bin", "manifest"]) {
        if (!(key in fields)) fail(`${where} has no line ${key}=`);
      }
      return { name, ...fields };
    });
}

// Builds the helper by a call of its own, always before it copies, and copies it to
// target/helpers/<name>-<triple> when its bytes differ, so an unchanged copy keeps
// its time and tauri-build's rerun-if-changed does not rebuild the app.
function stage(helper, triple) {
  const metadata = read("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked", "--manifest-path", helper.manifest]);
  const built = path.join(JSON.parse(metadata).target_directory, "release", helper.bin + exe);
  const status = run("cargo", ["build", "--release", "--locked", "--manifest-path", helper.manifest, "-p", helper.package, "--bin", helper.bin], { cwd: root });
  if (status !== 0) fail(`the build of the helper ${helper.name} exited ${status}`);
  const staged = path.join(root, "target", "helpers", `${helper.name}-${triple}${exe}`);
  fs.mkdirSync(path.dirname(staged), { recursive: true });
  const bytes = fs.readFileSync(built);
  if (!fs.existsSync(staged) || !fs.readFileSync(staged).equals(bytes)) {
    fs.copyFileSync(built, staged);
    fs.chmodSync(staged, 0o755);
  }
  console.error(`scripts/tauri.js: staged ${helper.name} for ${triple}`);
  return `../target/helpers/${helper.name}`;
}

// The place of signing and notarization, the owner's (item I-116): it runs after a
// build or bundle that made a bundle, and does nothing in this cut. The ad hoc seal
// uses no certificate and no identity, so it is not this step.
function sign_step() {}

const args = process.argv.slice(2);
const end = args.indexOf("--");
const own = end === -1 ? args : args.slice(0, end);
const command = own.find((arg) => !arg.startsWith("-"));
const stages = command === "build" || command === "bundle";
const mac = stages && process.platform === "darwin";
let cliArgs = args;
if (stages) {
  if (own.some((arg) => arg === "--target" || arg.startsWith("--target=") || /^-t/.test(arg))) {
    fail("a build for another target than the host is not served: the helpers are staged for the host triple alone");
  }
  if (env.CARGO_BUILD_TARGET) fail("CARGO_BUILD_TARGET is set: the helpers are staged for the host triple alone");
  const ruled = run("sh", ["scripts/r1/ci.d/54-tag-needs-store4.sh"], { cwd: root, env: { ...env, CI_D_REPO: root } });
  if (ruled !== 0) fail(`scripts/r1/ci.d/54-tag-needs-store4.sh exited ${ruled}, so nothing is built`);
  const host = /^host: (\S+)$/m.exec(read("rustc", ["-vV"]));
  if (!host) fail("rustc -vV names no host triple");
  const externalBin = helpers().map((helper) => stage(helper, host[1]));
  const bundle = mac ? { externalBin, macOS: { signingIdentity: "-" } } : { externalBin };
  const at = args.indexOf(command) + 1;
  cliArgs = [...args.slice(0, at), "--config", JSON.stringify({ bundle }), ...args.slice(at)];
}

// hdiutil's answer when it cannot make the disk image's file because the system holds
// something that it needs; a hosted macOS runner gives it now and then.
const busy = "hdiutil: create failed - Resource busy";

// Runs the CLI of a Mac build with its output written on to this process's own as it
// comes, and answers its exit status and whether that output held hdiutil's busy line.
function watched() {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [cli, ...cliArgs], { env: { ...env, CI: "true" }, stdio: ["inherit", "pipe", "pipe"] });
    let held = false;
    for (const [from, to] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
      let tail = "";
      from.on("data", (chunk) => {
        to.write(chunk);
        const text = tail + chunk.toString("latin1");
        held ||= text.includes(busy);
        tail = text.slice(-busy.length);
      });
    }
    child.on("error", (error) => fail(`${process.execPath}: ${error.message}`));
    child.on("close", (code) => resolve({ status: code === null ? 1 : code, held }));
  });
}

// A Mac build whose CLI failed on hdiutil's busy line runs the CLI once more, and says so
// on standard error; any other failure, and a second failure, ends the build.
async function main() {
  let status;
  if (mac) {
    const first = await watched();
    status = first.status;
    if (status !== 0 && first.held) {
      console.error(`scripts/tauri.js: the Tauri CLI exited ${status} after "${busy}" while it made the disk image, so it runs once more`);
      status = (await watched()).status;
    }
  } else {
    status = run(process.execPath, [cli, ...cliArgs], {});
  }
  if (status === 0 && stages && !own.includes("--no-bundle")) sign_step();
  process.exitCode = status;
}

main();
