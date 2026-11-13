declare module "node:fs" {
  export function readdirSync(path: string): string[];
  export function openSync(path: string, flags: "a"): number;
}

declare module "node:child_process" {
  export function spawn(
    command: string,
    args: string[],
    options: { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[] },
  ): Child;
}
