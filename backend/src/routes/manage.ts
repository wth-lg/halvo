import type { AppHandler } from "../contract/routes.ts";

export const manage: AppHandler = async (request, env) =>
  env.ACCOUNT.get(env.ACCOUNT.idFromName(request.accountId)).manage(request.credentialHash);
