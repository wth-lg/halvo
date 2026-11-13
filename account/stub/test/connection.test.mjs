import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { connect } from "node:net";
import { join } from "node:path";
import { exchange, request, parse, parseAll, appHeaders, portOf, spawnStub, spawnShell, exited, fresh, startStub, answersIn, keepsPromise, openPeer, STUB, ID, CRED } from "./support.mjs";
import { BODY_CAP } from "../src/stub.mjs";

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

test("a request with expect: 100-continue gets 100 Continue before the stub reads its body, then its answer and one line", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const client = await halfOpen(portOf(stub.app));
  const head = request("POST", "/v1/checkout", [...appHeaders(), ...json, ["expect", "100-continue"]]).toString("latin1").replace("\r\n\r\n", `\r\ncontent-length: ${CAP.length}\r\n\r\n`);
  client.socket.write(head);
  const interim = "HTTP/1.1 100 Continue\r\n\r\n";
  for (let waited = 0; waited < 1000 && client.got().length < interim.length; waited += 50) await pause(50);
  assert.equal(client.got().toString("latin1"), interim, "the interim answer came within 1,000 ms and nothing else did");
  assert.deepEqual(lines(dir), []);
  client.socket.write(CAP);
  for (let waited = 0; waited < 2000 && !client.got().toString("latin1").includes('"result"'); waited += 50) await pause(50);
  client.socket.destroy();
  const answer = parse(client.got().subarray(interim.length));
  assert.equal(answer.status, 200);
  assert.equal(JSON.parse(answer.body).result, "session");
  const got = lines(dir);
  assert.deepEqual(got.map((line) => [line.method, line.target, line.status, line.bytes]), [["POST", "/v1/checkout", 200, CAP.length]]);
  assert.deepEqual(got[0].headers.find(([name]) => name === "expect"), ["expect", "100-continue"]);
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

test("a request still open when the stub stops gets one line incomplete, and a connection with no request begun gets none", async () => {
  for (const how of ["close", "stop"]) {
    const dir = fresh();
    const stub = await startStub({ record: dir, idleMs: 1500 });
    const port = portOf(stub.app);
    const clients = [];
    for (const bytes of [`GET /v1/done HTTP/1.1\r\nhost: a\r\n`, `PUT /v1/cap HTTP/1.1\r\nhost: a\r\ncontent-length: 36\r\n\r\n{"cap"`, "GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n", ""]) {
      const client = await halfOpen(port);
      client.socket.write(bytes);
      clients.push(client);
    }
    await pause(200);
    if (how === "close") await stub.close();
    else await exchange(portOf(stub.control), request("POST", "/stop"));
    await stub.stopped;
    for (const client of clients) client.socket.destroy();
    assert.deepEqual(lines(dir).map((line) => [line.connection, line.status ?? line.error]), [[3, 200], [1, "incomplete"], [2, "incomplete"]], how);
  }
});

test("at the stop, each request that the stub read and held unjudged at the bound gets its own line incomplete", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 500 });
  const one = `GET /v1/done HTTP/1.1\r\nhost: a\r\nx: ${"p".repeat(25)}\r\n\r\n`;
  assert.equal(one.length, 64);
  const socket = connect({ host: "127.0.0.1", port: portOf(stub.app) });
  socket.on("error", () => {});
  await new Promise((resolve) => socket.on("connect", resolve));
  socket.pause();
  socket.write(one.repeat(12000));
  let held = -1;
  for (let waited = 0; waited < 20000; waited += 500) {
    await pause(500);
    const now = lines(dir).length;
    if (now === held) break;
    held = now;
  }
  const begun = Math.ceil(readFileSync(join(dir, "connection-000001.raw")).length / one.length);
  await stub.close();
  socket.destroy();
  const got = lines(dir);
  assert.ok(begun < 12000, "the stub read only part of the pipeline");
  assert.equal(got.length, begun + 1, "one line for each request that the stub read before the stop, and forced-close");
  assert.equal(got.at(-1).error, "forced-close", "the client read nothing, so the stop closed with answers unwritten");
  const judged = got.filter((line) => line.status === 200).length;
  assert.ok(judged >= held && judged < begun, `${judged} judged of ${begun} read`);
  assert.ok(got.slice(0, judged).every((line) => line.status === 200));
  assert.ok(got.slice(judged, -1).every((line) => line.error === "incomplete"));
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
    const stub = await startStub({ record: fresh(), idleMs: 500, ...(tls ? { tlsCa: join(fresh(), "ca.pem") } : {}) });
    const socket = connect({ host: "127.0.0.1", port: portOf(stub.app) });
    await new Promise((resolve) => socket.on("connect", resolve));
    socket.on("error", () => {});
    if (!tls) socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n");
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.equal(await within(stub.close(), 2000), true, tls ? "tls" : "plain");
    socket.destroy();
  }
});

if (process.platform !== "win32") {
  test("the line of a request is written before its answer: with a record that cannot be written, no answer goes out", async () => {
    const dir = fresh();
    const { child, ready } = await spawnShell(`ulimit -f 1 && exec "$0" "$1" --record "$2"`, [process.execPath, STUB, dir]);
    const sent = request("GET", "/v1/done", [["x-quote", '"'.repeat(480)]]);
    assert.ok(sent.length < 1024, "the raw file stays under the limit of 1,024 bytes");
    const got = await exchange(portOf(ready.app), sent).catch(() => Buffer.alloc(0));
    assert.equal(got.toString("latin1").includes("HTTP/1.1 200"), false);
    assert.notEqual(await exited(child), 0);
    assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), sent);
    assert.equal(readFileSync(join(dir, "requests.jsonl")).length, 1024);
  });
}

test("a connection whose raw file cannot be opened is closed with the line raw-open, and the stub serves the next one", async () => {
  const dir = fresh();
  const { child, ready } = await spawnStub(["--record", dir]);
  const port = portOf(ready.app);
  writeFileSync(join(dir, "connection-000002.raw"), "keep");
  assert.equal(parse(await exchange(port, request("GET", "/v1/done"))).status, 200);
  const started = performance.now();
  assert.equal((await exchange(port, request("GET", "/v1/done")).catch(() => Buffer.alloc(0))).length, 0);
  const waited = performance.now() - started;
  assert.ok(waited < 2000, `the refused connection closed after ${Math.round(waited)} ms`);
  assert.equal(readFileSync(join(dir, "connection-000002.raw"), "utf8"), "keep");
  assert.equal(parse(await exchange(port, request("GET", "/v1/done"))).status, 200);
  assert.equal(child.exitCode, null);
  assert.deepEqual(lines(dir).map((line) => [line.seq, line.connection, line.status ?? line.error]), [[1, 1, 200], [2, 2, "raw-open"], [3, 3, 200]]);
  await exchange(portOf(ready.control), request("POST", "/stop"));
  assert.equal(await exited(child), 0);
});

test("a request whose body file cannot be opened gets the line body-open, changes no state, and ends its connection after the answers before it", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const port = portOf(stub.app);
  mkdirSync(join(dir, "request-000002.body"));
  const keep = "GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n";
  const checkout = request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP).toString("latin1").replace("connection: close\r\n", "");
  const answers = parseAll(await exchange(port, Buffer.from(keep + checkout + keep, "latin1")));
  assert.deepEqual(answers.map((a) => a.status), [200]);
  assert.equal(parse(await exchange(port, request("GET", "/v1/subscription", appHeaders()))).body, '{"result":"no-such-account"}');
  assert.deepEqual(lines(dir).map((line) => [line.seq, line.connection, line.status ?? line.error]), [[1, 1, 200], [2, 1, "body-open"], [3, 2, 200]]);
});

if (process.platform !== "win32") {
  test("with the open-file limit reached, each connection that finds no file is closed alone, and the stub serves once files are free", async () => {
    const stubs = await Promise.all(
      [64, 65].map(async (limit) => {
        const dir = fresh();
        const { child, ready } = await spawnShell(`ulimit -n ${limit} && exec "$0" "$1" --record "$2"`, [process.execPath, STUB, dir]);
        assert.notEqual(ready, null, `the stub started under ulimit -n ${limit}`);
        return { limit, dir, child, ready, held: [] };
      }),
    );
    for (const stub of stubs) {
      for (let index = 0; index < 60; index += 1) {
        const socket = connect({ host: "127.0.0.1", port: portOf(stub.ready.app) });
        socket.on("error", () => {});
        await new Promise((resolve) => {
          socket.on("connect", resolve);
          socket.on("close", resolve);
        });
        socket.write("GET /v1/done HTTP/1.1\r\n");
        stub.held.push(socket);
      }
    }
    await pause(5600);
    let refused = 0;
    for (const { limit, dir, child, held } of stubs) {
      assert.equal(child.exitCode, null, `the stub under ulimit -n ${limit} runs on past the idle limit`);
      const got = lines(dir);
      refused += got.filter((line) => line.error === "raw-open").length;
      assert.ok(got.filter((line) => line.error === "idle").length > 10, `ulimit -n ${limit}: the idle lines were written with the table full`);
      for (const socket of held) socket.destroy();
    }
    assert.ok(refused > 0, "at one of the two limits an accepted connection met EMFILE at its raw file");
    await pause(300);
    for (const { dir, child, ready } of stubs) {
      const answer = parse(await exchange(portOf(ready.app), request("GET", "/v1/subscription", appHeaders())));
      assert.equal(answer.body, '{"result":"no-such-account"}');
      const last = lines(dir).at(-1);
      assert.deepEqual([last.target, last.status], ["/v1/subscription", 200]);
      await exchange(portOf(ready.control), request("POST", "/stop"));
      assert.equal(await exited(child), 0);
    }
  });

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
  const answer = parse(await exchange(portOf(stub.app), Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n")));
  assert.equal(answer.status, 400);
  assert.equal(answer.body, REJECTED);
  assert.equal(answer.headers.get("connection"), "close");
  assert.deepEqual(lines(dir).map((line) => [line.seq, line.connection, line.error]), [[1, 1, "idle"]]);
});

test("requests whose lines end in a bare LF are answered at once and recorded as sent", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 2000 });
  const sent = Buffer.from(`GET /v1/done HTTP/1.1\nhost: a\n\nPUT /v1/cap HTTP/1.1\nhost: a\nhalvo-account: ${ID}\nauthorization: Bearer ${CRED}\ncontent-type: application/json\ncontent-length: 36\nconnection: close\n\n${CAP}`);
  const started = performance.now();
  const answers = parseAll(await exchange(portOf(stub.app), sent));
  const waited = performance.now() - started;
  assert.ok(waited < 1000, `the answers took ${Math.round(waited)} ms, so the idle limit of 2,000 ms did not send them`);
  assert.deepEqual(answers.map((a) => a.status), [200, 200]);
  assert.equal(answers[1].body, '{"result":"no-such-account"}');
  const got = lines(dir);
  assert.deepEqual(got.map((line) => [line.connection, line.method, line.target, line.status]), [[1, "GET", "/v1/done", 200], [1, "PUT", "/v1/cap", 200]]);
  assert.deepEqual(got[1].headers.map(([name]) => name), ["host", "halvo-account", "authorization", "content-type", "content-length", "connection"]);
  assert.equal(got[1].bytes, 36);
  assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), sent);
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

test("for a client that pipelines and reads nothing, the stub stops reading at the bound and serves the rest once the client reads", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const count = 10000;
  const one = "GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n";
  const socket = connect({ host: "127.0.0.1", port: portOf(stub.app) });
  const got = [];
  const closed = new Promise((resolve) => socket.on("close", resolve));
  socket.on("data", (chunk) => got.push(chunk));
  socket.on("error", () => {});
  await new Promise((resolve) => socket.on("connect", resolve));
  socket.pause();
  socket.write(one.repeat(count - 1) + request("GET", "/v1/done").toString("latin1"));
  let held = -1;
  for (let waited = 0; waited < 20000; waited += 500) {
    await pause(500);
    const now = lines(dir).length;
    if (now === held) break;
    held = now;
  }
  await pause(1000);
  held = lines(dir).length;
  assert.ok(held < count / 2, `${held} of ${count} requests were answered while the client read nothing`);
  socket.resume();
  assert.equal(await within(closed, 40000), true, "the stub serves the rest and closes after the last request");
  const record = lines(dir);
  assert.equal(record.length, count);
  assert.equal(record.filter((line) => line.status === 200).length, count);
  assert.equal(Buffer.concat(got).toString("latin1").split("HTTP/1.1 200 ").length - 1, count, "every 200 of the record reached the client");
});

test("a client that reads slowly and ends inside a request gets every answer that the record holds and the 400", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const count = 3000;
  const one = "GET /v1/done HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n";
  const client = await halfOpen(portOf(stub.app));
  client.socket.pause();
  client.socket.end(one.repeat(count) + "GET /v1/done HTTP/1.1\r\n");
  await pause(1500);
  client.socket.resume();
  assert.equal(await within(client.closed, 8000), true, "the stub closes the connection");
  const text = client.got().toString("latin1");
  const got = lines(dir);
  assert.equal(got.filter((line) => line.status === 200).length, count);
  assert.deepEqual(got.filter((line) => line.error !== undefined).map((line) => line.error), ["incomplete"]);
  assert.equal(text.split("HTTP/1.1 200 ").length - 1, count, "every 200 of the record reached the client");
  assert.equal(text.split("HTTP/1.1 400 ").length - 1, 1, "the 400 of the cut request reached the client");
});

const settled = (promise, ms) => Promise.race([promise, pause(ms).then(() => "timeout")]);

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

test("D1 (i) a peer that reads late after a closing request gets its answer and the stub's end, and later bytes reach the raw file alone", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  const port = portOf(stub.app);
  const sent = Buffer.concat([request("GET", "/v1/done"), request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP), Buffer.alloc(200000, 0x61)]);
  const peer = await openPeer(port);
  peer.socket.pause();
  peer.socket.write(sent);
  await pause(1000);
  peer.socket.resume();
  assert.equal(await settled(peer.ended, 3000), true, "the stub's end arrived after its answer");
  peer.socket.end();
  assert.equal(await settled(peer.closed, 3000), true, "the stub closed once the peer ended its side");
  assert.equal(answersIn(peer.got()), 1);
  assert.equal(parse(peer.got()).status, 200);
  assert.deepEqual(lines(dir).map((line) => [line.connection, line.status ?? line.error]), [[1, 200]]);
  assert.deepEqual(keepsPromise(dir, { 1: 1 }), []);
  assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), sent);
  assert.equal(parse(await exchange(port, request("GET", "/v1/subscription", appHeaders()))).body, '{"result":"no-such-account"}');
});

test("D1 (ii) past BODY_CAP after a closing request the stub stops reading, waits out the idle limit, and closes with forced-close", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 2000 });
  const port = portOf(stub.app);
  const closing = request("GET", "/v1/done");
  const peer = await openPeer(port);
  peer.socket.pause();
  peer.socket.write(Buffer.concat([closing, Buffer.alloc(16 * 1048576, 0x61)]));
  await pause(500);
  peer.socket.resume();
  await pause(300);
  assert.equal(parse(await exchange(port, request("GET", "/v1/done"))).status, 200, "a second connection is served while the first one waits");
  assert.equal(await settled(peer.closed, 6000), true, "the stub closed the first connection at the idle limit");
  assert.equal(answersIn(peer.got()), 1);
  assert.equal(parse(peer.got()).status, 200);
  assert.deepEqual(lines(dir).filter((line) => line.connection === 1).map((line) => line.status ?? line.error), [200, "forced-close"]);
  assert.deepEqual(keepsPromise(dir, { 1: 1 }), [1]);
  const raw = readFileSync(join(dir, "connection-000001.raw")).length;
  assert.ok(raw <= 65536 + BODY_CAP + 65536, `the raw file holds ${raw} bytes: at most the read that held the closing request, BODY_CAP and one more read`);
  assert.ok(raw >= BODY_CAP, `the raw file holds ${raw} bytes`);
});

test("D1 (iii) a close inside a held pipeline: a slow reader that ends its side gets every answer, the closing one among them", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 3000 });
  const count = 3000;
  const peer = await openPeer(portOf(stub.app));
  peer.socket.pause();
  peer.socket.write(Buffer.concat([Buffer.from("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n".repeat(count)), request("GET", "/v1/done"), Buffer.alloc(100000, 0x61)]));
  await pause(1500);
  peer.socket.resume();
  assert.equal(await settled(peer.ended, 10000), true, "the stub's end arrived after the closing answer");
  peer.socket.end();
  assert.equal(await settled(peer.closed, 3000), true);
  assert.equal(answersIn(peer.got()), count + 1);
  assert.deepEqual(keepsPromise(dir, { 1: count + 1 }), []);
});

test("D2 the close waits the idle limit from the last answer; a byte inside it reaches the raw file, a byte after it does not, and no line follows", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 400 });
  const sent = request("GET", "/v1/done");
  const peer = await openPeer(portOf(stub.app));
  peer.socket.write(sent);
  for (let waited = 0; waited < 2000 && answersIn(peer.got()) === 0; waited += 10) await pause(10);
  assert.equal(answersIn(peer.got()), 1);
  await pause(200);
  peer.socket.write("a");
  await pause(1000);
  peer.socket.write("b");
  await pause(300);
  peer.socket.write("c");
  assert.equal(await settled(peer.closed, 3000), true, "the byte after the bound ends the connection for the peer, which a later write sees");
  assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), Buffer.concat([sent, Buffer.from("a")]));
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), [200]);
});

for (const how of ["close", "stop"]) {
  test(`D3 (i) a client that reads only once the stop has begun gets every answer of the record, through ${how}`, async () => {
    const dir = fresh();
    const stub = await startStub({ record: dir, idleMs: 3000 });
    const peer = await openPeer(portOf(stub.app));
    peer.socket.pause();
    peer.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n".repeat(6000));
    await plateau(dir);
    if (how === "close") stub.close();
    else exchange(portOf(stub.control), request("POST", "/stop"));
    await pause(100);
    peer.socket.resume();
    assert.equal(await settled(peer.ended, 5000), true, "the stub's end arrived after its answers");
    peer.socket.end();
    assert.equal(await settled(stub.stopped.then(() => true), 5000), true, "the stop settled");
    assert.deepEqual(keepsPromise(dir, { 1: answersIn(peer.got()) }), []);
  });

  test(`D3 (ii) a client that never reads: the stop settles inside the idle limit and the record ends with forced-close, through ${how}`, async () => {
    const dir = fresh();
    const stub = await startStub({ record: dir, idleMs: 500 });
    const peer = await openPeer(portOf(stub.app));
    peer.socket.pause();
    peer.socket.write(`${"GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n".repeat(6000)}GET /v1/done HTTP/1.1\r\n`);
    await plateau(dir);
    const started = performance.now();
    if (how === "close") stub.close();
    else exchange(portOf(stub.control), request("POST", "/stop"));
    assert.equal(await settled(stub.stopped.then(() => true), 3000), true, "the stop settled");
    const took = performance.now() - started;
    assert.ok(took < 500 + 1500, `the stop took ${Math.round(took)} ms`);
    peer.socket.destroy();
    const mine = lines(dir).filter((line) => line.connection === 1).map((line) => line.status ?? line.error);
    const order = mine.map((kind) => (kind === 200 ? 0 : kind === "incomplete" ? 1 : 2));
    assert.deepEqual(order, [...order].sort(), "status lines, then incomplete lines, then forced-close");
    assert.ok(mine.includes("incomplete"));
    assert.equal(mine.at(-1), "forced-close");
    assert.deepEqual(keepsPromise(dir, { 1: 0 }), [1]);
  });
}

test("D3 a request cut by the stop gets the line incomplete and no answer, so a reading client gets as many answers as status lines", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: 1000 });
  const peer = await openPeer(portOf(stub.app));
  peer.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\nGET /v1/done HTTP/1.1\r\nhost: a\r\n");
  for (let waited = 0; waited < 2000 && answersIn(peer.got()) === 0; waited += 10) await pause(10);
  stub.close();
  assert.equal(await settled(peer.ended, 3000), true, "the stub's end arrived");
  peer.socket.end();
  assert.equal(await settled(stub.stopped.then(() => true), 3000), true);
  assert.deepEqual(lines(dir).map((line) => line.status ?? line.error), [200, "incomplete"]);
  assert.equal(answersIn(peer.got()), 1, "the cut request got no 400");
  assert.deepEqual(keepsPromise(dir, { 1: 1 }), []);
});

if (process.platform !== "win32") {
  test("D3 (ii) on SIGTERM the command line waits its idle limit of 5,000 ms for a client that never reads, writes forced-close and exits 0", async () => {
    const dir = fresh();
    const { child, ready } = await spawnStub(["--record", dir]);
    const peer = await openPeer(portOf(ready.app));
    peer.socket.pause();
    peer.socket.write("GET /v1/done HTTP/1.1\r\nhost: a\r\n\r\n".repeat(6000));
    await plateau(dir);
    const started = performance.now();
    child.kill("SIGTERM");
    assert.equal(await exited(child), 0);
    const took = performance.now() - started;
    assert.ok(took < 5000 + 2000, `the stop took ${Math.round(took)} ms`);
    peer.socket.destroy();
    assert.equal(lines(dir).filter((line) => line.connection === 1).at(-1).error, "forced-close");
    assert.deepEqual(keepsPromise(dir, { 1: 0 }), [1]);
  });
}
