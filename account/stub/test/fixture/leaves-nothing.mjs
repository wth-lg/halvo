import { test } from "node:test";
import { join } from "node:path";
import { appHeaders, exchange, fresh, portOf, request, spawnStub, startStub } from "../support.mjs";

const made = [];
const recorded = (dir) => {
  made.push(dir);
  return dir;
};

test("a plain stub, a TLS stub and a command line that is left running each get a record directory", async () => {
  const plain = await startStub({ record: recorded(fresh()) });
  await exchange(portOf(plain.app), request("GET", "/v1/subscription", appHeaders()));
  await startStub({ record: recorded(fresh()), tlsCa: join(recorded(fresh()), "ca.pem") });
  const { ready } = await spawnStub(["--record", recorded(fresh())]);
  await exchange(portOf(ready.app), request("GET", "/v1/done"));
  process.stdout.write(`made ${JSON.stringify(made)}\n`);
});
