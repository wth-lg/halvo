import { readFileSync } from "node:fs";
import { isDeepStrictEqual } from "node:util";
import { costOf } from "../../src/ledger/cost.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { WEEK_MS } from "../../src/ledger/periods.ts";
import { LOW, seededRig } from "./harness.ts";

const BAD_USAGE = 64;
const MICRO_OF_ONE_MODEL_UNIT = 10_000;
const TOKENS_OF_ONE_MODEL_UNIT = { inputTokens: 3084, outputTokens: 28 };
const FIRST_OF_FEBRUARY_2031 = Date.UTC(2031, 1, 1);
const FEBRUARY_MS = 28 * 86_400_000;
const CALL_OF = {
  reserve: (line) => ({ kind: "reserve", requestId: line.r, amount: micro(line.amt) }),
  dispatch: (line) => ({ kind: "dispatch", requestId: line.r }),
  settle: (line) => ({ kind: "settle", requestId: line.r, usage: usageOf(line.cost) }),
  void: (line) => ({ kind: "void", requestId: line.r }),
  expire: () => ({ kind: "alarm" }),
  setcap: (line) => ({ kind: "capSet", cap: { amount: line.cap, period: line.type } }),
  capread: () => ({ kind: "capGet" }),
  restart: () => ({ kind: "restart" }),
};
const ENDED_NOW = (line, before, test) => Object.entries(line.s.holds).some(([id, hold]) => test(hold) && before.holds[id]?.state === "open");
const ENDS_A_HOLD_BY_TIME = (line, before) => ENDED_NOW(line, before, (hold) => hold.state === "expired" || hold.voidAtExpiry);
const ELEMENTS = {
  admitted: (line) => line.k === "reserve" && line.ans.res === "admitted",
  "reserve-repeated": (line) => line.k === "reserve" && line.ans.res === "repeat",
  "reserve-with-other-arguments": (line) => line.ans.reason === "other-arguments",
  "refused-not-active": (line) => line.ans.reason === "not-active",
  "refused-no-cap": (line) => line.ans.reason === "no-cap",
  "refused-over-cap": (line) => line.ans.reason === "over-cap",
  dispatched: (line) => line.k === "dispatch" && line.ans.res === "go",
  "dispatch-repeated": (line) => line.k === "dispatch" && line.ans.res === "stop" && line.s.holds[line.r]?.mark === true,
  settled: (line) => line.k === "settle" && line.ans.res === "settled" && !line.ans.repeat,
  "settle-repeated": (line) => line.k === "settle" && line.ans.res === "settled" && line.ans.repeat,
  overrun: (line) => line.k === "settle" && line.ans.res === "settled" && !line.ans.repeat && line.ans.overrun > 0,
  "settle-after-expiry": (line) => line.k === "settle" && line.ans.res === "expired",
  "settle-before-its-reserve": (line) => line.k === "settle" && line.ans.reason === "unknown-request",
  voided: (line, was) => line.k === "void" && line.ans.res === "voided" && was.state.holds[line.r]?.state === "open",
  "void-repeated": (line, was) => line.k === "void" && line.ans.res === "voided" && was.state.holds[line.r]?.state === "voided",
  "void-refused-for-a-dispatched-hold": (line) => line.k === "void" && line.ans.reason === "dispatched",
  "call-after-the-end": (line) => line.ans.reason === "already-ended",
  "expiry-by-the-alarm": (line, was) => line.k === "expire" && ENDS_A_HOLD_BY_TIME(line, was.state),
  "expiry-inside-another-call": (line, was) => line.k in CALL_OF && line.k !== "expire" && ENDS_A_HOLD_BY_TIME(line, was.state),
  "expiry-by-the-test-clock": (line, was) => line.k === "clock" && ENDS_A_HOLD_BY_TIME(line, was.state),
  "restart-with-a-dispatched-hold-open": (line) =>
    line.k === "restart" && Object.values(line.s.holds).some((hold) => hold.state === "open" && hold.mark),
  "gateway-stopped-after-dispatch": (line, was) => line.k === "stop" && was.gateway === "upstream",
  "cap-below-counted-spend-with-a-hold-open": (line, was) => {
    if (line.k !== "setcap" || line.s.cap === null) return false;
    const row = line.s.totals[String(periodStartOf(line.s, was.lengths))];
    return row !== undefined && row.open > 0 && row.reported + row.unreported + row.open > line.s.cap.amount;
  },
  "period-type-switched": (line, was) => line.s.cap !== null && was.state.cap !== null && line.s.cap.anchor > was.state.cap.anchor,
  "period-boundary-with-a-hold-open": (line, was) =>
    line.k in CALL_OF &&
    line.s.cap !== null &&
    Object.values(line.s.holds).some((hold) => hold.state === "open" && hold.period < periodStartOf(line.s, was.lengths)),
  "clock-behind-the-stored-time": (line) => line.k === "back" && line.at < line.s.lastNow,
  "voided-at-expiry-without-the-mark": (line, was) => ENDED_NOW(line, was.state, (hold) => hold.voidAtExpiry),
  "expired-with-the-mark": (line, was) => ENDED_NOW(line, was.state, (hold) => hold.state === "expired" && hold.mark),
  "call-after-a-void-at-expiry": (line) =>
    (line.k === "settle" || line.k === "void") && line.ans.reason === "already-ended" && line.s.holds[line.r]?.voidAtExpiry === true,
};

function periodStartOf(state, lengths) {
  const length = lengths[state.cap.type];
  return state.lastNow - ((((state.lastNow - state.cap.anchor) % length) + length) % length);
}

function micro(units) {
  return units * MICRO_OF_ONE_MODEL_UNIT;
}

function usageOf(units) {
  return { inputTokens: TOKENS_OF_ONE_MODEL_UNIT.inputTokens * units, outputTokens: TOKENS_OF_ONE_MODEL_UNIT.outputTokens * units };
}

function monthAfter(at) {
  const date = new Date(at);
  return Date.UTC(
    date.getUTCFullYear(),
    date.getUTCMonth() + 1,
    date.getUTCDate(),
    date.getUTCHours(),
    date.getUTCMinutes(),
    date.getUTCSeconds(),
    date.getUTCMilliseconds(),
  );
}

function timesOf(header) {
  const tickMs = WEEK_MS / header.weekLen;
  if (!Number.isSafeInteger(tickMs) || header.monthLen * tickMs !== FEBRUARY_MS) {
    throw new RangeError(
      `the traces count ${header.weekLen} ticks to a week and ${header.monthLen} to a month; the replay needs a month of 28 days, February 2031`,
    );
  }
  const real = (tick) => FIRST_OF_FEBRUARY_2031 + tick * tickMs;
  const orNull = (tick) => (tick === null ? null : real(tick));
  const endOf = (start, length) => (length === header.weekLen ? real(start) + WEEK_MS : monthAfter(real(start)));
  return { tickMs, real, orNull, endOf, lengths: { week: header.weekLen, month: header.monthLen } };
}

function expectedTables(state, times) {
  return {
    cap:
      state.cap === null
        ? []
        : [
            {
              amount: state.cap.amount,
              period: state.cap.type,
              anchor: times.real(state.cap.anchor),
              next_period: state.cap.next,
              switch_at: times.orNull(state.cap.switchAt),
            },
          ],
    periods: Object.entries(state.totals)
      .map(([start, row]) => ({
        period_start: times.real(Number(start)),
        period_end: times.endOf(Number(start), row.end - Number(start)),
        reported: micro(row.reported),
        unreported: micro(row.unreported),
        open: micro(row.open),
      }))
      .sort((a, b) => a.period_start - b.period_start),
    holds: Object.entries(state.holds)
      .map(([id, hold]) => ({
        request_id: id,
        state: hold.state,
        amount: micro(hold.amount),
        period_start: times.real(hold.period),
        expires_at: times.real(hold.expiresAt),
        dispatched: hold.mark ? 1 : 0,
        cost: hold.cost === null ? null : micro(hold.cost),
        overrun: micro(hold.overrun),
        settled_late: hold.late,
        voided_at_expiry: hold.voidAtExpiry,
        tier: "low_cost_hosted",
        input_price: LOW.input,
        output_price: LOW.output,
        sync_interval: 0,
      }))
      .sort((a, b) => a.request_id.localeCompare(b.request_id)),
    due: times.orNull(state.alarm),
  };
}

function expectedIntervalTotals(state) {
  const holds = Object.values(state.holds);
  const sumOf = (ended, part) => holds.filter((hold) => hold.state === ended).reduce((sum, hold) => sum + micro(hold[part]), 0);
  return { reported: sumOf("settled", "cost"), unreported: sumOf("expired", "amount"), open: holds.filter((hold) => hold.state === "open").length };
}

function actualTables(read, snapshot) {
  return {
    cap: read.cap.map((row) => ({
      amount: row.amount,
      period: row.period,
      anchor: row.anchor,
      next_period: row.next_period,
      switch_at: row.switch_at,
    })),
    periods: read.periods
      .map((row) => ({
        period_start: row.period_start,
        period_end: row.period_end,
        reported: row.reported,
        unreported: row.unreported,
        open: row.open,
      }))
      .sort((a, b) => a.period_start - b.period_start),
    holds: read.holds
      .map((row) => ({
        request_id: row.request_id,
        state: row.state,
        amount: row.amount,
        period_start: row.period_start,
        expires_at: row.expires_at,
        dispatched: row.dispatched,
        cost: row.cost,
        overrun: row.overrun,
        settled_late: row.late_at !== null,
        voided_at_expiry: row.state === "voided" && row.ended_at >= row.expires_at,
        tier: row.tier,
        input_price: row.input_price,
        output_price: row.output_price,
        sync_interval: row.sync_interval,
      }))
      .sort((a, b) => a.request_id.localeCompare(b.request_id)),
    due: snapshot.due,
  };
}

function expectedAnswer(line, times) {
  const answer = line.ans;
  const refused = { outcome: "refused", reason: answer.reason };
  const receipt = { requestId: line.r, amount: micro(answer.amount), now: times.real(line.s.lastNow), expiresAt: times.orNull(answer.expiresAt) };
  switch (line.k) {
    case "reserve":
      if (answer.res === "admitted") return { outcome: "admitted", receipt };
      if (answer.res === "repeat") return { outcome: "repeat", state: answer.state, dispatched: answer.mark, receipt };
      return refused;
    case "dispatch":
      return answer.res;
    case "settle":
      if (answer.res === "settled") return { outcome: "settled", cost: micro(answer.cost), overrun: micro(answer.overrun), repeat: answer.repeat };
      if (answer.res === "expired") return { outcome: "expired", cost: micro(answer.cost), overrun: micro(answer.overrun) };
      return refused;
    case "void":
      return answer.res === "voided" ? { outcome: "voided" } : refused;
    case "setcap":
    case "capread": {
      if (answer.res === "no-cap") return { result: "no-cap" };
      const length = times.lengths[line.s.cap.type];
      return {
        result: "cap",
        cap: { amount: answer.amount, period: answer.type },
        spend: { spent: micro(answer.spent), held: micro(answer.held), period_end: times.endOf(answer.periodEnd - length, length) },
      };
    }
    default:
      return null;
  }
}

function firstDifference(expected, actual, path = "") {
  if (isDeepStrictEqual(expected, actual)) return null;
  const bothObjects = expected !== null && actual !== null && typeof expected === "object" && typeof actual === "object";
  if (bothObjects) {
    for (const key of new Set([...Object.keys(expected), ...Object.keys(actual)])) {
      const found = firstDifference(expected[key], actual[key], path === "" ? key : `${path}.${key}`);
      if (found !== null) return found;
    }
  }
  return `${path}: the model has ${JSON.stringify(expected)} and the object has ${JSON.stringify(actual)}`;
}

async function replayOne(lines, times, life, counts, seen) {
  const first = lines[0];
  const rig = await seededRig({
    at: FIRST_OF_FEBRUARY_2031,
    subscription: first.active ? "active" : "cancelled",
    holdLifeMs: life * times.tickMs,
  });
  const ledger = makeLedger({ holdLifeMs: life * times.tickMs });
  try {
    const onTheTestClock = first.path === "test";
    if (onTheTestClock) {
      const set = await rig.entries().testSetClock(FIRST_OF_FEBRUARY_2031);
      if (set.result !== "clock-set") return `trace ${first.n}: the first call of the clock path answered ${set.result}`;
    }
    let before = { cap: null, holds: {}, totals: {}, alarm: null, lastNow: 0 };
    let gatewayBefore = "idle";
    for (const line of lines) {
      const where = `trace ${line.n} action ${line.i} (${line.k}${line.r === null ? "" : ` ${line.r}`} at tick ${line.at}, ${line.path} clock)`;
      counts.actions += 1;
      let differs = null;
      if (line.k in CALL_OF) {
        counts.calls += 1;
        const got = await rig.run({ at: onTheTestClock ? FIRST_OF_FEBRUARY_2031 : times.real(line.at), call: CALL_OF[line.k](line) });
        const want = expectedAnswer(line, times);
        if (want !== null) differs = firstDifference({ answer: want }, { answer: got });
      } else if (line.k === "back") {
        counts.calls += 1;
        await rig.run({ at: onTheTestClock ? FIRST_OF_FEBRUARY_2031 : times.real(line.s.lastNow), call: { kind: "restart" } });
        if (onTheTestClock) {
          const set = await rig.entries().testSetClock(times.real(line.at));
          differs = firstDifference({ clock: "behind" }, { clock: set.result });
        }
      } else if (onTheTestClock && line.k === "clock") {
        counts.calls += 1;
        const set = await rig.entries().testSetClock(times.real(line.at));
        differs = firstDifference({ clock: "clock-set" }, { clock: set.result });
      }
      if (differs !== null) return `${where}: ${differs}`;
      const snapshot = await rig.entries().testInspect();
      if (snapshot.result !== "snapshot") return `${where}: the inspect call answered ${snapshot.result}`;
      differs = firstDifference(expectedTables(line.s, times), actualTables(rig.read(), snapshot));
      if (differs !== null) return `${where}: ${differs}`;
      differs = firstDifference({ intervalTotals: expectedIntervalTotals(line.s) }, { intervalTotals: rig.sql((sql) => ledger.intervalTotals(sql, 0)) });
      if (differs !== null) return `${where}: ${differs}`;
      const alarm = onTheTestClock ? null : times.orNull(line.s.alarm);
      if (rig.alarmAt() !== alarm) return `${where}: alarm: the model has ${alarm} and the platform alarm is ${rig.alarmAt()}`;
      for (const [name, holds] of Object.entries(ELEMENTS)) if (holds(line, { state: before, gateway: gatewayBefore, lengths: times.lengths })) seen.add(name);
      before = line.s;
      gatewayBefore = line.gw;
    }
    return null;
  } finally {
    rig.close();
  }
}

async function main(path) {
  const records = readFileSync(path, "utf8")
    .split("\n")
    .filter((text) => text !== "")
    .map((text) => JSON.parse(text));
  const header = records[0];
  if (header === undefined || header.header !== true) throw new RangeError("the first line of the traces is not their header");
  if (costOf(LOW, usageOf(1)) !== MICRO_OF_ONE_MODEL_UNIT) throw new RangeError("the tokens of one model unit no longer cost 10,000 micro-units at the low price");
  const times = timesOf(header);
  const traces = new Map();
  for (const line of records.slice(1)) {
    if (!traces.has(line.n)) traces.set(line.n, []);
    traces.get(line.n).push(line);
  }
  const counts = { actions: 0, calls: 0 };
  const seen = new Set();
  const differences = [];
  for (const lines of traces.values()) {
    const difference = await replayOne(lines, times, header.life, counts, seen);
    if (difference !== null) differences.push(difference);
  }
  const missing = Object.keys(ELEMENTS).filter((name) => !seen.has(name));
  console.log(
    `replay: seed=${header.seed} traces=${traces.size} actions=${counts.actions} calls=${counts.calls} differences=${differences.length} elements=${seen.size} of ${Object.keys(ELEMENTS).length}`,
  );
  for (const difference of differences.slice(0, 5)) console.log(`difference: ${difference}`);
  if (missing.length > 0) console.log(`missing: ${missing.join(" ")}`);
  return differences.length === 0 ? 0 : 1;
}

const [path, ...more] = process.argv.slice(2);
if (path === undefined || more.length > 0) {
  console.error("usage: node test/ledger/model-replay.mjs TRACES\n\nTRACES is the file of JSON lines that TLC prints from formal/r7-ledger/CapLedgerTrace.tla: a header, then one line for each action.\nExit 0 when the object follows every trace, 1 when it differs, 64 on bad usage or a file that is no such trace.");
  process.exit(BAD_USAGE);
}
try {
  process.exit(await main(path));
} catch (error) {
  if (!(error instanceof RangeError) && !(error instanceof SyntaxError) && error?.code !== "ENOENT") throw error;
  console.error(`replay: ${error.message}`);
  process.exit(BAD_USAGE);
}
