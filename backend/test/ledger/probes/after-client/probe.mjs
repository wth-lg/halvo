import { execFileSync, spawn } from "node:child_process";
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from "node:fs";
import http from "node:http";
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..", "..", "..");
const CLIENT_LEAVES_AFTER_MS = 1000;
const WATCH_MS = 70_000;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const freePort = () =>
  new Promise((resolve, reject) => {
    const probe = createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });

const env = Object.fromEntries(
  execFileSync("sh", [join(ROOT, "scripts/with-env.sh"), "env"], { encoding: "utf8" })
    .split("\n")
    .filter((line) => line.includes("="))
    .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]),
);
env.XDG_CONFIG_HOME = env.HALVO_WRANGLER_CONFIG_HOME;
mkdirSync(join(ROOT, ".tmp"), { recursive: true });
const dir = mkdtempSync(join(ROOT, ".tmp", "after-client."));
const compatibilityDate = JSON.parse(readFileSync(join(ROOT, "wrangler.jsonc"), "utf8")).compatibility_date;
writeFileSync(join(dir, "wrangler.jsonc"), JSON.stringify({ name: "after-client-probe", main: join(HERE, "worker.js"), compatibility_date: compatibilityDate, send_metrics: false }));
const port = await freePort();
const inspector = await freePort();
const logPath = join(dir, "wrangler.log");
const log = openSync(logPath, "a");
const child = spawn(
  join(ROOT, "node_modules/.bin/wrangler"),
  ["dev", "--config", join(dir, "wrangler.jsonc"), "--ip", "127.0.0.1", "--port", String(port), "--inspector-port", String(inspector), "--persist-to", join(dir, "state"), "--show-interactive-dev-session=false"],
  { detached: true, stdio: ["ignore", log, log], env },
);
closeSync(log);
const stop = () => {
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {}
};
process.on("exit", stop);

for (let tries = 0; ; tries += 1) {
  const ready = await new Promise((resolve) => {
    http.get({ host: "127.0.0.1", port, path: "/", agent: false }, (response) => resolve(response.statusCode === 200)).on("error", () => resolve(false));
  });
  if (ready) break;
  if (tries > 300) {
    console.log(`FAIL probe after-client: wrangler did not answer; see ${logPath}`);
    process.exit(1);
  }
  await sleep(100);
}

const id = `p${Date.now()}`;
const leftAt = await new Promise((resolve) => {
  const request = http.get({ host: "127.0.0.1", port, path: `/start?id=${id}`, agent: false }, (response) => {
    response.once("data", () => {
      setTimeout(() => {
        request.destroy();
        resolve(Date.now());
      }, CLIENT_LEAVES_AFTER_MS);
    });
  });
});
await sleep(WATCH_MS);
stop();
const lines = stripVTControlCharacters(readFileSync(logPath, "utf8"))
  .split("\n")
  .filter((line) => line.includes(`"probe":"${id}"`))
  .map((line) => JSON.parse(line.slice(line.indexOf("{"))));
const gone = lines.find((line) => line.gone !== undefined)?.gone ?? null;
const ticks = lines.filter((line) => line.tick !== undefined).map((line) => line.tick);
const lastTick = ticks.length === 0 ? null : Math.max(...ticks);
const ended = lines.find((line) => line.end !== undefined)?.end ?? null;
const versions = execFileSync(join(ROOT, "node_modules/.bin/wrangler"), ["--version"], { encoding: "utf8", env }).trim();
const after = gone === null || lastTick === null ? null : lastTick - gone;
console.log(JSON.stringify({ leftAtMs: leftAt, gone, lastTick, ended, ticks: ticks.length, workAfterClientGoneMs: after, watchedMs: WATCH_MS, wrangler: versions, log: logPath }));
console.log(
  gone === null
    ? "FAIL probe after-client: the worker never saw the client leave"
    : `PASS probe after-client: wrangler ${versions}: the waitUntil task went on ${after} ms after its first failed write to the client that left, and ${ended === null ? "had not ended" : `ended by itself at ${ended} ms`} when the probe stopped watching at ${WATCH_MS} ms`,
);
