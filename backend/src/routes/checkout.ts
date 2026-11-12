import type { AppHandler } from "../contract/routes.ts";
import type { CapBody } from "../contract/schemas.ts";

export const checkoutStart: AppHandler = async (request, env) => {
  const { cap } = request.body as CapBody;
  return env.ACCOUNT.get(env.ACCOUNT.idFromName(request.accountId)).start(request.accountId, request.credentialHash, cap);
};
