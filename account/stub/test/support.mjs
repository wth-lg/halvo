import { connect } from "node:net";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after } from "node:test";
import { startStub as start } from "../src/stub.mjs";

const ends = [];
after(async () => {
  for (const end of ends) await end();
});

/** Starts the stub in this process, as startStub does, and closes it after the last test of the file, also after a failure. */
export async function startStub(options) {
  const stub = await start(options);
  ends.push(() => stub.close());
  return stub;
}

export const STUB = fileURLToPath(new URL("../src/stub.mjs", import.meta.url));
export const ID = "0f8fad5b-d9cb-469f-a165-70867728950e";
export const CRED = "0123456789abcdef".repeat(4);

/** Parses the bytes of one HTTP/1.1 response that ends with the connection. */
export function parse(bytes) {
  const text = bytes.toString("latin1");
  const end = text.indexOf("\r\n\r\n");
  const head = text.slice(0, end).split("\r\n");
  const status = Number(head[0].split(" ")[1]);
  const headers = new Map(head.slice(1).map((line) => [line.slice(0, line.indexOf(":")).toLowerCase(), line.slice(line.indexOf(":") + 1).trim()]));
  return { status, headers, body: bytes.subarray(end + 4).toString("utf8") };
}

/** Parses the bytes of HTTP/1.1 responses that each carry content-length, in order. */
export function parseAll(bytes) {
  const out = [];
  let rest = bytes;
  while (rest.length > 0) {
    const end = rest.indexOf("\r\n\r\n");
    const one = parse(rest.subarray(0, end + 4));
    const length = Number(one.headers.get("content-length"));
    one.body = rest.subarray(end + 4, end + 4 + length).toString("utf8");
    out.push(one);
    rest = rest.subarray(end + 4 + length);
  }
  return out;
}

/**
 * Sends raw bytes to a port of 127.0.0.1 through `open` (net.connect by default) and resolves to every byte read until
 * the peer closes. `bytes` may be a list of parts, written 50 ms apart; with `half`, the client ends its side after the last part.
 */
export function exchange(port, bytes, open = (options) => connect(options), { half = false } = {}) {
  return new Promise((resolve, reject) => {
    const socket = open({ host: "127.0.0.1", port });
    const chunks = [];
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("error", reject);
    socket.on("close", () => resolve(Buffer.concat(chunks)));
    const parts = Array.isArray(bytes) ? bytes : [bytes];
    const next = (index) => {
      if (index === parts.length) {
        if (half) socket.end();
        return;
      }
      socket.write(parts[index]);
      setTimeout(() => next(index + 1), 50);
    };
    next(0);
  });
}

/** Builds the bytes of one request that asks the server to close the connection after its answer. */
export function request(method, target, headers = [], body = "") {
  const bytes = Buffer.isBuffer(body) ? body : Buffer.from(body, "utf8");
  const lines = [`${method} ${target} HTTP/1.1`, "host: 127.0.0.1", "connection: close", ...headers.map(([name, value]) => `${name}: ${value}`)];
  if (bytes.length > 0) lines.push(`content-length: ${bytes.length}`);
  return Buffer.concat([Buffer.from(`${lines.join("\r\n")}\r\n\r\n`, "latin1"), bytes]);
}

export const appHeaders = (credential = CRED) => [
  ["halvo-account", ID],
  ["authorization", `Bearer ${credential}`],
];

export const portOf = (url) => Number(new URL(url).port);

/** Makes a new empty directory under the system's temp directory for one record. */
export const fresh = () => mkdtempSync(join(tmpdir(), "halvo-stub-record-"));

/** Starts the stub's command line with `args`; resolves to the child, its first line of standard output parsed as JSON, and its standard error. */
export function spawnStub(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [STUB, ...args], { stdio: ["ignore", "pipe", "pipe"] });
    ends.push(() => {
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    });
    let out = "";
    let err = "";
    child.stderr.on("data", (chunk) => {
      err += chunk;
    });
    child.stdout.on("data", (chunk) => {
      out += chunk;
      const end = out.indexOf("\n");
      if (end >= 0) resolve({ child, ready: JSON.parse(out.slice(0, end)), stderr: () => err });
    });
    child.on("error", reject);
    child.on("exit", (code) => resolve({ child, ready: null, code, stderr: () => err }));
  });
}

/** Resolves to the exit code of a child once it has exited. */
export const exited = (child) => new Promise((resolve) => (child.exitCode !== null ? resolve(child.exitCode) : child.on("exit", (code) => resolve(code))));
