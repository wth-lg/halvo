import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { request, portOf, fresh, startStub, openPeer, answerIn, waitFor } from "./support.mjs";
import { BODY_CAP } from "../src/stub.mjs";

const IDLE = 1000;
const closing = request("GET", "/v1/done");
const kinds = (dir) =>
  readFileSync(join(dir, "requests.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line))
    .filter((line) => line.connection === 1)
    .map((line) => line.status ?? line.error);
const rawFile = (dir) => {
  const file = join(dir, "connection-000001.raw");
  return existsSync(file) ? statSync(file).size : 0;
};

async function afterClosing() {
  const dir = fresh();
  const stub = await startStub({ record: dir, idleMs: IDLE });
  const peer = await openPeer(portOf(stub.app));
  const sent = performance.now();
  peer.socket.write(closing);
  await answerIn(peer, "the answer to the closing request", () => `the record holds ${JSON.stringify(kinds(dir))}`);
  const raw = () => rawFile(dir) - closing.length;
  const seen = () => `record ${JSON.stringify(kinds(dir))}; ${raw()} bytes after the closing request in the raw file; open connections ${stub.connections()}`;
  let forcedAt = null;
  const look = () => {
    if (forcedAt === null && kinds(dir).includes("forced-close")) forcedAt = performance.now() - sent;
    return forcedAt;
  };
  return {
    dir,
    raw,
    seen,
    send: (count) => peer.socket.write(Buffer.alloc(count, 0x61)),
    read: (count) => waitFor(() => raw() === count, `the stand-in's read of ${count} bytes after the closing request`, { detail: seen }),
    forced: () => waitFor(() => look() !== null, "the line forced-close", { ms: IDLE + 2000, detail: seen }),
    closed: (ms) =>
      waitFor(
        () => {
          look();
          return stub.connections() === 0;
        },
        "the close of the connection",
        { ms, detail: seen },
      ),
    notBeforeIdle: () => {
      look();
      assert.ok(
        forcedAt !== null && forcedAt >= IDLE - 5,
        `forced-close stood in the record ${forcedAt === null ? "never" : `${Math.round(forcedAt)} ms`} after the closing request was sent, which the idle limit of ${IDLE} ms counts from its answer; ${seen()}`,
      );
    },
    end: () => peer.socket.end(),
    reset: () => peer.socket.resetAndDestroy(),
  };
}

test("cap: BODY_CAP - 1 bytes after a closing request, then the peer's end: the close leaves no line", async () => {
  const c = await afterClosing();
  c.send(BODY_CAP - 1);
  await c.read(BODY_CAP - 1);
  c.end();
  await c.closed(2000);
  assert.deepEqual([kinds(c.dir), c.raw()], [[200], BODY_CAP - 1], c.seen());
});

test("cap: BODY_CAP bytes that the stand-in reads whole, then the peer's end: the close leaves no line", async () => {
  const c = await afterClosing();
  c.send(BODY_CAP);
  await c.read(BODY_CAP);
  c.end();
  await c.closed(2000);
  assert.deepEqual([kinds(c.dir), c.raw()], [[200], BODY_CAP], c.seen());
});

test("cap: BODY_CAP bytes read whole, then one byte more in a later write, then the peer's end: forced-close at the idle limit, the raw file stops at the cap", async () => {
  const c = await afterClosing();
  c.send(BODY_CAP);
  await c.read(BODY_CAP);
  c.send(1);
  c.end();
  await c.forced();
  await c.closed(2000);
  assert.deepEqual([kinds(c.dir), c.raw()], [[200, "forced-close"], BODY_CAP], c.seen());
  c.notBeforeIdle();
});

test("cap: BODY_CAP + 1 bytes in one write, then the peer's end: no line when the last byte falls in the read that reaches the cap, forced-close when it falls in a later read", async (t) => {
  const c = await afterClosing();
  c.send(BODY_CAP + 1);
  c.end();
  await c.closed(IDLE + 2000);
  const outcome = [kinds(c.dir), c.raw()];
  t.diagnostic(`cap+1 outcome: ${outcome[0].includes("forced-close") ? "forced-close" : "no-line"}`);
  assert.ok(
    [JSON.stringify([[200], BODY_CAP + 1]), JSON.stringify([[200, "forced-close"], BODY_CAP])].includes(JSON.stringify(outcome)),
    `the code allows no line with the last byte read, or forced-close with the raw file at the cap; ${c.seen()}`,
  );
});

test("cap: BODY_CAP bytes and a silent peer: forced-close at the idle limit", async () => {
  const c = await afterClosing();
  c.send(BODY_CAP);
  await c.read(BODY_CAP);
  await c.forced();
  await c.closed(2000);
  assert.deepEqual([kinds(c.dir), c.raw()], [[200, "forced-close"], BODY_CAP], c.seen());
  c.notBeforeIdle();
});

test("cap: BODY_CAP bytes read whole, then the peer's reset: the close leaves no line", async () => {
  const c = await afterClosing();
  c.send(BODY_CAP);
  await c.read(BODY_CAP);
  c.reset();
  await c.closed(2000);
  assert.deepEqual([kinds(c.dir), c.raw()], [[200], BODY_CAP], c.seen());
});

test("cap: BODY_CAP + 200,000 bytes, then the peer's reset: forced-close at the idle limit when the reset waits behind a full read buffer, no line when the stand-in sees it, and the raw file holds at most one read past the cap", async (t) => {
  const c = await afterClosing();
  c.send(BODY_CAP + 200000);
  await waitFor(() => c.raw() >= BODY_CAP, "the stand-in's read that reaches the cap", { detail: c.seen });
  c.reset();
  await c.closed(IDLE + 2000);
  const lines = kinds(c.dir);
  t.diagnostic(`cap+200000 reset outcome: ${lines.includes("forced-close") ? "forced-close" : "no-line"}`);
  assert.ok([JSON.stringify([200]), JSON.stringify([200, "forced-close"])].includes(JSON.stringify(lines)), `the code allows no line or forced-close; ${c.seen()}`);
  assert.ok(c.raw() >= BODY_CAP && c.raw() < BODY_CAP + 65536, c.seen());
  if (lines.includes("forced-close")) c.notBeforeIdle();
});

test("cap: BODY_CAP - 50 bytes read whole, then 100 bytes in one write that cross the cap, then the peer's reset: the close leaves no line, whether those bytes come in one read or more", async () => {
  const c = await afterClosing();
  c.send(BODY_CAP - 50);
  await c.read(BODY_CAP - 50);
  c.send(100);
  await waitFor(() => c.raw() >= BODY_CAP, "the stand-in's read that crosses the cap", { detail: c.seen });
  c.reset();
  await c.closed(2000);
  assert.deepEqual(kinds(c.dir), [200], c.seen());
  assert.ok(c.raw() >= BODY_CAP && c.raw() <= BODY_CAP + 50, c.seen());
});
