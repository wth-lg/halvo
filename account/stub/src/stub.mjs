import { createServer } from "node:http";
import { createServer as createSecureServer } from "node:https";
import { readFileSync } from "node:fs";
import { loadSchema } from "./schema.mjs";
import { makeApp } from "./app.mjs";
import { openRecord, RecordError } from "./record.mjs";
import { makeTestAuthority, writeAuthority, TlsError } from "./tls.mjs";

/** The stub keeps the chunks of a body up to and with the one that takes it past this many bytes, and reads the rest without keeping it. */
export const BODY_CAP = 1048576;
const HOST = "127.0.0.1";
const vendored = (name) => JSON.parse(readFileSync(new URL(`../../schema/${name}`, import.meta.url), "utf8"));

/** Reads the three vendored files of account/schema/ that the stub uses: the schema, the route table and the samples. */
export function readVendored() {
  return { schema: loadSchema(vendored("schema.v1.json")), routes: vendored("routes.v1.json"), samples: vendored("samples.v1.json") };
}

const listen = (server, scheme) =>
  new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, HOST, () => {
      const { address, port } = server.address();
      resolve(`${scheme}://${address}:${port}`);
    });
  });

const shut = (server) =>
  new Promise((resolve) => {
    server.close(() => resolve());
    server.closeAllConnections();
  });

function send(res, { status, type, body }) {
  const bytes = Buffer.from(body, "utf8");
  const headers = { "content-length": String(bytes.length) };
  if (bytes.length > 0) headers["content-type"] = type;
  res.sendDate = false;
  res.writeHead(status, headers);
  res.end(bytes);
}

function pairs(raw) {
  const out = [];
  for (let index = 0; index < raw.length; index += 2) out.push([raw[index], raw[index + 1]]);
  return out;
}

function serveApp(app, record, connectionOf) {
  return (req, res) => {
    const chunks = [];
    let size = 0;
    req.on("data", (chunk) => {
      if (size > BODY_CAP) return;
      chunks.push(chunk);
      size += chunk.length;
    });
    req.on("end", () => {
      const body = Buffer.concat(chunks);
      const headers = pairs(req.rawHeaders);
      const answer = app.handle({ method: req.method, target: req.url, headers, body });
      record.request({ connection: connectionOf(req.socket), method: req.method, target: req.url, headers, body, status: answer.status, result: answer.result });
      send(res, answer);
    });
    req.on("error", () => {});
  };
}

function serveControl(app, stop) {
  const actions = {
    "/complete": app.complete,
    "/expire": app.expire,
    "/cancel": app.cancel,
    "/fail-payment": app.failPayment,
    "/arm-not-finished": app.armNotFinished,
  };
  return (req, res) => {
    req.resume();
    req.on("end", () => {
      if (req.method === "POST" && req.url === "/stop") {
        send(res, { status: 200, type: "application/json", body: '{"stopped":true}' });
        res.on("finish", stop);
        return;
      }
      if (req.method !== "POST" || !Object.hasOwn(actions, req.url)) {
        send(res, { status: 404, type: null, body: "" });
        return;
      }
      send(res, { status: 200, type: "application/json", body: JSON.stringify({ changed: actions[req.url]() }) });
    });
  };
}

const PARSE_REFUSAL = "HTTP/1.1 400 Bad Request\r\nconnection: close\r\ncontent-length: 0\r\n\r\n";

/**
 * Starts the stub: the app-facing routes and the control routes, each on a port of 127.0.0.1 that the system picks,
 * with every request to the app-facing port kept in the record directory `record` (see openRecord). With `tlsCa`,
 * the app-facing port serves HTTPS with a server certificate of a test authority made at this start
 * (see makeTestAuthority), whose certificate is written to the file `tlsCa`, which must not exist yet.
 * Resolves to `{app, control, stopped, close}`: the two base URLs, a promise that settles when the stub has stopped,
 * through `close()` or the control route `POST /stop`, and `close()`. Throws RecordError or TlsError for a bad directory or file.
 */
export async function startStub({ record: dir, tlsCa }) {
  const record = openRecord(dir);
  const app = makeApp(readVendored());
  let done;
  const stopped = new Promise((resolve) => {
    done = resolve;
  });
  const numbers = new WeakMap();
  const refusedOn = new WeakSet();
  const refuse = (error, socket) => {
    if (error.code === "ECONNRESET" || refusedOn.has(socket)) return;
    refusedOn.add(socket);
    record.refused({ connection: numbers.get(socket), error: String(error.code) });
  };
  const handler = serveApp(app, record, (socket) => numbers.get(socket));
  let appServer = createServer(handler);
  if (tlsCa !== undefined) {
    const made = makeTestAuthority(Date.now());
    writeAuthority(tlsCa, made.caPem);
    appServer = createSecureServer({ key: made.keyPem, cert: made.certPem }, handler);
    appServer.on("tlsClientError", refuse);
  }
  appServer.on(tlsCa === undefined ? "connection" : "secureConnection", (socket) => numbers.set(socket, record.connection(socket)));
  appServer.on("clientError", (error, socket) => {
    refuse(error, socket);
    if (socket.writable) socket.end(PARSE_REFUSAL);
    else socket.destroy();
  });
  let closing = null;
  const close = () => {
    closing ??= Promise.all([shut(appServer), shut(controlServer)]).then(() => done());
    return closing;
  };
  const controlServer = createServer(serveControl(app, close));
  return { app: await listen(appServer, tlsCa === undefined ? "http" : "https"), control: await listen(controlServer, "http"), stopped, close };
}

const USAGE = "usage: node account/stub/src/stub.mjs --record DIR [--tls-ca FILE]\n";

function options(argv) {
  const known = ["--record", "--tls-ca"];
  const given = {};
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (!known.includes(flag) || value === undefined || Object.hasOwn(given, flag)) return null;
    given[flag] = value;
  }
  return Object.hasOwn(given, "--record") ? { record: given["--record"], tlsCa: given["--tls-ca"] } : null;
}

async function main(argv) {
  const given = options(argv);
  if (given === null) {
    process.stderr.write(USAGE);
    process.exitCode = 64;
    return;
  }
  let stub;
  try {
    stub = await startStub(given);
  } catch (error) {
    if (!(error instanceof RecordError || error instanceof TlsError)) throw error;
    process.stderr.write(`${error.message}\n${USAGE}`);
    process.exitCode = 64;
    return;
  }
  for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, () => stub.close());
  process.stdout.write(`${JSON.stringify({ app: stub.app, control: stub.control })}\n`);
  await stub.stopped;
}

if (import.meta.main) await main(process.argv.slice(2));
