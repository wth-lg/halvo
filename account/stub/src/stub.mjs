import { createServer, STATUS_CODES } from "node:http";
import { createServer as createPlainServer } from "node:net";
import { createServer as createTlsServer } from "node:tls";
import { readFileSync } from "node:fs";
import { loadSchema } from "./schema.mjs";
import { makeApp } from "./app.mjs";
import { makeReader, BODY_CAP } from "./wire.mjs";
import { openRecord, RecordError } from "./record.mjs";
import { makeTestAuthority, writeAuthority, TlsError } from "./tls.mjs";

export { BODY_CAP } from "./wire.mjs";
/**
 * How long, in ms, a connection of the app-facing port may send nothing before the stub ends it: far above any gap
 * between the bytes of one request on loopback, and below the 10 s that a test of the stub waits for an answer.
 */
export const IDLE_MS = 5000;
const HOST = "127.0.0.1";
const CONTINUE = Buffer.from("HTTP/1.1 100 Continue\r\n\r\n", "latin1");
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
  res.writeHead(status, headers);
  res.end(bytes);
}

function answerBytes(method, { status, type, body }, close) {
  const bytes = Buffer.from(body, "utf8");
  const lines = [`HTTP/1.1 ${status} ${STATUS_CODES[status]}`, `content-length: ${bytes.length}`];
  lines.push(`content-type: ${type}`);
  if (close) lines.push("connection: close");
  const head = Buffer.from(`${lines.join("\r\n")}\r\n\r\n`, "latin1");
  return method === "HEAD" ? head : Buffer.concat([head, bytes]);
}

function closes({ version, headers }) {
  const tokens = headers.filter(([name]) => name.toLowerCase() === "connection").flatMap(([, value]) => value.split(","));
  return version === "1.0" || tokens.some((token) => token.trim().toLowerCase() === "close");
}

function serve(socket, connection, app, record, idleMs) {
  let timer = null;
  let held = false;
  let ended = false;
  let stopping = false;
  let after = 0;
  let capped = false;
  let written = null;
  let closer = null;
  const quiet = () => clearTimeout(timer);
  const wrote = () => {
    written = Date.now();
  };
  const due = () => (written === null ? 0 : written + idleMs);
  const closeNow = () => {
    if (socket.destroyed) return;
    clearTimeout(closer);
    if (capped || !socket.writableFinished) record.refused({ connection, error: "forced-close" });
    socket.destroy();
  };
  const linger = () => {
    if (socket.destroyed) return;
    const left = due() - Date.now();
    if (left <= 0) closeNow();
    else closer = setTimeout(linger, left);
  };
  const take = (chunk) => {
    after += chunk.length;
    if (after < BODY_CAP) return;
    capped = true;
    socket.pause();
  };
  const settle = () => {
    if (socket.writableEnded) return;
    if (socket.writableNeedDrain) {
      held = true;
      quiet();
      socket.pause();
      return;
    }
    if (ended) {
      reader.end();
      stop();
      return;
    }
    socket.resume();
    arm();
  };
  const onData = (chunk) => {
    if (socket.writableEnded) {
      take(chunk);
      return;
    }
    reader.feed(chunk);
    settle();
  };
  const stop = (last) => {
    if (socket.writableEnded) return;
    quiet();
    if (last !== undefined) socket.write(last, wrote);
    socket.end(linger);
    if (!capped) socket.resume();
  };
  const reader = makeReader({
    request(message) {
      if (stopping) {
        record.refused({ connection, error: "incomplete" });
        return true;
      }
      const { method, target, headers, trailers, body } = message;
      const answer = record.request({ connection, method, target, headers, trailers, body, judge: () => app.handle({ method, target, headers, body }) });
      if (answer === null) {
        stop();
        return false;
      }
      const close = closes(message);
      if (close) stop(answerBytes(method, answer, close));
      else socket.write(answerBytes(method, answer, close), wrote);
      return !close;
    },
    malformed(cause) {
      record.refused({ connection, error: cause });
      stop(stopping ? undefined : answerBytes("", app.malformed(), true));
    },
    full: () => !stopping && socket.writableNeedDrain,
    interim: () => socket.write(CONTINUE),
  });
  function arm() {
    quiet();
    timer = setTimeout(() => {
      if (reader.idle()) return;
      record.refused({ connection, error: "idle-close" });
      stop();
    }, idleMs);
  }
  arm();
  socket.on("data", onData);
  socket.on("close", () => {
    quiet();
    clearTimeout(closer);
  });
  socket.on("drain", () => {
    if (!held || socket.writableEnded) return;
    held = false;
    reader.resume();
    settle();
  });
  socket.on("end", () => {
    ended = true;
    if (!held) settle();
  });
  socket.on("error", () => {});
  const cut = () => {
    if (socket.writableEnded) return;
    stopping = true;
    reader.resume();
    reader.end();
    stop();
  };
  return { cut, closeNow };
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
 * through `close()` or the control route `POST /stop`, and `close()`, which ends every served connection, waits for
 * them no longer than `idleMs` and then closes what is left; a connection that holds part of a request, or requests
 * read and not judged, at that moment gets the record line `incomplete` for each, with no answer. A connection
 * that sends nothing for `idleMs` (IDLE_MS unless a test gives another) gets the rejection of a malformed request
 * when a request had begun, and is closed with the record line `idle-close` when none had. Once the stub ends a
 * connection, after the idle limit, a refusal, an answer that closes, the peer's end or the stop, it reads no request
 * of it again: it ends its side after the last answer, takes what still arrives into the raw file alone, at most
 * BODY_CAP bytes and the rest of that read, and closes the socket once every answer is written and the peer has ended
 * its side or `idleMs` has passed since the last answer was written. Where it closes with answers unwritten, or after
 * it stopped reading at BODY_CAP, it first writes the record line `forced-close`.
 * Throws RecordError or TlsError for a bad directory or file.
 */
export async function startStub({ record: dir, tlsCa, now = Date.now, idleMs = IDLE_MS }) {
  const made = tlsCa === undefined ? null : makeTestAuthority(Date.now());
  const record = openRecord(dir, () => {
    if (made !== null) writeAuthority(tlsCa, made.caPem);
  });
  const app = makeApp({ ...readVendored(), now });
  let done;
  const stopped = new Promise((resolve) => {
    done = resolve;
  });
  const served = new Map();
  const onSocket = (socket) => {
    const { connection, opened } = record.connection(socket);
    if (opened) {
      served.set(socket, serve(socket, connection, app, record, idleMs));
      socket.once("close", () => served.delete(socket));
      return;
    }
    socket.destroy();
    record.refused({ connection, error: "raw-open" });
  };
  let appServer;
  if (made === null) {
    appServer = createPlainServer({ allowHalfOpen: true }, onSocket);
  } else {
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
    if (closing === null) {
      const peer = (socket) => `${socket.remoteAddress}:${socket.remotePort}`;
      const kept = new Set([...served.keys()].map(peer));
      for (const socket of sockets) if (!kept.has(peer(socket))) socket.destroy();
      for (const { cut } of served.values()) cut();
      let timer;
      const closed = [...served.keys()].map((socket) => new Promise((resolve) => socket.once("close", resolve)));
      const limit = new Promise((resolve) => {
        timer = setTimeout(resolve, idleMs);
      });
      const waited = Promise.race([Promise.all(closed), limit]).then(() => {
        clearTimeout(timer);
        for (const { closeNow } of served.values()) closeNow();
      });
      closing = Promise.all([waited.then(() => shut(appServer, sockets)), shut(controlServer, controlSockets)]).then(() => {
        record.close();
        done();
      });
    }
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
