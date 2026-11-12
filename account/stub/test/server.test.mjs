import { test } from "node:test";
import assert from "node:assert/strict";
import { connect } from "node:net";
import { startStub, BODY_CAP } from "../src/stub.mjs";
import { exchange, request, parse, appHeaders, portOf, spawnStub, exited, fresh } from "./support.mjs";

const CAP = '{"cap":{"amount":1,"period":"week"}}';
const json = [["content-type", "application/json"]];
const refused = (port) =>
  new Promise((resolve) => {
    const socket = connect({ host: "127.0.0.1", port });
    socket.on("connect", () => {
      socket.destroy();
      resolve(false);
    });
    socket.on("error", (error) => resolve(error.code === "ECONNREFUSED"));
  });

test("the stub listens on 127.0.0.1 at ports that the system picks", async () => {
  const one = await startStub({ record: fresh() });
  const two = await startStub({ record: fresh() });
  try {
    for (const url of [one.app, one.control, two.app, two.control]) {
      assert.equal(new URL(url).hostname, "127.0.0.1", url);
      assert.ok(portOf(url) > 0, url);
    }
    assert.equal(new Set([one.app, one.control, two.app, two.control]).size, 4);
  } finally {
    await one.close();
    await two.close();
  }
});

test("over the network an accepted request gets its JSON answer and a rejection gets the rejected body", async () => {
  const stub = await startStub({ record: fresh() });
  try {
    const port = portOf(stub.app);
    const start = parse(await exchange(port, request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP)));
    assert.equal(start.status, 200);
    assert.equal(start.headers.get("content-type"), "application/json");
    assert.equal(start.headers.get("content-length"), String(Buffer.byteLength(start.body)));
    assert.equal(start.headers.has("date"), false);
    assert.equal(JSON.parse(start.body).result, "session");
    const status = parse(await exchange(port, request("GET", "/v1/subscription", appHeaders())));
    assert.equal(status.body, '{"result":"pending"}');
    const missing = parse(await exchange(port, request("GET", "/v1/nope", appHeaders())));
    assert.equal(missing.status, 404);
    assert.equal(missing.headers.get("content-type"), "application/json");
    assert.equal(missing.body, '{"result":"rejected"}');
    const twice = parse(await exchange(port, request("GET", "/v1/subscription", [...appHeaders(), ["authorization", `Bearer ${"0".repeat(64)}`]])));
    assert.equal(twice.status, 400);
  } finally {
    await stub.close();
  }
});

test("a body above the read cap is read to its end and answered, and the stub serves on", async () => {
  const stub = await startStub({ record: fresh() });
  try {
    const port = portOf(stub.app);
    const big = Buffer.alloc(BODY_CAP + 4096, 0x20);
    const answer = parse(await exchange(port, request("PUT", "/v1/cap", [...appHeaders(), ...json], big)));
    assert.equal(answer.status, 413);
    assert.equal(answer.body, '{"result":"rejected"}');
    const hook = parse(await exchange(port, request("POST", "/v1/webhook", [], big)));
    assert.equal(hook.status, 413);
    const after = parse(await exchange(port, request("GET", "/v1/subscription", appHeaders())));
    assert.equal(after.body, '{"result":"no-such-account"}');
  } finally {
    await stub.close();
  }
});

test("the control routes move the states, refuse anything else, and stop the stub", async () => {
  const stub = await startStub({ record: fresh() });
  const port = portOf(stub.app);
  const control = portOf(stub.control);
  const call = async (method, path) => parse(await exchange(control, request(method, path)));
  await exchange(port, request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP));
  assert.equal((await call("POST", "/fail-payment")).body, '{"changed":0}');
  const complete = await call("POST", "/complete");
  assert.equal(complete.status, 200);
  assert.equal(complete.headers.get("content-type"), "application/json");
  assert.equal(complete.body, '{"changed":1}');
  assert.equal(parse(await exchange(port, request("GET", "/v1/subscription", appHeaders()))).body, '{"result":"active"}');
  assert.equal((await call("POST", "/fail-payment")).body, '{"changed":1}');
  assert.equal((await call("POST", "/cancel")).body, '{"changed":1}');
  assert.equal((await call("POST", "/arm-not-finished")).body, '{"changed":1}');
  assert.equal(parse(await exchange(port, request("DELETE", "/v1/account", appHeaders()))).body, '{"result":"not-finished"}');
  await exchange(port, request("DELETE", "/v1/account", appHeaders()));
  await exchange(port, request("POST", "/v1/checkout", [...appHeaders(), ...json], CAP));
  assert.equal((await call("POST", "/expire")).body, '{"changed":1}');
  assert.equal(parse(await exchange(port, request("GET", "/v1/subscription", appHeaders()))).body, '{"result":"no-such-account"}');
  for (const [method, path] of [["GET", "/complete"], ["POST", "/nope"], ["POST", "/complete?"], ["GET", "/stop"], ["POST", "/hasOwnProperty"]]) {
    const answer = await call(method, path);
    assert.equal(answer.status, 404, `${method} ${path}`);
    assert.equal(answer.body, "", `${method} ${path}`);
  }
  const stop = await call("POST", "/stop");
  assert.equal(stop.body, '{"stopped":true}');
  await stub.stopped;
  assert.equal(await refused(port), true);
  assert.equal(await refused(control), true);
});

test("the command line prints the two addresses, serves, and exits 0 on the control route stop", async () => {
  const { child, ready } = await spawnStub(["--record", fresh()]);
  assert.notEqual(ready, null);
  assert.deepEqual(Object.keys(ready).sort(), ["app", "control"]);
  const status = parse(await exchange(portOf(ready.app), request("GET", "/v1/subscription", appHeaders())));
  assert.equal(status.body, '{"result":"no-such-account"}');
  await exchange(portOf(ready.control), request("POST", "/stop"));
  assert.equal(await exited(child), 0);
});

test("the command line refuses an unknown argument with exit 64 and serves nothing", async () => {
  const run = await spawnStub(["--port", "8080"]);
  assert.equal(run.ready, null);
  assert.equal(await exited(run.child), 64);
  assert.match(run.stderr(), /usage/);
});
