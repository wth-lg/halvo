import { copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { MODEL, missingTool, scratch, tlc, WORKERS } from "./tlc.mjs";

const BAD_USAGE = 64;
const STATE_PARTS = ["A", "B", "F", "G"];
const LONGEST_SEARCH_OF_ALL_MS = 5_400_000;
const VIEW_STEPS = 6;
const REACH = [
  "DuplicateReserve",
  "DuplicateDispatch",
  "DuplicateSettle",
  "DuplicateVoid",
  "DuplicateExpire",
  "SettleBeforeItsReserve",
  "SettleAfterExpiry",
  "Overrun",
  "CapBelowCountedSpendWithAHoldOpen",
  "ChangeOfPeriodType",
  "PeriodBoundaryWithAHoldOpen",
  "RestartBetweenDispatchAndSettle",
  "GatewayCrashAfterDispatch",
  "VoidRefusedForADispatchedHold",
  "OpenHoldPastItsExpiryAfterARestart",
  "RefusedReserveAdmittedWhenSentAgain",
  "LateOverrunInNoTotal",
];
const BREAKS = [
  {
    property: "PropA",
    what: "the admission leaves out the open holds",
    find: "counted == row.reported + row.unreported + row.open",
    replace: "counted == row.reported + row.unreported",
  },
  {
    property: "PropB",
    what: "a hold whose expiry is exactly now stays open",
    find: 'ob.holds[r].state = "open" /\\ ob.holds[r].expiresAt <= now}',
    replace: 'ob.holds[r].state = "open" /\\ ob.holds[r].expiresAt < now}',
  },
  {
    property: "PropC",
    what: "a settle after expiry makes the hold settled",
    find: "[h EXCEPT !.cost = mv.cost, !.late = TRUE,",
    replace: '[h EXCEPT !.state = "settled", !.cost = mv.cost, !.late = TRUE,',
  },
  {
    property: "PropD",
    what: "a repeated reserve renews the expiry of its hold",
    find: "THEN Unchanged(p, IF h.amount = mv.amt",
    replace: "THEN Unchanged([p EXCEPT !.holds[mv.r].expiresAt = now + Life], IF h.amount = mv.amt",
  },
  {
    property: "PropE",
    what: "a reserve that would reach the cap exactly is refused",
    find: "IN IF counted + mv.amt > p.cap.amount",
    replace: "IN IF counted + mv.amt >= p.cap.amount",
  },
  {
    property: "PropF",
    what: "a void leaves the open total as it was",
    find: "!.totals[h.period].open = @ - h.amount],",
    replace: "!.totals[h.period].open = @],",
  },
  {
    property: "PropG",
    what: "a void does not look at the dispatch mark",
    find: 'IF h.mark THEN Unchanged(p, Refused("dispatched"))',
    replace: 'IF FALSE THEN Unchanged(p, Refused("dispatched"))',
  },
  {
    property: "PropH",
    what: "a step takes the time it is given, also when the stored time is later",
    find: "Now(ob, given) == Later(given, ob.lastNow)",
    replace: "Now(ob, given) == given",
  },
  {
    property: "PropACut",
    what: "a cut of the cap by a setCap of the same period type does not take",
    find: 'THEN [cap EXCEPT !.amount = mv.cap, !.next = "none", !.switchAt = NoTime]',
    replace: 'THEN [cap EXCEPT !.amount = IF mv.cap < cap.amount THEN cap.amount ELSE mv.cap, !.next = "none", !.switchAt = NoTime]',
  },
  {
    property: "PropERaise",
    what: "a raise of the cap by a setCap of the same period type does not take",
    find: 'THEN [cap EXCEPT !.amount = mv.cap, !.next = "none", !.switchAt = NoTime]',
    replace: 'THEN [cap EXCEPT !.amount = IF mv.cap > cap.amount THEN cap.amount ELSE mv.cap, !.next = "none", !.switchAt = NoTime]',
  },
  {
    property: "PropBEarly",
    what: "a hold ends one tick before its expiry",
    find: 'ob.holds[r].state = "open" /\\ ob.holds[r].expiresAt <= now}',
    replace: 'ob.holds[r].state = "open" /\\ ob.holds[r].expiresAt <= now + 1}',
  },
];
const countedSteps = (steps) => [
  "VARIABLE steps",
  "CountedInit == Init /\\ steps = 0",
  "CountedNext == Next /\\ steps' = steps + 1",
  `WithinTheSteps == steps <= ${steps}`,
  'WeakerSlot(r) == LET h == o.holds[r] IN <<h.state, h.amount, h.mark, h.cost, h.late, h.overrun, IF gw.r = r THEN gw.pc ELSE "idle", o.goes[r]>>',
  "WeakerView == <<TimeOfTheNextStep - clock, CapInView, RowInView, [slot \\in {WeakerSlot(r) : r \\in Ids} |-> Cardinality({r \\in Ids : WeakerSlot(r) = slot})], hist>>",
  "KeptView == <<View, steps>>",
  "KeptWeakerView == <<WeakerView, steps>>",
  `ViewIsPrinted == steps > ${steps} \\/ PrintT(ToString(<<"v", View, steps>>))`,
  `WeakerViewIsPrinted == steps > ${steps} \\/ PrintT(ToString(<<"w", WeakerView, steps>>))`,
];

function usage() {
  console.error(
    "usage: node formal/r7-ledger/controls.mjs [reach | break | view] [NAME...]\n       node formal/r7-ledger/controls.mjs all\n\nShows that the model reaches what it must reach, that each property can fail and that the view hides no state.\nreach: for each element, TLC must violate the invariant that says the model never reaches it.\nbreak: one change of a copy of the spec for each break, after which TLC must report the property that the break aims at.\nview: three searches of a copy of the spec that counts its steps, up to a number of steps: without a view, with the view, and with a weaker view that leaves out when a hold is due. The search with the view must keep exactly the views that the search without one reaches, and the weaker view must not.\nNAME limits the run to those elements or breaks, as in the two lists of this file; after view it is the number of steps.\nall: one search of formal/r7-ledger/CapLedgerAll.cfg, three request ids with every feature at once. It is no part of check 95 and of no other kind: it takes far longer, at most 90 minutes, and prints its count and TLC's time.\nEach run keeps its copy of the spec and TLC's output, with the trace, in a new directory under .tmp/.\nExit 0 when every control ends as expected, 1 when one does not, 64 on bad usage.",
  );
  process.exit(BAD_USAGE);
}

function constantsOf(config) {
  return config.split("\n").filter((each) => each.startsWith("CONSTANT ") && !each.startsWith("CONSTANT History "));
}

function control(kind, name, spec, config, expected) {
  const work = scratch(`model-control-${kind}-${name}`);
  writeFileSync(join(work, "CapLedger.tla"), spec);
  writeFileSync(join(work, "CapLedger.cfg"), `${config.join("\n")}\n`);
  const run = tlc(work, "control", ["-workers", WORKERS, "-config", "CapLedger.cfg", "CapLedger"]);
  const asExpected = run.violated.length === 1 && expected.includes(run.violated[0]);
  const found = run.violated.length > 0 ? `${run.violated.join(",")} violated` : run.complete ? "nothing violated" : `TLC exited ${run.status}`;
  console.log(`control ${kind} ${name}: ${found}${asExpected ? " as expected" : `, expected ${expected.join(" or ")}`}; output ${run.file}`);
  return asExpected;
}

function controlOfTheView(spec, constants, steps) {
  const end = spec.lastIndexOf("\n====");
  const counted = `${spec.slice(0, end)}\n${countedSteps(steps).join("\n")}${spec.slice(end)}`;
  const base = [...constants, "CONSTANT History = FALSE", "INIT CountedInit", "NEXT CountedNext", "CONSTRAINT WithinTheSteps"];
  const searches = {
    "without a view": [...base, "INVARIANT ViewIsPrinted", "INVARIANT WeakerViewIsPrinted"],
    "with the view": [...base, "VIEW KeptView", "INVARIANT ViewIsPrinted"],
    "with the weaker view": [...base, "VIEW KeptWeakerView", "INVARIANT WeakerViewIsPrinted"],
  };
  const found = {};
  for (const [name, config] of Object.entries(searches)) {
    const work = scratch(`model-control-view-${name.replaceAll(" ", "-")}`);
    writeFileSync(join(work, "CapLedger.tla"), counted);
    writeFileSync(join(work, "CapLedger.cfg"), `${config.join("\n")}\n`);
    const run = tlc(work, "control", ["-workers", WORKERS, "-config", "CapLedger.cfg", "CapLedger"]);
    if (!run.complete) {
      console.log(`control view: the search ${name} did not complete (exit ${run.status}); output ${run.file}`);
      return false;
    }
    const printed = (mark) => new Set(run.output.split(/\r?\n/).filter((each) => each.startsWith(`"<<\\"${mark}\\", `)));
    found[name] = { views: printed("v"), weaker: printed("w"), states: run.distinct, file: run.file };
  }
  const full = found["without a view"];
  const kept = found["with the view"].views;
  const weaker = found["with the weaker view"].weaker;
  const missing = [...full.views].filter((each) => !kept.has(each));
  const extra = [...kept].filter((each) => !full.views.has(each));
  const hidden = [...full.weaker].filter((each) => !weaker.has(each)).length;
  const same = missing.length === 0 && extra.length === 0 && kept.size > 0;
  console.log(
    `control view: within ${steps} steps the search without a view finds ${full.states} states with ${full.views.size} views, and the search with the view keeps ${kept.size}: ${same ? "the same views, as expected" : `${missing.length} are missing and ${extra.length} are not among them, expected the same views`}; output ${full.file}`,
  );
  for (const each of [...missing.slice(0, 3), ...extra.slice(0, 3)]) console.log(`control view: differs in ${each.slice(0, 400)}`);
  console.log(
    `control view: a weaker view that leaves out when a hold is due keeps ${weaker.size} of ${full.weaker.size} and misses ${hidden}: ${hidden > 0 ? "the comparison can fail, as expected" : "expected it to miss some, so this run does not show that the comparison can fail"}`,
  );
  return same && hidden > 0;
}

const [kind, ...names] = process.argv.slice(2);
if (kind !== undefined && kind !== "reach" && kind !== "break" && kind !== "view" && kind !== "all") usage();
if (kind === "all" && names.length > 0) usage();
if (kind === "view") {
  if (names.length > 1 || (names.length === 1 && !/^[1-9][0-9]?$/.test(names[0]))) usage();
} else {
  for (const name of names) if (!REACH.includes(name) && !BREAKS.some((each) => each.property === name)) usage();
}
const missing = missingTool();
if (missing !== null) {
  console.error(`controls: ${missing}`);
  process.exit(1);
}
if (kind === "all") {
  const work = scratch("model-all");
  for (const name of ["CapLedger.tla", "CapLedgerAll.cfg"]) copyFileSync(join(MODEL, name), join(work, name));
  const run = tlc(work, "all", ["-workers", WORKERS, "-config", "CapLedgerAll.cfg", "CapLedger"], LONGEST_SEARCH_OF_ALL_MS);
  const time = /^Finished in (.*) at /m.exec(run.output)?.[1] ?? "no time";
  const found = run.violated.length > 0 ? `${run.violated.join(",")} violated` : run.complete ? `no property violated in ${run.distinct} distinct states` : `TLC did not complete (exit ${run.status})`;
  console.log(`all: ${found} at the bounds of formal/r7-ledger/CapLedgerAll.cfg; TLC's time ${time}; output ${run.file}`);
  process.exit(run.complete ? 0 : 1);
}
const spec = readFileSync(join(MODEL, "CapLedger.tla"), "utf8");
const constants = constantsOf(readFileSync(join(MODEL, "CapLedgerGateway.cfg"), "utf8"));
const wanted = (name) => names.length === 0 || names.includes(name);
let held = true;
let ran = 0;
if (kind === undefined || kind === "reach") {
  for (const name of REACH.filter(wanted)) {
    ran += 1;
    const config = [...constants, "CONSTANT History = TRUE", "INIT Init", "NEXT Next", `INVARIANT No${name}`];
    if (!control("reach", name, spec, config, [`No${name}`])) held = false;
  }
}
if (kind === undefined || kind === "break") {
  for (const each of BREAKS.filter((one) => wanted(one.property))) {
    ran += 1;
    const at = spec.indexOf(each.find);
    if (at < 0 || spec.indexOf(each.find, at + 1) >= 0) {
      console.log(`control break ${each.property}: the text to change is not in formal/r7-ledger/CapLedger.tla exactly once: ${each.find}`);
      held = false;
      continue;
    }
    const broken = `${spec.slice(0, at)}${each.replace}${spec.slice(at + each.find.length)}`;
    const letter = each.property.charAt(4);
    const onEveryState = STATE_PARTS.includes(letter) ? [`INVARIANT State${letter}`] : [];
    const config = [...constants, "CONSTANT History = FALSE", "INIT Init", "NEXT Next", "VIEW View", ...onEveryState, `PROPERTY Step${letter}`];
    console.log(`control break ${each.property}: ${each.what}`);
    if (!control("break", each.property, broken, config, [`State${letter}`, `Step${letter}`])) held = false;
  }
}
if (kind === undefined || kind === "view") {
  ran += 1;
  if (!controlOfTheView(spec, constants, Number(kind === "view" && names.length === 1 ? names[0] : VIEW_STEPS))) held = false;
}
console.log(`controls: ${ran} run, ${held ? "all as expected" : "not all as expected"}`);
process.exit(held ? 0 : 1);
