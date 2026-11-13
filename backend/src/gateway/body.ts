import * as z from "zod";
import { Tier } from "../contract/schemas.ts";
import type { Tier as TierName } from "../types.ts";

export const CHAT_BODY_LIMIT = 1_048_576;
export const OUTPUT_LIMIT_MAX = 8192;
export const CHOICES_MAX = 8;

const TextPart = z.strictObject({ type: z.enum(["text"]), text: z.string() });
const Message = z.strictObject({
  role: z.enum(["system", "user", "assistant"]),
  content: z.union([z.string(), z.array(TextPart).min(1)]),
});
const MockInstructions = z.strictObject({
  switch: z.enum(["usage-last", "usage-missing", "zero-placeholder", "slow", "over-report", "error-status", "reset", "anthropic-cut"]),
  shape: z.enum(["openai", "anthropic"]),
  chunks: z.int().min(0).max(1000),
  chunk_ms: z.int().min(0).max(60_000),
  input_tokens: z.int().min(0).max(Number.MAX_SAFE_INTEGER),
  output_tokens: z.int().min(0).max(Number.MAX_SAFE_INTEGER),
  wave: z.string().max(200).optional(),
});
const ChatBody = z.strictObject({
  tier: Tier,
  max_tokens: z.int().min(1).max(Number.MAX_SAFE_INTEGER).optional(),
  n: z.int().min(1).max(CHOICES_MAX).optional(),
  stream: z.boolean().optional(),
  messages: z.array(Message).min(1),
  mock: MockInstructions.optional(),
});

export interface ChatRequest {
  tier: TierName;
  outputLimit: number;
  choices: number;
  forward: string;
}

export function readChatBody(text: string): ChatRequest | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  const body = ChatBody.safeParse(parsed);
  if (!body.success) return null;
  const { tier, max_tokens: asked, ...rest } = body.data;
  const outputLimit = Math.min(asked ?? OUTPUT_LIMIT_MAX, OUTPUT_LIMIT_MAX);
  return { tier, outputLimit, choices: rest.n ?? 1, forward: JSON.stringify({ ...rest, model: tier, max_tokens: outputLimit, stream: true }) };
}
