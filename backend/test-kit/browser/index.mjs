import { chromium } from "playwright";

export const ENDPOINT_VARIABLE = "HALVO_BROWSER_ENDPOINT";
export const PROXY_REFUSED =
  "test-kit/browser refuses a browser context that sets a proxy of its own: a proxy is a process of the check, inside the strict sandbox, and would carry the browser's requests where the net log cannot follow them";

function refuseOwnProxy(browser) {
  for (const name of ["newContext", "newPage"]) {
    const original = browser[name].bind(browser);
    browser[name] = (options) => {
      if (options?.proxy !== undefined) throw new Error(PROXY_REFUSED);
      return original(options);
    };
  }
  return browser;
}

export async function openBrowser() {
  const endpoint = process.env[ENDPOINT_VARIABLE];
  if (endpoint) return refuseOwnProxy(await chromium.connectOverCDP(endpoint));
  if (process.env.OUTBOUND_RUN) {
    throw new Error(
      `no browser for this run: ${ENDPOINT_VARIABLE} is not set. A browser started inside the outbound check ends at its first call outside loopback. Run the command through "sh scripts/outbound-hosts.sh browser COMMAND", as the gate runs its checks`,
    );
  }
  return refuseOwnProxy(await chromium.launch({ headless: true }));
}
