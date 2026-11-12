import type { Usage } from "../ledger/index.ts";

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const count = (value: unknown): number | undefined => (typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined);
const usageOf = (input: number | undefined, output: number | undefined): Usage | undefined =>
  input === undefined || output === undefined ? undefined : { inputTokens: input, outputTokens: output };

type Outcome = { kind: "terminated"; usage: Usage | undefined } | { kind: "claim-lost" };

export class FinalUsage {
  private readonly decoder = new TextDecoder();
  private pending = "";
  private lastChunkUsage: Usage | undefined;
  private started = false;
  private inputInForce: number | undefined;
  private outputInForce: number | undefined;
  private outcome: Outcome | null = null;

  feed(bytes: Uint8Array): void {
    this.consume(this.pending + this.decoder.decode(bytes, { stream: true }), false);
  }

  end(): void {
    this.consume(this.pending + this.decoder.decode(), true);
  }

  result(): Usage | undefined {
    return this.outcome?.kind === "terminated" ? this.outcome.usage : undefined;
  }

  private consume(text: string, atEnd: boolean): void {
    const heldBack = !atEnd && text.endsWith("\r") ? "\r" : "";
    let lines = text.slice(0, text.length - heldBack.length).replace(/\r\n?/g, "\n");
    for (let end = lines.indexOf("\n\n"); end >= 0; end = lines.indexOf("\n\n")) {
      this.event(lines.slice(0, end));
      lines = lines.slice(end + 2);
    }
    this.pending = lines + heldBack;
  }

  private event(block: string): void {
    if (this.outcome !== null) return;
    const dataLines = block.split("\n").filter((line) => line.startsWith("data:"));
    if (dataLines.length === 0) return;
    const data = dataLines.map((line) => line.slice(5).trim()).join("\n");
    if (data === "[DONE]") {
      this.outcome = { kind: "terminated", usage: this.lastChunkUsage };
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(data);
    } catch {
      this.outcome = { kind: "claim-lost" };
      return;
    }
    if (!isRecord(parsed)) return;
    const usage = isRecord(parsed.usage) ? parsed.usage : undefined;
    switch (parsed.type) {
      case undefined:
        this.lastChunkUsage = usageOf(count(usage?.prompt_tokens), count(usage?.completion_tokens));
        return;
      case "message_start": {
        if (this.started) {
          this.outcome = { kind: "claim-lost" };
          return;
        }
        this.started = true;
        const start = isRecord(parsed.message) && isRecord(parsed.message.usage) ? parsed.message.usage : undefined;
        this.inputInForce = count(start?.input_tokens);
        return;
      }
      case "message_delta":
        if (usage !== undefined && "input_tokens" in usage) this.inputInForce = count(usage.input_tokens);
        if (usage !== undefined && "output_tokens" in usage) this.outputInForce = count(usage.output_tokens);
        return;
      case "message_stop":
        this.outcome = { kind: "terminated", usage: usageOf(this.inputInForce, this.outputInForce) };
        return;
      case "error":
        this.outcome = { kind: "claim-lost" };
        return;
      default:
        return;
    }
  }
}
