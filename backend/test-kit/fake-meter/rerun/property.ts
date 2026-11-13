import fc from "fast-check";
import { DOCUMENTED, FakeMeter, type FakeMeterConfig, type RecordedEvent } from "../fake-meter.ts";
import {
  BILLING_PERIOD,
  type CancelForm,
  type Command,
  DAY,
  type Fault,
  type FaultKind,
  type Finding,
  HOLD_LIFE,
  HOUR,
  READ_WIDTH_SECONDS,
  RETRY_AFTER,
  type Report,
  RULE_MICRO_PER_MINOR,
  runScenario,
  type Scenario,
  WINDOW_MARGIN,
} from "./world.ts";

export const RULE_DOWNTIME_BUDGET = 20 * DAY;
export { READ_WIDTH_SECONDS };
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
  {
    weight: 1,
    arbitrary: fc.record({
      kind: fc.constant("cancel" as const),
      account,
      form: fc.constantFrom<CancelForm>("stated", "stated", "unstated-then-stated", "unstated-reversed"),
      lagSeconds: fc.constantFrom(1, 30, HOLD_LIFE, HOUR),
    }),
  },
  { weight: 2, arbitrary: fc.record({ kind: fc.constant("late-cancel-event" as const), account }) },
  { weight: 1, arbitrary: fc.record({ kind: fc.constant("reactivate" as const), account }) },
  { weight: 1, arbitrary: fc.record({ kind: fc.constant("delete" as const), account }) },
);

type MeterSettings = Omit<FakeMeterConfig, "invoicePeriodSeconds">;

const meter: fc.Arbitrary<MeterSettings> = fc.record({
  uniquenessWindowSeconds: fc.constantFrom(DAY, DAY, 36 * HOUR, 40 * DAY, Number.POSITIVE_INFINITY),
  duplicateAnswer: fc.constantFrom("error" as const, "same-as-recorded" as const),
  graceSeconds: fc.constantFrom(HOUR, 72 * HOUR),
  readLagSeconds: fc.constantFrom(0, 60, HOUR, 23 * HOUR),
  readGranularitySeconds: fc.constant(READ_WIDTH_SECONDS),
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

export type FailingOrdering = { readonly index: number; readonly check: string; readonly detail: string; readonly facts: Readonly<Record<string, number>> };

export type RunSummary = {
  readonly seed: number;
  readonly planned: number;
  readonly passed: number;
  readonly failing: readonly FailingOrdering[];
  readonly shortest: Readonly<Record<string, Scenario>>;
  readonly failure: (Finding & { readonly scenario: Scenario | null }) | null;
  readonly coverage: Readonly<Record<string, number>>;
};

const ruleOptions = { downtimeBudgetSeconds: RULE_DOWNTIME_BUDGET, onFailedCheck: "stop" as const };

export async function propertyRun(params: { seed: number; orderings: number }): Promise<RunSummary> {
  const coverage: Record<string, number> = {};
  let passed = 0;
  let index = 0;
  const failing: FailingOrdering[] = [];
  const shortest: Record<string, Scenario> = {};
  const details = await fc.check(
    fc.asyncProperty(scenarios(), async (scenario) => {
      const report = await runScenario(scenario, ruleOptions);
      index += 1;
      for (const [key, value] of Object.entries(report.coverage)) coverage[key] = (coverage[key] ?? 0) + value;
      const bump = (key: string): void => {
        coverage[key] = (coverage[key] ?? 0) + 1;
      };
      bump("orderings");
      if (report.periods.some((row) => row.provider > 0)) bump("orderings.with-usage-at-the-provider");
      if (report.periods.every((row) => row.reported === 0)) bump("orderings.with-no-reported-usage");
      if ((report.coverage["call.before-the-closing-phase"] ?? 0) > 0) bump("orderings.with-a-provider-call-before-the-closing-phase");
      const finding = report.findings[0];
      if (finding === undefined) {
        passed += 1;
        return true;
      }
      failing.push({ index, check: finding.check, detail: finding.detail, facts: report.facts });
      const known = shortest[finding.check];
      if (known === undefined || scenario.commands.length < known.commands.length) shortest[finding.check] = scenario;
      return true;
    }),
    { seed: params.seed, numRuns: params.orderings },
  );
  const first = failing[0];
  let failure: RunSummary["failure"] = null;
  if (first !== undefined) failure = { check: first.check, detail: first.detail, scenario: shortest[first.check] ?? null };
  else if (details.failed) failure = { check: "the run threw", detail: details.errorInstance instanceof Error ? details.errorInstance.message.split("\n")[0] ?? "" : "", scenario: null };
  return { seed: params.seed, planned: params.orderings, passed, failing, shortest, failure, coverage };
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

export type SearchCase = { readonly scenario: Scenario; readonly faultsOfEarlierCalls: readonly Fault[]; readonly faults?: readonly Fault[] };

export type SearchSummary = {
  readonly settings: number;
  readonly sequences: number;
  readonly rangesWithAnotherInterval: number;
  readonly failure: (Finding & { readonly scenario: Scenario; readonly facts: Readonly<Record<string, number>> }) | null;
};

export async function searchOne(searchCase: SearchCase): Promise<SearchSummary> {
  let sequences = 0;
  let rangesWithAnotherInterval = 0;
  let failure: SearchSummary["failure"] = null;
  const visit = async (faults: readonly Fault[], reportOfTheSameRun: Report | null): Promise<boolean> => {
    const scenario = { ...searchCase.scenario, faultsByProviderCall: [...searchCase.faultsOfEarlierCalls, ...faults] };
    const report = reportOfTheSameRun ?? (await runScenario(scenario, ruleOptions));
    if (reportOfTheSameRun === null) {
      sequences += 1;
      rangesWithAnotherInterval += report.coverage["read.range-holds-another-sealed-interval"] ?? 0;
    }
    const finding = report.findings[0];
    if (finding !== undefined) {
      failure = { ...finding, scenario, facts: report.facts };
      return false;
    }
    const noCallLeftToFault = report.callsWhileFaultsOn <= searchCase.faultsOfEarlierCalls.length + faults.length;
    if (faults.length === SEARCH_DEPTH || noCallLeftToFault) return true;
    for (const next of searchCase.faults ?? SEARCH_FAULTS) {
      if (!(await visit([...faults, next], next.kind === "none" ? report : null))) return false;
    }
    return true;
  };
  await visit([], null);
  return { settings: 1, sequences, rangesWithAnotherInterval, failure };
}

export async function runOnce(searchCase: SearchCase): Promise<SearchSummary> {
  const scenario = { ...searchCase.scenario, faultsByProviderCall: [...searchCase.faultsOfEarlierCalls] };
  const report = await runScenario(scenario, ruleOptions);
  const finding = report.findings[0];
  return {
    settings: 1,
    sequences: 1,
    rangesWithAnotherInterval: report.coverage["read.range-holds-another-sealed-interval"] ?? 0,
    failure: finding === undefined ? null : { ...finding, scenario, facts: report.facts },
  };
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
  const inOneMinute = (first: number, second: number): Command[] => [
    { kind: "advance-to-period-end", account: 0, secondsBeforePeriodEnd: 30 },
    { kind: "reserve", account: 0, amountMicro: first, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "advance", seconds: 31 },
    { kind: "reserve", account: 0, amountMicro: second, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "cancel", account: 0, form: "stated", lagSeconds: 0 },
    hourPasses,
  ];
  const theFirstIntervalIsAcknowledgedAtOnce: Fault[] = [{ kind: "none" }];
  const aHoldSettlesAfterItsPeriodEndWhenALaterIntervalCutByACancelHasEnded: Command[] = [
    { kind: "advance-to-period-end", account: 0, secondsBeforePeriodEnd: 361 },
    { kind: "reserve", account: 0, amountMicro: 9_999, dispatch: true },
    { kind: "advance", seconds: 120 },
    { kind: "reserve", account: 0, amountMicro: 15_000, dispatch: true },
    { kind: "advance", seconds: 250 },
    { kind: "reserve", account: 0, amountMicro: 9_000, dispatch: true },
    { kind: "settle", account: 0, holdsBackFromNewest: 0, percentOfHold: 100 },
    { kind: "cancel", account: 0, form: "stated", lagSeconds: 0 },
    { kind: "advance", seconds: 600 },
    { kind: "settle", account: 0, holdsBackFromNewest: 1, percentOfHold: 100 },
  ];
  for (const uniquenessWindowSeconds of [DAY, Number.POSITIVE_INFINITY]) {
    for (const readLagSeconds of [0, 23 * HOUR]) {
      const base = { uniquenessWindowSeconds, readLagSeconds, graceSeconds: HOUR };
      for (const duplicateAnswer of ["error", "same-as-recorded"] as const) {
        const narrow = { ...base, duplicateAnswer, readGranularitySeconds: READ_WIDTH_SECONDS };
        cases.push({ scenario: scenario(narrow, oneIntervalOnAnAccountThatStays), faultsOfEarlierCalls: [] });
        cases.push({ scenario: scenario(narrow, oneIntervalOnAnAccountDeletedWithItUnsent), faultsOfEarlierCalls: [] });
      }
      const minute = { ...base, duplicateAnswer: "error" as const, readGranularitySeconds: READ_WIDTH_SECONDS };
      cases.push({ scenario: scenario(minute, inOneMinute(ONE_MINOR_UNIT_AND_A_REMAINDER, RULE_MICRO_PER_MINOR)), faultsOfEarlierCalls: theFirstIntervalIsAcknowledgedAtOnce });
    }
  }
  cases.push({
    scenario: scenario(
      { uniquenessWindowSeconds: DAY, readLagSeconds: 0, graceSeconds: HOUR, duplicateAnswer: "error", readGranularitySeconds: READ_WIDTH_SECONDS },
      aHoldSettlesAfterItsPeriodEndWhenALaterIntervalCutByACancelHasEnded,
    ),
    faultsOfEarlierCalls: [],
  });
  const crashes = (downtimes: readonly number[]): Fault[] =>
    downtimes.flatMap((d): Fault[] => [
      { kind: "crash-before-call", downSeconds: d },
      { kind: "crash-after-call", downSeconds: d },
    ]);
  const IN_THE_MARGIN = [JUST_INSIDE_THE_SYNC_WINDOW, JUST_OUTSIDE_THE_SYNC_WINDOW, DAY - 120, DAY - 75];
  const marginFaults: Fault[] = [
    { kind: "none" },
    { kind: "refused" },
    { kind: "request-lost" },
    { kind: "response-lost" },
    { kind: "late-delivery" },
    ...crashes(IN_THE_MARGIN),
  ];
  for (const readLagSeconds of [0, 23 * HOUR]) {
    cases.push({
      scenario: scenario(
        { uniquenessWindowSeconds: DAY, readLagSeconds, graceSeconds: HOUR, duplicateAnswer: "error", readGranularitySeconds: READ_WIDTH_SECONDS },
        oneIntervalOnAnAccountThatStays,
      ),
      faultsOfEarlierCalls: [],
      faults: marginFaults,
    });
  }
  const rangeFaults: Fault[] = [
    { kind: "none" },
    { kind: "request-lost" },
    { kind: "response-lost" },
    { kind: "late-delivery" },
    ...crashes([JUST_OUTSIDE_THE_SYNC_WINDOW, PAST_THE_FAKES_WINDOW]),
  ];
  const minuteAndLagging = { uniquenessWindowSeconds: DAY, readLagSeconds: 23 * HOUR, graceSeconds: HOUR, duplicateAnswer: "error" as const, readGranularitySeconds: READ_WIDTH_SECONDS };
  const recordedThenRefusedToTheLimit: Fault[] = [{ kind: "response-lost" }, { kind: "refused" }, { kind: "none" }, { kind: "refused" }, { kind: "refused" }];
  const refusedToTheLimit: Fault[] = [{ kind: "refused" }, { kind: "refused" }, { kind: "refused" }];
  cases.push({ scenario: scenario(minuteAndLagging, inOneMinute(ONE_MINOR_UNIT_AND_A_REMAINDER, 3 * RULE_MICRO_PER_MINOR)), faultsOfEarlierCalls: recordedThenRefusedToTheLimit, faults: rangeFaults });
  cases.push({ scenario: scenario(minuteAndLagging, inOneMinute(2 * RULE_MICRO_PER_MINOR, 2 * RULE_MICRO_PER_MINOR)), faultsOfEarlierCalls: refusedToTheLimit, faults: rangeFaults });
  const fewFaults: Fault[] = [{ kind: "none" }, { kind: "request-lost" }, { kind: "response-lost" }, { kind: "crash-after-call", downSeconds: 0 }];
  const exact = { uniquenessWindowSeconds: DAY, readLagSeconds: 0, graceSeconds: HOUR, duplicateAnswer: "error" as const, readGranularitySeconds: READ_WIDTH_SECONDS };
  const twoHoursPass: Command = { kind: "advance", seconds: 2 * HOUR };
  const lateCancel = (form: CancelForm, lateAfterSeconds: number): Command[] => [
    ...usageWithARemainder,
    { kind: "cancel", account: 0, form, lagSeconds: HOLD_LIFE },
    { kind: "advance", seconds: lateAfterSeconds },
    { kind: "late-cancel-event", account: 0 },
    twoHoursPass,
  ];
  for (const lateAfterSeconds of [2 * HOUR, 10]) {
    for (const form of ["unstated-then-stated", "unstated-reversed"] as const) {
      cases.push({ scenario: scenario(exact, lateCancel(form, lateAfterSeconds)), faultsOfEarlierCalls: [], faults: fewFaults });
    }
  }
  const stated: Command = { kind: "cancel", account: 0, form: "stated", lagSeconds: 0 };
  cases.push({
    scenario: scenario(exact, [...usageWithARemainder, stated, { kind: "advance", seconds: 5 }, { kind: "reserve", account: 0, amountMicro: RULE_MICRO_PER_MINOR, dispatch: true }, twoHoursPass]),
    faultsOfEarlierCalls: [],
    faults: fewFaults,
  });
  cases.push({ scenario: scenario(exact, [...usageWithARemainder, stated, twoHoursPass, { kind: "delete", account: 0 }, hourPasses]), faultsOfEarlierCalls: [], faults: fewFaults });
  const reactivated = (afterSeconds: number): Command[] => [
    ...usageWithARemainder,
    stated,
    { kind: "advance", seconds: afterSeconds },
    { kind: "reactivate", account: 0 },
    ...usageWithARemainder,
    twoHoursPass,
    ...sameAmount,
    twoHoursPass,
  ];
  for (const afterSeconds of [2 * HOUR, 5]) {
    cases.push({ scenario: scenario(exact, reactivated(afterSeconds)), faultsOfEarlierCalls: [], faults: fewFaults });
  }
  cases.push({
    scenario: scenario(
      { uniquenessWindowSeconds: DAY, readLagSeconds: 23 * HOUR, graceSeconds: HOUR, duplicateAnswer: "error", readGranularitySeconds: READ_WIDTH_SECONDS },
      oneIntervalOnAnAccountThatStays,
    ),
    faultsOfEarlierCalls: [{ kind: "response-lost" }],
    faults: [
      { kind: "none" },
      { kind: "refused" },
      { kind: "request-lost" },
      { kind: "response-lost" },
      ...crashes([HALF_A_WINDOW, TWO_OF_THESE_PASS_A_WINDOW]),
    ],
  });
  const node = globalThis as unknown as { process: { getBuiltinModule(id: string): unknown } };
  const fs = node.process.getBuiltinModule("node:fs") as { readFileSync(path: URL, encoding: "utf8"): string };
  const here = (import.meta as unknown as { url: string }).url;
  const ordering = JSON.parse(fs.readFileSync(new URL("../../../gate/fixtures/metering/ordering-seed3-5701.json", here), "utf8")) as Scenario;
  cases.push({
    scenario: { ...ordering, meter: { ...ordering.meter, readGranularitySeconds: READ_WIDTH_SECONDS }, faultsByProviderCall: [] },
    faultsOfEarlierCalls: ordering.faultsByProviderCall,
    faults: [{ kind: "none" }],
  });
  const searchedAfterTheSequence: Fault[] = [{ kind: "none" }, { kind: "refused" }, { kind: "request-lost" }, { kind: "response-lost" }];
  const recordedLateInTheWindowThenRefused: Fault[] = [
    { kind: "crash-before-call", downSeconds: 80_000 },
    { kind: "response-lost" },
    { kind: "refused" },
    { kind: "none" },
    { kind: "refused" },
    { kind: "refused" },
  ];
  const resentPastTheWindowThenRefused: Fault[] = [
    { kind: "crash-before-call", downSeconds: PAST_THE_FAKES_WINDOW },
    { kind: "none" },
    { kind: "response-lost" },
    { kind: "none" },
    { kind: "refused" },
    { kind: "none" },
    { kind: "refused" },
    { kind: "none" },
    { kind: "refused" },
  ];
  cases.push({ scenario: scenario(minuteAndLagging, oneIntervalOnAnAccountThatStays), faultsOfEarlierCalls: recordedLateInTheWindowThenRefused, faults: searchedAfterTheSequence });
  cases.push({ scenario: scenario(minuteAndLagging, oneIntervalOnAnAccountThatStays), faultsOfEarlierCalls: resentPastTheWindowThenRefused, faults: searchedAfterTheSequence });
  cases.push({
    scenario: { ...scenario(minuteAndLagging, oneIntervalOnAnAccountThatStays), repeatAnswer: "failed" },
    faultsOfEarlierCalls: [{ kind: "response-lost" }],
    faults: [...searchedAfterTheSequence, ...crashes([HALF_A_WINDOW])],
  });
  return cases;
}

const OBJECT_ENDED_QUERY = "SELECT start FROM metering_interval WHERE outcome IS NULL";

type Statement = { all(...bindings: unknown[]): unknown[] };
type Prepare = (this: unknown, query: string) => Statement;

async function withObjectEndedRows<T>(rows: unknown[], run: () => Promise<T>): Promise<T> {
  const node = globalThis as unknown as { process: { getBuiltinModule(id: string): unknown } };
  const sqlite = node.process.getBuiltinModule("node:sqlite") as { DatabaseSync: { prototype: { prepare: Prepare } } };
  const proto = sqlite.DatabaseSync.prototype;
  const prepare = proto.prepare;
  proto.prepare = function handOver(this: unknown, query: string): Statement {
    return query === OBJECT_ENDED_QUERY ? { all: () => rows.map((row) => ({ ...(row as object) })) } : prepare.call(this, query);
  };
  try {
    return await run();
  } finally {
    proto.prepare = prepare;
  }
}

async function withAnEventBeforeTheFirstPeriod<T>(run: () => Promise<T>): Promise<T> {
  const periods = FakeMeter.prototype.periods;
  let handedOver = false;
  FakeMeter.prototype.periods = function handOver(this: FakeMeter, account: string, now: number) {
    const list = periods.call(this, account, now);
    const first = list[0];
    if (!handedOver && first !== undefined) {
      handedOver = true;
      (this as unknown as { events: RecordedEvent[] }).events.push({ account, identifier: "outside-every-period", amountMinor: 1, timestamp: first.start - 1, receivedAt: now });
    }
    return list;
  };
  try {
    return await run();
  } finally {
    FakeMeter.prototype.periods = periods;
  }
}

export type ModelCase = { readonly name: string; readonly expected: string; readonly got: string };

export async function modelCases(): Promise<ModelCase[]> {
  const settings = searchCases();
  const firstFinding = (report: Report) => report.findings[0]?.check ?? "no finding";
  const scenarioOf = (index: number, faults: Fault[]): Scenario => {
    const searchCase = settings[index];
    if (searchCase === undefined) throw new RangeError(`no search setting ${index}`);
    return { ...searchCase.scenario, faultsByProviderCall: [...searchCase.faultsOfEarlierCalls, ...faults] };
  };
  return [
    {
      name: "an event at the meter stamped one second before the first period, setting 0 with no fault",
      expected: "an event outside every billing period",
      got: firstFinding(await withAnEventBeforeTheFirstPeriod(() => runScenario(scenarioOf(0, []), ruleOptions))),
    },
    {
      name: "the object reads as ended while its last send is lost, setting 29 with a lost request",
      expected: "an event recorded after the last acknowledgement",
      got: firstFinding(await withObjectEndedRows([], () => runScenario(scenarioOf(29, [{ kind: "request-lost" }]), ruleOptions))),
    },
    {
      name: "the object never reads as ended, setting 29 with no fault",
      expected: "the end did not complete",
      got: firstFinding(await withObjectEndedRows([{ start: 0 }], () => runScenario(scenarioOf(29, []), ruleOptions))),
    },
  ];
}
