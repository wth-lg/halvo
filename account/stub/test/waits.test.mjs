import { test } from "node:test";
import assert from "node:assert/strict";
import { holdsAnswer, waitFor, answerIn, stopBegun, startStub, fresh } from "./support.mjs";

const said = (pattern) => (error) => {
  assert.match(error.message, pattern);
  return true;
};

test("holdsAnswer holds a whole answer and no part of one", () => {
  const whole = Buffer.from("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok");
  assert.equal(holdsAnswer(whole), true);
  for (let cut = 0; cut < whole.length; cut += 1) assert.equal(holdsAnswer(whole.subarray(0, cut)), false, `${cut} bytes`);
  assert.equal(holdsAnswer(Buffer.from("HTTP/1.1 405 Method Not Allowed\r\n\r\n")), true, "a head with no content-length");
});

test("waitFor resolves once its fact holds, and at its bound fails with what did not arrive, the ms waited and the detail", async () => {
  let calls = 0;
  assert.ok((await waitFor(() => (calls += 1) === 3, "x")) >= 20);
  assert.equal(calls, 3);
  assert.equal(await waitFor(async () => true, "y"), 0);
  const started = performance.now();
  await assert.rejects(waitFor(() => false, "the answer", { ms: 200, detail: () => "the client had read 0 bytes" }), said(/^the answer did not arrive within (2\d\d) ms; the client had read 0 bytes$/));
  assert.ok(performance.now() - started < 1000);
  await assert.rejects(waitFor(() => false, "z", { ms: 50 }), said(/^z did not arrive within \d+ ms$/));
});

test("answerIn waits 2,000 ms for a whole answer and names the bytes that the client had read", async () => {
  let bytes = Buffer.from("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\no");
  const client = { got: () => bytes };
  setTimeout(() => {
    bytes = Buffer.concat([bytes, Buffer.from("k")]);
  }, 100);
  assert.ok((await answerIn(client)) >= 90);
  bytes = Buffer.from("HTTP/1.1 2");
  await assert.rejects(answerIn(client, "the 400"), said(/^the 400 did not arrive within (2\d\d\d) ms; the client had read 10 bytes$/));
});

test("stopBegun fails while the stand-in serves and holds once its stop has begun", async () => {
  const stub = await startStub({ record: fresh() });
  await assert.rejects(stopBegun(stub), said(/^the begin of the stop did not arrive within (2\d\d\d) ms; the control port's last answer: [1-9]\d*$/));
  stub.close();
  assert.ok((await stopBegun(stub)) < 2000);
});
