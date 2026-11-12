import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { loadSchema } from "./schema.mjs";
import { makeApp } from "./app.mjs";

/** The most body bytes that the stub reads of one request; past it the stub answers and closes the connection. */
export const BODY_CAP = 1048576;
const HOST = "127.0.0.1";
const vendored = (name) => JSON.parse(readFileSync(new URL(`../../schema/${name}`, import.meta.url), "utf8"));

/** Reads the three vendored files of account/schema/ that the stub uses: the schema, the route table and the samples. */
export function readVendored() {
  return { schema: loadSchema(vendored("schema.v1.json")), routes: vendored("routes.v1.json"), samples: vendored("samples.v1.json") };
}

const listen = (server) =>
  new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, HOST, () => {
      const { address, port } = server.address();
      resolve(`http://${address}:${port}`);
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

function serveApp(app) {
  return (req, res) => {
    const chunks = [];
    let size = 0;
    let over = false;
    const finish = () => {
      const answer = app.handle({ method: req.method, target: req.url, headers: pairs(req.rawHeaders), body: Buffer.concat(chunks) });
      if (over) {
        res.shouldKeepAlive = false;
        res.on("finish", () => req.socket.destroy());
      }
      send(res, answer);
    };
    req.on("data", (chunk) => {
      if (over) return;
      chunks.push(chunk);
      size += chunk.length;
      if (size > BODY_CAP) {
        over = true;
        req.pause();
        finish();
      }
    });
    req.on("end", () => {
      if (!over) finish();
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

/**
 * Starts the stub: the app-facing routes and the control routes, each on a port of 127.0.0.1 that the system picks.
 * Resolves to `{app, control, stopped, close}`: the two base URLs, a promise that settles when the stub has stopped,
 * through `close()` or the control route `POST /stop`, and `close()`.
 */
export async function startStub() {
  const app = makeApp(readVendored());
  let done;
  const stopped = new Promise((resolve) => {
    done = resolve;
  });
  const appServer = createServer(serveApp(app));
  let closing = null;
  const close = () => {
    closing ??= Promise.all([shut(appServer), shut(controlServer)]).then(() => done());
    return closing;
  };
  const controlServer = createServer(serveControl(app, close));
  return { app: await listen(appServer), control: await listen(controlServer), stopped, close };
}

async function main(argv) {
  if (argv.length > 0) {
    process.stderr.write("usage: node account/stub/src/stub.mjs\n");
    process.exitCode = 64;
    return;
  }
  const stub = await startStub();
  for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, () => stub.close());
  process.stdout.write(`${JSON.stringify({ app: stub.app, control: stub.control })}\n`);
  await stub.stopped;
}

if (import.meta.main) await main(process.argv.slice(2));
