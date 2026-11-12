import { vi } from "vitest";
import type { Entries } from "../../src/do/entries.ts";
import { chat } from "../../src/gateway/chat.ts";
import type { Usage } from "../../src/ledger/index.ts";
import type { Cap, Env, Ms } from "../../src/types.ts";
import { HASH, type Rig, seededRig } from "./harness.ts";

const ACCOUNT = "6f1e2d3c-4b5a-4968-8776-5a4b3c2d1e0f";
const CREDENTIAL = "4e".repeat(32);
const encoder = new TextEncoder();

export type ObjectCall = "reserve" | "dispatch" | "settle" | "void";
export type Upstream = (init: RequestInit) => Promise<Response>;

export interface Sent {
  status: number;
  text: string;
  receipt: { requestId: string; amount: number; now: Ms; expiresAt: Ms } | null;
}

export interface ChatRig {
  rig: Rig;
  calls: ObjectCall[];
  before: Partial<Record<ObjectCall, () => Promise<void> | void>>;
  upstreamCalls(): number;
  setUpstream(upstream: Upstream): void;
  send(requestId: string, body: object | string, read?: (response: Response) => Promise<string>): Promise<Sent>;
  hold(requestId: string): Record<string, string | number | null> | undefined;
  close(): void;
}

export const chunk = (data: string) => encoder.encode(`data: ${data}\n\n`);

export function upstreamOf(parts: Uint8Array[], end: "close" | "reset" | "open"): Upstream {
  return async (init) => {
    let at = 0;
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        if (end === "open") init.signal?.addEventListener("abort", () => controller.error(new DOMException("cut", "AbortError")), { once: true });
      },
      pull(controller) {
        const next = parts[at];
        at += 1;
        if (next !== undefined) controller.enqueue(next);
        else if (end === "close") controller.close();
        else if (end === "reset") controller.error(new TypeError("Network connection lost."));
      },
    });
    return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
  };
}

export async function chatRig(o: { at: Ms; cap?: Cap; holdLifeMs?: number; deadlineMs?: number; marginMs?: number; subscription?: "active" | "payment-failed" }): Promise<ChatRig> {
  const rig = await seededRig({ cap: o.cap ?? { amount: 100_000_000, period: "week" }, at: o.at, holdLifeMs: o.holdLifeMs, subscription: o.subscription });
  const calls: ObjectCall[] = [];
  const before: ChatRig["before"] = {};
  const object = {
    async reserve(_hash: string, req: Parameters<Entries["reserve"]>[1]) {
      calls.push("reserve");
      await before.reserve?.();
      return rig.entries().reserve(HASH, req);
    },
    async dispatch(requestId: string) {
      calls.push("dispatch");
      await before.dispatch?.();
      return rig.entries().dispatch(requestId);
    },
    async settle(requestId: string, usage: Usage) {
      calls.push("settle");
      await before.settle?.();
      return rig.entries().settle(requestId, usage);
    },
    async void(requestId: string) {
      calls.push("void");
      await before.void?.();
      return rig.entries().void(requestId);
    },
  };
  let upstream: Upstream = async () => {
    throw new Error("the test set no upstream");
  };
  let upstreamCalls = 0;
  vi.stubGlobal("fetch", (_url: string, init: RequestInit) => {
    upstreamCalls += 1;
    return upstream(init);
  });
  const env = {
    ACCOUNT: { idFromName: () => "id", get: () => object },
    UPSTREAM_BASE_URL: "http://127.0.0.1:9",
    UPSTREAM_DEADLINE_MS: String(o.deadlineMs ?? 100),
    DISPATCH_MARGIN_MS: String(o.marginMs ?? 400),
  } as unknown as Env;
  return {
    rig,
    calls,
    before,
    upstreamCalls: () => upstreamCalls,
    setUpstream(next) {
      upstream = next;
    },
    async send(requestId, body, read = (response) => response.text()) {
      const waits: Promise<unknown>[] = [];
      const ctx = { waitUntil: (promise: Promise<unknown>) => waits.push(promise) } as unknown as ExecutionContext;
      const request = new Request("http://backend.test/v1/chat", {
        method: "POST",
        headers: { "halvo-account": ACCOUNT, authorization: `Bearer ${CREDENTIAL}`, "content-type": "application/json", "x-request-id": requestId },
        body: typeof body === "string" ? body : JSON.stringify(body),
      });
      const response = await chat(request, env, ctx);
      const text = await read(response);
      await Promise.all(waits);
      const receipt = response.headers.get("x-ledger-receipt");
      return { status: response.status, text, receipt: receipt === null ? null : JSON.parse(receipt) };
    },
    hold: (requestId) => rig.read().holds.find((row) => row.request_id === requestId),
    close() {
      vi.unstubAllGlobals();
      rig.close();
    },
  };
}
