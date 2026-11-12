import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { call, mustCall } from "./http.mjs";

const int = z.number().int();
export const Snapshot = z.object({
  now: int,
  account: z.object({ cap: int.nullable(), clock_ms: int, seq: int }),
  periods: z.array(z.object({ period_start: int, reported: int, unreported: int, open: int })),
  holds: z.array(
    z.object({
      request_id: z.string(),
      period_start: int,
      amount: int,
      expires_at: int,
      state: z.enum(["open", "settled", "voided", "expired"]),
      created_at: int,
      created_seq: int,
      ended_at: int.nullable(),
      ended_seq: int.nullable(),
      ended_by: z.string().nullable(),
      input_tokens: int.nullable(),
      output_tokens: int.nullable(),
      cost: int.nullable(),
      overrun: int,
      late_at: int.nullable(),
      late_seq: int.nullable(),
    }),
  ),
  alarm: int.nullable(),
  databaseSize: int.nullable(),
});

export const MICRO_PER_MINOR = 10_000;
export const WEEK_MS = 604_800_000;

export const spikeApi = {
  chatStatus: { refused: 402, voided: 503, upstreamFailed: 502 },
  ledger: { alarmMark: "alarm", lateSettleAddsOverrun: true },
  health: (port) => call(port, "GET", "/health"),
  async setCap(port, run, cap, anchor, periodMs) {
    await mustCall(port, "POST", `/cap?account=${run.account}`, JSON.stringify({ cap, period_ms: periodMs, anchor_ms: anchor }));
  },
  async snapshot(port, run) {
    return Snapshot.parse(JSON.parse(await mustCall(port, "GET", `/ledger?account=${run.account}`)));
  },
  chat: (run) => ({ path: `/v1/chat?account=${run.account}`, headers: {} }),
};

const HEALTH_ACCOUNT = "00000000-0000-4000-8000-000000000000";
const rowsOf = (tables, name) => tables[name] ?? [];

export function fromInspect(answer) {
  if (answer.result !== "snapshot") throw new Error(`POST /test/inspect answered ${JSON.stringify(answer).slice(0, 200)}`);
  const capRow = rowsOf(answer.tables, "ledger_cap")[0];
  const state = rowsOf(answer.tables, "ledger_state")[0];
  const skeleton = rowsOf(answer.tables, "skeleton")[0];
  return Snapshot.parse({
    now: answer.now,
    account: {
      cap: capRow === undefined ? null : capRow.amount * MICRO_PER_MINOR,
      clock_ms: skeleton?.last_now ?? 0,
      seq: state?.seq ?? 0,
    },
    periods: rowsOf(answer.tables, "ledger_periods").map((row) => ({
      period_start: row.period_start,
      reported: row.reported,
      unreported: row.unreported,
      open: row.open,
    })),
    holds: rowsOf(answer.tables, "ledger_holds").map((row) => ({ ...row })),
    alarm: answer.alarm,
    databaseSize: null,
  });
}

export const testEntryApi = {
  chatStatus: { refused: 403, voided: 503, upstreamFailed: 502 },
  ledger: { alarmMark: "expire", lateSettleAddsOverrun: false },
  health: (port) => call(port, "POST", "/test/inspect", JSON.stringify({ account: HEALTH_ACCOUNT })),
  async setCap(port, run, cap, anchor, periodMs) {
    if (periodMs !== WEEK_MS) throw new Error(`the test entry's cap period is a week, not ${periodMs} ms`);
    if (cap % MICRO_PER_MINOR !== 0) throw new Error(`the cap ${cap} is not a whole number of minor units`);
    const body = { account: run.account, credential: run.credential, subscription: "active", cap: { amount: cap / MICRO_PER_MINOR, period: "week" }, anchor };
    const text = await mustCall(port, "POST", "/test/seed", JSON.stringify(body));
    if (JSON.parse(text).result !== "seeded") throw new Error(`POST /test/seed answered ${text.slice(0, 200)}`);
  },
  async snapshot(port, run) {
    return fromInspect(JSON.parse(await mustCall(port, "POST", "/test/inspect", JSON.stringify({ account: run.account }))));
  },
  chat: (run) => ({ path: "/v1/chat", headers: { "halvo-account": run.account, authorization: `Bearer ${run.credential}` } }),
};

function envOf(root, tmp) {
  const env = Object.fromEntries(
    execFileSync("sh", [join(root, "scripts/with-env.sh"), "env"], { encoding: "utf8" })
      .split("\n")
      .filter((line) => line.includes("="))
      .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]),
  );
  env.XDG_CONFIG_HOME = env.HALVO_WRANGLER_CONFIG_HOME;
  mkdirSync(tmp, { recursive: true });
  env.TMPDIR = tmp;
  return env;
}

export function testEntryTarget(root, cfg, tmp) {
  const env = envOf(root, tmp);
  return {
    api: testEntryApi,
    command: ({ port, inspectorPort, stateDir, mockPort, holdLifeMs }) => ({
      file: join(root, "node_modules/.bin/wrangler"),
      args: [
        "dev",
        ...["--config", join(root, "wrangler.test.jsonc")],
        ...["--ip", "127.0.0.1", "--port", String(port), "--inspector-port", String(inspectorPort)],
        ...["--persist-to", stateDir, "--show-interactive-dev-session=false"],
        ...Object.entries({
          HOLD_LIFE_MS: holdLifeMs,
          UPSTREAM_DEADLINE_MS: cfg.deadlineMs,
          DISPATCH_MARGIN_MS: cfg.marginMs,
          UPSTREAM_BASE_URL: `http://127.0.0.1:${mockPort}`,
          PUBLIC_ORIGIN: `http://127.0.0.1:${port}`,
        }).flatMap(([name, value]) => ["--var", `${name}:${value}`]),
      ],
      env,
    }),
  };
}
