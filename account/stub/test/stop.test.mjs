import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { connect } from "node:net";
import { connect as tlsConnect } from "node:tls";
import { join } from "node:path";
import { exchange, request, portOf, spawnStub, exited, fresh, startStub, openPeer, answersIn, appHeaders } from "./support.mjs";

const CAP = '{"cap":{"amount":1,"period":"week"}}';
const json = [["content-type", "application/json"]];
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const settled = (promise, ms) => Promise.race([promise, pause(ms).then(() => "timeout")]);
const lines = (dir) =>
  readFileSync(join(dir, "requests.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));

async function plateau(dir) {
  let held = -1;
  for (let waited = 0; waited < 20000; waited += 500) {
    await pause(500);
    const now = lines(dir).length;
    if (now === held) return held;
    held = now;
  }
  return held;
}

const keepAlive = "GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n";

async function holder(port, ca) {
  const open = ca === undefined ? undefined : (where) => tlsConnect({ ...where, ca });
  const peer = await openPeer(port, open, ca === undefined ? "connect" : "secureConnect");
  peer.socket.write(keepAlive);
  for (let waited = 0; waited < 3000 && answersIn(peer.got()) === 0; waited += 10) await pause(10);
  assert.equal(answersIn(peer.got()), 1, "the holder got its answer before the stop");
  return peer;
}

function tryNew(port, ca) {
  return new Promise((resolve) => {
    const options = { host: "127.0.0.1", port };
    const socket = ca === undefined ? connect(options) : tlsConnect({ ...options, ca });
    const got = [];
    let outcome = null;
    const finish = (kind) => {
      if (outcome !== null) return;
      outcome = kind;
      socket.destroy();
      resolve({ kind, bytes: Buffer.concat(got) });
    };
    socket.on("data", (chunk) => got.push(chunk));
    socket.on("error", (error) => finish(error.code === "ECONNREFUSED" ? "refused" : `error ${error.code}`));
    socket.on("close", () => finish("ended"));
    socket.once(ca === undefined ? "connect" : "secureConnect", () => {
      socket.write(Buffer.concat([request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP).subarray(0), Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n")]));
    });
    setTimeout(() => finish("open after 1,500 ms"), 1500);
  });
}

async function checkNew(t, dir, port, ca, stillRunning) {
  await pause(200);
  assert.equal(stillRunning(), true, "the stop was not done when the new connection was tried");
  const outcome = await tryNew(port, ca);
  t.diagnostic(`new connection 200 ms into the stop: ${outcome.kind}, ${outcome.bytes.length} bytes read`);
  assert.equal(outcome.bytes.length, 0, "the new connection read no byte of an answer");
  assert.ok(outcome.kind === "refused" || outcome.kind === "ended" || outcome.kind.startsWith("error "), outcome.kind);
  return outcome;
}

function checkRecord(dir) {
  const got = lines(dir);
  assert.deepEqual([...new Set(got.map((line) => line.connection))], [1], "only the holder's connection has lines, and no line without a connection");
  assert.equal(got.filter((line) => line.connection === 1 && line.status !== undefined).length, 1, "no new status line");
  assert.equal(got.filter((line) => line.error === "incomplete").length, 0, "no line incomplete for the new connection's half request");
  assert.equal(existsSync(join(dir, "connection-000002.raw")), false, "the new connection was never served");
}

for (const tls of [false, true]) {
  const kind = tls ? "over TLS" : "plain";

  test(`L1 after close() the stand-in accepts no connection, ${kind}`, async (t) => {
    const dir = fresh();
    const keys = fresh();
    const stub = await startStub({ record: dir, idleMs: 1500, ...(tls ? { tlsCa: join(keys, "ca.pem") } : {}) });
    const ca = tls ? readFileSync(join(keys, "ca.pem")) : undefined;
    const port = portOf(stub.app);
    const held = await holder(port, ca);
    if (tls) {
      const handshaking = connect({ host: "127.0.0.1", port });
      handshaking.on("error", () => {});
      await new Promise((resolve) => handshaking.on("connect", resolve));
    }
    let done = false;
    stub.close().then(() => {
      done = true;
    });
    await checkNew(t, dir, port, ca, () => !done);
    held.socket.end();
    assert.equal(await settled(stub.stopped.then(() => true), 3000), true);
    checkRecord(dir);
  });

  test(`L1 after POST /stop the command line accepts no connection, ${kind}`, async (t) => {
    const dir = fresh();
    const keys = fresh();
    const { child, ready } = await spawnStub(["--record", dir, ...(tls ? ["--tls-ca", join(keys, "ca.pem")] : [])]);
    const ca = tls ? readFileSync(join(keys, "ca.pem")) : undefined;
    const port = portOf(ready.app);
    const held = await holder(port, ca);
    await exchange(portOf(ready.control), request("POST", "/stop"));
    await checkNew(t, dir, port, ca, () => child.exitCode === null);
    held.socket.end();
    assert.equal(await exited(child), 0);
    checkRecord(dir);
  });
}

if (process.platform !== "win32") {
  for (const tls of [false, true]) {
    const kind = tls ? "over TLS" : "plain";
    test(`L1 after SIGTERM the command line accepts no connection, ${kind}`, async (t) => {
      const dir = fresh();
      const keys = fresh();
      const { child, ready } = await spawnStub(["--record", dir, ...(tls ? ["--tls-ca", join(keys, "ca.pem")] : [])]);
      const ca = tls ? readFileSync(join(keys, "ca.pem")) : undefined;
      const port = portOf(ready.app);
      const held = await holder(port, ca);
      child.kill("SIGTERM");
      await checkNew(t, dir, port, ca, () => child.exitCode === null);
      held.socket.end();
      assert.equal(await exited(child), 0);
      checkRecord(dir);
    });
  }

  for (const tls of [false, true]) {
    test(`L2 a second SIGTERM and a SIGINT during the stop do not end the command line before its stop is done${tls ? ", over TLS" : ""}`, async (t) => {
      const dir = fresh();
      const keys = fresh();
      const { child, ready } = await spawnStub(["--record", dir, ...(tls ? ["--tls-ca", join(keys, "ca.pem")] : [])]);
      const ca = tls ? readFileSync(join(keys, "ca.pem")) : undefined;
      const open = ca === undefined ? undefined : (where) => tlsConnect({ ...where, ca });
      const peer = await openPeer(portOf(ready.app), open, ca === undefined ? "connect" : "secureConnect");
      peer.socket.pause();
      peer.socket.write(keepAlive.repeat(6000));
      await plateau(dir);
      const gone = exited(child);
      const started = performance.now();
      child.kill("SIGTERM");
      await pause(1000);
      if (child.exitCode === null) child.kill("SIGTERM");
      await pause(1000);
      if (child.exitCode === null) child.kill("SIGINT");
      const code = await gone;
      const took = performance.now() - started;
      t.diagnostic(`exit ${code} ${Math.round(took)} ms after the first signal`);
      peer.socket.destroy();
      assert.equal(code, 0);
      assert.ok(took >= 4000, `the command line exited ${Math.round(took)} ms after the first signal`);
      assert.equal(lines(dir).filter((line) => line.connection === 1).at(-1).error, "forced-close");
    });
  }
}

test("when the stop settles, no connection of the stand-in is open any more", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 300 });
  const peer = await openPeer(portOf(stub.app));
  peer.socket.pause();
  peer.socket.write(keepAlive.repeat(6000));
  await plateau(dir);
  stub.close();
  await stub.stopped;
  assert.equal(stub.connections(), 0, "the stop settled after the last served socket had closed");
  assert.equal(lines(dir).at(-1).error, "forced-close", "the stop's limit closed the connection");
  peer.socket.destroy();
});

test("L3 the control port is closed at the begin of the stop: a test call 200 ms into the wait gets no answer", async () => {
  const stub = await startStub({ record: fresh(), idleMs: 3000 });
  const held = await holder(portOf(stub.app));
  held.socket.pause();
  let done = false;
  stub.close().then(() => {
    done = true;
  });
  await pause(200);
  const answer = await exchange(portOf(stub.control), request("POST", "/complete")).then(
    (bytes) => `${bytes.length} bytes`,
    (error) => error.code,
  );
  assert.equal(done, false, "the stop was not done when the test call was answered");
  assert.ok(answer === "ECONNREFUSED" || answer === "ECONNRESET" || answer === "0 bytes", answer);
  held.socket.destroy();
  assert.equal(await settled(stub.stopped.then(() => true), 5000), true);
});

test("L5 from the begin of the stop the stand-in writes no 100 Continue: held heads that expect it get incomplete and nothing else", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 3000 });
  const peer = await openPeer(portOf(stub.app));
  peer.socket.pause();
  peer.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\nexpect: 100-continue\r\ncontent-length: 1\r\n\r\nx".repeat(12000));
  await plateau(dir);
  stub.close();
  await pause(100);
  peer.socket.resume();
  assert.equal(await settled(peer.ended, 8000), true, "the stand-in's end arrived");
  peer.socket.end();
  assert.equal(await settled(stub.stopped.then(() => true), 5000), true);
  const text = peer.got().toString("latin1");
  const continues = text.split("HTTP/1.1 100 Continue").length - 1;
  const mine = lines(dir).filter((line) => line.connection === 1);
  const statuses = mine.filter((line) => line.status !== undefined).length;
  const incomplete = mine.filter((line) => line.error === "incomplete").length;
  assert.ok(incomplete >= 3, `${incomplete} lines incomplete at the stop`);
  assert.ok(continues <= statuses + 1, `${continues} 100 Continue read against ${statuses} status lines`);
});

test("L6 the command line exits soon after its stop is done, with no connection and with one closed before the stop", async (t) => {
  for (const before of [false, true]) {
    const { child, ready } = await spawnStub(["--record", fresh()]);
    if (before) assert.equal((await exchange(portOf(ready.app), request("GET", "/v1/done"))).length > 0, true);
    const gone = exited(child);
    await exchange(portOf(ready.control), request("POST", "/stop"));
    const read = performance.now();
    assert.equal(await gone, 0);
    const took = performance.now() - read;
    t.diagnostic(`${before ? "one connection closed before the stop" : "no connection"}: exit ${Math.round(took)} ms after the answer to POST /stop was read`);
    assert.ok(took < 4000, `the command line exited ${Math.round(took)} ms after the answer to POST /stop`);
  }
});

test("L7 (a) the close's bound counts from the last answer written, not from the first", async (t) => {
  const stub = await startStub({ record: fresh(), idleMs: 3000 });
  const peer = await openPeer(portOf(stub.app));
  peer.socket.write(keepAlive);
  for (let waited = 0; waited < 3000 && answersIn(peer.got()) === 0; waited += 10) await pause(10);
  await pause(1800);
  peer.socket.write(request("GET", "/v1/done"));
  for (let waited = 0; waited < 3000 && answersIn(peer.got()) < 2; waited += 10) await pause(10);
  const second = performance.now();
  for (let waited = 0; waited < 8000 && stub.connections() > 0; waited += 20) await pause(20);
  const took = performance.now() - second;
  t.diagnostic(`the stand-in closed ${Math.round(took)} ms after the client saw the second answer`);
  assert.equal(stub.connections(), 0);
  assert.ok(took > 2100, `the stand-in closed ${Math.round(took)} ms after the second answer`);
  peer.socket.destroy();
});
