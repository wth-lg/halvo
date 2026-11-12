import { createServer } from "node:http";
import { chromium } from "playwright";
import { openBrowser } from "../../../test-kit/browser/index.mjs";

const mode = process.argv[2];
const TEST_NET_1 = "192.0.2.1";
const LOOPBACK = "127.0.0.1";
const webTransport = (address) => new WebTransport(`https://${address}/`).ready.then(() => "ready", (error) => error.message);
const inPage = {
  loopback: null,
  own: null,
  tcp: (address) => fetch(`http://${address}/`, { mode: "no-cors" }).then(() => "answered", (error) => error.message),
  tcp443: (address) => fetch(`https://${address}/`, { mode: "no-cors" }).then(() => "answered", (error) => error.message),
  udp443: webTransport,
  "udp443-loopback": webTransport,
  proxy: null,
};
const addressFor = (mode) => (mode === "udp443-loopback" ? LOOPBACK : TEST_NET_1);
if (!(mode in inPage)) {
  console.error("usage: node gate/fixtures/outbound/attempt-browser.mjs loopback|tcp|tcp443|udp443|udp443-loopback|own|proxy");
  process.exit(64);
}

const server = createServer((_request, response) => {
  response.setHeader("content-type", "text/html");
  response.end("<!doctype html><title>loaded</title>");
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = mode === "own" ? await chromium.launch() : await openBrowser();
  const context = mode === "proxy" ? await browser.newContext({ proxy: { server: "http://127.0.0.1:9" } }) : await browser.newContext();
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`, { timeout: 10_000 });
  console.log("browser page:", await page.title());
  if (inPage[mode]) console.log(`browser page: ${mode}:`, await page.evaluate(inPage[mode], addressFor(mode)));
} catch (error) {
  console.log("browser:", String(error.message).split("\n")[0]);
  process.exitCode = 1;
} finally {
  await browser?.close().catch(() => {});
  server.close();
}
