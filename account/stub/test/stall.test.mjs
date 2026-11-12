import { test } from "node:test";
import assert from "node:assert/strict";
import { exchange, request, parse, portOf, spawnStub, exited, fresh } from "./support.mjs";

test("a field value with a long run of inner spaces keeps a second client waiting under one second", async () => {
  const { child, ready } = await spawnStub(["--record", fresh()]);
  const port = portOf(ready.app);
  const slow = Buffer.from(`GET /v1/done HTTP/1.1\r\nhost: a\r\nconnection: close\r\nx-pad: a${" ".repeat(65400)}b\r\n\r\n`, "latin1");
  const first = exchange(port, slow);
  await new Promise((resolve) => setTimeout(resolve, 200));
  const start = performance.now();
  const second = parse(await exchange(port, request("GET", "/v1/done")));
  const waited = performance.now() - start;
  assert.equal(second.status, 200);
  assert.ok(waited < 1000, `the second client waited ${Math.round(waited)} ms`);
  assert.equal(parse(await first).status, 200);
  await exchange(portOf(ready.control), request("POST", "/stop"));
  assert.equal(await exited(child), 0);
});
