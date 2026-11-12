import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { BODY_CAP } from "../src/stub.mjs";
import { openRecord, RecordError } from "../src/record.mjs";
import { exchange, request, parse, appHeaders, portOf, spawnStub, exited, fresh, startStub, ID, CRED } from "./support.mjs";

const lines = (dir) =>
  readFileSync(join(dir, "requests.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
const CAP = '{"cap":{"amount":1,"period":"week"}}';

test("the recorder refuses a directory that is missing, not a directory, or not empty", () => {
  const base = fresh();
  assert.throws(() => openRecord(join(base, "missing")), RecordError);
  writeFileSync(join(base, "file"), "");
  assert.throws(() => openRecord(join(base, "file")), RecordError);
  mkdirSync(join(base, "full"));
  writeFileSync(join(base, "full", "x"), "");
  assert.throws(() => openRecord(join(base, "full")), RecordError);
  assert.doesNotThrow(() => openRecord(fresh()));
});

test("every request, accepted or rejected, is recorded with its method, target, headers in order, raw body and answer", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  try {
    const port = portOf(stub.app);
    const odd = Buffer.concat([Buffer.from('{"cap":{"amount":1,"period":"'), Buffer.from([0xff, 0x00, 0x0a]), Buffer.from('"}}')]);
    const sent = [
      request("POST", "/v1/checkout", [...appHeaders(), ["Content-Type", "application/json"], ["X-Canary", "c1"], ["x-canary", "c2"]], CAP),
      request("GET", "/v1/x/../subscription?q=canary", appHeaders()),
      request("PUT", "/v1/cap", [...appHeaders(), ["content-type", "application/json"]], odd),
      request("GET", "/nowhere", [["authorization", `Bearer ${CRED}`]]),
    ];
    for (const bytes of sent) await exchange(port, bytes);
    const got = lines(dir);
    assert.equal(got.length, 4);
    assert.deepEqual(got.map((line) => line.seq), [1, 2, 3, 4]);
    assert.deepEqual(got.map((line) => line.method), ["POST", "GET", "PUT", "GET"]);
    assert.deepEqual(got.map((line) => line.target), ["/v1/checkout", "/v1/x/../subscription?q=canary", "/v1/cap", "/nowhere"]);
    assert.deepEqual(got.map((line) => line.status), [200, 400, 400, 404]);
    assert.deepEqual(got.map((line) => line.result), ["session", "rejected", "rejected", "rejected"]);
    assert.deepEqual(got[0].headers, [
      ["host", "127.0.0.1"],
      ["connection", "close"],
      ["halvo-account", ID],
      ["authorization", `Bearer ${CRED}`],
      ["Content-Type", "application/json"],
      ["X-Canary", "c1"],
      ["x-canary", "c2"],
      ["content-length", "36"],
    ]);
    assert.deepEqual(readFileSync(join(dir, got[0].body)), Buffer.from(CAP));
    assert.equal(got[0].bytes, 36);
    assert.deepEqual(readFileSync(join(dir, got[2].body)), odd);
    assert.equal(got[2].bytes, odd.length);
    assert.equal(readFileSync(join(dir, got[1].body)).length, 0);
    assert.equal(new Set(got.map((line) => line.connection)).size, 4);
    for (const [index, line] of got.entries()) {
      assert.deepEqual(readFileSync(join(dir, `connection-${String(line.connection).padStart(6, "0")}.raw`)), sent[index]);
    }
  } finally {
    await stub.close();
  }
});

test("two requests on one connection are two records of that connection, and its raw file holds both byte for byte", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  try {
    const first = Buffer.from(`GET /v1/subscription HTTP/1.1\r\nhost: 127.0.0.1\r\nhalvo-account: ${ID}\r\nauthorization: Bearer ${CRED}\r\n\r\n`);
    const second = request("GET", "/v1/cap", appHeaders());
    const answer = (await exchange(portOf(stub.app), Buffer.concat([first, second]))).toString("latin1");
    assert.equal(answer.split("HTTP/1.1 200").length, 3);
    const got = lines(dir);
    assert.deepEqual(got.map((line) => [line.connection, line.target]), [[1, "/v1/subscription"], [1, "/v1/cap"]]);
    assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), Buffer.concat([first, second]));
  } finally {
    await stub.close();
  }
});

test("a request that the HTTP parser refuses is recorded with its connection and the error's code", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  try {
    const bad = Buffer.from("GET /v1/subscription HTTP/1.1\r\nhost: 127.0.0.1\r\nbad header canary\r\n\r\n");
    const answer = parse(await exchange(portOf(stub.app), bad));
    assert.equal(answer.status, 400);
    const got = lines(dir);
    assert.equal(got.length, 1);
    assert.deepEqual(Object.keys(got[0]).sort(), ["connection", "error", "seq"]);
    assert.equal(got[0].seq, 1);
    assert.equal(got[0].connection, 1);
    assert.match(got[0].error, /^HPE_[A-Z_]+$/);
    assert.deepEqual(readFileSync(join(dir, "connection-000001.raw")), bad);
  } finally {
    await stub.close();
  }
});

test("a body above the read cap is recorded up to and with the chunk that passed the cap", async () => {
  const dir = fresh();
  const stub = await startStub({ record: dir });
  try {
    const big = Buffer.alloc(BODY_CAP * 2, 0x61);
    const answer = parse(await exchange(portOf(stub.app), request("POST", "/v1/webhook", [], big)));
    assert.equal(answer.status, 413);
    const [line] = lines(dir);
    assert.equal(line.status, 413);
    const kept = readFileSync(join(dir, line.body));
    assert.equal(kept.length, line.bytes);
    assert.ok(kept.length > BODY_CAP && kept.length < big.length, String(kept.length));
    assert.deepEqual(kept, big.subarray(0, kept.length));
  } finally {
    await stub.close();
  }
});

test("the command line writes no value of a request to its output", async () => {
  const dir = fresh();
  const { child, ready, stderr } = await spawnStub(["--record", dir]);
  let out = "";
  child.stdout.on("data", (chunk) => {
    out += chunk;
  });
  const port = portOf(ready.app);
  await exchange(port, request("POST", "/v1/checkout", [...appHeaders(), ["content-type", "application/json"], ["x-canary", "canary-header"]], CAP));
  await exchange(port, request("GET", "/canary-path", [["x-canary", "canary-header"]], "canary-body"));
  await exchange(port, Buffer.from("GET /v1/cap HTTP/1.1\r\ncanary bad\r\n\r\n"));
  await exchange(portOf(ready.control), request("POST", "/stop"));
  assert.equal(await exited(child), 0);
  assert.equal(out, "");
  assert.equal(stderr(), "");
  assert.equal(lines(dir).length, 3);
  assert.equal(readdirSync(dir).filter((name) => name.startsWith("connection-")).length, 3);
});

test("the command line refuses a missing --record, a directory that is not empty, and a second --record", async () => {
  const full = fresh();
  writeFileSync(join(full, "x"), "");
  for (const args of [[], ["--record"], ["--record", full], ["--record", fresh(), "--record", fresh()], ["--record", join(fresh(), "missing")]]) {
    const run = await spawnStub(args);
    assert.equal(run.ready, null, args.join(" "));
    assert.equal(await exited(run.child), 64, args.join(" "));
  }
});
