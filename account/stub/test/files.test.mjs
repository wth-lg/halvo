import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { connect } from "node:net";
import { exchange, request, parse, portOf, fresh, startStub } from "./support.mjs";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

if (process.platform !== "win32") {
  test("the raw file of a connection is closed with it, and close() closes every file of the record", async () => {
    const open = () => readdirSync("/dev/fd").length;
    const before = open();
    const stub = await startStub({ record: fresh() });
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
  });
}
