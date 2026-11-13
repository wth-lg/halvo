import type { Cap, Period, Tier } from "./contract/schemas.ts";
import type { AccountObject } from "./do/account-object.ts";

export type { Cap, Period, Tier };
export type Ms = number;
export type Seconds = number;
export type Micro = number;
export type Minor = number;
export const MICRO_PER_MINOR = 10_000;
export interface BillingPeriod {
  start: Ms;
  end: Ms;
}
export interface Sql {
  exec(query: string, ...bindings: (string | number | null)[]): Record<string, string | number | null>[];
}
export type Step = <T>(run: (sql: Sql) => T) => T;
export interface Env {
  ACCOUNT: DurableObjectNamespace<AccountObject>;
  PROVIDER_BASE_URL: string;
  PROVIDER_API_KEY?: string;
  WEBHOOK_SIGNING_SECRET?: string;
  PROVIDER_PRICE_ID: string;
  PROVIDER_METER_ID: string;
  PROVIDER_METER_EVENT: string;
  PUBLIC_ORIGIN: string;
  HOLD_LIFE_MS: string;
  UPSTREAM_DEADLINE_MS: string;
  DISPATCH_MARGIN_MS: string;
  SYNC_INTERVAL_SECONDS: string;
  SYNC_MARGIN_SECONDS: string;
  TEST_CLOCK?: string;
  UPSTREAM_BASE_URL?: string;
}
