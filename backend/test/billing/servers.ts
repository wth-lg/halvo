import { spawn } from "node:child_process";
import { createServer as httpServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createServer } from "node:net";
import { localValues } from "../../test-kit/provider-fake/main.mjs";

export const ROOT = new URL("../../", import.meta.url).pathname;

function localValue(name: string): string {
  const value = localValues(`${ROOT}stack.d/vars.env`)[name];
  if (value === undefined) throw new Error(`stack.d/vars.env has no ${name}`);
  return value;
}

export const PROVIDER_API_KEY = localValue("PROVIDER_API_KEY");
export const WEBHOOK_SIGNING_SECRET = localValue("WEBHOOK_SIGNING_SECRET");

export interface Recorded {
  method: string;
  path: string;
  params: string[];
  values: Record<string, string>;
  headers: Record<string, string | string[] | undefined>;
  body: string;
}

export async function freePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  await new Promise<void>((resolve) => server.close(() => resolve()));
  return port;
}

async function answers(url: string): Promise<boolean> {
  try {
    await fetch(url, { signal: AbortSignal.timeout(500) });
    return true;
  } catch {
    return false;
  }
}

async function until(test: () => Promise<boolean>, what: string): Promise<void> {
  for (let tries = 0; tries < 200; tries += 1) {
    if (await test()) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`${what} did not answer within 10 s`);
}

export async function startStripeMock(): Promise<{ url: string; stop(): Promise<void> }> {
  const port = await freePort();
  const tls = await freePort();
  const child = spawn(`${ROOT}.tools/stripe-mock/stripe-mock`, ["-http-addr", `127.0.0.1:${port}`, "-https-addr", `127.0.0.1:${tls}`], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  let ended = false;
  child.on("exit", () => {
    ended = true;
  });
  const url = `http://127.0.0.1:${port}`;
  await until(async () => ended || (await answers(url)), "stripe-mock");
  if (ended) throw new Error("stripe-mock ended before it answered");
  return {
    url,
    async stop() {
      if (ended) return;
      const gone = new Promise<void>((resolve) => child.on("exit", () => resolve()));
      child.kill("SIGTERM");
      await gone;
    },
  };
}

export function paramsOf(path: string, body: string): { names: string[]; values: Record<string, string> } {
  const names: string[] = [];
  const values: Record<string, string> = {};
  const query = path.includes("?") ? path.slice(path.indexOf("?") + 1) : "";
  for (const text of [query, body]) {
    for (const [name, value] of new URLSearchParams(text)) {
      names.push(name);
      values[name] = value;
    }
  }
  return { names, values };
}

export async function startRecorder(
  answer: (request: Recorded) => { status: number; body: string; afterMs?: number } | "hang" | "drop",
): Promise<{ url: string; requests: Recorded[]; close(): Promise<void> }> {
  const requests: Recorded[] = [];
  const server = httpServer((request: IncomingMessage, response: ServerResponse) => {
    const chunks: Uint8Array[] = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      const raw = new TextDecoder().decode(Uint8Array.from(chunks.flatMap((chunk) => [...chunk])));
      const url = request.url ?? "/";
      const { names, values } = paramsOf(url, raw);
      const recorded = { method: request.method ?? "", path: url.split("?")[0] ?? url, params: names, values, headers: request.headers, body: raw };
      requests.push(recorded);
      const reply = answer(recorded);
      if (reply === "hang") return;
      if (reply === "drop") {
        response.socket?.destroy();
        return;
      }
      setTimeout(() => {
        response.writeHead(reply.status, { "content-type": "application/json" });
        response.end(reply.body);
      }, reply.afterMs ?? 0);
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    requests,
    async close() {
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}
