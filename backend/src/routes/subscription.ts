import type { AppHandler } from "../contract/routes.ts";

export const status: AppHandler = async (request, env) =>
  env.ACCOUNT.get(env.ACCOUNT.idFromName(request.accountId)).status(request.credentialHash);
