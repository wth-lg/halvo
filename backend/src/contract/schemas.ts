import * as z from "zod";
import type { RouteName } from "./routes.ts";

export const CAP_AMOUNT_MIN = 1;
export const CAP_AMOUNT_MAX = 100_000_000;
export const URL_MAX_LENGTH = 2048;
const LARGEST_EXACT_INTEGER = Number.MAX_SAFE_INTEGER;

const wholeFromZero = () => z.int().min(0).max(LARGEST_EXACT_INTEGER);

export const Period = z.enum(["week", "month"]);
export const Cap = z.strictObject({
  amount: z.int().min(CAP_AMOUNT_MIN).max(CAP_AMOUNT_MAX),
  period: Period,
});
export const Spend = z.strictObject({
  spent: wholeFromZero(),
  held: wholeFromZero(),
  period_end: wholeFromZero(),
});
export const Tier = z.enum(["low_cost_hosted", "frontier"]);
export const Url = z.string().max(URL_MAX_LENGTH);
export const CapBody = z.strictObject({ cap: Cap });
export const CheckoutAnswer = z.strictObject({
  result: z.enum(["session", "account-exists", "credential-mismatch", "unavailable"]),
  url: Url.optional(),
});
export const StatusAnswer = z.strictObject({
  result: z.enum(["pending", "active", "payment-failed", "cancelled", "no-such-account", "credential-mismatch"]),
});
export const CapGetAnswer = z.strictObject({
  result: z.enum(["cap", "no-cap", "pending", "no-such-account", "credential-mismatch"]),
  cap: Cap.optional(),
  spend: Spend.optional(),
});
export const CapSetAnswer = z.strictObject({
  result: z.enum(["cap", "pending", "no-such-account", "credential-mismatch"]),
  cap: Cap.optional(),
  spend: Spend.optional(),
});
export const ManageAnswer = z.strictObject({
  result: z.enum(["page", "pending", "no-such-account", "credential-mismatch", "unavailable"]),
  url: Url.optional(),
});
export const DeleteAnswer = z.strictObject({
  result: z.enum(["deleted", "pending", "not-finished", "no-such-account", "credential-mismatch"]),
});
export const Rejected = z.strictObject({ result: z.enum(["rejected"]) });

export type Period = z.infer<typeof Period>;
export type Cap = z.infer<typeof Cap>;
export type Spend = z.infer<typeof Spend>;
export type Tier = z.infer<typeof Tier>;
export type Url = z.infer<typeof Url>;
export type CapBody = z.infer<typeof CapBody>;
export type CheckoutAnswer = z.infer<typeof CheckoutAnswer>;
export type StatusAnswer = z.infer<typeof StatusAnswer>;
export type CapGetAnswer = z.infer<typeof CapGetAnswer>;
export type CapSetAnswer = z.infer<typeof CapSetAnswer>;
export type ManageAnswer = z.infer<typeof ManageAnswer>;
export type DeleteAnswer = z.infer<typeof DeleteAnswer>;
export type Rejected = z.infer<typeof Rejected>;

export const definitions = {
  Cap,
  Period,
  Spend,
  Tier,
  Url,
  CapBody,
  CheckoutAnswer,
  StatusAnswer,
  CapGetAnswer,
  CapSetAnswer,
  ManageAnswer,
  DeleteAnswer,
  Rejected,
} as const;

export const requestSchemas: Record<RouteName, typeof CapBody | null> = {
  "checkout-start": CapBody,
  status: null,
  "cap-get": null,
  "cap-set": CapBody,
  manage: null,
  "account-delete": null,
};

export const answerSchemas = {
  "checkout-start": CheckoutAnswer,
  status: StatusAnswer,
  "cap-get": CapGetAnswer,
  "cap-set": CapSetAnswer,
  manage: ManageAnswer,
  "account-delete": DeleteAnswer,
} as const satisfies Record<RouteName, z.ZodType>;
