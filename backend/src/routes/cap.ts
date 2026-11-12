import type { AppHandler } from "../contract/routes.ts";
import type { CapBody } from "../contract/schemas.ts";
import type { Env } from "../types.ts";

const objectOf = (env: Env, accountId: string) => env.ACCOUNT.get(env.ACCOUNT.idFromName(accountId));

export const capGet: AppHandler = async (request, env) => objectOf(env, request.accountId).capGet(request.credentialHash);

export const capSet: AppHandler = async (request, env) => objectOf(env, request.accountId).capSet(request.credentialHash, (request.body as CapBody).cap);
