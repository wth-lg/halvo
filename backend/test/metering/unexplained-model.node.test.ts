import { describe, expect, test } from "vitest";
import { RULE_DOWNTIME_BUDGET } from "../../test-kit/fake-meter/rerun/property.ts";
import { type Command, DAY, type Fault, HOUR, runScenario, type Scenario } from "../../test-kit/fake-meter/rerun/world.ts";

const DOWN_PAST_THE_WINDOW: Fault = { kind: "crash-before-call", downSeconds: 86_101 };
const DELETE: Command[] = [
  { kind: "delete", account: 0 },
  { kind: "advance", seconds: 2 * DAY },
];

function behindAForeignEvent(first: Fault, deleted: boolean): Scenario {
  return {
    subscribeSecondsAfterClockStart: [0],
    meter: { uniquenessWindowSeconds: DAY, readLagSeconds: 0, graceSeconds: HOUR, duplicateAnswer: "error", readGranularitySeconds: 60 },
    intervalSeconds: HOUR,
    capPeriod: "week",
    capAnchorSecondsBeforeSubscription: 1,
    commands: [
      { kind: "reserve", account: 0, amountMicro: 30_000, dispatch: true },
      { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
      { kind: "advance", seconds: HOUR },
      { kind: "foreign-event", account: 0, amountMinor: 5 },
      { kind: "advance", seconds: 3 * DAY },
      ...(deleted ? DELETE : []),
    ],
    faultsByProviderCall: [first, DOWN_PAST_THE_WINDOW],
  };
}

describe("check 55's model with a foreign event at the meter in an interval's minute (unit 5.7, ruling P7-86 B)", { timeout: 300_000 }, () => {
  for (const [first, held, deleted] of [
    [{ kind: "response-lost" }, true, true],
    [{ kind: "request-lost" }, false, true],
    [{ kind: "response-lost" }, true, false],
  ] as const) {
    test(`the first send ${held ? "recorded with its answer lost" : "lost"}, then the object down past the window${deleted ? ", and the account deleted" : ", and the account kept"}: no finding, and the close with the cause unexplained-total is a counted fact, ${held ? "held" : "not held"} by the meter`, async () => {
      const report = await runScenario(behindAForeignEvent(first, deleted), { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "list" });
      expect(report.findings).toEqual([]);
      expect([report.facts["foreign-events"], report.facts["not-billable-unexplained-total"], report.facts["not-billable-unexplained-total-held-by-the-meter"]]).toEqual([1, 1, held ? 1 : 0]);
      expect(report.coverage[held ? "unexplained-total.closed-held-by-the-meter" : "unexplained-total.closed-not-at-the-meter"]).toBe(1);
      expect(report.coverage["read.holds-a-foreign-event"]).toBeGreaterThanOrEqual(20);
      expect(report.coverage["delete.completed"]).toBe(deleted ? 1 : undefined);
      expect(report.coverage["interval.not-billable-after-an-unexplained-run"]).toBe(deleted ? 0 : 1);
    });
  }

  test("a refused interval closed by a run is counted once in the equation: the first send lost, the retry refused while the mark is set, with the account deleted and with it kept", async () => {
    const faults: Fault[] = [{ kind: "request-lost" }, { kind: "refused" }];
    const seen = [];
    for (const deleted of [true, false]) {
      const report = await runScenario({ ...behindAForeignEvent(faults[0] as Fault, deleted), faultsByProviderCall: faults }, { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "list" });
      seen.push([deleted, report.findings, report.facts["not-billable-unexplained-total"]]);
    }
    expect(seen).toEqual([
      [true, [], 1],
      [false, [], 1],
    ]);
  });
});
