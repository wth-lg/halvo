import type { AppHandler } from "../contract/routes.ts";

export const accountDelete: AppHandler = async (request, env) =>
  env.ACCOUNT.get(env.ACCOUNT.idFromName(request.accountId)).destroy(request.credentialHash);
