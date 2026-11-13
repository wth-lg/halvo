import fc from "fast-check";
import { DOCUMENTED, type FakeMeterConfig } from "../fake-meter.ts";
import {
  BILLING_PERIOD,
  type Command,
  DAY,
  type Fault,
  type FaultKind,
  type Finding,
  HOLD_LIFE,
  HOUR,
  RETRY_AFTER,
  type Report,
  RULE_MICRO_PER_MINOR,
  runScenario,
  type Scenario,
  WINDOW_MARGIN,
} from "./world.ts";

export const RULE_DOWNTIME_BUDGET = 20 * DAY;
if (!(RULE_DOWNTIME_BUDGET < DOCUMENTED.maxTimestampAgeSeconds)) {
  throw new RangeError("the downtime budget of the rule is under the oldest timestamp that the provider takes");
}

const JUST_INSIDE_THE_SYNC_WINDOW = DAY - WINDOW_MARGIN - 61;
const JUST_OUTSIDE_THE_SYNC_WINDOW = DAY - WINDOW_MARGIN + 1;

const downSeconds = fc.constantFrom(0, 0, 0, 300, JUST_INSIDE_THE_SYNC_WINDOW, JUST_OUTSIDE_THE_SYNC_WINDOW, 26 * HOUR, 3 * DAY, 6 * DAY);

const fault: fc.Arbitrary<Fault> = fc.oneof(
  { weight: 8, arbitrary: fc.constant<Fault>({ kind: "none" }) },
  { weight: 3, arbitrary: fc.constant<Fault>({ kind: "request-lost" }) },
  { weight: 3, arbitrary: fc.constant<Fault>({ kind: "response-lost" }) },
  { weight: 2, arbitrary: fc.constant<Fault>({ kind: "refused" }) },
  { weight: 2, arbitrary: fc.constant<Fault>({ kind: "late-delivery" }) },
  { weight: 2, arbitrary: fc.constant<Fault>({ kind: "late-response" }) },
  { weight: 3, arbitrary: downSeconds.map((d): Fault => ({ kind: "crash-before-call", downSeconds: d })) },
  { weight: 3, arbitrary: downSeconds.map((d): Fault => ({ kind: "crash-after-call", downSeconds: d })) },
);

const account = fc.integer({ min: 0, max: 1 });

const amountMicro = fc.oneof(
  { weight: 1, arbitrary: fc.integer({ min: 1, max: RULE_MICRO_PER_MINOR - 1 }) },
  { weight: 2, arbitrary: fc.integer({ min: RULE_MICRO_PER_MINOR, max: 25 * RULE_MICRO_PER_MINOR }) },
);

const mostlyDispatched = fc.oneof({ weight: 3, arbitrary: fc.constant(true) }, { weight: 1, arbitrary: fc.constant(false) });

const shortEnoughToSettle = fc.integer({ min: 0, max: 120 });
const aroundTheHoldLife = fc.constantFrom(HOLD_LIFE - 1, HOLD_LIFE, HOLD_LIFE + 1, HOUR, 6 * HOUR);
const aroundTheWindow = fc.constantFrom(JUST_INSIDE_THE_SYNC_WINDOW, JUST_OUTSIDE_THE_SYNC_WINDOW, 25 * HOUR, 3 * DAY);
const asLongAsThePeriods = fc.constantFrom(7 * DAY, 15 * DAY, BILLING_PERIOD);
const seconds = fc.oneof(
  { weight: 5, arbitrary: shortEnoughToSettle },
  { weight: 2, arbitrary: aroundTheHoldLife },
  { weight: 2, arbitrary: aroundTheWindow },
  { weight: 2, arbitrary: asLongAsThePeriods },
);

const mostlyTheNewestHold = fc.oneof(fc.constantFrom(0, 0, 0, 1, 2), fc.nat({ max: 40 }));
const PERCENT_ABOVE_THE_HOLD = 120;
const lastSecondsOfABillingPeriod = fc.constantFrom(HOLD_LIFE + 1, HOLD_LIFE, HOLD_LIFE - 1, RETRY_AFTER + 1, RETRY_AFTER, 2, 1, 0);

const command: fc.Arbitrary<Command> = fc.oneof(
  { weight: 20, arbitrary: fc.record({ kind: fc.constant("reserve" as const), account, amountMicro, dispatch: mostlyDispatched }) },
  {
    weight: 20,
    arbitrary: fc.record({
      kind: fc.constant("settle" as const),
      account,
      holdsBackFromNewest: mostlyTheNewestHold,
      percentOfHold: fc.integer({ min: 0, max: PERCENT_ABOVE_THE_HOLD }),
    }),
  },
  { weight: 20, arbitrary: fc.record({ kind: fc.constant("advance" as const), seconds }) },
  {
    weight: 4,
    arbitrary: fc.record({ kind: fc.constant("advance-to-period-end" as const), account, secondsBeforePeriodEnd: lastSecondsOfABillingPeriod }),
  },
  { weight: 3, arbitrary: fc.record({ kind: fc.constant("crash" as const), account, downSeconds }) },
  { weight: 1, arbitrary: fc.record({ kind: fc.constant("cancel" as const), account, unstatedFirstSeconds: fc.constantFrom(0, 0, 1, 30, HOLD_LIFE) }) },
  { weight: 1, arbitrary: fc.record({ kind: fc.constant("delete" as const), account }) },
);

type MeterSettings = Omit<FakeMeterConfig, "invoicePeriodSeconds">;

const meter: fc.Arbitrary<MeterSettings> = fc.record({
  uniquenessWindowSeconds: fc.constantFrom(DAY, DAY, 36 * HOUR, 40 * DAY, Number.POSITIVE_INFINITY),
  duplicateAnswer: fc.constantFrom("error" as const, "same-as-recorded" as const),
  graceSeconds: fc.constantFrom(HOUR, 72 * HOUR),
  readLagSeconds: fc.constantFrom(0, 60, HOUR, 23 * HOUR),
  readGranularitySeconds: fc.constantFrom(1, 60, HOUR, DAY),
});

const oneOrTwoAccountsAtAnySubscribeTimes = fc.array(fc.integer({ min: 0, max: 3 * DAY }), { minLength: 1, maxLength: 2 });
const twoAccountsOnOneIntervalGrid = fc.constantFrom([0, 0], [0, HOUR], [DAY, 0]);
const REACH_MAX_LENGTH = { size: "max" } as const;

export function scenarios(): fc.Arbitrary<Scenario> {
  return fc.record({
    subscribeSecondsAfterClockStart: fc.oneof(
      { weight: 3, arbitrary: oneOrTwoAccountsAtAnySubscribeTimes },
      { weight: 1, arbitrary: twoAccountsOnOneIntervalGrid },
    ),
    meter,
    intervalSeconds: fc.constantFrom(HOUR, DAY),
    capPeriod: fc.constantFrom("week" as const, "month" as const),
    capAnchorSecondsBeforeSubscription: fc.integer({ min: 1, max: 7 * DAY - 1 }),
    commands: fc.array(command, { minLength: 1, maxLength: 80, ...REACH_MAX_LENGTH }),
    faultsByProviderCall: fc.array(fault, { maxLength: 30, ...REACH_MAX_LENGTH }),
  });
}

export type RunSummary = {
  readonly seed: number;
  readonly planned: number;
  readonly passed: number;
  readonly failure: (Finding & { readonly scenario: Scenario | null }) | null;
  readonly coverage: Readonly<Record<string, number>>;
};

const ruleOptions = { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "stop" as const };

export async function propertyRun(params: { seed: number; orderings: number }): Promise<RunSummary> {
  const coverage: Record<string, number> = {};
  let shrinking = false;
  let passed = 0;
  const details = await fc.check(
    fc.asyncProperty(scenarios(), async (scenario) => {
      const report = await runScenario(scenario, ruleOptions);
      if (!shrinking) {
        for (const [key, value] of Object.entries(report.coverage)) coverage[key] = (coverage[key] ?? 0) + value;
        const bump = (key: string): void => {
          coverage[key] = (coverage[key] ?? 0) + 1;
        };
        bump("orderings");
        if (report.periods.some((row) => row.provider > 0)) bump("orderings.with-usage-at-the-provider");
        if (report.periods.every((row) => row.reported === 0)) bump("orderings.with-no-reported-usage");
        if ((report.coverage["call.before-the-closing-phase"] ?? 0) > 0) bump("orderings.with-a-provider-call-before-the-closing-phase");
      }
      if (report.findings.length > 0) {
        shrinking = true;
        return false;
      }
      if (!shrinking) passed += 1;
      return true;
    }),
    { seed: params.seed, numRuns: params.orderings },
  );
  let failure: RunSummary["failure"] = null;
  if (details.failed) {
    const smallest = details.counterexample?.[0];
    const finding = smallest === undefined ? undefined : (await runScenario(smallest, ruleOptions)).findings[0];
    const error = details.errorInstance instanceof Error ? details.errorInstance.message.split("\n")[0] : "fast-check gave no counterexample";
    failure = { check: finding?.check ?? "unknown", detail: finding?.detail ?? error ?? "", scenario: smallest ?? null };
  }
  return { seed: params.seed, planned: params.orderings, passed, failure, coverage };
}

export const SEARCH_DEPTH = 4;

const HALF_A_WINDOW = 12 * HOUR;
const TWO_OF_THESE_PASS_A_WINDOW = 50_000;
const PAST_THE_FAKES_WINDOW = 26 * HOUR;
if (!(2 * TWO_OF_THESE_PASS_A_WINDOW > DAY && TWO_OF_THESE_PASS_A_WINDOW < 23 * HOUR)) {
  throw new RangeError("two search downtimes of this length pass a window, and one stays under a read lag of 23 hours");
}
const SEARCH_DOWNTIMES = [0, HALF_A_WINDOW, TWO_OF_THESE_PASS_A_WINDOW, JUST_INSIDE_THE_SYNC_WINDOW, JUST_OUTSIDE_THE_SYNC_WINDOW, PAST_THE_FAKES_WINDOW];

type SearchedFaultKind = Exclude<FaultKind, "late-response" | "refused">;

const SEARCH_FAULTS_BY_KIND: { readonly [Kind in SearchedFaultKind]: readonly Fault[] } = {
  none: [{ kind: "none" }],
  "request-lost": [{ kind: "request-lost" }],
  "response-lost": [{ kind: "response-lost" }],
  "late-delivery": [{ kind: "late-delivery" }],
  "crash-before-call": SEARCH_DOWNTIMES.map((d): Fault => ({ kind: "crash-before-call", downSeconds: d })),
  "crash-after-call": SEARCH_DOWNTIMES.map((d): Fault => ({ kind: "crash-after-call", downSeconds: d })),
};
export const SEARCH_FAULTS: readonly Fault[] = Object.values(SEARCH_FAULTS_BY_KIND).flat();

export type SearchCase = { readonly scenario: Scenario; readonly faultsOfEarlierCalls: readonly Fault[] };

export type SearchSummary = {
  readonly settings: number;
  readonly sequences: number;
  readonly failure: (Finding & { readonly scenario: Scenario }) | null;
};

export async function searchOne(searchCase: SearchCase): Promise<SearchSummary> {
  let sequences = 0;
  let failure: SearchSummary["failure"] = null;
  const visit = async (faults: readonly Fault[], reportOfTheSameRun: Report | null): Promise<boolean> => {
    const scenario = { ...searchCase.scenario, faultsByProviderCall: [...searchCase.faultsOfEarlierCalls, ...faults] };
    const report = reportOfTheSameRun ?? (await runScenario(scenario, ruleOptions));
    if (reportOfTheSameRun === null) sequences += 1;
    const finding = report.findings[0];
    if (finding !== undefined) {
      failure = { ...finding, scenario };
      return false;
    }
    const noCallLeftToFault = report.callsWhileFaultsOn <= searchCase.faultsOfEarlierCalls.length + faults.length;
    if (faults.length === SEARCH_DEPTH || noCallLeftToFault) return true;
    for (const next of SEARCH_FAULTS) {
      if (!(await visit([...faults, next], next.kind === "none" ? report : null))) return false;
    }
    return true;
  };
  await visit([], null);
  return { settings: 1, sequences, failure };
}

const ONE_MINOR_UNIT_AND_A_REMAINDER = RULE_MICRO_PER_MINOR + 2_345;

export function searchCases(): SearchCase[] {
  const cases: SearchCase[] = [];
  const scenario = (meter: MeterSettings, commands: Command[]): Scenario => ({
    subscribeSecondsAfterClockStart: [0],
    meter,
    intervalSeconds: HOUR,
    capPeriod: "week",
    capAnchorSecondsBeforeSubscription: 1,
    commands,
    faultsByProviderCall: [],
  });
  const usageWithARemainder: Command[] = [
    { kind: "reserve", account: 0, amountMicro: ONE_MINOR_UNIT_AND_A_REMAINDER, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
  ];
  const hourPasses: Command = { kind: "advance", seconds: HOUR };
  const oneIntervalOnAnAccountThatStays: Command[] = [...usageWithARemainder, hourPasses];
  const oneIntervalOnAnAccountDeletedWithItUnsent: Command[] = [...usageWithARemainder, { kind: "delete", account: 0 }, hourPasses];
  const sameAmount: Command[] = [
    { kind: "reserve", account: 0, amountMicro: RULE_MICRO_PER_MINOR, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
  ];
  const twoIntervalsAnHourApartWithTheSameAmountToSend: Command[] = [...usageWithARemainder, hourPasses, ...sameAmount, hourPasses];
  const theFirstIntervalIsAcknowledgedAtOnce: Fault[] = [{ kind: "none" }];
  const aHoldSettlesAfterItsPeriodEndWhenALaterIntervalCutByACancelHasEnded: Command[] = [
    { kind: "advance-to-period-end", account: 0, secondsBeforePeriodEnd: 361 },
    { kind: "reserve", account: 0, amountMicro: 9_999, dispatch: true },
    { kind: "advance", seconds: 120 },
    { kind: "reserve", account: 0, amountMicro: 15_000, dispatch: true },
    { kind: "advance", seconds: 250 },
    { kind: "reserve", account: 0, amountMicro: 9_000, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "cancel", account: 0, unstatedFirstSeconds: 0 },
    { kind: "advance", seconds: 600 },
    { kind: "settle", account: 0, holdsBackFromNewest: 1, percentOfHold: 100 },
  ];
  for (const uniquenessWindowSeconds of [DAY, Number.POSITIVE_INFINITY]) {
    for (const readLagSeconds of [0, 23 * HOUR]) {
      const base = { uniquenessWindowSeconds, readLagSeconds, graceSeconds: HOUR };
      for (const duplicateAnswer of ["error", "same-as-recorded"] as const) {
        const narrow = { ...base, duplicateAnswer, readGranularitySeconds: 60 };
        cases.push({ scenario: scenario(narrow, oneIntervalOnAnAccountThatStays), faultsOfEarlierCalls: [] });
        cases.push({ scenario: scenario(narrow, oneIntervalOnAnAccountDeletedWithItUnsent), faultsOfEarlierCalls: [] });
      }
      const wide = { ...base, duplicateAnswer: "error" as const, readGranularitySeconds: DAY };
      cases.push({ scenario: scenario(wide, twoIntervalsAnHourApartWithTheSameAmountToSend), faultsOfEarlierCalls: theFirstIntervalIsAcknowledgedAtOnce });
    }
  }
  cases.push({
    scenario: scenario(
      { uniquenessWindowSeconds: DAY, readLagSeconds: 0, graceSeconds: HOUR, duplicateAnswer: "error", readGranularitySeconds: 1 },
      aHoldSettlesAfterItsPeriodEndWhenALaterIntervalCutByACancelHasEnded,
    ),
    faultsOfEarlierCalls: [],
  });
  return cases;
}
