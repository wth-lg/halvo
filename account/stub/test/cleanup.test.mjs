import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { fresh } from "./support.mjs";

const FIXTURE = fileURLToPath(new URL("./fixture/leaves-nothing.mjs", import.meta.url));

test("every record directory that a test file makes lies under TMPDIR and is gone when the file ends", () => {
  const dir = fresh();
  const env = { ...process.env, TMPDIR: dir, TEMP: dir, TMP: dir };
  delete env.NODE_TEST_CONTEXT;
  const run = spawnSync(process.execPath, ["--test-force-exit", FIXTURE], { env, encoding: "utf8", timeout: 30000 });
  assert.equal(run.status, 0, run.stdout + run.stderr);
  const line = run.stdout.split("\n").find((text) => text.startsWith("made "));
  assert.notEqual(line, undefined, run.stdout);
  const made = JSON.parse(line.slice("made ".length));
  assert.equal(made.length, 4);
  for (const path of made) {
    assert.equal(join(path, ".."), dir, path);
    assert.equal(existsSync(path), false, path);
  }
  assert.deepEqual(readdirSync(dir), []);
});
