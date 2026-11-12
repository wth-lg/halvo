import assert from "node:assert/strict";

const MICRO_PER_MINOR = 10_000;

export const config = {
  levels: [1, 20, 200],
  requestsPerClient: { 1: 8, 20: 3, 200: 2 },
  capHolds: { 1: 3, 20: 12, 200: 120 },
  switches: [
    "usage-last",
    "usage-missing",
    "zero-placeholder",
    "slow",
    "over-report",
    "error-status",
    "reset",
    "anthropic-cut",
  ],
  faultShare: 0.5,
  killRuns: 5,
  plainRuns: 1,
  attempts: 6,
  wave: { level: 200, k: 120 },
  extraLevel: 20,
  rowSize: { level: 50, requestsPerClient: 20 },

  holdLifeMs: 3000,
  deadlineMs: 1500,
  marginMs: 400,
  voidHoldLifeMs: 1700,
  alarmGraceMs: 1000,
  quietOverdueMs: 300,
  periodMs: 604_800_000,
  rolloverAfterMs: 400,

  price: { input: 3_100_000, output: 15_700_000 },
  maxTokens: [640, 1280, 2560],
  chunks: 6,
  chunkMs: 50,
  thinkMs: 150,
  killDelayMs: { min: 20, max: 200 },
  abortMidEvery: 4,
  jobs: 6,
  timeWaitLimit: 6000,
  readyTimeoutMs: 30_000,
};

export const expectedCostOf = (price, inputTokens, outputTokens) =>
  Math.ceil((inputTokens * price.input + outputTokens * price.output) / 1_000_000);

export const holdFor = (price, bodyBytes, maxTokens) => expectedCostOf(price, bodyBytes, maxTokens);

export function cells(cfg) {
  for (const level of cfg.levels) {
    assert(level === 1 || cfg.capHolds[level] < level, `the cap at level ${level} must fit fewer holds than the level's first burst sends, so that admission has to refuse`);
  }
  assert(cfg.voidHoldLifeMs < cfg.deadlineMs + cfg.marginMs, "the hold life of the void cell must be under the deadline plus the margin, so that the gateway voids every admitted request");
  assert(holdFor(cfg.price, 0, Math.min(...cfg.maxTokens)) >= MICRO_PER_MINOR, "the smallest output limit alone must make a hold of at least one minor unit, so that a cap of whole minor units at or above K holds fits exactly K of them");
  assert(cfg.price.input % 1_000_000 !== 0 && cfg.price.output % 1_000_000 !== 0, "neither price may be a whole number of micro-units per token, so that the round-up of a cost stays exercised");
  const list = [];
  for (const level of cfg.levels) {
    for (const name of cfg.switches) {
      list.push({ name: `${name}@${level}`, kind: "matrix", level, switch: name, killRuns: cfg.killRuns, plainRuns: cfg.plainRuns });
    }
  }
  const extra = { level: cfg.extraLevel, switch: "usage-last" };
  list.push({ name: `wave@${cfg.wave.level}`, kind: "wave", level: cfg.wave.level, switch: "usage-last", k: cfg.wave.k, killRuns: 0, plainRuns: cfg.killRuns });
  list.push({ name: `rollover@${extra.level}`, kind: "rollover", ...extra, killRuns: cfg.killRuns, plainRuns: cfg.plainRuns });
  list.push({ name: `quiet@${extra.level}`, kind: "quiet", ...extra, killRuns: cfg.killRuns, plainRuns: 0 });
  list.push({ name: `void@${extra.level}`, kind: "void", ...extra, killRuns: 0, plainRuns: cfg.killRuns });
  return list;
}

export const controlCells = (cfg) =>
  cells(cfg).filter((cell) => cell.name === "usage-last@20" || cell.name === "usage-last@200" || cell.kind === "wave");

export const rowSizeCell = (cfg) => ({
  name: `rowsize@${cfg.rowSize.level}`,
  kind: "rowsize",
  level: cfg.rowSize.level,
  switch: "error-status",
  requestsPerClient: cfg.rowSize.requestsPerClient,
  killRuns: 0,
  plainRuns: 1,
});
