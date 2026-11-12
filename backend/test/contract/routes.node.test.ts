import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { exportRoutes, largestBody } from "../../src/contract/export.ts";
import { routeTable } from "../../src/contract/routes.ts";

const ROOT = process.cwd();

test("a fresh export of the route table equals contract/routes/v1.json, also through contract-rules", () => {
  const text = readFileSync("contract/routes/v1.json", "utf8");
  expect(exportRoutes()).toBe(text);
  expect(text).toBe(`${JSON.stringify(routeTable, null, 2)}\n`);
  const printed = spawnSync("node", ["scripts/contract-rules", "export", "routes"], { cwd: ROOT, encoding: "utf8" });
  expect(printed.status).toBe(0);
  expect(printed.stdout).toBe(text);
});

test("the table holds the eight routes of contract 1.1, the three headers and the limits of the largest bodies", () => {
  expect(routeTable.routes.map((route) => `${route.method} ${route.path} ${route.kind}`)).toEqual([
    "POST /v1/checkout app",
    "GET /v1/subscription app",
    "GET /v1/cap app",
    "PUT /v1/cap app",
    "POST /v1/manage app",
    "DELETE /v1/account app",
    "POST /v1/webhook provider",
    "GET /v1/done page",
  ]);
  for (const [name, header] of Object.entries(routeTable.headers)) {
    const example = { "halvo-account": "0f8e2d4c-6b1a-4c3e-8d2f-1a2b3c4d5e6f", authorization: `Bearer ${"0".repeat(64)}`, "content-type": "application/json" }[name];
    expect(example?.length).toBe(header.length);
    expect(new RegExp(header.pattern).test(example ?? "")).toBe(true);
  }
  for (const route of routeTable.routes.filter((each) => each.kind === "app")) {
    const body = largestBody(route.name as "status");
    expect(route.limit).toBe(body === null ? 0 : new TextEncoder().encode(body).byteLength);
  }
  const webhook = routeTable.routes.find((route) => route.name === "webhook");
  expect(webhook?.limit).toBeGreaterThan(45);
});
