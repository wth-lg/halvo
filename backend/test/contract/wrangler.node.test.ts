import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { expect, test } from "vitest";

const ROOT = process.cwd();
const ACCOUNT = "4d2c6a8e-1f3b-4a5c-9e7d-0b1c2d3e4f5a";
const CREDENTIAL = "9c".repeat(32);
const VARIABLES = [
  "PROVIDER_BASE_URL",
  "PUBLIC_ORIGIN",
  "PROVIDER_PRICE_ID",
  "PROVIDER_METER_ID",
  "PROVIDER_METER_EVENT",
  "HOLD_LIFE_MS",
  "UPSTREAM_DEADLINE_MS",
  "DISPATCH_MARGIN_MS",
  "SYNC_INTERVAL_SECONDS",
  "SYNC_MARGIN_SECONDS",
];
const TOP_LEVEL = ["$schema", "name", "main", "compatibility_date", "send_metrics", "durable_objects", "migrations", "vars", "observability"];
const SWITCHES: [string, string | null][] = [
  ["HALVO_WRANGLER_CONFIG_HOME", null],
  ["CLOUDFLARE_CF_FETCH_ENABLED", "false"],
  ["WRANGLER_SEND_METRICS", "false"],
  ["WRANGLER_SEND_ERROR_REPORTS", "false"],
  ["WRANGLER_HIDE_BANNER", "true"],
];

function missingSwitch(): string | null {
  for (const [name, value] of SWITCHES) {
    const set = process.env[name];
    if (set === undefined || set === "" || (value !== null && set !== value)) return name;
  }
  return null;
}

const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

test("the two wrangler files differ in name, main and the two test variables, and hold the keys of contract 6", () => {
  const deployable = JSON.parse(readFileSync("wrangler.jsonc", "utf8"));
  const testEntry = JSON.parse(readFileSync("wrangler.test.jsonc", "utf8"));
  expect(Object.keys(deployable)).toEqual(TOP_LEVEL);
  expect(Object.keys(testEntry)).toEqual(TOP_LEVEL);
  expect([deployable.name, deployable.main, testEntry.name, testEntry.main]).toEqual([
    "halvo-backend",
    "src/index.ts",
    "halvo-backend-test",
    "src/gateway/index.ts",
  ]);
  expect(Object.keys(deployable.vars)).toEqual(VARIABLES);
  expect(testEntry.vars).toEqual({ ...deployable.vars, TEST_CLOCK: "1", UPSTREAM_BASE_URL: "" });
  const same = (file: Record<string, unknown>) => ({ ...file, name: null, main: null, vars: null });
  expect(same(testEntry)).toEqual(same(deployable));
  expect(deployable.durable_objects).toEqual({ bindings: [{ name: "ACCOUNT", class_name: "AccountObject" }] });
  expect(deployable.migrations).toEqual([{ tag: "v1", new_sqlite_classes: ["AccountObject"] }]);
  expect(deployable.observability).toEqual({ enabled: false, logs: { enabled: false, invocation_logs: false } });
  expect([deployable.compatibility_date, deployable.send_metrics]).toEqual(["2026-11-01", false]);
});

test("stack.d/vars.env holds the five synthetic values as NAME=VALUE lines", () => {
  expect(readFileSync("stack.d/vars.env", "utf8")).toBe(
    [
      "PROVIDER_API_KEY=sk_test_halvolocal",
      "WEBHOOK_SIGNING_SECRET=whsec_test_halvo_local",
      "PROVIDER_PRICE_ID=price_test_halvo_local",
      "PROVIDER_METER_ID=mtr_test_halvo_local",
      "PROVIDER_METER_EVENT=halvo_usage",
      "",
    ].join("\n"),
  );
});

test("on the real test entry --var passes a value with colons whole and wins over the file's value", async () => {
  const missing = missingSwitch();
  if (missing !== null) {
    throw new Error(
      `scripts/env.sh did not set the switch ${missing} of wrangler: start this test with pnpm test, or with sh scripts/with-env.sh node_modules/.bin/vitest run`,
    );
  }
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const state = mkdtempSync(join(ROOT, ".tmp", "wrangler-test-"));
  const port = await freePort();
  const inspector = await freePort();
  const origin = `http://127.0.0.1:${port}`;
  const child = spawn(
    join(ROOT, "node_modules/.bin/wrangler"),
    [
      "dev",
      "--config",
      join(ROOT, "wrangler.test.jsonc"),
      "--ip",
      "127.0.0.1",
      "--port",
      String(port),
      "--inspector-port",
      String(inspector),
      "--persist-to",
      state,
      "--var",
      "HOLD_LIFE_MS:1234",
      "--var",
      `PUBLIC_ORIGIN:${origin}`,
    ],
    { cwd: ROOT, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME } },
  );
  let output = "";
  child.stdout.on("data", (chunk) => {
    output += String(chunk);
  });
  child.stderr.on("data", (chunk) => {
    output += String(chunk);
  });
  let exited = false;
  const exit = new Promise<void>((resolve) =>
    child.on("exit", () => {
      exited = true;
      resolve();
    }),
  );
  try {
    let up = false;
    for (let tries = 0; tries < 240 && !exited && !up; tries += 1) {
      try {
        await fetch(`${origin}/v1/done`, { signal: AbortSignal.timeout(2000) });
        up = true;
      } catch {
        await pause(250);
      }
    }
    expect(up, output).toBe(true);
    const post = async (path: string, body: unknown): Promise<Record<string, unknown>> => {
      const response = await fetch(`${origin}${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
      return response.json();
    };
    expect(await post("/test/seed", { account: ACCOUNT, credential: CREDENTIAL, subscription: "active" })).toEqual({ result: "seeded" });
    const snapshot = await post("/test/inspect", { account: ACCOUNT });
    expect(snapshot.result).toBe("snapshot");
    expect(snapshot.config).toEqual({ holdLifeMs: 1234, intervalSeconds: 3600, marginSeconds: 300, publicOrigin: origin, testClock: true });
  } finally {
    for (const signal of ["SIGINT", "SIGTERM", "SIGKILL"]) {
      if (exited) break;
      child.kill(signal);
      await Promise.race([exit, pause(8000)]);
    }
    writeFileSync(join(state, "wrangler.log"), output);
  }
}, 120_000);
