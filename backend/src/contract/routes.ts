import type { Env } from "../types.ts";

export type RouteName = "checkout-start" | "status" | "cap-get" | "cap-set" | "manage" | "account-delete";
export interface AppRequest {
  accountId: string;
  credentialHash: string;
  body: unknown;
}
export type AppHandler = (request: AppRequest, env: Env) => Promise<object | null>;
export interface Handlers {
  "checkout-start": AppHandler;
  status: AppHandler;
  "cap-get": AppHandler;
  "cap-set": AppHandler;
  manage: AppHandler;
  "account-delete": AppHandler;
  webhook: (rawBody: Uint8Array, headers: Headers, env: Env) => Promise<Response>;
  done: () => Response;
}

export const WEBHOOK_BODY_LIMIT = 16_384;

export const routeTable = {
  version: 1,
  headers: {
    "halvo-account": { pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$", length: 36 },
    authorization: { pattern: "^Bearer [0-9a-f]{64}$", length: 71 },
    "content-type": { pattern: "^application/json$", length: 16 },
  },
  routes: [
    {
      name: "checkout-start",
      kind: "app",
      method: "POST",
      path: "/v1/checkout",
      request: "CapBody",
      answer: "CheckoutAnswer",
      limit: 45,
      results: { session: ["url"], "account-exists": [], "credential-mismatch": [], unavailable: [] },
    },
    {
      name: "status",
      kind: "app",
      method: "GET",
      path: "/v1/subscription",
      request: null,
      answer: "StatusAnswer",
      limit: 0,
      results: {
        pending: [],
        active: [],
        "payment-failed": [],
        cancelled: [],
        "no-such-account": [],
        "credential-mismatch": [],
      },
    },
    {
      name: "cap-get",
      kind: "app",
      method: "GET",
      path: "/v1/cap",
      request: null,
      answer: "CapGetAnswer",
      limit: 0,
      results: { cap: ["cap", "spend"], "no-cap": [], pending: [], "no-such-account": [], "credential-mismatch": [] },
    },
    {
      name: "cap-set",
      kind: "app",
      method: "PUT",
      path: "/v1/cap",
      request: "CapBody",
      answer: "CapSetAnswer",
      limit: 45,
      results: { cap: ["cap", "spend"], pending: [], "no-such-account": [], "credential-mismatch": [] },
    },
    {
      name: "manage",
      kind: "app",
      method: "POST",
      path: "/v1/manage",
      request: null,
      answer: "ManageAnswer",
      limit: 0,
      results: { page: ["url"], pending: [], "no-such-account": [], "credential-mismatch": [], unavailable: [] },
    },
    {
      name: "account-delete",
      kind: "app",
      method: "DELETE",
      path: "/v1/account",
      request: null,
      answer: "DeleteAnswer",
      limit: 0,
      results: { deleted: [], pending: [], "not-finished": [], "no-such-account": [], "credential-mismatch": [] },
    },
    {
      name: "webhook",
      kind: "provider",
      method: "POST",
      path: "/v1/webhook",
      request: null,
      answer: null,
      limit: WEBHOOK_BODY_LIMIT,
      results: {},
    },
    { name: "done", kind: "page", method: "GET", path: "/v1/done", request: null, answer: null, limit: 0, results: {} },
  ],
} as const;

export type Route = (typeof routeTable.routes)[number];
