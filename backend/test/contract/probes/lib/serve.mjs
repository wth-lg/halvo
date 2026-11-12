import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join } from "node:path";

const [config, persist, runsText, startSecondsText] = process.argv.slice(2);
const runs = JSON.parse(runsText);
const startSeconds = Number(startSecondsText ?? "90");
const wrangler = join(process.env.HALVO_ROOT, "node_modules/.bin/wrangler");

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function oneRun(run, index) {
  const port = await freePort();
  const inspector = await freePort();
  const args = ["dev", "--config", config, "--ip", "127.0.0.1", "--port", String(port)];
  args.push("--inspector-port", String(inspector), "--persist-to", persist);
  for (const variable of run.vars ?? []) args.push("--var", variable);
  const child = spawn(wrangler, args, {
    cwd: dirname(config),
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME },
  });
  let output = "";
  child.stdout.on("data", (chunk) => {
    output += chunk;
  });
  child.stderr.on("data", (chunk) => {
    output += chunk;
  });
  let exited = null;
  const exit = new Promise((resolve) =>
    child.on("exit", (code, signal) => {
      exited = { code, signal };
      resolve();
    }),
  );
  const base = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + startSeconds * 1000;
  let started = false;
  while (!exited && Date.now() < deadline) {
    try {
      await fetch(`${base}/__ready`, { signal: AbortSignal.timeout(3000) });
      started = true;
      break;
    } catch {
      await pause(250);
    }
  }
  const answers = [];
  if (started && !exited) {
    for (const path of run.paths ?? []) {
      const response = await fetch(`${base}${path}`, { signal: AbortSignal.timeout(20000) });
      answers.push({ path, status: response.status, body: await response.text() });
    }
  }
  for (const signal of ["SIGINT", "SIGTERM", "SIGKILL"]) {
    if (exited) break;
    child.kill(signal);
    await Promise.race([exit, pause(8000)]);
  }
  writeFileSync(join(dirname(config), `wrangler-${index}.log`), output);
  return { started, answers, exited };
}

const results = [];
for (const [index, run] of runs.entries()) results.push(await oneRun(run, index));
console.log(`PROBE-RESULT ${JSON.stringify({ runs: results })}`);
