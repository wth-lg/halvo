import { startServer } from './servers.mjs';

const server = await startServer(['--port', '0', ...process.argv.slice(2)]);
process.stdout.write(`server ${server.pid} ${server.port}\n`);
setInterval(() => {}, 60000);
