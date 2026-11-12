
import fc from "fast-check";
import { DOCUMENTED, type FakeMeterConfig } from "../../test-kit/fake-meter/fake-meter.ts";
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
} from "./harness.ts";
import type { Variant } from "./sync.ts";

export const RULE_DOWNTIME_BUDGET = 20 * DAY;
if (!(RULE_DOWNTIME_BUDGET < DOCUMENTED.maxTimestampAgeSeconds)) {
  throw new RangeError("the downtime budget of the rule is under the oldest timestamp that the provider takes");
}

const JUST_INSIDE_THE_SYNC_WINDOW = DAY - WINDOW_MARGIN - 61;
const JUST_OUTSIDE_THE_SYNC_WINDOW = DAY - WINDOW_MARGIN + 1;

const downSeconds = fc.constantFrom(
  0,
  0,
  0,
  300,
  JUST_INSIDE_THE_SYNC_WINDOW,
  JUST_OUTSIDE_THE_SYNC_WINDOW,
  26 * HOUR,
  3 * DAY,
  6 * DAY,
);

const fault: fc.Arbitrary<Fault> = fc.oneof(
  { weight: 8, arbitrary: fc.constant<Fault>({ kind: "none" }) },
  { weight: 3, arbitrary: fc.constant<Fault>({ kind: "request-lost" }) },
  { weight: 3, arbitrary: fc.constant<Fault>({ kind: "response-lost" }) },
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
  { weight: 20, arbitrary: fc.record({ kind: fc.constant("reserve" as const), account, amountMicro }) },
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
    arbitrary: fc.record({
      kind: fc.constant("advance-to-period-end" as const),
      account,
      secondsBeforePeriodEnd: lastSecondsOfABillingPeriod,
    }),
  },
  { weight: 3, arbitrary: fc.record({ kind: fc.constant("crash" as const), account, downSeconds }) },
  { weight: 1, arbitrary: fc.record({ kind: fc.constant("cancel" as const), account }) },
  { weight: 1, arbitrary: fc.record({ kind: fc.constant("delete" as const), account }) },
);

type MeterSettings = Omit<FakeMeterConfig, "invoicePeriodSeconds">;

const READ_WIDTHS_THAT_HOLD_OTHER_INTERVALS = [HOUR, DAY];

const meter: fc.Arbitrary<MeterSettings> = fc.record({
  uniquenessWindowSeconds: fc.constantFrom(DAY, DAY, 36 * HOUR, 40 * DAY, Number.POSITIVE_INFINITY),
  duplicateAnswer: fc.constantFrom("error" as const, "same-as-recorded" as const),
  graceSeconds: fc.constantFrom(HOUR, 72 * HOUR),
  readLagSeconds: fc.constantFrom(0, 60, HOUR, 23 * HOUR),
  readGranularitySeconds: fc.constantFrom(1, 60, ...READ_WIDTHS_THAT_HOLD_OTHER_INTERVALS),
});

const oneOrTwoAccountsAtAnySubscribeTimes = fc.array(fc.integer({ min: 0, max: 3 * DAY }), { minLength: 1, maxLength: 2 });
const twoAccountsOnOneIntervalGrid = fc.constantFrom([0, 0], [0, HOUR], [DAY, 0]);
const REACH_MAX_LENGTH = { size: "max" } as const;

export function scenarios(meterOverride: Partial<MeterSettings> = {}): fc.Arbitrary<Scenario> {
  return fc.record({
    subscribeSecondsAfterClockStart: fc.oneof(
      { weight: 3, arbitrary: oneOrTwoAccountsAtAnySubscribeTimes },
      { weight: 1, arbitrary: twoAccountsOnOneIntervalGrid },
    ),
    meter: meter.map((m) => ({ ...m, ...meterOverride })),
    intervalSeconds: fc.constantFrom(HOUR, DAY, null),
    capPeriodSeconds: fc.constantFrom(7 * DAY, 30 * DAY),
    capAnchorSecondsBeforeSubscription: fc.integer({ min: 1, max: 7 * DAY - 1 }),
    commands: fc.array(command, { minLength: 1, maxLength: 80, ...REACH_MAX_LENGTH }),
    faultsByProviderCall: fc.array(fault, { maxLength: 30, ...REACH_MAX_LENGTH }),
  });
}

export type InvoiceTally = { periods: number; providerMinor: number; invoicedMinor: number; providerCalls: number };

export type RunSummary = {
  readonly variant: Variant;
  readonly seed: number;
  readonly planned: number;
  readonly orderingsThroughFirstFailure: number;
  readonly failure: (Finding & { readonly scenario: Scenario | null }) | null;
  readonly coverage: Readonly<Record<string, number>>;
  readonly invoices: Readonly<Record<string, InvoiceTally>>;
};

export function propertyRun(params: {
  variant: Variant;
  seed: number;
  orderings: number;
  meterOverride?: Partial<MeterSettings>;
}): RunSummary {
  const coverage: Record<string, number> = {};
  const invoices: Record<string, InvoiceTally> = {};
  let shrinking = false;
  const run = (scenario: Scenario): Report =>
    runScenario(scenario, {
      variant: params.variant,
      downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET,
      onFailedCheck: "stop",
    });
  const details = fc.check(
    fc.property(scenarios(params.meterOverride), (scenario) => {
      const report = run(scenario);
      if (!shrinking) {
        for (const [key, value] of Object.entries(report.coverage)) coverage[key] = (coverage[key] ?? 0) + value;
        const bump = (key: string): void => {
          coverage[key] = (coverage[key] ?? 0) + 1;
        };
        bump("orderings");
        if (report.periods.some((row) => row.provider > 0)) bump("orderings.with-usage-at-the-provider");
        if (report.periods.every((row) => row.reported === 0)) bump("orderings.with-no-reported-usage");
        if ((report.coverage["call.before-the-closing-phase"] ?? 0) > 0) {
          bump("orderings.with-a-provider-call-before-the-closing-phase");
        }
        tally(invoices, scenario, report);
      }
      if (report.findings.length > 0) {
        shrinking = true;
        return false;
      }
      return true;
    }),
    { seed: params.seed, numRuns: params.orderings },
  );
  let failure: RunSummary["failure"] = null;
  if (details.failed) {
    const smallest = details.counterexample?.[0];
    const finding = smallest === undefined ? undefined : run(smallest).findings[0];
    failure = {
      check: finding?.check ?? "unknown",
      detail: finding?.detail ?? "fast-check gave no counterexample",
      scenario: smallest ?? null,
    };
  }
  return {
    variant: params.variant,
    seed: params.seed,
    planned: params.orderings,
    orderingsThroughFirstFailure: details.numRuns,
    failure,
    coverage,
    invoices,
  };
}

function tally(invoices: Record<string, InvoiceTally>, scenario: Scenario, report: Report): void {
  const interval = scenario.intervalSeconds === null ? "whole period" : `${scenario.intervalSeconds / HOUR} h`;
  const key = `interval ${interval}, grace ${scenario.meter.graceSeconds / HOUR} h`;
  let entry = invoices[key];
  if (entry === undefined) {
    entry = { periods: 0, providerMinor: 0, invoicedMinor: 0, providerCalls: 0 };
    invoices[key] = entry;
  }
  for (const row of report.periods) {
    if (row.provider === 0) continue;
    entry.periods += 1;
    entry.providerMinor += row.provider / RULE_MICRO_PER_MINOR;
    entry.invoicedMinor += row.invoicedMinor;
  }
  for (const call of ["send.first-attempt", "send.retry-inside-the-window", "send.resend-after-the-window", "call.read"]) {
    entry.providerCalls += report.coverage[call] ?? 0;
  }
}

const DOWNTIME_PAST_THE_TIMESTAMP_LIMIT = 36 * DAY;

export function downPastTheTimestampLimit(): Report {
  const scenario: Scenario = {
    subscribeSecondsAfterClockStart: [0],
    meter: {
      uniquenessWindowSeconds: DAY,
      duplicateAnswer: "error",
      graceSeconds: 72 * HOUR,
      readLagSeconds: 0,
      readGranularitySeconds: 60,
    },
    intervalSeconds: null,
    capPeriodSeconds: 7 * DAY,
    capAnchorSecondsBeforeSubscription: 1,
    commands: [
      { kind: "reserve", account: 0, amountMicro: 250_000 },
      { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
      { kind: "advance", seconds: 30 * DAY },
      { kind: "advance", seconds: DOWNTIME_PAST_THE_TIMESTAMP_LIMIT + DAY },
    ],
    faultsByProviderCall: [{ kind: "crash-before-call", downSeconds: DOWNTIME_PAST_THE_TIMESTAMP_LIMIT }],
  };
  return runScenario(scenario, { variant: "as-designed", downtimeBudgetSeconds: 40 * DAY, onFailedCheck: "list" });
}

export const SEARCH_DEPTH = 4;

const HALF_A_WINDOW = 12 * HOUR;
const TWO_OF_THESE_PASS_A_WINDOW = 50_000;
const PAST_THE_FAKES_WINDOW = 26 * HOUR;
if (!(2 * TWO_OF_THESE_PASS_A_WINDOW > DAY && TWO_OF_THESE_PASS_A_WINDOW < 23 * HOUR)) {
  throw new RangeError("two search downtimes of this length pass a window, and one stays under a read lag of 23 hours");
}
const SEARCH_DOWNTIMES = [
  0,
  HALF_A_WINDOW,
  TWO_OF_THESE_PASS_A_WINDOW,
  JUST_INSIDE_THE_SYNC_WINDOW,
  JUST_OUTSIDE_THE_SYNC_WINDOW,
  PAST_THE_FAKES_WINDOW,
];

const ACTS_AS_NONE_IN_THE_SEARCH = ["late-response"] as const;
type SearchedFaultKind = Exclude<FaultKind, (typeof ACTS_AS_NONE_IN_THE_SEARCH)[number]>;

const SEARCH_FAULTS_BY_KIND: { readonly [Kind in SearchedFaultKind]: readonly Fault[] } = {
  none: [{ kind: "none" }],
  "request-lost": [{ kind: "request-lost" }],
  "response-lost": [{ kind: "response-lost" }],
  "late-delivery": [{ kind: "late-delivery" }],
  "crash-before-call": SEARCH_DOWNTIMES.map((d): Fault => ({ kind: "crash-before-call", downSeconds: d })),
  "crash-after-call": SEARCH_DOWNTIMES.map((d): Fault => ({ kind: "crash-after-call", downSeconds: d })),
};
const SEARCH_FAULTS: readonly Fault[] = Object.values(SEARCH_FAULTS_BY_KIND).flat();

export type SearchSummary = {
  readonly variant: Variant;
  readonly settings: number;
  readonly sequences: number;
  readonly failure: (Finding & { readonly scenario: Scenario }) | null;
};

export function faultSequenceSearch(variant: Variant, meterOverride: Partial<MeterSettings> = {}): SearchSummary {
  let sequences = 0;
  let failure: SearchSummary["failure"] = null;
  const options = { variant, downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "stop" as const };
  const visit = (
    base: Scenario,
    faultsOfEarlierCalls: readonly Fault[],
    faults: readonly Fault[],
    reportOfTheSameRun: Report | null,
  ): boolean => {
    const scenario = { ...base, faultsByProviderCall: [...faultsOfEarlierCalls, ...faults] };
    const report = reportOfTheSameRun ?? runScenario(scenario, options);
    if (reportOfTheSameRun === null) sequences += 1;
    const finding = report.findings[0];
    if (finding !== undefined) {
      failure = { ...finding, scenario };
      return false;
    }
    const noCallLeftToFault = report.callsWhileFaultsOn <= faultsOfEarlierCalls.length + faults.length;
    if (faults.length === SEARCH_DEPTH || noCallLeftToFault) return true;
    for (const next of SEARCH_FAULTS) {
      const sameRunAsJustMade = next.kind === "none";
      if (!visit(base, faultsOfEarlierCalls, [...faults, next], sameRunAsJustMade ? report : null)) return false;
    }
    return true;
  };
  const cases = searchCases(meterOverride);
  for (const { scenario, faultsOfEarlierCalls } of cases) {
    if (!visit(scenario, faultsOfEarlierCalls, [], null)) break;
  }
  return { variant, settings: cases.length, sequences, failure };
}

const ONE_MINOR_UNIT_AND_A_REMAINDER = RULE_MICRO_PER_MINOR + 2_345;

function searchCases(meterOverride: Partial<MeterSettings>): { scenario: Scenario; faultsOfEarlierCalls: Fault[] }[] {
  const cases: { scenario: Scenario; faultsOfEarlierCalls: Fault[] }[] = [];
  const scenario = (meter: MeterSettings, commands: Command[]): Scenario => ({
    subscribeSecondsAfterClockStart: [0],
    meter: { ...meter, ...meterOverride },
    intervalSeconds: HOUR,
    capPeriodSeconds: 7 * DAY,
    capAnchorSecondsBeforeSubscription: 1,
    commands,
    faultsByProviderCall: [],
  });
  const usageWithARemainder: Command[] = [
    { kind: "reserve", account: 0, amountMicro: ONE_MINOR_UNIT_AND_A_REMAINDER },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
  ];
  const hourPasses: Command = { kind: "advance", seconds: HOUR };
  const oneIntervalOnAnAccountThatStays: Command[] = [...usageWithARemainder, hourPasses];
  const oneIntervalOnAnAccountDeletedWithItUnsent: Command[] = [...usageWithARemainder, { kind: "delete", account: 0 }, hourPasses];
  const sameAmount: Command[] = [
    { kind: "reserve", account: 0, amountMicro: RULE_MICRO_PER_MINOR },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
  ];
  const twoIntervalsAnHourApartWithTheSameAmountToSend: Command[] = [...usageWithARemainder, hourPasses, ...sameAmount, hourPasses];
  const theFirstIntervalIsAcknowledgedAtOnce: Fault[] = [{ kind: "none" }];
  const aHoldSettlesAfterItsPeriodEndWhenALaterIntervalCutByACancelHasEnded: Command[] = [
    { kind: "advance-to-period-end", account: 0, secondsBeforePeriodEnd: 361 },
    { kind: "reserve", account: 0, amountMicro: 9_999 },
    { kind: "advance", seconds: 120 },
    { kind: "reserve", account: 0, amountMicro: 15_000 },
    { kind: "advance", seconds: 250 },
    { kind: "reserve", account: 0, amountMicro: 9_000 },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "cancel", account: 0 },
    { kind: "advance", seconds: 600 },
    { kind: "settle", account: 0, holdsBackFromNewest: 1, percentOfHold: 100 },
  ];
  for (const uniquenessWindowSeconds of [DAY, Number.POSITIVE_INFINITY]) {
    for (const readLagSeconds of [0, 23 * HOUR]) {
      const meter = { uniquenessWindowSeconds, readLagSeconds, graceSeconds: HOUR };
      for (const duplicateAnswer of ["error", "same-as-recorded"] as const) {
        const narrow = { ...meter, duplicateAnswer, readGranularitySeconds: 60 };
        cases.push({ scenario: scenario(narrow, oneIntervalOnAnAccountThatStays), faultsOfEarlierCalls: [] });
        cases.push({ scenario: scenario(narrow, oneIntervalOnAnAccountDeletedWithItUnsent), faultsOfEarlierCalls: [] });
      }
      const wide = { ...meter, duplicateAnswer: "error" as const, readGranularitySeconds: DAY };
      cases.push({
        scenario: scenario(wide, twoIntervalsAnHourApartWithTheSameAmountToSend),
        faultsOfEarlierCalls: theFirstIntervalIsAcknowledgedAtOnce,
      });
    }
  }
  cases.push({
    scenario: scenario(
      {
        uniquenessWindowSeconds: DAY,
        readLagSeconds: 0,
        graceSeconds: HOUR,
        duplicateAnswer: "error",
        readGranularitySeconds: 1,
      },
      aHoldSettlesAfterItsPeriodEndWhenALaterIntervalCutByACancelHasEnded,
    ),
    faultsOfEarlierCalls: [],
  });
  return cases;
}
