// A plain W3C WebDriver client on the Node standard library, for the trial's embedded servers.
import http from 'node:http';

export const ELEMENT = 'element-6066-11e4-a52e-4f735466cecf';

export function request(port, method, path, body, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const data = body === undefined ? null : Buffer.from(JSON.stringify(body));
    const req = http.request({
      host: '127.0.0.1', port, method, path, timeout: timeoutMs,
      headers: data ? { 'content-type': 'application/json', 'content-length': data.length } : {},
    }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        let parsed = null;
        try {
          parsed = JSON.parse(Buffer.concat(chunks).toString('utf8'));
        } catch {
          parsed = null;
        }
        resolve({ status: res.statusCode, value: parsed?.value });
      });
    });
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', reject);
    if (data) req.write(data);
    req.end();
  });
}

function ok(reply, what) {
  if (reply.status !== 200) throw new Error(`${what}: status ${reply.status} ${reply.value?.error ?? ''}`.trim());
  return reply.value;
}

export function elementId(value) {
  const id = value?.[ELEMENT];
  if (typeof id !== 'string') throw new Error('no element reference');
  return id;
}

export async function waitReady(port, boundMs, sleep) {
  const until = Date.now() + boundMs;
  while (Date.now() < until) {
    const reply = await request(port, 'GET', '/status', undefined, 2000).catch(() => null);
    if (reply?.status === 200) return true;
    await sleep(200);
  }
  return false;
}

export async function newSession(port) {
  const value = ok(await request(port, 'POST', '/session', { capabilities: { alwaysMatch: {} } }), 'new session');
  if (typeof value?.sessionId !== 'string') throw new Error('new session: no id');
  return value.sessionId;
}

export async function findAll(port, session, css) {
  const value = ok(await request(port, 'POST', `/session/${session}/elements`, { using: 'css selector', value: css }), 'find elements');
  return value.map(elementId);
}

export async function find(port, session, css) {
  return elementId(ok(await request(port, 'POST', `/session/${session}/element`, { using: 'css selector', value: css }), 'find element'));
}

export async function click(port, session, element) {
  ok(await request(port, 'POST', `/session/${session}/element/${element}/click`, {}), 'click');
}

export async function text(port, session, element) {
  return ok(await request(port, 'GET', `/session/${session}/element/${element}/text`), 'text');
}

export async function execute(port, session, script) {
  return ok(await request(port, 'POST', `/session/${session}/execute/sync`, { script, args: [] }), 'execute');
}

export async function endSession(port, session) {
  return (await request(port, 'DELETE', `/session/${session}`)).status === 200;
}
