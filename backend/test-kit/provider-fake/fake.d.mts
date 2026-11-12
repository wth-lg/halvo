export const API_VERSION: string;
export const PERIOD_SECONDS: number;
export interface ProviderFake {
  origin: string;
  pageOrigin: string;
  close(): Promise<void>;
}
export function startProviderFake(o: {
  port?: number;
  mockUrl: string;
  apiKey: string;
  signingSecret: string;
  entries: string[];
  clock?: () => number;
}): Promise<ProviderFake>;
