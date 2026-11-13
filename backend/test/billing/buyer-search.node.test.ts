import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import { searchBuyerValues } from "./buyer-search.mjs";
import { ROOT } from "./servers.ts";

interface Database {
  exec(query: string): void;
  prepare(query: string): { run(...bindings: (string | Uint8Array)[]): unknown };
  close(): void;
}
interface Builtins {
  "node:sqlite": { DatabaseSync: new (path: string) => Database };
  "node:zlib": { gzipSync(data: Uint8Array): Uint8Array };
}

function builtin<K extends keyof Builtins>(id: K): Builtins[K] {
  const node = globalThis as unknown as { process: { getBuiltinModule(id: string): unknown } };
  return node.process.getBuiltinModule(id) as Builtins[K];
}

const BUYER = JSON.parse(readFileSync(`${ROOT}test-kit/provider-fake/buyer.json`, "utf8")) as Record<string, string>;
const VALUES = ["name", "email", "phone", "line1", "line2", "city", "postal_code"].map((key) => BUYER[key] as string);
const EMAIL = BUYER.email as string;

function scratch(): string {
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  return mkdtempSync(join(ROOT, ".tmp", "buyer-search-"));
}

function store(cell: string | Uint8Array): { dir: string; scratch: string } {
  const dir = mkdtempSync(join(scratch(), "worker-"));
  const db = new (builtin("node:sqlite").DatabaseSync)(join(dir, "object.sqlite"));
  db.exec("CREATE TABLE skeleton (copy)");
  db.prepare("INSERT INTO skeleton (copy) VALUES (?)").run(cell);
  db.close();
  return { dir, scratch: scratch() };
}

describe("the search of check 10 for the buyer's values", () => {
  test("finds a value stored as text, in a BLOB as gzip and in a BLOB as base64, and nothing in a file without one", () => {
    const encoder = new TextEncoder();
    const cases: [string, string | Uint8Array, number][] = [
      ["text", EMAIL, 1],
      ["gzip", builtin("node:zlib").gzipSync(encoder.encode(EMAIL)), 1],
      ["base64", encoder.encode(btoa(EMAIL)), 1],
      ["nothing", "cus_test_1", 0],
    ];
    for (const [name, cell, want] of cases) {
      const at = store(cell);
      expect({ name, count: searchBuyerValues([at.dir], VALUES, at.scratch).count }).toEqual({ name, count: want });
    }
  });

  test("a planted value makes the check fail, and what the search prints names the file, the table and a count and holds no byte of the value", () => {
    const encoder = new TextEncoder();
    for (const cell of [EMAIL, builtin("node:zlib").gzipSync(encoder.encode(EMAIL)), encoder.encode(btoa(EMAIL))]) {
      const at = store(cell);
      const result = searchBuyerValues([at.dir], VALUES, at.scratch);
      expect(result.count).not.toBe(0);
      expect(result.lines).toEqual(expect.arrayContaining([expect.stringMatching(/^buyer values in worker-[^/]+\/object\.sqlite, table skeleton: 1$/)]));
      expect(result.lines.at(-1)).toBe("buyer values found in the state directories: 1");
      const printed = result.lines.join("\n");
      for (const form of [...VALUES, btoa(EMAIL), EMAIL.split("@")[0] as string]) expect(printed).not.toContain(form);
    }
  });
});
