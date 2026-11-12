import { test } from "node:test";
import assert from "node:assert/strict";
import { chmodSync, readFileSync } from "node:fs";
import { connect } from "node:net";
import { join } from "node:path";
import { exchange, request, parse, parseAll, appHeaders, portOf, spawnStub, exited, fresh, startStub, ID, CRED } from "./support.mjs";

const CAP = '{"cap":{"amount":1,"period":"week"}}';
const json = [["content-type", "application/json"]];
const lines = (dir) =>
  readFileSync(join(dir, "requests.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
const REJECTED = '{"result":"rejected"}';
const within = (promise, ms) => Promise.race([promise.then(() => true), new Promise((resolve) => setTimeout(() => resolve(false), ms))]);

test("requests that Node's HTTP layer would answer itself each get one line and the answer of contract 1.4", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const port = portOf(stub.app);
  const expect = parse(await exchange(port, request("POST", "/v1/checkout", [...appHeaders(), ...json, ["expect", "canary-expect"]], CAP)));
  assert.equal(expect.status, 200);
  assert.equal(JSON.parse(expect.body).result, "session");
  const noHost = `GET /v1/subscription HTTP/1.1\r\nconnection: close\r\nhalvo-account: ${ID}\r\nauthorization: Bearer ${CRED}\r\n\r\n`;
  assert.equal(parse(await exchange(port, Buffer.from(noHost))).body, '{"result":"pending"}');
  const tunnel = parse(await exchange(port, Buffer.from("CONNECT 127.0.0.1:1 HTTP/1.1\r\nhost: 127.0.0.1:1\r\nconnection: close\r\n\r\n")));
  assert.equal(tunnel.status, 404);
  assert.equal(tunnel.body, REJECTED);
  const upgrade = `GET /v1/done HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: upgrade\r\nupgrade: canary-proto\r\n\r\n`;
  const answers = parseAll(await exchange(port, Buffer.concat([Buffer.from(upgrade), request("GET", "/v1/subscription", appHeaders())])));
  assert.deepEqual(answers.map((a) => a.status), [200, 200]);
  assert.equal(answers[1].body, '{"result":"pending"}');
  const lower = parse(await exchange(port, request("get", "/v1/done")));
  assert.equal(lower.status, 405);
  assert.equal(lower.body, REJECTED);
  const got = lines(dir);
  assert.deepEqual(got.map((line) => [line.method, line.target, line.status]), [
    ["POST", "/v1/checkout", 200],
    ["GET", "/v1/subscription", 200],
    ["CONNECT", "127.0.0.1:1", 404],
    ["GET", "/v1/done", 200],
    ["GET", "/v1/subscription", 200],
    ["get", "/v1/done", 405],
  ]);
  assert.deepEqual(got[0].headers.find(([name]) => name === "expect"), ["expect", "canary-expect"]);
  assert.equal(got[3].connection, got[4].connection);
});

test("bytes that are no request get one line with the cause and the contract's 400, and the connection closes", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const port = portOf(stub.app);
  const answers = parseAll(await exchange(port, Buffer.from(`GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\nGET /v1/done HTTP/1.1\r\nbad header\r\n\r\nGET /v1/done HTTP/1.1\r\n\r\n`)));
  assert.deepEqual(answers.map((a) => a.status), [200, 400]);
  assert.equal(answers[1].body, REJECTED);
  assert.equal(answers[1].headers.get("content-type"), "application/json");
  assert.equal(answers[1].headers.get("connection"), "close");
  assert.deepEqual(lines(dir).map((line) => [line.seq, line.connection, line.status ?? line.error]), [[1, 1, 200], [2, 1, "field-line"]]);
});

test("a peer that ends its side inside a request gets the line incomplete and the contract's 400", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const head = `PUT /v1/cap HTTP/1.1\r\nhost: a\r\nhalvo-account: ${ID}\r\nauthorization: Bearer ${CRED}\r\ncontent-type: application/json\r\ncontent-length: 36\r\n\r\n{"cap"`;
  const answer = parse(await exchange(portOf(stub.app), Buffer.from(head), undefined, { half: true }));
  assert.equal(answer.status, 400);
  assert.equal(answer.body, REJECTED);
  const [line] = lines(dir);
  assert.deepEqual([line.seq, line.connection, line.error], [1, 1, "incomplete"]);
});

test("a keep-alive connection serves requests in order until one asks to close, and HTTP/1.0 closes after its answer", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const port = portOf(stub.app);
  const keep = `GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n`;
  const answers = parseAll(await exchange(port, [Buffer.from(keep), Buffer.from(keep), request("GET", "/v1/subscription", appHeaders())]));
  assert.deepEqual(answers.map((a) => [a.status, a.headers.get("connection") ?? "-"]), [[200, "-"], [200, "-"], [200, "close"]]);
  const old = parseAll(await exchange(port, Buffer.from(`GET /v1/done HTTP/1.0\r\n\r\nGET /v1/done HTTP/1.0\r\n\r\n`)));
  assert.deepEqual(old.map((a) => [a.status, a.headers.get("connection")]), [[200, "close"]]);
  assert.equal(lines(dir).length, 4);
});

test("a peer that ends its side after a whole request gets its answer, and the stub closes the connection", async () => {
  const stub = await startStub({ record: fresh() });
  const answer = parse(await exchange(portOf(stub.app), Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n"), undefined, { half: true }));
  assert.equal(answer.status, 200);
});

test("an answer to HEAD carries its head alone", async () => {
  const stub = await startStub({ record: fresh() });
  const bytes = (await exchange(portOf(stub.app), request("HEAD", "/v1/done"))).toString("latin1");
  assert.match(bytes, /^HTTP\/1\.1 405 Method Not Allowed\r\n/);
  assert.match(bytes, /\r\ncontent-length: 21\r\n/);
  assert.ok(bytes.endsWith("\r\n\r\n"), bytes);
});

test("the raw file keeps every part of a request that arrives in several reads", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const bytes = request("PUT", "/v1/cap", [...appHeaders(), ...json], CAP);
  const parts = [bytes.subarray(0, 10), bytes.subarray(10, 100), bytes.subarray(100)];
  assert.equal(parse(await exchange(portOf(stub.app), parts)).status, 200);
  assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), bytes);
  assert.equal(lines(dir)[0].bytes, 36);
});

test("close ends a connection that holds half a request, over HTTP and over TLS before its handshake", async () => {
  for (const tls of [false, true]) {
    const stub = await startStub({ record: fresh(), ...(tls ? { tlsCa: join(fresh(), "ca.pem") } : {}) });
    const socket = connect({ host: "127.0.0.1", port: portOf(stub.app) });
    await new Promise((resolve) => socket.on("connect", resolve));
    socket.on("error", () => {});
    if (!tls) socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n");
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.equal(await within(stub.close(), 2000), true, tls ? "tls" : "plain");
    socket.destroy();
  }
});

test("the line of a request is written before its answer: with a record that cannot be written, no answer goes out", async () => {
  const dir = fresh();
  const { child, ready } = await spawnStub(["--record", dir]);
  chmodSync(join(dir, "requests.jsonl"), 0o444);
  const got = await exchange(portOf(ready.app), request("GET", "/v1/done")).catch(() => Buffer.alloc(0));
  assert.equal(got.toString("latin1").includes("HTTP/1.1 200"), false);
  assert.notEqual(await exited(child), 0);
});

if (process.platform !== "win32") {
  test("the command line stops and exits 0 on SIGINT", async () => {
    const { child, ready } = await spawnStub(["--record", fresh()]);
    assert.notEqual(ready, null);
    child.kill("SIGINT");
    assert.equal(await exited(child), 0);
  });
}

test("the idle limit is 5,000 ms", async () => {
  const { IDLE_MS } = await import("../src/stub.mjs");
  assert.equal(IDLE_MS, 5000);
});

test("a request that sends nothing for the idle limit gets the contract's 400 and the line idle", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const answer = parse(await exchange(portOf(stub.app), Buffer.from("GET /v1/done HTTP/1.1\nhost: a\n\n")));
  assert.equal(answer.status, 400);
  assert.equal(answer.body, REJECTED);
  assert.equal(answer.headers.get("connection"), "close");
  assert.deepEqual(lines(dir).map((line) => [line.seq, line.connection, line.error]), [[1, 1, "idle"]]);
});

test("a connection that sends no byte of a request for the idle limit is closed with the line idle-close", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const port = portOf(stub.app);
  assert.equal((await exchange(port, Buffer.alloc(0))).length, 0);
  assert.equal((await exchange(port, Buffer.from("\r\n"))).length, 0);
  const served = parseAll(await exchange(port, Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n")));
  assert.deepEqual(served.map((a) => a.status), [200]);
  assert.deepEqual(lines(dir).map((line) => [line.connection, line.status ?? line.error]), [[1, "idle-close"], [2, "idle-close"], [3, 200], [3, "idle-close"]]);
});

test("the idle limit counts from the last byte, so a request sent in parts with short gaps is served", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 300 });
  const bytes = request("GET", "/v1/subscription", appHeaders());
  const parts = [];
  for (let at = 0; at < bytes.length; at += 12) parts.push(bytes.subarray(at, at + 12));
  assert.ok(parts.length * 50 > 600, String(parts.length));
  assert.equal(parse(await exchange(portOf(stub.app), parts)).status, 200);
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), [200]);
});

function halfOpen(port) {
  return new Promise((resolve) => {
    const socket = connect({ host: "127.0.0.1", port, allowHalfOpen: true });
    const got = [];
    const closed = new Promise((done) => socket.on("close", done));
    socket.on("data", (chunk) => got.push(chunk));
    socket.on("error", () => {});
    socket.on("connect", () => resolve({ socket, closed, got: () => Buffer.concat(got) }));
  });
}

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("a request sent after the idle close leaves no line and changes no state", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const port = portOf(stub.app);
  const client = await halfOpen(port);
  await pause(500);
  client.socket.write(request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP));
  await pause(200);
  client.socket.write(request("GET", "/v1/done"));
  assert.equal(await within(client.closed, 1000), true, "the stub's socket is gone, so the late writes end the connection");
  await pause(200);
  client.socket.destroy();
  assert.equal(client.got().length, 0);
  assert.deepEqual(lines(dir).map((line) => [line.connection, line.status ?? line.error]), [[1, "idle-close"]]);
  assert.equal(parse(await exchange(port, request("GET", "/v1/subscription", appHeaders()))).body, '{"result":"no-such-account"}');
});

test("after the peer ends its side behind an answer, no idle-close line follows", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const client = await halfOpen(portOf(stub.app));
  client.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n");
  await pause(100);
  client.socket.end();
  await pause(600);
  client.socket.destroy();
  assert.equal(parse(client.got()).status, 200);
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), [200]);
});

test("after a 400, no idle-close line follows and bytes sent after it leave no line", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const client = await halfOpen(portOf(stub.app));
  client.socket.write("GET /v1/done HTTP/1.1\r\nbad header\r\n\r\n");
  await pause(100);
  client.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n");
  await pause(600);
  client.socket.destroy();
  assert.equal(parse(client.got()).status, 400);
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), ["field-line"]);
});

test("after an answer that closes, no idle-close line follows", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const client = await halfOpen(portOf(stub.app));
  client.socket.write(request("GET", "/v1/done"));
  await pause(600);
  client.socket.destroy();
  assert.equal(parse(client.got()).status, 200);
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), [200]);
});

test("after the peer resets the connection behind an answer, no idle-close line follows", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 200 });
  const client = await halfOpen(portOf(stub.app));
  client.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n");
  await pause(100);
  client.socket.resetAndDestroy();
  await pause(600);
  assert.equal(parse(client.got()).status, 200);
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), [200]);
});
