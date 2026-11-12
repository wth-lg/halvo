
import { selfCheck } from "../../test-kit/fake-meter/self-check.ts";
import { downPastTheTimestampLimit, faultSequenceSearch, propertyRun, SEARCH_DEPTH } from "./property.ts";

const SEEDS = [1, 2, 3, 4, 5];
const ORDERINGS = 20000;

const REQUIRED_CONTROLS = [
  ["new-identifier-on-retry", "a retry draws a new identifier"],
  ["resend-without-read", "a resend after the window skips the read"],
  ["drop-remainder", "the remainder is dropped at each interval"],
  ["round-up-last", "the last remainder is rounded up and sent"],
  ["cut-at-cap-period", "intervals are cut at the cap period's ends"],
];
const EXTRA_CONTROLS = [
  ["timestamp-at-interval-start", "the event carries the interval's start as its timestamp"],
  ["seal-with-open-holds", "an interval is sealed while one of its holds is open"],
  ["window-restarts-at-resend", "the 24 hours are counted again from each resend"],
  ["read-ignores-neighbours", "a read takes its whole range for the one interval"],
  ["seal-out-of-time-order", "a later interval is sealed before an older one with an open hold"],
  ["identifier-without-account", "the identifier leaves out the account"],
  ["settle-counts-in-the-newest-interval", "a settle counts in the newest interval, not the hold's own"],
];

const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(name);
  return at === -1 ? undefined : args[at + 1];
};
const seeds = option("--seeds")?.split(",").map(Number) ?? SEEDS;
const orderings = Number(option("--orderings") ?? ORDERINGS);
const development = option("--seeds") !== undefined || option("--orderings") !== undefined;
const show = args.includes("--show");
const smallest = (failure) => {
  const size = `${failure.scenario?.commands.length ?? 0} commands, ${failure.scenario?.faultsByProviderCall.length ?? 0} faults`;
  print(`    smallest failing ordering: ${size}`);
  if (show) print(`    ${JSON.stringify(failure.scenario)}`);
};
const started = performance.now();
const print = (line) => console.log(line);
const printOutsideTheRecord = (line) => console.error(line);

if (development) print("DEVELOPMENT RUN: not the measurement of RESULT.md");

const rules = selfCheck();
for (const rule of rules.filter((r) => !r.ok)) print(`fake meter: FAILED its own check: ${rule.name}`);
const fakeOk = rules.every((r) => r.ok);
print(`fake meter: ${rules.filter((r) => r.ok).length} of ${rules.length} of its own checks hold`);

print("");
print(`rule run: the sync as designed, ${seeds.length} seeds of ${orderings} orderings`);
const coverage = {};
const invoices = {};
let ruleFailures = 0;
let ruleOrderings = 0;
for (const seed of seeds) {
  const run = propertyRun({ variant: "as-designed", seed, orderings });
  ruleOrderings += run.orderingsThroughFirstFailure;
  for (const [key, value] of Object.entries(run.coverage)) coverage[key] = (coverage[key] ?? 0) + value;
  for (const [key, value] of Object.entries(run.invoices)) {
    invoices[key] ??= { periods: 0, providerMinor: 0, invoicedMinor: 0, providerCalls: 0 };
    for (const field of Object.keys(value)) invoices[key][field] += value[field];
  }
  if (run.failure === null) {
    print(`  seed ${seed}: ${run.orderingsThroughFirstFailure} orderings, every check passed`);
  } else {
    ruleFailures += 1;
    print(`  seed ${seed}: MISSED at ordering ${run.orderingsThroughFirstFailure}: ${run.failure.check} (${run.failure.detail});`);
    smallest(run.failure);
  }
}
const search = faultSequenceSearch("as-designed");
const searched = `${search.sequences} sequences of up to ${SEARCH_DEPTH} faults on one interval, over ${search.settings} settings`;
if (search.failure === null) {
  print(`  fault-sequence search: ${searched}, every check passed`);
} else {
  ruleFailures += 1;
  print(`  fault-sequence search: MISSED after ${search.sequences} sequences: ${search.failure.check} (${search.failure.detail})`);
  smallest(search.failure);
}

print("");
print(`what the ${ruleOrderings} generated orderings held (counts over all of them):`);
for (const key of Object.keys(coverage).sort()) print(`  ${key}: ${coverage[key]}`);

print("");
print("usage on an invoice, by interval length and grace period (billing periods with usage):");
for (const key of Object.keys(invoices).sort()) {
  const { periods, providerMinor, invoicedMinor, providerCalls } = invoices[key];
  const missed = providerMinor - invoicedMinor;
  const share = providerMinor === 0 ? "0.0" : ((100 * missed) / providerMinor).toFixed(1);
  print(
    `  ${key}: ${periods} periods, ${providerMinor} minor units at the provider, ${missed} of them (${share}%) arrived after the invoice was finalized, ${providerCalls} provider calls`,
  );
}

const runControl = ([variant, words], meterOverride) => {
  for (const seed of seeds) {
    const run = propertyRun({ variant, seed, orderings, meterOverride });
    if (run.failure !== null) {
      print(`  ${words}: missed the rule at ordering ${run.orderingsThroughFirstFailure} of seed ${seed}`);
      print(`    check: ${run.failure.check} (${run.failure.detail})`);
      smallest(run.failure);
      return true;
    }
  }
  const found = faultSequenceSearch(variant, meterOverride);
  if (found.failure !== null) {
    print(`  ${words}: missed the rule in the fault-sequence search, at sequence ${found.sequences}, and in none of ${seeds.length * orderings} generated orderings`);
    print(`    check: ${found.failure.check} (${found.failure.detail})`);
    smallest(found.failure);
    return true;
  }
  print(`  ${words}: DID NOT MISS the rule in ${seeds.length * orderings} generated orderings and ${found.sequences} fault sequences`);
  return false;
};

print("");
print("controls that must miss the rule:");
const controlsMissed = REQUIRED_CONTROLS.map((control) => runControl(control));
print("");
print("further controls (reported, not part of the rule line):");
for (const control of EXTRA_CONTROLS) runControl(control);

print("");
print("limits (outside the rule line):");
runControl(["as-designed", "a read that lags 30 hours behind a 24-hour window, the sync as designed"], {
  readLagSeconds: 30 * 3600,
  uniquenessWindowSeconds: 24 * 3600,
});
const down = downPastTheTimestampLimit();
print("  the object is down for 36 days after a billing period ends, the sync as designed:");
const refusal = down.findings.find((finding) => finding.check === "refused by the provider");
print(`    the fake's answer to the send: ${refusal === undefined ? "no refusal" : `refused (${refusal.detail})`}`);
for (const row of down.periods.filter((r) => r.reported > 0)) {
  print(
    `    ${row.account} period ${row.period}: reported ${row.reported}, provider-side ${row.provider}, short by ${row.reported + row.carryIn - row.provider} micro-units`,
  );
}

print("");
const allControlsMissed = controlsMissed.every(Boolean);
let exit = 0;
if (!fakeOk || !allControlsMissed) {
  exit = 2;
  print(
    `rule: DOES NOT COUNT (${!fakeOk ? "the fake meter failed its own check" : "a control that must miss the rule did not"})`,
  );
} else if (ruleFailures > 0) {
  exit = 1;
  print(`rule: MISSED (${ruleFailures} of the ${seeds.length} seeds and the fault-sequence search held an ordering that failed a check)`);
} else {
  print(
    `rule: MET (${ruleOrderings} generated orderings over ${seeds.length} seeds, ${coverage["orderings.with-usage-at-the-provider"] ?? 0} of which put usage at the provider, and ${search.sequences} fault sequences passed every check, and all ${REQUIRED_CONTROLS.length} controls missed the rule)`,
  );
}
printOutsideTheRecord(`wall time: ${((performance.now() - started) / 1000).toFixed(1)} s`);
process.exitCode = exit;
