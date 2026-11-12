// usage: node tools/r4-trials/floor/selftest.mjs
// Runs the cases of every tests/*.test.mjs of the floor trial, one after another, and prints a
// line per failed case and `floor self-test: K of N ok`. It starts no app and no browser.
// Exit: 0 when every case passes, 1 when one fails.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), 'tests');
const files = fs.readdirSync(dir).filter((f) => f.endsWith('.test.mjs')).sort();
let total = 0;
let passed = 0;
for (const file of files) {
  let cases;
  try {
    ({ cases } = await import(pathToFileURL(path.join(dir, file)).href));
  } catch (error) {
    total += 1;
    process.stdout.write(`fail ${file}: it does not load: ${String(error && error.message).split('\n')[0]}\n`);
    continue;
  }
  for (const [name, run] of cases) {
    total += 1;
    try {
      await run();
      passed += 1;
    } catch (error) {
      process.stdout.write(`fail ${name}: ${String(error && error.message).split('\n')[0]}\n`);
    }
  }
}
process.stdout.write(`floor self-test: ${passed} of ${total} ok\n`);
process.exitCode = passed === total && total > 0 ? 0 : 1;
