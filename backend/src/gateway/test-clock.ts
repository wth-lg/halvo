import * as z from "zod";
import { credentialHash } from "../app.ts";
import { routeTable } from "../contract/routes.ts";
import { Cap } from "../contract/schemas.ts";
import { errorName, log } from "../log.ts";
import type { Env } from "../types.ts";

const ACCOUNT_PATTERN = new RegExp(routeTable.headers["halvo-account"].pattern);
const CREDENTIAL_PATTERN = new RegExp(routeTable.headers.authorization.pattern);
const JSON_TYPE = "application/json";
const PATHS = ["/test/clock", "/test/seed", "/test/inspect"];

const account = z.string().refine((value) => ACCOUNT_PATTERN.test(value));
const DATE_MAX_MS = 8_640_000_000_000_000;
const instant = z.int().min(0).max(DATE_MAX_MS);
const ClockBody = z.strictObject({ account, time: instant });
const SeedBody = z.strictObject({
  account,
  credential: z.string().refine((value) => CREDENTIAL_PATTERN.test(`Bearer ${value}`)),
  subscription: z.enum(["active", "payment-failed", "cancelled"]),
  customer: z.string().min(1).max(255).optional(),
  cap: Cap.optional(),
  anchor: instant.optional(),
  time: instant.optional(),
});
const InspectBody = z.strictObject({ account });

function answer(value: object): Response {
  return new Response(JSON.stringify(value), { status: 200, headers: { "content-type": JSON_TYPE } });
}

function rejected(): Response {
  log({ event: "rejected", route: "none", status: 400 });
  return new Response('{"result":"rejected"}', { status: 400, headers: { "content-type": JSON_TYPE } });
}

function objectOf(env: Env, accountId: string) {
  return env.ACCOUNT.get(env.ACCOUNT.idFromName(accountId));
}

async function serve(path: string, body: unknown, env: Env): Promise<Response> {
  if (path === "/test/clock") {
    const clock = ClockBody.safeParse(body);
    if (!clock.success) return rejected();
    return answer(await objectOf(env, clock.data.account).testSetClock(clock.data.time));
  }
  if (path === "/test/seed") {
    const seed = SeedBody.safeParse(body);
    if (!seed.success) return rejected();
    const { account: accountId, credential, subscription, customer, cap, anchor, time } = seed.data;
    return answer(
      await objectOf(env, accountId).testSeed({
        accountId,
        hash: await credentialHash(credential),
        subscription,
        ...(customer === undefined ? {} : { customerId: customer }),
        ...(cap === undefined ? {} : { cap }),
        ...(anchor === undefined ? {} : { anchor }),
        ...(time === undefined ? {} : { time }),
      }),
    );
  }
  const inspect = InspectBody.safeParse(body);
  if (!inspect.success) return rejected();
  return answer(await objectOf(env, inspect.data.account).testInspect());
}

export async function testPaths(request: Request, env: Env): Promise<Response | null> {
  const path = new URL(request.url).pathname;
  if (request.method !== "POST" || !PATHS.includes(path)) return null;
  let body: unknown;
  try {
    body = JSON.parse(await request.text());
  } catch {
    return rejected();
  }
  try {
    return await serve(path, body, env);
  } catch (error) {
    log({ event: "handler-error", route: "none", status: 500, error: errorName(error) });
    return new Response(null, { status: 500 });
  }
}
