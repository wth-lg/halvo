import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { codeOf, FLOAT_SIGNS } from "./float-signs.ts";

test("the gateway rounds nothing itself: its code holds no sign of floating-point money", () => {
  const found: string[] = [];
  for (const file of readdirSync("src/gateway").filter((name) => name.endsWith(".ts")).sort()) {
    codeOf(readFileSync(`src/gateway/${file}`, "utf8")).forEach((line, index) => {
      for (const [what, sign] of FLOAT_SIGNS.slice(0, 4)) if (sign.test(line)) found.push(`${file}:${index + 1}: ${what}`);
    });
  }
  expect(found).toEqual([]);
});
