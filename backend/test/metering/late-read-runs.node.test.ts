import { describe, expect, test } from "vitest";
import { RULE_DOWNTIME_BUDGET, searchCases } from "../../test-kit/fake-meter/rerun/property.ts";
import { DAY, type Fault, HOUR, runScenario, type Scenario } from "../../test-kit/fake-meter/rerun/world.ts";

const SETTING_23 = 23;
const LOST_ANSWER_THEN_THREE_REFUSALS: Fault[] = [{ kind: "response-lost" }, { kind: "refused" }, { kind: "none" }, { kind: "refused" }, { kind: "refused" }];
const crashBefore = (downSeconds: number): Fault => ({ kind: "crash-before-call", downSeconds });

const oneMinute = (): Scenario => {
  const setting = searchCases()[SETTING_23];
  if (setting === undefined) throw new Error("check 55 has no setting 23");
  return setting.scenario;
};

const anHourApart: Scenario = {
  subscribeSecondsAfterClockStart: [0],
  meter: { uniquenessWindowSeconds: DAY, readLagSeconds: 23 * HOUR, graceSeconds: HOUR, duplicateAnswer: "error", readGranularitySeconds: 60 },
  intervalSeconds: HOUR,
  capPeriod: "week",
  capAnchorSecondsBeforeSubscription: 1,
  commands: [
    { kind: "reserve", account: 0, amountMicro: 12_345, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "advance", seconds: HOUR },
    { kind: "reserve", account: 0, amountMicro: 30_000, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "advance", seconds: HOUR },
  ],
  faultsByProviderCall: [],
};

async function run(scenario: Scenario, faults: Fault[], meter: Partial<Scenario["meter"]> = {}) {
  const report = await runScenario(
    { ...scenario, meter: { ...scenario.meter, ...meter }, faultsByProviderCall: faults },
    { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "stop" },
  );
  return {
    findings: [...new Set(report.findings.map((each) => each.check))],
    provider: report.periods.reduce((sum, row) => sum + row.provider, 0),
    reported: report.periods.reduce((sum, row) => sum + row.reported, 0),
  };
}

const RIGHT = { findings: [], provider: 40_000, reported: 42_345 };

describe("the builder's runs of check 55 after a lost answer and refusals (unit 5.4, ruling P7-77, part 7, cases 1 to 6)", { timeout: 900_000 }, () => {
  test("case 1, the replay at the minute-wide port, its two intervals an hour apart (the run that reads one minute wide): the first interval is acknowledged, and the provider holds 4 of 4 minor units", async () => {
    expect(await run(anHourApart, [...LOST_ANSWER_THEN_THREE_REFUSALS, crashBefore(86_101)])).toEqual(RIGHT);
  });

  test("case 1, the replay in setting 23's one-minute form, B down 86,101 s before its first call: both intervals are acknowledged, and the provider holds 4 of 4", async () => {
    expect(await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, crashBefore(86_101)])).toEqual(RIGHT);
  });

  test("case 2: B down 26 hours, and a read lag of 1 hour: 4 of 4, both acknowledged", async () => {
    expect([
      await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, crashBefore(26 * HOUR)]),
      await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, crashBefore(86_101)], { readLagSeconds: HOUR }),
    ]).toEqual([RIGHT, RIGHT]);
  });

  test("case 3: B down 86,039 s, and B sent inside its window with no fault, its answer lost or its request lost: the first interval is no longer recorded as not billable", async () => {
    expect([
      await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, crashBefore(DAY - 361)]),
      await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS]),
      await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, { kind: "response-lost" }]),
      await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, { kind: "request-lost" }]),
    ]).toEqual([RIGHT, RIGHT, RIGHT, RIGHT]);
  });

  test("case 4, unchanged: three refusals and nothing recorded close the first interval as not billable, and the provider holds B alone", async () => {
    const refusedThrice: Fault[] = [{ kind: "refused" }, { kind: "refused" }, { kind: "refused" }, crashBefore(86_101)];
    expect(await run(oneMinute(), refusedThrice)).toEqual({ findings: [], provider: 30_000, reported: 42_345 });
  });

  test("case 5, unchanged: with no read lag the read in the refusal's chain shows the amount and acknowledges", async () => {
    expect(await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, crashBefore(86_101)], { readLagSeconds: 0 })).toEqual(RIGHT);
  });

  test("case 6: B recorded with its answer lost and B down 86,101 s, then read past its window: the first interval is acknowledged, so B's read is explained and acknowledges B", async () => {
    expect(await run(oneMinute(), [...LOST_ANSWER_THEN_THREE_REFUSALS, { kind: "crash-after-call", downSeconds: 86_101 }])).toEqual(RIGHT);
  });
});
