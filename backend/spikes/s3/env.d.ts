declare namespace Cloudflare {
  interface Env {
    readonly STORE: DurableObjectNamespace<import("./src/worker.ts").Store>;
  }
}

declare module "*.json?raw" {
  const text: string;
  export default text;
}
