import { appendFileSync } from 'node:fs';
import test from 'node:test';
import { startServer } from './servers.mjs';

const { FIXTURE_SITE: site, FIXTURE_PIDS: pids, FIXTURE_MODE: mode } = process.env;

test('runner child: starts servers and then waits or throws', async () => {
  for (let i = 0; i < (mode === 'throw' ? 1 : 2); i += 1) {
    const server = await startServer(['--port', '0', site]);
    appendFileSync(pids, `${server.pid}\n`);
  }
  if (mode === 'throw') throw new Error('a failure before the cleanup');
  await new Promise(() => { setInterval(() => {}, 60000); });
});
