import { mkdir, readFile, writeFile } from 'node:fs/promises';
import os from 'node:os';
import { openBrowser } from '../../test-kit/browser/index.mjs';
import { requiredRows } from '../../test-kit/checkout-page/fields.mjs';
import { loadFields } from '../../test-kit/checkout-page/server.mjs';
import { startStandIns, TEST_BUYER_EMAIL } from './standin.mjs';

const ROOT = new URL('../../', import.meta.url);
const LOOPBACK = new Set(['127.0.0.1', 'localhost', '[::1]']);
const TEST_NET_1_URL = 'http://192.0.2.1/';
const PROVIDER_TEST_CARD_NUMBER = '4242 4242 4242 4242';
const CARD = {
  email: TEST_BUYER_EMAIL,
  'card-number': PROVIDER_TEST_CARD_NUMBER,
  'card-expiry': '12/34',
  'card-cvc': '123',
  'cardholder-name': 'Test Buyer',
};
const BUYERS = {
  us: { ...CARD, country: 'US', 'postal-code': '12345' },
  gb: { ...CARD, country: 'GB' },
};
const PATHS = [
  { id: 'device-us', candidate: 'device', buyer: 'us' },
  { id: 'device-gb', candidate: 'device', buyer: 'gb' },
  { id: 'passkey', candidate: 'passkey', buyer: 'us' },
  { id: 'google', candidate: 'google', buyer: 'us' },
];
const CONTROLS = [
  { id: 'unlogged-input', candidate: 'device', buyer: 'us', control: 'unlogged-input' },
  { id: 'no-payment', candidate: 'device', buyer: 'us', control: 'no-payment' },
  { id: 'remote-request', candidate: 'device', buyer: 'us', control: 'remote-request' },
];

const UNLOGGED_PAGE = Symbol('raw page');

function handle(page, log) {
  async function act(meta, kind, selector, perform) {
    await perform();
    const target = selector.replace(/^\[data-row="(.+)"\]$/, '$1').replace(/^#/, '');
    log.push({ n: log.length + 1, where: meta.where, class: meta.class, kind, target });
  }
  return {
    click: (meta, selector) => act(meta, 'click', selector, () => page.locator(selector).click()),
    type: (meta, selector, value) => act(meta, 'typed', selector, () => page.locator(selector).fill(value)),
    select: (meta, selector, value) =>
      act(meta, 'selection', selector, () => page.locator(selector).selectOption(value)),
    async clickOpensPage(meta, selector) {
      const opened = page.context().waitForEvent('page');
      opened.catch(() => {});
      await act(meta, 'click', selector, () => page.locator(selector).click());
      const next = await opened;
      await next.waitForLoadState();
      return handle(next, log);
    },
    waitFor: (selector) => page.locator(selector).waitFor(),
    attribute: (selector, name) => page.locator(selector).getAttribute(name),
    async waitForUrlOr(pattern, errorSelector) {
      const arrived = page.waitForURL(pattern).then(() => null);
      const failed = page
        .locator(errorSelector)
        .waitFor()
        .then(() => page.locator(errorSelector).textContent());
      arrived.catch(() => {});
      failed.catch(() => {});
      const message = await Promise.race([arrived, failed]);
      if (message !== null) throw new Error(`the sign-in page: ${message}`);
    },
    [UNLOGGED_PAGE]: page,
  };
}

function withoutRunToRunValues(error) {
  return String(error?.message ?? error)
    .split('\n')[0]
    .replace(/:\d{4,5}\b/g, ':PORT')
    .replace(/[0-9a-f]{64}/g, 'REFERENCE')
    .replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, 'ID');
}

async function openSubscriptionViewUncounted(context, standIns, spec, log) {
  const page = await context.newPage();
  await page.goto(`${standIns.origin}/app?candidate=${spec.candidate}`);
  const app = handle(page, log);
  await app.waitFor('body[data-ready]');
  return app;
}

async function flow(context, standIns, fields, spec, record) {
  const buyer = BUYERS[spec.buyer];
  const app = await openSubscriptionViewUncounted(context, standIns, spec, record.log);
  if (spec.control === 'remote-request') {
    await app[UNLOGGED_PAGE].evaluate((url) => fetch(url, { mode: 'no-cors' }).catch(() => {}), TEST_NET_1_URL);
  }

  const browserPage = await app.clickOpensPage({ where: 'app', class: 'start' }, '#subscribe');
  if (spec.candidate !== 'device') {
    await browserPage.waitFor('body[data-ready]');
    await browserPage.click({ where: 'browser', class: 'outside' }, `#${spec.candidate}`);
    await browserPage.waitForUrlOr(`${standIns.checkoutOrigin}/c/**`, '#status[data-state="error"]');
  }

  await browserPage.waitFor('#checkout[data-ready]');
  const session = { country: buyer.country, emailLocked: spec.candidate === 'google' };
  for (const row of requiredRows(fields, session)) {
    if (spec.control === 'no-payment' && row.id === 'card-number') continue;
    const meta = { where: 'checkout page', class: row.class };
    const selector = `[data-row="${row.id}"]`;
    if (row.kind === 'typed') {
      await browserPage.type(meta, selector, buyer[row.id]);
    } else if (row.kind === 'selection') {
      await browserPage.select(meta, selector, buyer[row.id]);
    } else {
      if (spec.control === 'unlogged-input') {
        await browserPage[UNLOGGED_PAGE].locator('[data-row="cardholder-name"]').fill('Test Buyer Again');
      }
      await browserPage.click(meta, selector);
    }
  }
  await browserPage.waitFor('#status[data-state]');
  record.checkoutStatus = await browserPage.attribute('#status', 'data-state');
  if (record.checkoutStatus !== 'paid') return;
  await app.waitFor('#state[data-state="active-with-cap"]');
}

async function drive(browser, fields, spec) {
  const record = {
    ...spec,
    log: [],
    outside: [],
    checkoutStatus: null,
    reached: null,
    page: null,
    auth: null,
    passkeysInAuthenticator: null,
    error: null,
  };
  const standIns = await startStandIns();
  const context = await browser.newContext();
  context.setDefaultTimeout(15_000);
  try {
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      if (!url.protocol.startsWith('http') || LOOPBACK.has(url.hostname)) return route.continue();
      record.outside.push(url.origin);
      return route.abort('blockedbyclient');
    });
    context.on('dialog', (dialog) => {
      record.log.push({
        n: record.log.length + 1,
        where: 'browser',
        class: 'outside',
        kind: 'dialog',
        target: dialog.type(),
      });
      dialog.accept().catch(() => {});
    });
    if (spec.candidate === 'passkey') await context.credentials.install();
    await flow(context, standIns, fields, spec, record);
  } catch (error) {
    record.error = withoutRunToRunValues(error);
  } finally {
    const [account] = standIns.accounts();
    record.reached = account
      ? { subscription: account.subscription, cap: account.cap, signedIn: account.signedIn }
      : null;
    record.page = standIns.checkoutSessions()[0]?.inputs ?? null;
    record.auth = standIns.authRows();
    if (spec.candidate === 'passkey') {
      record.passkeysInAuthenticator = await context.credentials.get().then(
        (list) => list.length,
        () => null,
      );
    }
    await context.close();
    await standIns.close();
  }
  return record;
}

const fields = await loadFields();
const pins = JSON.parse(await readFile(new URL('package.json', ROOT), 'utf8')).devDependencies;
const spikePins = JSON.parse(await readFile(new URL('package.json', import.meta.url), 'utf8')).dependencies;
await mkdir(new URL('.tmp/s4/', ROOT), { recursive: true });
const browser = await openBrowser();
const measured = {
  runId: process.env.S4_RUN_ID ?? null,
  sandboxed: Boolean(process.env.OUTBOUND_RUN),
  browserFromCheck: Boolean(process.env.HALVO_BROWSER_ENDPOINT),
  environment: {
    chromium: browser.version(),
    playwright: pins.playwright,
    oauthMock: pins['oauth2-mock-server'],
    betterAuth: spikePins['better-auth'],
    passkeyPlugin: spikePins['@better-auth/passkey'],
    node: process.version,
    os: `${os.type()} ${os.release()}`,
    cpu: os.cpus()[0].model,
    memoryGiB: Math.round(os.totalmem() / 2 ** 30),
    telemetrySwitch: process.env.BETTER_AUTH_TELEMETRY ?? 'unset',
  },
  paths: [],
  controls: [],
};
try {
  for (const spec of PATHS) measured.paths.push(await drive(browser, fields, spec));
  for (const spec of CONTROLS) measured.controls.push(await drive(browser, fields, spec));
} finally {
  await browser.close();
}
await writeFile(new URL('.tmp/s4/measured.json', ROOT), `${JSON.stringify(measured, null, 2)}\n`);
for (const record of [...measured.paths, ...measured.controls]) {
  const reached = record.reached ? `${record.reached.subscription}, cap ${record.reached.cap ? 'set' : 'not set'}` : 'no account';
  console.log(
    `s4: ${record.id}: ${record.log.length} actions logged, the page counted ${record.page?.count ?? 'nothing'}, reached ${reached}, ${record.outside.length} requests outside loopback${record.error ? `, error: ${record.error}` : ''}`,
  );
}
