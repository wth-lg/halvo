import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";

const ROOT = process.cwd();
const FILE = "contract/names/forbidden-names.json";
const CONTRACT_KEYS = [
  "account",
  "credential",
  "subscription",
  "customer",
  "cap",
  "anchor",
  "time",
  "result",
  "now",
  "test_time",
  "due",
  "alarm",
  "config",
  "tables",
  "tier",
  "reason",
  "holdLifeMs",
  "intervalSeconds",
  "marginSeconds",
  "publicOrigin",
  "testClock",
];
const SOURCE = /^(app@685d846:crates\/halvo-capture\/src\/event\.rs|kickoff:plan\.md):[0-9]+$/;

type Entry = { name: string; sources: string[] };

function propertyNames(node: unknown, out: Set<string>): Set<string> {
  if (Array.isArray(node)) for (const each of node) propertyNames(each, out);
  else if (node !== null && typeof node === "object") {
    for (const [key, value] of Object.entries(node)) {
      if (key === "properties") for (const name of Object.keys(value as object)) out.add(name);
      propertyNames(value, out);
    }
  }
  return out;
}

function names(env: Record<string, string | undefined>, args: string[] = []) {
  const clean = { ...process.env, HALVO_APP_REPO: undefined, HALVO_KICKOFF_PLAN: undefined, ...env };
  return spawnSync("sh", ["scripts/names-v0.sh", ...args], { cwd: ROOT, encoding: "utf8", env: clean });
}

test("the first names file holds 35 names once each, in byte order, state and model_tier among them, with their sources", () => {
  const text = readFileSync(FILE, "utf8");
  const file: { version: number; names: Entry[] } = JSON.parse(text);
  expect(text).toBe(`${JSON.stringify(file, null, 2)}\n`);
  expect(Object.keys(file)).toEqual(["version", "names"]);
  expect(file.version).toBe(0);
  const list = file.names.map((entry) => entry.name);
  expect(list).toHaveLength(35);
  expect(new Set(list).size).toBe(35);
  expect([...list].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))).toEqual(list);
  expect(list).toContain("state");
  expect(list).toContain("model_tier");
  for (const entry of file.names) {
    expect(Object.keys(entry)).toEqual(["name", "sources"]);
    expect(entry.sources.length).toBeGreaterThan(0);
    for (const source of entry.sources) expect(source).toMatch(SOURCE);
  }
});

test("no name of the file is a key of a request or an answer of contracts 1, 5 and 10", () => {
  const schema = JSON.parse(readFileSync("contract/schema/v1.json", "utf8"));
  const keys = propertyNames(schema, new Set(CONTRACT_KEYS));
  const list: Entry[] = JSON.parse(readFileSync(FILE, "utf8")).names;
  expect(list.map((entry) => entry.name).filter((name) => keys.has(name))).toEqual([]);
});

test("a rerun of scripts/names-v0.sh gives the same bytes when both variables are set", () => {
  const repo = process.env.HALVO_APP_REPO;
  const plan = process.env.HALVO_KICKOFF_PLAN;
  if (!repo || !plan) {
    console.log("names test: skipped the rerun of scripts/names-v0.sh, because HALVO_APP_REPO or HALVO_KICKOFF_PLAN is unset");
    return;
  }
  const result = names({ HALVO_APP_REPO: repo, HALVO_KICKOFF_PLAN: plan });
  expect(result.status).toBe(0);
  expect(result.stdout).toBe(readFileSync(FILE, "utf8"));
});

test("the script exits 64 when a variable is unset or names nothing, and 1 for a plan with another sha256", () => {
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const dir = mkdtempSync(join(ROOT, ".tmp", "names-test-"));
  writeFileSync(join(dir, "plan.md"), "another plan\n");
  expect(names({}).status).toBe(64);
  expect(names({ HALVO_APP_REPO: dir }).status).toBe(64);
  expect(names({ HALVO_APP_REPO: join(dir, "none"), HALVO_KICKOFF_PLAN: join(dir, "plan.md") }).status).toBe(64);
  expect(names({ HALVO_APP_REPO: dir, HALVO_KICKOFF_PLAN: join(dir, "none.md") }).status).toBe(64);
  const otherPlan = names({ HALVO_APP_REPO: process.env.HALVO_APP_REPO ?? dir, HALVO_KICKOFF_PLAN: join(dir, "plan.md") });
  expect(otherPlan.status).toBe(1);
  expect(otherPlan.stderr).toContain("names-v0: the kickoff plan has the sha256");
  expect(otherPlan.stdout).toBe("");
  expect(names({ HALVO_APP_REPO: dir, HALVO_KICKOFF_PLAN: join(dir, "plan.md") }, ["--bogus"]).status).toBe(64);
});
