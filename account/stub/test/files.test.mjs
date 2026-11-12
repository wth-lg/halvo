import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { exchange, request, parse, portOf, startStub } from "./support.mjs";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const posix = process.platform !== "win32";
const open = () => (posix ? readdirSync("/dev/fd").length : 0);

test("the raw file of a connection is closed with it, and close() closes every file of the record", async () => {
  const dir = mkdtempSync(join(tmpdir(), "halvo-stub-record-"));
  const before = open();
  const stub = await startStub({ record: dir });
  const port = portOf(stub.app);
  const running = open();
  for (let index = 0; index < 5; index += 1) assert.equal(parse(await exchange(port, request("GET", "/v1/done"))).status, 200);
  await pause(200);
  assert.equal(open(), running, "five closed connections hold no file");
  const socket = connect({ host: "127.0.0.1", port });
  socket.on("error", () => {});
  await new Promise((resolve) => socket.on("connect", resolve));
  socket.write("GET /v1/done HTTP/1.1\r\n");
  await pause(200);
  await stub.close();
  socket.destroy();
  await pause(200);
  assert.equal(open(), before, "the stopped stub holds no file");
  rmSync(dir, { recursive: true });
  assert.equal(existsSync(dir), false, "the record directory of a stopped stub is removed at the first try");
});
