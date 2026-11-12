import { test } from "node:test";
import assert from "node:assert/strict";
import { makeReader, BODY_CAP, HEAD_MAX } from "../src/wire.mjs";

function read(chunks, { stopAfter = Infinity, end = true } = {}) {
  const messages = [];
  const causes = [];
  const reader = makeReader({
    request: (message) => (messages.push(message), messages.length < stopAfter),
    malformed: (cause) => causes.push(cause),
  });
  for (const chunk of chunks) reader.feed(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk, "latin1"));
  if (end) reader.end();
  return { messages, causes };
}

const bytewise = (text) => [...Buffer.from(text, "latin1")].map((byte) => Buffer.from([byte]));
const GET = "GET /v1/subscription HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n";

test("a request gives its method, target, version and fields in the order received, each byte as one character", () => {
  const { messages, causes } = read(["GET /v1/x?q=1 HTTP/1.1\r\nHost: a\r\nX-A:  one two \t\r\nx-a:\r\nx-b: caf\xc3\xa9\r\n\r\n"]);
  assert.deepEqual(causes, []);
  assert.equal(messages.length, 1);
  const [message] = messages;
  assert.equal(message.method, "GET");
  assert.equal(message.target, "/v1/x?q=1");
  assert.equal(message.version, "1.1");
  assert.deepEqual(message.headers, [["Host", "a"], ["X-A", "one two"], ["x-a", ""], ["x-b", "caf\xc3\xa9"]]);
  assert.deepEqual(message.trailers, []);
  assert.equal(message.body.length, 0);
});

test("requests that Node's HTTP layer would answer itself are read as any other request", () => {
  const sent = [
    "POST /v1/checkout HTTP/1.1\r\nexpect: canary\r\ncontent-length: 2\r\n\r\n{}",
    "GET /v1/subscription HTTP/1.1\r\n\r\n",
    "CONNECT 127.0.0.1:1 HTTP/1.1\r\nhost: 127.0.0.1:1\r\n\r\n",
    "GET /v1/done HTTP/1.1\r\nconnection: upgrade\r\nupgrade: canary\r\n\r\n",
    "get /v1/done HTTP/1.1\r\n\r\n",
    "OPTIONS * HTTP/1.0\r\n\r\n",
  ];
  const { messages, causes } = read([sent.join("")]);
  assert.deepEqual(causes, []);
  assert.deepEqual(messages.map((m) => `${m.method} ${m.target} ${m.version}`), [
    "POST /v1/checkout 1.1",
    "GET /v1/subscription 1.1",
    "CONNECT 127.0.0.1:1 1.1",
    "GET /v1/done 1.1",
    "get /v1/done 1.1",
    "OPTIONS * 1.0",
  ]);
  assert.equal(messages[0].body.toString(), "{}");
});

test("bytes fed one at a time give the same requests as one chunk", () => {
  const text = `\r\n${GET}POST /a HTTP/1.1\r\ncontent-length: 3\r\n\r\nabcPUT /b HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n2;x=y\r\nhi\r\n0\r\nt: v\r\n\r\n`;
  const whole = read([text]);
  const split = read(bytewise(text));
  assert.deepEqual(split, whole);
  assert.deepEqual(whole.causes, []);
  assert.deepEqual(whole.messages.map((m) => [m.method, m.body.toString(), m.trailers]), [["GET", "", []], ["POST", "abc", []], ["PUT", "hi", [["t", "v"]]]]);
});

test("a chunked body is joined, its extensions skipped, and its trailer fields kept in order", () => {
  const { messages, causes } = read([
    "POST /v1/cap HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n3\r\nabc\r\nA ; e=1\r\n0123456789\r\n0\r\nX-T: canary\r\nauthorization: Bearer x\r\nX-T: two\r\n\r\n",
  ]);
  assert.deepEqual(causes, []);
  assert.equal(messages[0].body.toString(), "abc0123456789");
  assert.deepEqual(messages[0].trailers, [["X-T", "canary"], ["authorization", "Bearer x"], ["X-T", "two"]]);
});

test("a body keeps its first BODY_CAP bytes and the reader goes on after it", () => {
  const big = Buffer.alloc(BODY_CAP + 70000, 0x61);
  big[BODY_CAP - 1] = 0x62;
  const head = Buffer.from(`POST /v1/webhook HTTP/1.1\r\ncontent-length: ${big.length}\r\n\r\n`);
  const chunks = [head];
  for (let at = 0; at < big.length; at += 65536) chunks.push(big.subarray(at, at + 65536));
  chunks.push(Buffer.from(GET));
  const { messages, causes } = read(chunks);
  assert.deepEqual(causes, []);
  assert.equal(messages.length, 2);
  assert.equal(messages[0].body.length, BODY_CAP);
  assert.deepEqual(messages[0].body, big.subarray(0, BODY_CAP));
  const chunked = read([`PUT /c HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n${(BODY_CAP + 5).toString(16)}\r\n`, big.subarray(0, BODY_CAP + 5), "\r\n0\r\n\r\n"]);
  assert.deepEqual(chunked.causes, []);
  assert.equal(chunked.messages[0].body.length, BODY_CAP);
});

test("bytes that are no request get one cause, and nothing after them is read", () => {
  const cases = {
    "request-line": [
      "GET /v1/x HTTP/2.0\r\n\r\n",
      "GET  /v1/x HTTP/1.1\r\n\r\n",
      "GET /v1/x\r\n\r\n",
      "G(T /v1/x HTTP/1.1\r\n\r\n",
      "GET /v1/ x HTTP/1.1\r\n\r\n",
      "GET /v1/x HTTP/1.1 \r\n\r\n",
    ],
    "field-line": [
      "GET / HTTP/1.1\r\nhost : a\r\n\r\n",
      "GET / HTTP/1.1\r\nhost: a\r\n folded\r\n\r\n",
      "GET / HTTP/1.1\r\nno colon\r\n\r\n",
      "GET / HTTP/1.1\r\nx: a\x00b\r\n\r\n",
      "GET / HTTP/1.1\r\nx: a\rb\r\n\r\n",
      "GET / HTTP/1.1\r\nx: a\nb: c\r\n\r\n",
      "GET / HTTP/1.1\r\n: a\r\n\r\n",
      "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n0\r\nbad trailer\r\n\r\n",
    ],
    framing: ["POST / HTTP/1.1\r\ncontent-length: 1\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n"],
    "transfer-encoding": [
      "POST / HTTP/1.1\r\ntransfer-encoding: gzip, chunked\r\n\r\n",
      "POST / HTTP/1.1\r\ntransfer-encoding: identity\r\n\r\n",
      "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\ntransfer-encoding: chunked\r\n\r\n",
    ],
    "content-length": [
      "POST / HTTP/1.1\r\ncontent-length: 1\r\ncontent-length: 2\r\n\r\nab",
      "POST / HTTP/1.1\r\ncontent-length: -1\r\n\r\n",
      "POST / HTTP/1.1\r\ncontent-length: 1, 1\r\n\r\na",
      "POST / HTTP/1.1\r\ncontent-length: 1234567890123456\r\n\r\n",
      "POST / HTTP/1.1\r\ncontent-length: \r\n\r\n",
    ],
    chunk: [
      "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\nz\r\n",
      "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n2\r\nabc\r\n",
      "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n12345678901234\r\n",
      `POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n1;${"e".repeat(5000)}`,
      "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n-1\r\n",
    ],
    "head-too-large": [
      `GET / HTTP/1.1\r\nx: ${"a".repeat(HEAD_MAX)}`,
      `GET / HTTP/1.1\r\nx: ${"a".repeat(HEAD_MAX)}\r\n\r\n`,
      `POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n0\r\n${"t: v\r\n".repeat(HEAD_MAX / 6 + 1)}\r\n`,
    ],
  };
  for (const [cause, texts] of Object.entries(cases)) {
    for (const text of texts) {
      const result = read([text, GET], { end: false });
      assert.deepEqual(result.causes, [cause], JSON.stringify(text.slice(0, 80)));
      assert.equal(result.messages.length, 0, JSON.stringify(text.slice(0, 80)));
    }
  }
});

test("a head of exactly HEAD_MAX bytes is read", () => {
  const start = "GET / HTTP/1.1\r\nx: ";
  const head = start + "a".repeat(HEAD_MAX - start.length);
  assert.equal(head.length, HEAD_MAX);
  const { messages, causes } = read([`${head}\r\n\r\n`]);
  assert.deepEqual(causes, []);
  assert.equal(messages.length, 1);
  const split = read([`${head}\r\n\r`, "\n"]);
  assert.deepEqual(split.causes, []);
  assert.equal(split.messages.length, 1);
  const trailer = `t: ${"v".repeat(HEAD_MAX - 7)}`;
  const long = read([`POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n0\r\n${trailer}\r\n\r\n`]);
  assert.deepEqual(long.causes, []);
  assert.deepEqual(long.messages[0].trailers, [["t", "v".repeat(HEAD_MAX - 7)]]);
});

test("a peer that stops inside a request gives the cause incomplete, and one that stops between requests gives none", () => {
  for (const text of ["GET / HTTP/1.1\r\nhost: a\r\n", "POST / HTTP/1.1\r\ncontent-length: 5\r\n\r\nab", "POST / HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n5\r\nab", "G"]) {
    assert.deepEqual(read([text]).causes, ["incomplete"], JSON.stringify(text));
  }
  assert.deepEqual(read([GET]).causes, []);
  assert.deepEqual(read([]).causes, []);
  assert.deepEqual(read(["\r\n"]).causes, []);
});

test("the reader reads no request after one that its caller stops at", () => {
  const { messages, causes } = read([GET + GET + GET], { stopAfter: 1 });
  assert.equal(messages.length, 1);
  assert.deepEqual(causes, []);
});
