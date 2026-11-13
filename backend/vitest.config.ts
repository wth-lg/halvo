import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { cloudflareTest } from "@cloudflare/vitest-plugin";
import { defineConfig } from "vitest/config";

export function localValues(path = "stack.d/vars.env"): Record<string, string> {
  const values: Record<string, string> = {};
  const lines = readFileSync(path, "utf8").split("\n");
  for (const [index, line] of lines.entries()) {
    if (line === "" || line.startsWith("#")) continue;
    const match = /^([A-Z][A-Z0-9_]*)=([A-Za-z0-9_]+)$/.exec(line);
    if (!match) throw new Error(`stack.d/vars.env line ${index + 1} is not NAME=VALUE`);
    values[match[1] as string] = match[2] as string;
  }
  return values;
}

function stripeMock(): string {
  try {
    return execFileSync("sh", ["scripts/stack.sh", "url", "stripe-mock"], { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

export default defineConfig({
  test: {
    projects: [
      {
        plugins: [
          cloudflareTest({
            wrangler: { configPath: "./wrangler.jsonc" },
            miniflare: { bindings: { ...localValues(), PROVIDER_BASE_URL: stripeMock(), PUBLIC_ORIGIN: "http://halvo.test" } },
          }),
        ],
        test: { name: "workerd", include: ["test/**/*.test.ts"], exclude: ["test/**/*.node.test.ts"] },
      },
      {
        test: { name: "node", include: ["test/**/*.node.test.ts"], environment: "node" },
      },
    ],
  },
});
