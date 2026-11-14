import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Duplex } from "node:stream";
import { join } from "node:path";
import { serve, readVendored } from "../src/stub.mjs";
import { makeApp } from "../src/app.mjs";
import { openRecord } from "../src/record.mjs";
import { fresh, request, appHeaders } from "./support.mjs";

const CAP = '{"cap":{"amount":1,"period":"week"}}';
const json = [["content-type", "application/json"]];
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const kinds = (dir) =>
  readFileSync(join(dir, "requests.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line))
    .map((line) => line.status ?? line.error);

/** A socket for serve() whose writes stay unflushed until the test calls flush(). */
function heldSocket() {
  const held = [];
  const socket = new Duplex({
    allowHalfOpen: true,
    read() {},
    write(chunk, encoding, done) {
      held.push(done);
    },
  });
  return { socket, flush: () => held.splice(0).forEach((done) => done()) };
}

function setUp(idleMs) {
  const dir = fresh();
  const record = openRecord(dir);
  const app = makeApp(readVendored());
  const { socket, flush } = heldSocket();
  const { connection } = record.connection(socket);
  const handle = serve(socket, connection, app, record, idleMs);
  return { dir, record, app, socket, flush, handle };
}

const subscription = (app) => app.handle({ method: "GET", target: "/v1/subscription", headers: appHeaders(), body: Buffer.alloc(0) }).result;

test("L4 after its idle close, with an answer still unflushed, the stand-in reads no request: no line, no state, the bytes in the raw file alone", async () => {
  const { dir, record, app, socket, flush } = setUp(100);
  const first = Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n");
  socket.push(first);
  await pause(400);
  assert.deepEqual(kinds(dir), [200, "idle-close"], "the idle close came");
  assert.equal(socket.destroyed, false, "the connection is still open with its answer unflushed");
  assert.equal(socket.writableFinished, false);
  const checkout = request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP);
  socket.push(checkout);
  await pause(100);
  assert.deepEqual(kinds(dir), [200, "idle-close"], "no line for the request after the end");
  assert.equal(subscription(app), "no-such-account", "the checkout changed no state");
  assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), Buffer.concat([first, checkout]));
  flush();
  for (let waited = 0; waited < 2000 && !socket.destroyed; waited += 20) await pause(20);
  assert.equal(socket.destroyed, true);
  record.close();
});

test("L7 (b) at the stop, a peer that has ended its side with answers unwritten gets forced-close", async () => {
  const { dir, record, socket, handle } = setUp(1000);
  socket.push(Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n"));
  await pause(50);
  socket.push(null);
  await pause(50);
  assert.equal(socket.writableEnded, true, "the stand-in ended the connection at the peer's end");
  assert.equal(socket.writableFinished, false, "its answer is still unwritten");
  handle.cut();
  handle.closeNow();
  assert.deepEqual(kinds(dir), [200, "forced-close"]);
  assert.equal(socket.destroyed, true);
  record.close();
});
