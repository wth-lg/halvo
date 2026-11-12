import { createServer, STATUS_CODES } from "node:http";
import { createServer as createPlainServer } from "node:net";
import { createServer as createTlsServer } from "node:tls";
import { readFileSync } from "node:fs";
import { loadSchema } from "./schema.mjs";
import { makeApp } from "./app.mjs";
import { makeReader } from "./wire.mjs";
import { openRecord, RecordError } from "./record.mjs";
import { makeTestAuthority, writeAuthority, TlsError } from "./tls.mjs";

export { BODY_CAP } from "./wire.mjs";
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

const shut = (server, sockets) =>
  new Promise((resolve) => {
    server.close(() => resolve());
    for (const socket of sockets) socket.destroy();
  });

function send(res, { status, type, body }) {
  const bytes = Buffer.from(body, "utf8");
  const headers = { "content-length": String(bytes.length) };
  if (bytes.length > 0) headers["content-type"] = type;
  res.sendDate = false;
  res.writeHead(status, headers);
  res.end(bytes);
}

function answerBytes(method, { status, type, body }, close) {
  const bytes = Buffer.from(body, "utf8");
  const lines = [`HTTP/1.1 ${status} ${STATUS_CODES[status]}`, `content-length: ${bytes.length}`];
  if (bytes.length > 0) lines.push(`content-type: ${type}`);
  if (close) lines.push("connection: close");
  const head = Buffer.from(`${lines.join("\r\n")}\r\n\r\n`, "latin1");
  return method === "HEAD" ? head : Buffer.concat([head, bytes]);
}

function closes({ version, headers }) {
  const tokens = headers.filter(([name]) => name.toLowerCase() === "connection").flatMap(([, value]) => value.split(","));
  return version === "1.0" || tokens.some((token) => token.trim().toLowerCase() === "close");
}

function serve(socket, connection, app, record) {
  const reader = makeReader({
    request(message) {
      const { method, target, headers, trailers, body } = message;
      const answer = app.handle({ method, target, headers, body });
      record.request({ connection, method, target, headers, trailers, body, status: answer.status, result: answer.result });
      const close = closes(message);
      socket.write(answerBytes(method, answer, close));
      if (close) socket.end();
      return !close;
    },
    malformed(cause) {
      record.refused({ connection, error: cause });
      socket.end(answerBytes("", app.malformed(), true));
    },
  });
  socket.on("data", (chunk) => reader.feed(chunk));
  socket.on("end", () => {
    reader.end();
    socket.end();
  });
  socket.on("error", () => {});
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

/**
 * Starts the stub: the app-facing routes and the control routes, each on a port of 127.0.0.1 that the system picks,
 * with every request to the app-facing port kept in the record directory `record` (see openRecord). The app-facing
 * port reads each request with its own HTTP/1.1 reader (see makeReader), so that every request reaches the judge of
 * makeApp, and answers bytes that are no request with the rejection of a malformed request. With `tlsCa`, the
 * app-facing port serves TLS with a server certificate of a test authority made at this start (see makeTestAuthority),
 * whose certificate is written to the file `tlsCa`, which must not exist yet.
 * Resolves to `{app, control, stopped, close}`: the two base URLs, a promise that settles when the stub has stopped,
 * through `close()` or the control route `POST /stop`, and `close()`, which ends every open connection.
 * Throws RecordError or TlsError for a bad directory or file.
 */
export async function startStub({ record: dir, tlsCa, now = Date.now }) {
  const record = openRecord(dir);
  const app = makeApp({ ...readVendored(), now });
  let done;
  const stopped = new Promise((resolve) => {
    done = resolve;
  });
  const onSocket = (socket) => serve(socket, record.connection(socket), app, record);
  let appServer;
  if (tlsCa === undefined) {
    appServer = createPlainServer({ allowHalfOpen: true }, onSocket);
  } else {
    const made = makeTestAuthority(Date.now());
    writeAuthority(tlsCa, made.caPem);
    appServer = createTlsServer({ key: made.keyPem, cert: made.certPem, allowHalfOpen: true }, onSocket);
    appServer.on("tlsClientError", (error) => {
      if (error.code !== "ECONNRESET") record.refused({ error: String(error.code) });
    });
  }
  const sockets = new Set();
  appServer.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  const controlSockets = new Set();
  let closing = null;
  const close = () => {
    closing ??= Promise.all([shut(appServer, sockets), shut(controlServer, controlSockets)]).then(() => done());
    return closing;
  };
  const controlServer = createServer(serveControl(app, close));
  controlServer.on("connection", (socket) => {
    controlSockets.add(socket);
    socket.on("close", () => controlSockets.delete(socket));
  });
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
