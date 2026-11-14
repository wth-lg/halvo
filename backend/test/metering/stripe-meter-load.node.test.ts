import { spawn } from "node:child_process";
import { expect, test, vi } from "vitest";
import { localValues } from "../../test-kit/provider-fake/main.mjs";
import { ROOT, startRecorder } from "../billing/servers.ts";

vi.setConfig({ testTimeout: 19_884 });

const VARS = localValues(`${ROOT}stack.d/vars.env`);
const INDEX = new URL("../../src/billing/index.ts", import.meta.url).href;
const METER = new URL("../../src/metering/stripe-meter.ts", import.meta.url).href;
const ACCOUNT = "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";

const CHILD = `
const order = JSON.parse(process.env.LOAD_ORDER);
for (const url of order) await import(url);
const billing = await import(process.env.LOAD_INDEX);
const mapping = await import(process.env.LOAD_METER);
const env = JSON.parse(process.env.LOAD_ENV);
const usage = JSON.parse(process.env.LOAD_USAGE);
const viaPort = await billing.providerPort(env).sendUsage(usage);
const direct = await mapping.makeStripeMeter({ baseUrl: env.PROVIDER_BASE_URL, apiKey: env.PROVIDER_API_KEY, meterId: env.PROVIDER_METER_ID, eventName: env.PROVIDER_METER_EVENT }).sendUsage(usage);
console.log(JSON.stringify([viaPort.kind, direct.kind]));
`;

const LATE_BINDING = `
import { registerHooks } from "node:module";
const meterUrl = process.env.LOAD_METER;
const stub = "data:text/javascript," + encodeURIComponent(
  "import { makeStripeMeter } from " + JSON.stringify(meterUrl) + "; export const PROVIDER_CALL_TIMEOUT_MS = 1500; export { makeStripeMeter };",
);
const redirected = [];
registerHooks({
  resolve(specifier, context, next) {
    if (context.parentURL === meterUrl && specifier === "../billing/index.ts") {
      redirected.push(specifier);
      return { url: stub, shortCircuit: true };
    }
    return next(specifier, context);
  },
});
const { makeStripeMeter } = await import(stub);
const env = JSON.parse(process.env.LOAD_ENV);
const usage = JSON.parse(process.env.LOAD_USAGE);
const meter = makeStripeMeter({ baseUrl: env.PROVIDER_BASE_URL, apiKey: env.PROVIDER_API_KEY, meterId: env.PROVIDER_METER_ID, eventName: env.PROVIDER_METER_EVENT });
const timed = async (run) => {
  const began = Date.now();
  const answer = await run();
  return { kind: answer.kind, took: Date.now() - began };
};
const asked = [];
const realTimeout = AbortSignal.timeout;
AbortSignal.timeout = (delay) => {
  asked.push(delay);
  return realTimeout.call(AbortSignal, delay);
};
const send = await timed(() => meter.sendUsage(usage));
const read = await timed(() => meter.readUsageTotal({ accountId: usage.accountId, customerId: usage.customerId, from: 1794000000, to: 1794000060 }));
console.log(JSON.stringify({ redirected, asked, send, read }));
`;

function runNode(code: string, vars: Record<string, string>, limitMs = 30_000): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--input-type=module", "-e", code], { env: { ...process.env, ...vars }, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    const limit = setTimeout(() => {
      stderr += `ended by the test after ${limitMs} ms`;
      child.kill("SIGKILL");
    }, limitMs);
    child.stdout.on("data", (chunk: unknown) => {
      stdout += String(chunk);
    });
    child.stderr.on("data", (chunk: unknown) => {
      stderr += String(chunk);
    });
    child.on("close", (exit: number | null) => {
      clearTimeout(limit);
      resolve({ code: exit, stdout, stderr });
    });
  });
}

test("the mapping reads PROVIDER_CALL_TIMEOUT_MS of the port module when it is called, also when that binding is initialized after the mapping's module ran", async () => {
  const hang = await startRecorder(() => "hang");
  try {
    const run = await runNode(
      LATE_BINDING,
      {
        LOAD_METER: METER,
        LOAD_ENV: JSON.stringify({ ...VARS, PROVIDER_BASE_URL: hang.url }),
        LOAD_USAGE: JSON.stringify({ accountId: ACCOUNT, customerId: "cus_test_1", identifier: `${ACCOUNT}:1794000000`, amountMinor: 1, timestamp: 1_794_003_599 }),
      },
      8_000,
    );
    expect({ code: run.code, stderr: run.stderr }).toEqual({ code: 0, stderr: "" });
    const result = JSON.parse(run.stdout) as { redirected: string[]; asked: number[]; send: { kind: string; took: number }; read: { kind: string; took: number } };
    expect(result.redirected).toEqual(["../billing/index.ts"]);
    expect(result.asked).toEqual([1500, 1500]);
    for (const call of [result.send, result.read]) {
      expect(call.kind).toBe("failed");
      expect(call.took).toBeGreaterThanOrEqual(1_450);
      expect(call.took).toBeLessThan(6_000);
    }
    expect(hang.requests.map((request) => request.method)).toEqual(["POST", "GET"]);
  } finally {
    await hang.close();
  }
}, 15_000);

test("the port module and the meter mapping load in either order in Node, and the mapping answers after each", async () => {
  const server = await startRecorder((request) => ({ status: 200, body: JSON.stringify({ object: "billing.meter_event", identifier: request.values.identifier }) }));
  try {
    for (const order of [
      [INDEX, METER],
      [METER, INDEX],
    ]) {
      const run = await runNode(CHILD, {
        LOAD_ORDER: JSON.stringify(order),
        LOAD_INDEX: INDEX,
        LOAD_METER: METER,
        LOAD_ENV: JSON.stringify({ ...VARS, PROVIDER_BASE_URL: server.url }),
        LOAD_USAGE: JSON.stringify({ accountId: ACCOUNT, customerId: "cus_test_1", identifier: `${ACCOUNT}:1794000000`, amountMinor: 1, timestamp: 1_794_003_599 }),
      });
      const first = order[0] === INDEX ? "index" : "meter";
      expect({ first, code: run.code, stdout: run.stdout.trim(), stderr: run.stderr }).toEqual({ first, code: 0, stdout: '["recorded","recorded"]', stderr: "" });
    }
    expect(server.requests.map((request) => request.path)).toEqual(Array.from({ length: 4 }, () => "/v1/billing/meter_events"));
  } finally {
    await server.close();
  }
});
