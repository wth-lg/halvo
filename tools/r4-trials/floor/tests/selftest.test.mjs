import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const RUNNER = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'selftest.mjs');

function runIn(files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'r4-floor-runner-'));
  for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), text);
  return spawnSync(process.execPath, [RUNNER, dir], { encoding: 'utf8' });
}

/** The cases of the runner itself, each on a directory of its own. */
export const cases = [
  ['the_runner_fails_on_a_failing_case', () => {
    const r = runIn({ 'a.test.mjs': "export const cases = [['passes', () => {}], ['fails', () => { throw new Error('no'); }]];\n" });
    assert.equal(r.status, 1);
    assert.equal(r.stdout, 'fail fails: no\nfloor self-test: 1 of 2 ok\n');
  }],
  ['the_runner_fails_without_a_case_and_on_a_file_that_does_not_load', () => {
    assert.equal(runIn({}).status, 1);
    const r = runIn({ 'b.test.mjs': 'export const cases = [;\n' });
    assert.equal(r.status, 1);
    assert.match(r.stdout, /^fail b\.test\.mjs: it does not load: /);
  }],
  ['the_runner_passes_when_every_case_passes', () => {
    const r = runIn({ 'c.test.mjs': "export const cases = [['passes', () => {}]];\n", 'd.txt': 'not a test\n' });
    assert.equal(r.status, 0);
    assert.equal(r.stdout, 'floor self-test: 1 of 1 ok\n');
  }],
];
