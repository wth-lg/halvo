import { mkdirSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";

const DRIVER = "../../stress/driver.mjs";
const CELL = { name: "rollover@20", kind: "rollover", level: 20, switch: "usage-last", killRuns: 0, plainRuns: 1 };

function scriptedAttempt(crossedFrom: number) {
  return async (_ctx: unknown, cell: typeof CELL, variant: string, runNo: number, withKill: boolean, attempt: number) => ({
    key: `attempt-${attempt + 1}`,
    cell: cell.name,
    kind: cell.kind,
    level: cell.level,
    switch: cell.switch,
    variant,
    withKill,
    runNo,
    counted: attempt >= crossedFrom,
    violations: [],
    facts: { crossed: attempt >= crossedFrom, openAtKill: 0, streamAtKill: false },
    ms: 1,
    error: null,
  });
}

function contextOf(crossedFrom: number, stopAtFirstViolation: boolean) {
  mkdirSync(".tmp", { recursive: true });
  return {
    cfg: { attempts: 3 },
    target: { command: () => ({}) },
    outDir: mkdtempSync(join(".tmp", "driver-record-")),
    jobs: 1,
    progress: () => undefined,
    stopAtFirstViolation,
    runOnce: scriptedAttempt(crossedFrom),
  };
}

test("ruling B-15: the driver keeps crossed in the record of a discarded attempt and of the last one", async () => {
  const { runCells } = await import(DRIVER);
  const [run] = await runCells(contextOf(1, false), [CELL], "counts-open-holds");
  expect([run.key, run.facts.crossed, run.discarded]).toEqual([
    "attempt-2",
    true,
    [{ key: "attempt-1", error: null, ms: 1, openAtKill: 0, streamAtKill: false, crossed: false, violations: [] }],
  ]);
});

test("ruling B-15: under check 50, which stops at the first violation, an attempt that lacked the crossing stops the scheduling and is the last record", async () => {
  const { runCells } = await import(DRIVER);
  const ctx = contextOf(1, true);
  const [run] = await runCells(ctx, [CELL], "counts-open-holds");
  expect([run.key, run.facts.crossed, run.discarded, (ctx as { halted?: boolean }).halted]).toEqual(["attempt-1", false, [], true]);
});
