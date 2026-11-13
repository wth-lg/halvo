import { DurableObject } from "cloudflare:workers";
import { makeAccounts } from "../accounts/index.ts";
import { providerPort } from "../billing/index.ts";
import type { ProviderEvent, SubscriptionState } from "../billing/port.ts";
import type { Price, Usage } from "../ledger/index.ts";
import { makeLedger } from "../ledger/index.ts";
import { errorName, log, numberVariable } from "../log.ts";
import { makeMetering } from "../metering/index.ts";
import type { Cap, Env, Micro, Ms, Step, Tier } from "../types.ts";
import { type Entries, type Host, makeEntries } from "./entries.ts";

function semicolonRefused(): never {
  const error = new Error("one call of exec holds one statement and no semicolon");
  error.name = "SqlSemicolonError";
  throw error;
}

function alarmCallRejected(caught: unknown): void {
  log({ event: "alarm-error", error: errorName(caught) });
}

export function stepOf(storage: DurableObjectStorage): Step {
  return (run) =>
    storage.transactionSync(() =>
      run({
        exec(query, ...bindings) {
          if (query.includes(";")) semicolonRefused();
          return storage.sql.exec(query, ...bindings).toArray() as Record<string, string | number | null>[];
        },
      }),
    );
}

export class AccountObject extends DurableObject<Env> {
  private readonly entries: Entries;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    const config: Host["config"] = {
      holdLifeMs: numberVariable("HOLD_LIFE_MS", env.HOLD_LIFE_MS),
      intervalSeconds: numberVariable("SYNC_INTERVAL_SECONDS", env.SYNC_INTERVAL_SECONDS),
      marginSeconds: numberVariable("SYNC_MARGIN_SECONDS", env.SYNC_MARGIN_SECONDS),
      publicOrigin: env.PUBLIC_ORIGIN ?? "",
      testClock: env.TEST_CLOCK === "1",
    };
    const host: Host = {
      step: stepOf(ctx.storage),
      clock: () => Date.now(),
      setAlarm(at) {
        const call = at === null ? ctx.storage.deleteAlarm() : ctx.storage.setAlarm(at);
        void call.catch(alarmCallRejected);
      },
      getAlarm: () => ctx.storage.getAlarm(),
      async deleteAll() {
        await ctx.storage.deleteAlarm();
        await ctx.storage.deleteAll();
      },
      port: providerPort(env),
      config,
    };
    this.entries = makeEntries(host, {
      accounts: makeAccounts(),
      ledger: makeLedger({ holdLifeMs: config.holdLifeMs }),
      metering: makeMetering({ intervalSeconds: config.intervalSeconds, marginSeconds: config.marginSeconds }),
    });
    void ctx.blockConcurrencyWhile(() => this.entries.rearm());
  }

  start(accountId: string, hash: string, cap: Cap) {
    return this.entries.start(accountId, hash, cap);
  }

  webhookEvent(event: ProviderEvent) {
    return this.entries.webhookEvent(event);
  }

  status(hash: string) {
    return this.entries.status(hash);
  }

  capGet(hash: string) {
    return this.entries.capGet(hash);
  }

  capSet(hash: string, cap: Cap) {
    return this.entries.capSet(hash, cap);
  }

  manage(hash: string) {
    return this.entries.manage(hash);
  }

  destroy(hash: string) {
    return this.entries.destroy(hash);
  }

  reserve(hash: string, req: { requestId: string; tier: Tier; amount: Micro; price: Price }) {
    return this.entries.reserve(hash, req);
  }

  dispatch(requestId: string) {
    return this.entries.dispatch(requestId);
  }

  settle(requestId: string, usage: Usage) {
    return this.entries.settle(requestId, usage);
  }

  void(requestId: string) {
    return this.entries.void(requestId);
  }

  async alarm() {
    await this.entries.alarm();
  }

  testSetClock(time: Ms) {
    return this.entries.testSetClock(time);
  }

  testSeed(a: { accountId: string; hash: string; subscription: SubscriptionState; customerId?: string; cap?: Cap; anchor?: Ms; time?: Ms }) {
    return this.entries.testSeed(a);
  }

  testInspect() {
    return this.entries.testInspect();
  }
}
