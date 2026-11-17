declare module "node:fs" {
  export function appendFileSync(path: string, data: string): void;
}
