import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { DatabaseSync } from 'node:sqlite';
import { passkey } from '@better-auth/passkey';
import { betterAuth } from 'better-auth';
import { getMigrations } from 'better-auth/db/migration';
import { fromNodeHeaders, toNodeHandler } from 'better-auth/node';
import { genericOAuth } from 'better-auth/plugins/generic-oauth';
import { Events, OAuth2Server } from 'oauth2-mock-server';
import { startCheckoutPage } from '../../test-kit/checkout-page/server.mjs';

export const TEST_BUYER_EMAIL = 'buyer@example.com';
export const CANDIDATES = ['device', 'passkey', 'google'];
const HEX64 = /^[0-9a-f]{64}$/;
const PASSKEY_RP_ID = 'localhost';

async function startAuth(origin, issuer) {
  const database = new DatabaseSync(':memory:');
  const passkeyUsers = new Map();
  const options = {
    appName: 'Halvo (spike S4)',
    baseURL: origin,
    secret: randomBytes(32).toString('hex'),
    database,
    telemetry: { enabled: false },
    advanced: { ipAddress: { disableIpTracking: true } },
    databaseHooks: {
      session: { create: { before: async (session) => ({ data: { ...session, userAgent: '' } }) } },
    },
    plugins: [
      passkey({
        rpID: PASSKEY_RP_ID,
        rpName: 'Halvo (spike S4)',
        registration: {
          requireSession: false,
          resolveUser: async ({ ctx, context }) => {
            const key = context ?? '';
            let user = passkeyUsers.get(key);
            if (!user) {
              user = await ctx.context.internalAdapter.createUser({
                email: `${randomUUID()}@passkey.placeholder.invalid`,
                emailVerified: false,
                name: 'Halvo user',
              });
              passkeyUsers.set(key, user);
            }
            return { id: user.id, name: user.name };
          },
        },
      }),
      genericOAuth({
        config: [
          {
            providerId: 'google-style',
            clientId: 'halvo-s4',
            clientSecret: 'local-mock-only',
            discoveryUrl: `${issuer}/.well-known/openid-configuration`,
            scopes: ['openid', 'email', 'profile'],
            pkce: true,
          },
        ],
      }),
    ],
  };
  const { runMigrations } = await getMigrations(options);
  await runMigrations();
  return { auth: betterAuth(options), database };
}

async function readJson(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  return JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}');
}

async function startOAuthMockNamedByAddress() {
  const oauth = new OAuth2Server();
  await oauth.issuer.keys.generate('RS256');
  await oauth.start(undefined, '127.0.0.1');
  oauth.issuer.url = `http://127.0.0.1:${oauth.address().port}`;
  return oauth;
}

export async function startStandIns() {
  const accounts = new Map();
  const checkout = await startCheckoutPage({
    onPaid({ clientReferenceId }) {
      const account = accounts.get(clientReferenceId);
      if (account) account.subscription = 'active';
    },
  });

  const oauth = await startOAuthMockNamedByAddress();
  const profile = { sub: 's4-test-buyer', email: TEST_BUYER_EMAIL, email_verified: true, name: 'Test Buyer' };
  oauth.service.on(Events.BeforeTokenSigning, (token) => {
    Object.assign(token.payload, profile);
  });
  oauth.service.on(Events.BeforeUserinfo, (response) => {
    response.body = profile;
  });

  let handle = async (_req, res) => res.writeHead(503).end();
  const server = createServer((req, res) => {
    handle(req, res).catch((error) => {
      res.writeHead(500, { 'content-type': 'text/plain' }).end(String(error));
    });
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const origin = `http://${PASSKEY_RP_ID}:${server.address().port}`;
  const { auth, database } = await startAuth(origin, oauth.issuer.url);
  const authHandler = toNodeHandler(auth);

  function accountOf(req, reference) {
    const account = accounts.get(reference);
    const secret = /^Bearer ([0-9a-f]{64})$/.exec(req.headers.authorization ?? '')?.[1];
    if (!account || !secret) return null;
    return createHash('sha256').update(secret).digest('hex') === account.secretSha256 ? account : null;
  }

  handle = async (req, res) => {
    const url = new URL(req.url, origin);
    const sendJson = (status, body) =>
      res.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' }).end(JSON.stringify(body));
    const sendPage = async (name) =>
      res
        .writeHead(200, { 'content-type': 'text/html', 'cache-control': 'no-store' })
        .end(await readFile(new URL(`pages/${name}`, import.meta.url)));

    if (url.pathname.startsWith('/api/auth/')) return authHandler(req, res);
    if (req.method === 'GET' && url.pathname === '/app') return sendPage('app.html');
    if (req.method === 'GET' && url.pathname === '/sign-in') return sendPage('sign-in.html');

    if (req.method === 'POST' && url.pathname === '/api/start') {
      const { candidate, reference, secretSha256, cap } = await readJson(req);
      const validCap = Number.isInteger(cap?.amount) && cap.amount > 0 && /^[A-Z]{3}$/.test(cap?.currency ?? '');
      const valid = CANDIDATES.includes(candidate) && HEX64.test(reference) && HEX64.test(secretSha256) && validCap;
      if (!valid || accounts.has(reference)) return sendJson(400, { error: 'bad start request' });
      accounts.set(reference, {
        candidate,
        secretSha256,
        userId: null,
        subscription: 'pending',
        cap: { amount: cap.amount, currency: cap.currency },
      });
      if (candidate === 'device') return sendJson(200, { url: checkout.createSession({ clientReferenceId: reference }).url });
      return sendJson(200, { url: `${origin}/sign-in?candidate=${candidate}&reference=${reference}` });
    }

    if (req.method === 'POST' && url.pathname === '/api/checkout') {
      const { reference } = await readJson(req);
      const account = accounts.get(reference);
      const session = await auth.api.getSession({ headers: fromNodeHeaders(req.headers) });
      if (!session) return sendJson(401, { error: 'no session' });
      if (!account || account.candidate === 'device') return sendJson(400, { error: 'bad reference' });
      account.userId = session.user.id;
      const customerEmail = account.candidate === 'google' ? session.user.email : null;
      return sendJson(200, { url: checkout.createSession({ clientReferenceId: reference, customerEmail }).url });
    }

    if (req.method === 'GET' && url.pathname === '/api/state') {
      const account = accountOf(req, url.searchParams.get('reference'));
      if (!account) return sendJson(401, { error: 'unknown credential' });
      return sendJson(200, { subscription: account.subscription, cap: account.cap });
    }
    return sendJson(404, { error: 'not found' });
  };

  const count = (sql) => Number(database.prepare(sql).get().n);
  return {
    origin,
    checkoutOrigin: checkout.origin,
    accounts() {
      return [...accounts.values()].map(({ candidate, subscription, cap, userId }) => ({
        candidate,
        subscription,
        cap,
        signedIn: userId !== null,
      }));
    },
    checkoutSessions: () => checkout.sessionCounts(),
    authRows() {
      return {
        users: count('select count(*) as n from user'),
        sessions: count('select count(*) as n from session'),
        passkeys: count('select count(*) as n from passkey'),
        sessionsWithIpOrUserAgent: count(
          "select count(*) as n from session where coalesce(ipAddress, '') <> '' or coalesce(userAgent, '') <> ''",
        ),
      };
    },
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
      await oauth.stop();
      await checkout.close();
      database.close();
    },
  };
}
