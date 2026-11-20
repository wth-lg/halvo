import { describe, expect, test, vi } from "vitest";
import type { Metering } from "../../src/metering/index.ts";
import { RULE_DOWNTIME_BUDGET } from "../../test-kit/fake-meter/rerun/property.ts";
import { type Command, DAY, type Fault, HOUR, runScenario, type Scenario } from "../../test-kit/fake-meter/rerun/world.ts";

const DOWN_PAST_THE_WINDOW: Fault = { kind: "crash-before-call", downSeconds: 86_101 };
const scripted = vi.hoisted(() => ({ closesAnAcknowledgedRead: false }));

vi.mock("../../src/metering/index.ts", async (importOriginal) => {
  const real = await importOriginal<typeof import("../../src/metering/index.ts")>();
  return {
    ...real,
    makeMetering(config: Parameters<typeof real.makeMetering>[0]): Metering {
      const metering = real.makeMetering(config);
      return {
        ...metering,
        record(sql, made, answer, now) {
          const outcome = () => sql.exec("SELECT outcome FROM metering_interval WHERE start = ?", made.interval)[0]?.outcome ?? null;
          const before = outcome();
          const next = metering.record(sql, made, answer, now);
          if (!scripted.closesAnAcknowledgedRead || made.kind !== "read" || before !== null || outcome() !== "acknowledged") return next;
          sql.exec("UPDATE metering_interval SET outcome = ?, refusal = ? WHERE start = ?", "not-billable", "unexplained-total", made.interval);
          return { ...next, notBillable: next.notBillable + 1 };
        },
      };
    },
  };
});
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

  test("a cut pair behind a foreign event, the first held at the meter: both close with the cause and no finding, with the account kept and with it deleted", async () => {
    const seen = [];
    for (const deleted of [false, true]) {
      const pair: Scenario = {
        ...behindAForeignEvent({ kind: "response-lost" }, deleted),
        commands: [
          { kind: "advance-to-period-end", account: 0, secondsBeforePeriodEnd: 30 },
          { kind: "reserve", account: 0, amountMicro: 30_000, dispatch: true },
          { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
          { kind: "advance", seconds: 31 },
          { kind: "foreign-event", account: 0, amountMinor: 5 },
          { kind: "reserve", account: 0, amountMicro: 20_000, dispatch: true },
          { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
          { kind: "cancel", account: 0, form: "stated", lagSeconds: 0 },
          { kind: "advance", seconds: 5 * DAY },
          ...(deleted ? DELETE : []),
        ],
        faultsByProviderCall: [{ kind: "response-lost" }, DOWN_PAST_THE_WINDOW, ...Array.from({ length: 28 }, (): Fault => ({ kind: "none" })), { kind: "request-lost" }, DOWN_PAST_THE_WINDOW],
      };
      const report = await runScenario(pair, { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "list" });
      seen.push([deleted, report.findings.length, report.facts["not-billable-unexplained-total"], report.facts["not-billable-unexplained-total-held-by-the-meter"]]);
    }
    expect(seen).toEqual([
      [false, 0, 2, 1],
      [true, 0, 2, 1],
    ]);
  });

  test("a close with the cause where no foreign event and no lag can leave a read unexplained is a finding: a scripted module closes with the cause at a read that shows the amount", async () => {
    const kept = behindAForeignEvent({ kind: "response-lost" }, false);
    const lawless: Scenario = { ...kept, commands: kept.commands.filter((command) => command.kind !== "foreign-event") };
    scripted.closesAnAcknowledgedRead = true;
    try {
      const report = await runScenario(lawless, { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "list" });
      const checks = report.findings.map((finding) => finding.check);
      expect({ findings: checks.length, disagrees: checks.includes("the record disagrees with the provider"), facts: [report.facts["foreign-events"], report.facts["not-billable-unexplained-total"]] }).toEqual({
        findings: checks.length > 0 ? checks.length : "one or more",
        disagrees: true,
        facts: [0, 0],
      });
    } finally {
      scripted.closesAnAcknowledgedRead = false;
    }
  });
});
