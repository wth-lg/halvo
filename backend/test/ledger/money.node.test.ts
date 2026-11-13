import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { codeOf, FLOAT_SIGNS } from "./float-signs.ts";

const DIR = "src/ledger";

test("money in src/ledger/ is whole micro-units: the one division is the round-up of costOf in cost.ts, on bigints, and nothing else rounds", () => {
  const found: string[] = [];
  for (const file of readdirSync(DIR).filter((name) => name.endsWith(".ts")).sort()) {
    codeOf(readFileSync(`${DIR}/${file}`, "utf8")).forEach((line, index) => {
      for (const [what, sign] of FLOAT_SIGNS) if (sign.test(line)) found.push(`${file}:${index + 1}: ${what}: ${line.trim()}`);
    });
  }
  expect(found).toEqual(["cost.ts:16: the division operator: const cost = (total + TOKENS_PER_PRICE_UNIT - 1n) / TOKENS_PER_PRICE_UNIT;"]);
});

test("the search sees each sign it looks for", () => {
  const lines = ["Math.ceil(a)", "x = parseFloat(y)", "a.toFixed(2)", "const half = 0.5", "const tiny = 1e-6", "a / b"].flatMap(codeOf);
  expect(lines.map((line) => FLOAT_SIGNS.filter(([, sign]) => sign.test(line)).map(([what]) => what))).toEqual([
    ["a Math function that rounds or draws"],
    ["parseFloat"],
    ["toFixed or toPrecision"],
    ["a decimal or exponent literal"],
    ["a decimal or exponent literal"],
    ["the division operator"],
  ]);
  expect(codeOf('import { x } from "./api.ts";').filter((line) => /\//.test(line))).toEqual([]);
});
