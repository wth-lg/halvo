import app from "../app.ts";
import type { Env } from "../types.ts";
import { CHAT_PATH, chat } from "./chat.ts";
import { testPaths } from "./test-clock.ts";

export { AccountObject } from "../do/account-object.ts";

export default {
  async fetch(request, env, ctx) {
    const tested = await testPaths(request, env);
    if (tested !== null) return tested;
    if (request.method === "POST" && new URL(request.url).pathname === CHAT_PATH) return chat(request, env, ctx);
    return app(request, env);
  },
} satisfies ExportedHandler<Env>;
