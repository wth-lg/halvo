interface ImportMeta {
  url: string;
}

declare module "node:http" {
  export interface IncomingMessage {
    method?: string;
    url?: string;
    headers: Record<string, string | string[] | undefined>;
    on(event: "data", listener: (chunk: Uint8Array) => void): this;
    on(event: "end", listener: () => void): this;
  }
  export interface ServerResponse {
    socket: { destroy(): void } | null;
    writeHead(status: number, headers?: Record<string, string>): this;
    end(body?: string): void;
  }
  export interface Server {
    listen(port: number, host: string, listener: () => void): void;
    address(): { port: number };
    close(listener?: () => void): void;
    closeAllConnections(): void;
  }
  export function createServer(handler: (request: IncomingMessage, response: ServerResponse) => void): Server;
}
