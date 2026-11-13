import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { requiredRows, validateFields } from './fields.mjs';

const FILES = {
  '/page.js': ['page.js', 'text/javascript'],
  '/fields.mjs': ['fields.mjs', 'text/javascript'],
};
const BODY_LIMIT = 64 * 1024;

export async function loadFields() {
  const text = await readFile(new URL('../checkout-fields.json', import.meta.url), 'utf8');
  return validateFields(JSON.parse(text));
}

function luhn(digits) {
  let sum = 0;
  for (let i = 0; i < digits.length; i++) {
    let digit = Number(digits[digits.length - 1 - i]);
    if (i % 2 === 1) digit = digit * 2 > 9 ? digit * 2 - 9 : digit * 2;
    sum += digit;
  }
  return sum % 10 === 0;
}

function refusal(fields, session, values) {
  const emailLocked = session.customerEmail !== null;
  for (const row of requiredRows(fields, { country: String(values.country ?? ''), emailLocked })) {
    if (row.kind !== 'click' && String(values[row.id] ?? '').trim() === '') return `${row.label} is missing`;
  }
  const number = String(values['card-number']).replace(/\s/g, '');
  if (!/^\d{16}$/.test(number) || !luhn(number)) return 'the card number is not valid';
  if (!/^(0[1-9]|1[0-2])\/\d{2}$/.test(String(values['card-expiry']))) return 'the expiry is not MM/YY';
  if (!/^\d{3,4}$/.test(String(values['card-cvc']))) return 'the CVC is not 3 or 4 digits';
  if (!emailLocked && !/^[^@\s]+@[^@\s]+$/.test(String(values.email))) return 'the email is not valid';
  return null;
}

async function readBody(req) {
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > BODY_LIMIT) throw new Error('the request body is too large');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString('utf8');
}

export async function startCheckoutPage({ onPaid }) {
  const fields = await loadFields();
  const sessions = new Map();

  async function handle(req, res) {
    const send = (status, type, body) =>
      res.writeHead(status, { 'content-type': type, 'cache-control': 'no-store' }).end(body);
    const sendJson = (status, body) => send(status, 'application/json', JSON.stringify(body));
    const path = new URL(req.url, 'http://127.0.0.1').pathname;

    if (req.method === 'GET' && FILES[path]) {
      const [name, type] = FILES[path];
      return send(200, type, await readFile(new URL(name, import.meta.url)));
    }
    if (req.method === 'GET' && path === '/fields.json') return sendJson(200, fields);

    const match = /^\/c\/([0-9a-f-]{36})(\/session\.json|\/pay)?$/.exec(path);
    const session = match ? sessions.get(match[1]) : undefined;
    if (!match || !session) return send(404, 'text/plain', 'not found');
    if (req.method === 'GET' && !match[2]) {
      return send(200, 'text/html', await readFile(new URL('index.html', import.meta.url)));
    }
    if (req.method === 'GET' && match[2] === '/session.json') {
      return sendJson(200, { customerEmail: session.customerEmail, paid: session.paid });
    }
    if (req.method === 'POST' && match[2] === '/pay') {
      const { values = {}, inputs = {} } = JSON.parse(await readBody(req));
      session.attempts += 1;
      session.inputs = {
        count: Number(inputs.count),
        ids: Array.isArray(inputs.ids) ? inputs.ids.map(String) : [],
      };
      if (session.paid) return sendJson(409, { ok: false, error: 'this session is already paid' });
      const error = refusal(fields, session, values);
      if (error) {
        session.refused += 1;
        return sendJson(400, { ok: false, error });
      }
      session.paid = true;
      await onPaid({ clientReferenceId: session.clientReferenceId });
      return sendJson(200, { ok: true });
    }
    return send(405, 'text/plain', 'method not allowed');
  }

  const server = createServer((req, res) => {
    handle(req, res).catch((error) => {
      res.writeHead(500, { 'content-type': 'text/plain' }).end(String(error));
    });
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const origin = `http://127.0.0.1:${server.address().port}`;

  return {
    origin,
    createSession({ clientReferenceId, customerEmail = null }) {
      const id = randomUUID();
      sessions.set(id, { clientReferenceId, customerEmail, paid: false, attempts: 0, refused: 0, inputs: null });
      return { id, url: `${origin}/c/${id}` };
    },
    sessionCounts() {
      return [...sessions].map(([id, { paid, attempts, refused, inputs }]) => ({ id, paid, attempts, refused, inputs }));
    },
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
