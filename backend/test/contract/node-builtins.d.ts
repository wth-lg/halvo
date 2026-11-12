declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
  export function writeFileSync(path: string, data: string): void;
  export function mkdtempSync(prefix: string): string;
  export function mkdirSync(path: string, options?: { recursive?: boolean }): string | undefined;
  export function existsSync(path: string): boolean;
  export function copyFileSync(from: string, to: string): void;
}

declare module "node:path" {
  export function join(...parts: string[]): string;
  export function dirname(path: string): string;
}

declare module "node:child_process" {
  export interface SpawnSyncResult {
    status: number | null;
    stdout: string;
    stderr: string;
  }
  export interface Child {
    pid?: number;
    stdout: { on(event: "data", listener: (chunk: unknown) => void): void };
    stderr: { on(event: "data", listener: (chunk: unknown) => void): void };
    on(event: "exit", listener: (code: number | null, signal: string | null) => void): void;
    kill(signal?: string): boolean;
  }
  export function spawnSync(
    command: string,
    args: string[],
    options: { cwd?: string; encoding: "utf8"; env?: Record<string, string | undefined>; input?: string; timeout?: number },
  ): SpawnSyncResult;
  export function execFileSync(command: string, args: string[], options: { encoding: "utf8"; cwd?: string }): string;
  export function spawn(
    command: string,
    args: string[],
    options: { cwd?: string; env?: Record<string, string | undefined>; stdio?: ("ignore" | "pipe")[] },
  ): Child;
}

declare module "node:net" {
  export interface Server {
    listen(port: number, host: string, listener: () => void): void;
    address(): { port: number };
    close(listener: () => void): void;
    on(event: "error", listener: (error: Error) => void): void;
  }
  export function createServer(): Server;
}

declare module "node:crypto" {
  export function createHash(algorithm: "sha256"): { update(text: string, encoding: "utf8"): { digest(encoding: "hex"): string } };
  export function randomUUID(): string;
}

declare module "*?raw" {
  const text: string;
  export default text;
}
