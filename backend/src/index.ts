import app from "./app.ts";
import type { Env } from "./types.ts";

export { AccountObject } from "./do/account-object.ts";

export default {
  fetch(request, env) {
    return app(request, env);
  },
} satisfies ExportedHandler<Env>;
