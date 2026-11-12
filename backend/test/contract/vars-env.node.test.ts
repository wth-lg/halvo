import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";
import { localValues } from "../../vitest.config.ts";

const ROOT = process.cwd();
const GOOD = "# a comment\n\nPROVIDER_API_KEY=sk_test_halvolocal\nPROVIDER_METER_EVENT=halvo_usage\n";
const BAD: [string, string][] = [
  ["a NUL byte", "PROVIDER_API_KEY=sk_test\u0000_halvo\n"],
  ["a quote", "PROVIDER_API_KEY=\"sk_test_halvolocal\"\n"],
  ["a space", "PROVIDER_API_KEY=sk test\n"],
  ["a carriage return", "PROVIDER_API_KEY=sk_test_halvolocal\r\n"],
  ["no equals sign", "PROVIDER_API_KEY\n"],
  ["a lower-case name", "provider_api_key=x\n"],
  ["a command", "PROVIDER_API_KEY=$(touch x)\n"],
];

function scratch(vars: string | null): string {
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const dir = mkdtempSync(join(ROOT, ".tmp", "vars-env-"));
  mkdirSync(join(dir, "stack.d"));
  mkdirSync(join(dir, "scripts"));
  mkdirSync(join(dir, "bin"));
  for (const file of ["20-worker.sh", "30-worker-test.sh"]) copyFileSync(join(ROOT, "stack.d", file), join(dir, "stack.d", file));
  writeFileSync(join(dir, "scripts", "stack.sh"), "exit 64\n");
  writeFileSync(join(dir, "bin", "wrangler"), "#!/bin/sh\nfor each in \"$@\"; do printf '[%s]\\n' \"$each\"; done\n");
  spawnSync("chmod", ["+x", join(dir, "bin", "wrangler")], { encoding: "utf8" });
  if (vars !== null) writeFileSync(join(dir, "stack.d", "vars.env"), vars);
  return dir;
}

function start(dir: string, file: string) {
  return spawnSync("sh", ["-c", `. "$ROOT/stack.d/${file}"; set -eu; run`], {
    cwd: dir,
    encoding: "utf8",
    env: { ...process.env, ROOT: dir, STACK_PORT_OFFSET: "0", STACK_STATE: join(dir, "state"), HALVO_WRANGLER_CONFIG_HOME: join(dir, "config"), PATH: `${join(dir, "bin")}:${process.env.PATH}` },
  });
}

test("both start files pass each good line of stack.d/vars.env with --var, and skip comments and empty lines", () => {
  const dir = scratch(GOOD);
  for (const file of ["20-worker.sh", "30-worker-test.sh"]) {
    const run = start(dir, file);
    expect(run.status).toBe(0);
    expect(run.stdout).toContain("[--var]\n[PROVIDER_API_KEY:sk_test_halvolocal]\n[--var]\n[PROVIDER_METER_EVENT:halvo_usage]\n");
    expect(run.stdout).not.toContain("comment");
  }
});

test("both start files and vitest.config.ts refuse a line that is not NAME=VALUE and name it, and a missing file", () => {
  for (const [what, line] of BAD) {
    const dir = scratch(`${GOOD}${line}`);
    for (const file of ["20-worker.sh", "30-worker-test.sh"]) {
      const run = start(dir, file);
      expect({ what, file, status: run.status, wrangler: run.stdout.includes("[dev]") }).toEqual({ what, file, status: 1, wrangler: false });
      expect(run.stderr).toContain("stack.d/vars.env line 5 is not NAME=VALUE");
    }
    expect(() => localValues(join(dir, "stack.d", "vars.env")), what).toThrow("line 5 is not NAME=VALUE");
  }
  const missing = scratch(null);
  for (const file of ["20-worker.sh", "30-worker-test.sh"]) {
    const run = start(missing, file);
    expect(run.status).toBe(1);
    expect(run.stderr).toContain("stack.d/vars.env is missing");
  }
});
