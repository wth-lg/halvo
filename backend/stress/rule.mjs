import { config, expectedCostOf } from "./config.mjs";

/** The hold life, upstream deadline and dispatch margin that the driver starts the gateway with for a cell. */
export const gatewayOf = (cfg, cell) => ({
  holdLifeMs: cell.kind === "void" ? cfg.voidHoldLifeMs : cfg.holdLifeMs,
  deadlineMs: cfg.deadlineMs,
  marginMs: cfg.marginMs,
});

export const CLAUSES = {
  "wave-admits-k": "a held wave of an attempt that is not slow has exactly K hold rows, each under an id that the driver sent",
  "refused-no-charge": "a refused request has no charge",
  "settled-exact":
    "a request whose final usage was delivered and whose settle was acknowledged is charged exactly its price times that usage",
  "voided-clean": "an admitted request the gateway voided has no charge and no entry in the mock's log",
  "others-full-hold": "every other admitted request is charged its full hold, marked unreported",
  "row-has-request": "no ledger row lacks a driver request",
  cap: "counted spend passes the cap by 0 outside the over-report cell and by at most the sum of overruns inside it",
  "no-open-after-expiry": "no hold is open after its expiry time",
  "receipt-has-hold": "no receipt lacks a stored hold after the kill",
  "no-late-settle": "no settle arrives after expiry in a run without the kill, read in attempts that are not slow",
  integrity: "the three records can be joined and the run ran as planned",
};

export const CLAUSES_OF_THE_DISPATCH_MARK = {
  "voided-at-expiry":
    "a hold that passes its expiry without the dispatch mark ends voided and is charged nothing, a hold with the mark never ends voided, and every hold whose request reached the upstream carries the mark",
};

const endedAtItsExpiry = (row) => row.ended_at !== null && row.ended_at >= row.expires_at;
const answeredAtOrAfterItsExpiry = (request, row) => request.headersAt !== null && request.headersAt >= row.expires_at;
const bodyOf = (request) => {
  try {
    return JSON.parse(request.body);
  } catch {
    return null;
  }
};
const sentMoreThanOnce = (request) => request.lost.length > 0;
const settlesOf = (row) => (row.state === "settled" ? 1 : 0) + (row.late_at !== null ? 1 : 0);
const fitsTheRow = (body, row) =>
  body.dispatched === (row.dispatched === 1) && (body.state === "open" ? body.dispatched === true : body.state === row.state);

function legal409(request, row, upstream, ledger) {
  const body = bodyOf(request);
  if (!row || body === null) return false;
  if (body.result === "stopped") {
    const voidedWithoutTheMark = ledger.voidsUnmarkedAtExpiry === true && row.state === "voided" && row.dispatched === 0;
    if (voidedWithoutTheMark && endedAtItsExpiry(row) && answeredAtOrAfterItsExpiry(request, row) && !upstream) return true;
    if (!sentMoreThanOnce(request)) return false;
    if (row.dispatched === 1) return (upstream?.calls ?? 0) <= 1 && settlesOf(row) <= 1;
    return row.state === "voided" && row.ended_at < row.expires_at && !upstream;
  }
  return body.result === "repeat" && sentMoreThanOnce(request) && fitsTheRow(body, row);
}
/** The value at the given share of the values by nearest rank (0.5 is the median), or null when there is none. */
export function nearestRank(values, share) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.max(0, Math.ceil(share * sorted.length) - 1)];
}

const FAULTS_THAT_FAIL_THE_UPSTREAM = new Set(["error-status", "reset"]);
const lifeOf = (request, gateway) =>
  request.receipt?.now === undefined ? gateway.holdLifeMs : request.receipt.expiresAt - request.receipt.now;
function voidCameInTime(request, gateway) {
  const bound = lifeOf(request, gateway) - gateway.deadlineMs - gateway.marginMs;
  return bound < 0 || (request.headersAt !== null && request.headersAt - request.sentAt > bound);
}
function legal502(request, gateway) {
  const body = bodyOf(request);
  if (body?.result === "no-time-left") return voidCameInTime(request, gateway);
  if (body?.result !== "upstream-failed") return false;
  if (FAULTS_THAT_FAIL_THE_UPSTREAM.has(request.fault)) return true;
  return body.why === "AbortError" && request.headersAt !== null && request.headersAt >= request.sentAt + lifeOf(request, gateway) - gateway.marginMs;
}
const endedByTime = (row, ledger) => row.state === "expired" || (ledger.voidsUnmarkedAtExpiry === true && row.state === "voided" && endedAtItsExpiry(row));

function readMock(events) {
  const byId = new Map();
  for (const event of events) {
    let entry = byId.get(event.id);
    if (!entry) {
      entry = { requestAt: null, endAt: null, clean: false, finalUsage: null, calls: 0 };
      byId.set(event.id, entry);
    }
    if (event.ev === "request") {
      entry.requestAt = event.t;
      entry.calls++;
    }
    if (event.ev === "usage" && event.final) {
      entry.finalUsage = { inputTokens: event.input_tokens, outputTokens: event.output_tokens };
    }
    if (event.ev === "end") {
      entry.endAt = event.t;
      entry.clean = event.clean;
    }
  }
  return byId;
}

const GATEWAY_STACK = /\.ts:\d+|Durable Object/;
const gatewayFailed = (attempt) => attempt.why === "500" && GATEWAY_STACK.test(attempt.body);
const PROXY_LOST_THE_REQUEST = "Network connection lost";
const lostInTransport = (request) =>
  request.status === 500 && typeof request.body === "string" && request.body.includes(PROXY_LOST_THE_REQUEST) && !GATEWAY_STACK.test(request.body);
const EXPIRY_OF_DUE_HOLDS = 0;
const OWN_WORK_OF_THE_STEP = 1;

export function judge({
  cell,
  cap,
  price,
  requests,
  mockEvents,
  final,
  afterRestart,
  kill,
  firstLedgerReadAt,
  restartReadyAt,
  serverErrors,
  anchor,
  periodMs,
  chatStatus,
  ledger,
  gateway = gatewayOf(config, cell),
}) {
  const violations = [];
  const bad = (clause, id, detail) => violations.push({ clause, id, detail });

  const sent = new Map(requests.filter((request) => request.sentAt !== null).map((request) => [request.id, request]));
  const rows = new Map(final.holds.map((row) => [row.request_id, row]));
  const mock = readMock(mockEvents);
  const burstWaits = [...sent.values()].filter((request) => request.index < cell.level && request.headersAt !== null).map((request) => request.headersAt - request.sentAt);
  const burstMedianMs = nearestRank(burstWaits, 0.5);
  const slowBoundMs = gateway.holdLifeMs - gateway.marginMs;
  const slow = burstMedianMs !== null && burstMedianMs > slowBoundMs;
  const readsArrival = kill === null && !slow;

  for (const id of rows.keys()) {
    if (!sent.has(id)) bad("row-has-request", id, "a ledger row has no driver request");
  }
  const lostHoldClause = kill ? "receipt-has-hold" : "integrity";
  for (const [id, upstream] of mock) {
    if (!sent.has(id)) bad("integrity", id, "the mock logged a request the driver did not send");
    else if (!rows.has(id)) bad(lostHoldClause, id, "the mock was called under this id and no hold is stored");
    if (upstream.calls > 1) bad("integrity", id, `the upstream was called ${upstream.calls} times under this id, where contract 10 allows one call`);
  }

  for (const request of sent.values()) {
    const row = rows.get(request.id);
    const diedInKill = kill !== null && request.sentAt <= kill.at && (request.endedAt ?? kill.at) >= kill.at - 5;
    switch (request.status) {
      case chatStatus.refused:
        if (row) bad("refused-no-charge", request.id, `refused, but the ledger stores a ${row.state} hold of ${row.amount}`);
        if (mock.has(request.id)) bad("refused-no-charge", request.id, "refused, but the mock was called");
        break;
      case chatStatus.voided:
        if (row?.state !== "voided") {
          bad("voided-clean", request.id, `the gateway answered with a void, the row is ${row?.state ?? "missing"}`);
        }
        if (!voidCameInTime(request, gateway)) bad("integrity", request.id, "the gateway voided before the time left on the receipt fell under the deadline and the margin");
        break;
      case 200:
        if (!row) bad("integrity", request.id, `answered ${request.status} with a receipt, but no row is stored`);
        break;
      case chatStatus.upstreamFailed:
        if (!row) bad("integrity", request.id, `answered ${request.status} with a receipt, but no row is stored`);
        if (!legal502(request, gateway) && !(diedInKill && bodyOf(request) === null)) bad("integrity", request.id, `answered ${request.status} ${request.body}, which neither the request's fault nor the bound of src/gateway/timing.ts allows at that time`);
        break;
      case 409:
        if (!legal409(request, row, mock.get(request.id), ledger) && !(diedInKill && bodyOf(request) === null)) {
          bad("integrity", request.id, `the request ended with status ${request.status} and error ${request.error}`);
        }
        break;
      default:
        if (!(diedInKill && bodyOf(request) === null) && !lostInTransport(request)) bad("integrity", request.id, `the request ended with status ${request.status} and error ${request.error}`);
    }
    for (const attempt of request.lost) {
      if (gatewayFailed(attempt)) {
        bad("integrity", request.id, `the gateway failed: ${attempt.body.split("\n")[0].slice(0, 160)}`);
      }
    }
    if (request.trailer?.settled && request.trailer.outcome === "settled" && row?.state !== "settled") {
      bad("settled-exact", request.id, `the client saw the settle acknowledged, the row is ${row?.state ?? "missing"}`);
    }
    if (request.status === 200 && request.abortedAt === null && !request.trailer && !diedInKill) {
      bad("integrity", request.id, "the stream ended with no ledger event");
    }
    if (readsArrival && request.trailer?.settled && request.trailer.outcome === "expired") {
      bad("no-late-settle", request.id, "the ledger answered the gateway's settle as expired: it arrived after the hold's expiry");
    }
  }

  for (const line of serverErrors ?? []) bad("integrity", null, `the server logged an error: ${line}`);

  let settled = 0;
  let expired = 0;
  let voided = 0;
  let voidedAtExpiry = 0;
  let lateSettles = 0;
  let endedByAlarm = 0;
  const servedNeverSettled = { holds: 0, charged: 0, usageWorth: 0 };
  let worstExpiryLagMs = 0;
  for (const row of rows.values()) {
    const request = sent.get(row.request_id);
    const upstream = mock.get(row.request_id);
    if (request && row.amount !== request.hold) {
      const clause = row.state === "expired" ? "others-full-hold" : "integrity";
      bad(clause, row.request_id, `the hold is ${row.amount}, the upper bound for this request is ${request.hold}`);
    }
    if (ledger.voidsUnmarkedAtExpiry === true && upstream && row.dispatched !== 1) {
      bad("voided-at-expiry", row.request_id, "the upstream was called for this request, and its hold carries no dispatch mark");
    }
    const chargeFromMock = upstream?.finalUsage
      ? expectedCostOf(price, upstream.finalUsage.inputTokens, upstream.finalUsage.outputTokens)
      : null;
    switch (row.state) {
      case "settled":
        settled++;
        if (row.ended_at >= row.expires_at) {
          bad("no-open-after-expiry", row.request_id, `settled ${row.ended_at - row.expires_at} ms after its expiry`);
        }
        if (chargeFromMock === null) {
          bad("settled-exact", row.request_id, "settled, but the mock delivered no final usage");
        } else if (row.cost !== chargeFromMock) {
          bad("settled-exact", row.request_id, `charged ${row.cost}, price times the final usage is ${chargeFromMock}`);
        } else if (row.overrun !== Math.max(0, chargeFromMock - row.amount)) {
          bad("settled-exact", row.request_id, `the overrun is ${row.overrun}, the cost is ${chargeFromMock} on a hold of ${row.amount}`);
        }
        break;
      case "voided":
        if (endedByTime(row, ledger)) {
          voidedAtExpiry++;
          worstExpiryLagMs = Math.max(worstExpiryLagMs, row.ended_at - row.expires_at);
        } else {
          voided++;
          if (row.ended_at >= row.expires_at) {
            bad("no-open-after-expiry", row.request_id, `voided ${row.ended_at - row.expires_at} ms after its expiry`);
          }
        }
        if (ledger.voidsUnmarkedAtExpiry === true && row.dispatched !== 0) {
          bad("voided-at-expiry", row.request_id, "voided with the dispatch mark: it may have had an upstream call");
        }
        if (row.cost !== null || row.overrun !== 0) bad("voided-clean", row.request_id, "a voided hold carries a charge");
        if (upstream) bad("voided-clean", row.request_id, "voided, but the mock's log has the request");
        break;
      case "expired":
        expired++;
        if (row.ended_at < row.expires_at) {
          bad("others-full-hold", row.request_id, `expired ${row.expires_at - row.ended_at} ms before its expiry time`);
        }
        if (ledger.voidsUnmarkedAtExpiry === true && row.dispatched !== 1) {
          bad("voided-at-expiry", row.request_id, `expired at its amount of ${row.amount} without the dispatch mark: it had no upstream call`);
        }
        if (row.ended_by === ledger.alarmMark) endedByAlarm++;
        if (chargeFromMock !== null && row.late_at === null) {
          servedNeverSettled.holds++;
          servedNeverSettled.charged += row.amount;
          servedNeverSettled.usageWorth += chargeFromMock;
        }
        worstExpiryLagMs = Math.max(worstExpiryLagMs, row.ended_at - row.expires_at);
        if (row.late_at !== null) {
          lateSettles++;
          if (readsArrival) {
            bad("no-late-settle", row.request_id, `a settle arrived ${row.late_at - row.expires_at} ms after the expiry`);
          }
          if (row.cost !== chargeFromMock) {
            bad("settled-exact", row.request_id, `the late settle stored ${row.cost}, price times the final usage is ${chargeFromMock}`);
          } else if (row.overrun !== Math.max(0, row.cost - row.amount)) {
            bad("settled-exact", row.request_id, `the late settle's overrun is ${row.overrun}, the cost is ${row.cost} on a hold of ${row.amount}`);
          }
        }
        break;
      default:
        if (row.expires_at <= final.now) {
          bad("no-open-after-expiry", row.request_id, `open ${final.now - row.expires_at} ms after its expiry at the last read`);
        } else {
          bad("integrity", row.request_id, "open and not yet due at the last read: the driver read too early");
        }
    }
  }
  let held = null;
  let streams = null;
  if (cell.kind === "wave") {
    streams = [...sent.values()].filter((request) => request.status === 200).length;
    const reserveAnsweredAt = [...sent.values()].flatMap((request) => {
      if (request.receipt) return [request.receipt.now ?? request.headersAt];
      return request.status === chatStatus.refused && request.headersAt !== null ? [request.headersAt] : [];
    });
    const lastReserveAnsweredAt = reserveAnsweredAt.length === 0 ? null : Math.max(...reserveAnsweredAt);
    held = lastReserveAnsweredAt !== null && [...rows.values()].every((row) => row.ended_at === null || row.ended_at >= lastReserveAnsweredAt);
    const unsent = [...rows.keys()].filter((id) => !sent.has(id)).length;
    if (held && !slow && (rows.size !== cell.k || unsent > 0)) {
      bad("wave-admits-k", null, `the held wave has ${rows.size} hold rows, ${unsent} of them under an id the driver did not send; K is ${cell.k}`);
    }
  }

  for (const row of rows.values()) {
    const admitted = anchor + Math.floor((row.created_at - anchor) / periodMs) * periodMs;
    if (row.period_start !== admitted) {
      bad("cap", row.request_id, `counted in the period from ${row.period_start}, admitted in the period from ${admitted}`);
    }
  }

  const events = [];
  for (const row of rows.values()) {
    events.push({ seq: row.created_seq, rankWithinStep: OWN_WORK_OF_THE_STEP, kind: "reserve", row });
    if (row.ended_seq !== null) {
      events.push({ seq: row.ended_seq, rankWithinStep: endedByTime(row, ledger) ? EXPIRY_OF_DUE_HOLDS : OWN_WORK_OF_THE_STEP, kind: row.state, row });
    }
    if (row.late_seq !== null) events.push({ seq: row.late_seq, rankWithinStep: OWN_WORK_OF_THE_STEP, kind: "late", row });
  }
  events.sort((a, b) => a.seq - b.seq || a.rankWithinStep - b.rankWithinStep);
  const periods = new Map();
  const open = new Set();
  let crossed = false;
  let lastTime = 0;
  for (const { kind, row } of events) {
    const time = kind === "reserve" ? row.created_at : kind === "late" ? row.late_at : row.ended_at;
    if (time < lastTime) bad("integrity", row.request_id, "the ledger's step order and its clock disagree");
    lastTime = Math.max(lastTime, time);
    let period = periods.get(row.period_start);
    if (!period) {
      period = { open: 0, reported: 0, unreported: 0, overruns: 0, worst: Number.NEGATIVE_INFINITY, worstNet: Number.NEGATIVE_INFINITY };
      periods.set(row.period_start, period);
    }
    switch (kind) {
      case "reserve":
        for (const other of open) {
          if (other.expires_at <= row.created_at) {
            bad("no-open-after-expiry", other.request_id, `open at a reserve ${row.created_at - other.expires_at} ms after its expiry`);
          }
          if (other.period_start < row.period_start) crossed = true;
        }
        period.open += row.amount;
        open.add(row);
        break;
      case "settled":
        period.open -= row.amount;
        period.reported += row.cost;
        period.overruns += row.overrun;
        open.delete(row);
        break;
      case "voided":
        period.open -= row.amount;
        open.delete(row);
        break;
      case "expired":
        period.open -= row.amount;
        period.unreported += row.amount;
        open.delete(row);
        break;
      default:
        if (ledger.lateSettleAddsOverrun) {
          period.reported += row.overrun;
          period.overruns += row.overrun;
        }
    }
    period.worst = Math.max(period.worst, period.open + period.reported + period.unreported - cap);
    period.worstNet = Math.max(period.worstNet, period.open + period.reported + period.unreported - cap - period.overruns);
  }
  let worstOver = Number.NEGATIVE_INFINITY;
  let overruns = 0;
  for (const [start, period] of periods) {
    worstOver = Math.max(worstOver, period.worst);
    overruns += period.overruns;
    const over = cell.switch === "over-report" ? period.worstNet : period.worst;
    if (over > 0) {
      bad("cap", null, `counted spend passed the cap by ${period.worst} in the period from ${start}, ${over} more than is allowed`);
    }
    const stored = final.periods.find((candidate) => candidate.period_start === start);
    if (!stored || stored.open !== period.open || stored.reported !== period.reported) {
      bad("integrity", null, `the totals of the period from ${start} are not the sums of its rows`);
    } else if (stored.unreported !== period.unreported) {
      bad("others-full-hold", null, `unreported is ${stored.unreported} in the period from ${start}, the expired holds sum to ${period.unreported}`);
    }
  }
  for (const stored of final.periods) {
    if (!periods.has(stored.period_start) && stored.open + stored.reported + stored.unreported !== 0) {
      bad("integrity", null, `the period from ${stored.period_start} has totals and no row`);
    }
  }

  const openAtKill = kill
    ? final.holds.filter((row) => row.created_at <= kill.at && (row.ended_at === null || row.ended_at > kill.at))
    : [];
  let streamAtKill = false;
  if (kill) {
    const stored = new Map((afterRestart ?? final).holds.map((row) => [row.request_id, row]));
    for (const request of sent.values()) {
      if (!request.receipt || request.headersAt === null || request.headersAt > kill.at) continue;
      const row = stored.get(request.id);
      if (!row || row.amount !== request.receipt.amount || row.expires_at !== request.receipt.expiresAt) {
        bad("receipt-has-hold", request.id, `a receipt was held before the kill; after it the stored hold is ${row ? "another" : "missing"}`);
      }
    }
    for (const entry of mock.values()) {
      if (entry.requestAt !== null && entry.requestAt <= kill.at && (entry.endAt === null || entry.endAt >= kill.at)) {
        streamAtKill = true;
      }
    }
    streamAtKill = streamAtKill && kill.inFlight > 0;
    if (cell.kind === "quiet") {
      for (const row of openAtKill) {
        if (!endedByTime(row, ledger) || row.ended_by !== ledger.alarmMark || row.ended_at >= firstLedgerReadAt) {
          bad("no-open-after-expiry", row.request_id, `not ended by the alarm before the first request after the restart (${row.state}, ended by ${row.ended_by})`);
        }
      }
    }
  }

  const overdue = kill ? openAtKill.filter((row) => row.expires_at <= restartReadyAt) : [];
  const firstAdmittedAt = rows.size === 0 ? null : Math.min(...[...rows.values()].map((row) => row.created_at));
  const firstAdmissionMs = cell.kind !== "rollover" || firstAdmittedAt === null ? null : firstAdmittedAt - (anchor + periodMs);
  const crossingBuilt = cell.kind === "rollover" ? firstAdmissionMs !== null && firstAdmissionMs < 0 : null;

  let counted = true;
  if (kill) counted = openAtKill.length > 0 && streamAtKill;
  if (cell.kind === "rollover") counted = counted && crossed;
  if (cell.kind === "void") counted = rows.size > 0 && voided === rows.size;
  if (cell.kind === "wave") counted = counted && held;
  counted = counted && !slow;

  let latestStreamEndPastExpiryMs = null;
  const streamsEndOnTheirOwn = !kill;
  if (streamsEndOnTheirOwn) {
    for (const [id, entry] of mock) {
      const row = rows.get(id);
      if (!row || entry.endAt === null) continue;
      latestStreamEndPastExpiryMs = Math.max(latestStreamEndPastExpiryMs ?? Number.NEGATIVE_INFINITY, entry.endAt - row.expires_at);
    }
  }
  const cutByGateway = [...sent.values()].filter(
    (request) => request.fault === "slow" && request.trailer?.settled === false && request.trailer.why.startsWith("the stream was cut"),
  ).length;

  const answers = (status) => [...sent.values()].filter((request) => request.status === status).length;
  const charged = (behavior) => {
    const mine = [...sent.values()].filter((request) => request.behavior === behavior && request.abortedAt !== null && rows.has(request.id));
    const state = (name) => mine.filter((request) => rows.get(request.id).state === name).length;
    const free = mine.filter((request) => {
      const row = rows.get(request.id);
      return row.state === "voided" || (row.state === "settled" && row.cost === 0);
    }).length;
    return { admitted: mine.length, settled: state("settled"), fullHold: state("expired"), free };
  };
  return {
    violations,
    counted,
    facts: {
      requests: sent.size,
      resets: [...sent.values()].reduce((total, request) => total + request.lost.filter((attempt) => attempt.why === "reset").length, 0),
      proxyLost: [...sent.values()].reduce((total, request) => total + request.lost.filter((attempt) => attempt.why === "500").length, 0),
      lostInTransport: [...sent.values()].filter(lostInTransport).length,
      slow,
      burstWaits,
      burstMedianMs,
      slowBoundMs,
      held,
      streams,
      admitted: rows.size,
      settled,
      expired,
      voided,
      voidedAtExpiry,
      refused: answers(chatStatus.refused),
      worstOver: Number.isFinite(worstOver) ? worstOver : null,
      overruns,
      periods: periods.size,
      lateSettles,
      endedByAlarm,
      servedNeverSettled,
      worstExpiryLagMs,
      openAtKill: openAtKill.length,
      latestStreamEndPastExpiryMs,
      cutByGateway,
      overdueAtRestart: overdue.length,
      overdueByAlarm: overdue.filter((row) => endedByTime(row, ledger) && row.ended_by === ledger.alarmMark).length,
      overdueEndedAfterReadyMs: overdue.length > 0 ? Math.max(...overdue.map((row) => (row.ended_at ?? final.now) - restartReadyAt)) : null,
      settledBeforeKill: kill ? final.holds.filter((row) => row.state === "settled" && row.ended_at <= kill.at).length : 0,
      admittedAfterRestart: kill ? final.holds.filter((row) => row.created_at > kill.at).length : 0,
      crossed: cell.kind === "rollover" ? crossed : null,
      crossingBuilt,
      firstAdmissionMs,
      streamAtKill,
      settleMs: [...sent.values()].filter((request) => request.trailer?.settled).map((request) => request.trailer.settle_ms),
      databaseSize: final.databaseSize,
      abortedBeforeUsage: charged("abort-before-usage"),
      abortedMid: charged("abort-mid"),
    },
  };
}
