import { connect } from "node:net";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after } from "node:test";
import { startStub as start } from "../src/stub.mjs";

const ends = [];
const made = [];
after(async () => {
  for (const end of ends) await Promise.race([end(), new Promise((resolve) => setTimeout(resolve, 5000))]);
  for (const dir of made) rmSync(dir, { recursive: true, maxRetries: 5 });
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
 * the peer closes, or rejects when the peer has not closed after 10 s. `bytes` may be a list of parts, written 50 ms
 * apart; with `half`, the client ends its side after the last part.
 */
export function exchange(port, bytes, open = (options) => connect(options), { half = false } = {}) {
  return new Promise((resolve, reject) => {
    const socket = open({ host: "127.0.0.1", port });
    const chunks = [];
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error("the peer did not close within 10 s"));
    }, 10000);
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    socket.on("close", () => {
      clearTimeout(timer);
      resolve(Buffer.concat(chunks));
    });
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

/**
 * Makes a new empty directory under the system's temp directory for one record, and removes it with all it holds after
 * the last test of the file, once every stub and command line that the file started has stopped.
 */
export function fresh() {
  const dir = mkdtempSync(join(tmpdir(), "halvo-stub-record-"));
  made.push(dir);
  return dir;
}

/** Starts the stub's command line with `args`; resolves to the child, its first line of standard output parsed as JSON, and its standard error. */
export const spawnStub = (args) => spawnReady(process.execPath, [STUB, ...args]);

/** As spawnStub, for a command line that `sh -c script` starts with `args` as $0, $1 and so on. */
export const spawnShell = (script, args) => spawnReady("/bin/sh", ["-c", script, ...args]);

function spawnReady(file, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(file, args, { stdio: ["ignore", "pipe", "pipe"] });
    const gone = new Promise((done) => child.on("exit", done));
    ends.push(() => {
      if (child.exitCode !== null || child.signalCode !== null) return undefined;
      child.kill("SIGKILL");
      return gone;
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

/**
 * Opens a client through `open` (net.connect by default; resolves at `event`) that keeps its side open when the stub
 * ends its own. `ended` resolves true when the stub's end arrives and false when the socket closes first.
 */
export function openPeer(port, open = (options) => connect(options), event = "connect") {
  return new Promise((resolve) => {
    const socket = open({ host: "127.0.0.1", port, allowHalfOpen: true });
    const chunks = [];
    let sawEnd;
    let sawClose;
    const ended = new Promise((done) => {
      sawEnd = done;
    });
    const closed = new Promise((done) => {
      sawClose = done;
    });
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("end", () => sawEnd(true));
    socket.on("close", () => {
      sawEnd(false);
      sawClose(true);
    });
    socket.on("error", () => {});
    socket.once(event, () => resolve({ socket, ended, closed, got: () => Buffer.concat(chunks) }));
  });
}

/** The number of final answers in the bytes that a client read: each status line but those of 100 Continue. */
export const answersIn = (bytes) => {
  const text = Buffer.isBuffer(bytes) ? bytes.toString("latin1") : bytes;
  return text.split("HTTP/1.1 ").length - 1 - (text.split("HTTP/1.1 100 ").length - 1);
};

/**
 * Checks the record's promise for each connection of `read`, a map from connection number to the number of answers
 * its client read: either the client read as many answers as the record holds status lines for the connection, or the
 * record holds `forced-close` for it, with its three keys alone, once and as its last line, and the client read no more
 * answers than it has status lines. Returns the connections that hold `forced-close`.
 */
export function keepsPromise(dir, read) {
  const record = readFileSync(join(dir, "requests.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
  const forced = [];
  for (const [key, got] of Object.entries(read)) {
    const connection = Number(key);
    const mine = record.filter((line) => line.connection === connection);
    const statuses = mine.filter((line) => line.status !== undefined).length;
    const lines = mine.filter((line) => line.error === "forced-close");
    assert.ok(lines.length <= 1, `connection ${connection} holds forced-close ${lines.length} times`);
    if (lines.length === 0) {
      assert.equal(got, statuses, `connection ${connection}: answers read against status lines, with no forced-close`);
      continue;
    }
    assert.deepEqual(Object.keys(lines[0]).sort(), ["connection", "error", "seq"]);
    assert.equal(mine.at(-1), lines[0], `forced-close is the last line of connection ${connection}`);
    assert.ok(got <= statuses, `connection ${connection}: ${got} answers read, ${statuses} status lines`);
    forced.push(connection);
  }
  return forced;
}

/** Resolves to the exit code of a child once it has exited. */
export const exited = (child) => new Promise((resolve) => (child.exitCode !== null ? resolve(child.exitCode) : child.on("exit", (code) => resolve(code))));
