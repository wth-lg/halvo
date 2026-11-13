declare module "node:child_process" {
  interface Child {
    on(event: "close", listener: (code: number | null) => void): void;
  }
}

declare module "node:http" {
  interface ServerResponse {
    write(chunk: string): boolean;
    end(body: Uint8Array): void;
    on(event: "close", listener: () => void): this;
  }
}
