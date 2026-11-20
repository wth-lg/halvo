import { test } from "node:test";
import assert from "node:assert/strict";
import { holdsAnswer, waitFor, answerIn } from "./support.mjs";

const said = (pattern) => (error) => {
  assert.match(error.message, pattern);
  return true;
};

test("holdsAnswer holds a whole final answer and no part of one, and no 100 Continue alone", () => {
  const whole = Buffer.from("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok");
  assert.equal(holdsAnswer(whole), true);
  for (let cut = 0; cut < whole.length; cut += 1) assert.equal(holdsAnswer(whole.subarray(0, cut)), false, `${cut} bytes`);
  assert.equal(holdsAnswer(Buffer.from("HTTP/1.1 405 Method Not Allowed\r\n\r\n")), true, "a head with no content-length");
  const interim = Buffer.from("HTTP/1.1 100 Continue\r\n\r\n");
  assert.equal(holdsAnswer(interim), false, "an interim 100 Continue is no final answer");
  assert.equal(holdsAnswer(Buffer.concat([interim, interim, whole.subarray(0, whole.length - 1)])), false);
  assert.equal(holdsAnswer(Buffer.concat([interim, whole])), true);
});

test("waitFor resolves once its fact holds, and at its bound fails with what did not arrive, the ms waited and the detail", async () => {
  let calls = 0;
  assert.ok((await waitFor(() => (calls += 1) === 3, "x")) >= 20);
  assert.equal(calls, 3);
  const started = performance.now();
  await assert.rejects(waitFor(() => false, "the answer", { ms: 200, detail: () => "the client had read 0 bytes" }), said(/^the answer did not arrive within (2\d\d) ms; the client had read 0 bytes$/));
  assert.ok(performance.now() - started < 1000);
  await assert.rejects(waitFor(() => false, "z", { ms: 50 }), said(/^z did not arrive within \d+ ms$/));
});

test("waitFor counts the time of the look that held or failed last", async () => {
  const slow = (result) => () => {
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 300);
    return result;
  };
  assert.ok((await waitFor(slow(true), "x")) >= 300);
  const started = performance.now();
  await assert.rejects(waitFor(slow(false), "y", { ms: 200 }), said(/^y did not arrive within ([3-9]\d\d) ms$/));
  const took = performance.now() - started;
  assert.ok(took >= 300 && took < 700, `${Math.round(took)} ms`);
});

test("answerIn waits 2,000 ms for a whole answer and names the bytes that the client had read and what the caller adds", async () => {
  let bytes = Buffer.from("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\no");
  const client = { got: () => bytes };
  setTimeout(() => {
    bytes = Buffer.concat([bytes, Buffer.from("k")]);
  }, 100);
  assert.ok((await answerIn(client)) >= 90);
  bytes = Buffer.from("HTTP/1.1 2");
  await assert.rejects(answerIn(client, "the 400"), said(/^the 400 did not arrive within (2\d\d\d) ms; the client had read 10 bytes$/));
  await assert.rejects(answerIn(client, "the answer", () => "the record holds []"), said(/^the answer did not arrive within (2\d\d\d) ms; the client had read 10 bytes; the record holds \[\]$/));
});
