import type { RouteName } from "./contract/routes.ts";

export type LogEvent =
  | "rejected"
  | "handler-error"
  | "answer-defect"
  | "bad-variable"
  | "alarm-error"
  | "alarm-stopped"
  | "pending-dropped"
  | "pending-unanswered"
  | "completed-no-record"
  | "usage-not-billable"
  | "cancel-overrides-order"
  | "usage-total-unexplained";

export interface LogFields {
  event: LogEvent;
  route?: RouteName | "webhook" | "done" | "none";
  status?: number;
  error?: string;
  variable?: string;
}

const FIELD_ORDER = ["event", "route", "status", "error", "variable"] as const;
const POSITIVE_WHOLE_NUMBER = /^[1-9][0-9]{0,14}$/;
const ERROR_NAME = /^[A-Za-z]{0,59}Error$/;

export function log(fields: LogFields): void {
  const line: Partial<Record<(typeof FIELD_ORDER)[number], unknown>> = {};
  for (const key of FIELD_ORDER) {
    if (fields[key] !== undefined) line[key] = fields[key];
  }
  console.log(JSON.stringify(line));
}

export function errorName(caught: unknown): string {
  try {
    const name = (caught as { name?: unknown } | null | undefined)?.name;
    return typeof name === "string" && ERROR_NAME.test(name) ? name : "Error";
  } catch {
    return "Error";
  }
}

export function numberVariable(name: string, value: string | undefined): number {
  if (value !== undefined && POSITIVE_WHOLE_NUMBER.test(value)) return Number(value);
  log({ event: "bad-variable", variable: name });
  const error = new Error("a number variable is not a positive whole number in decimal digits");
  error.name = name;
  throw error;
}
