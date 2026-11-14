import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { countHolds } from '../count.mjs';
import { tempDir } from './temp.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

function fixture(files) {
  const dir = tempDir('count');
  for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), text);
  return dir;
}

/** The cases of ruling RD-45 F3: the caller of the self-test reads its count line. */
export const cases = [
  ['f3_the_count_line_holds_only_when_k_is_n_and_above_0', () => {
    assert.equal(countHolds('fail x: y\nfloor self-test: 108 of 108 ok\n'), true);
    assert.equal(countHolds('fail a: b\nfail c: d\nfloor self-test: 106 of 108 ok\n'), false);
    assert.equal(countHolds('floor self-test: 0 of 0 ok\n'), false);
    assert.equal(countHolds('fail a: b\n'), false);
    assert.equal(countHolds(''), false);
    assert.equal(countHolds('floor self-test: 2 of 2 ok\nfloor self-test: 1 of 2 ok\n'), false);
  }],
  ['f3_count_mjs_fails_on_a_count_line_that_an_exit_0_would_hide', () => {
    const bad = spawnSync(process.execPath, [path.join(HERE, 'count.mjs'), 'floor self-test: 106 of 108 ok\n'], { encoding: 'utf8' });
    assert.equal(bad.status, 1);
    assert.match(bad.stdout, /^miss the self-test's count line is not K of K ok with K above 0$/m);
    const good = spawnSync(process.execPath, [path.join(HERE, 'count.mjs'), 'floor self-test: 3 of 3 ok\n'], { encoding: 'utf8' });
    assert.equal(good.status, 0);
  }],
  ['f3_selftest_sh_passes_a_passing_run_and_fails_a_failing_one', () => {
    const pass = spawnSync('sh', [path.join(HERE, 'selftest.sh'), fixture({ 'a.test.mjs': "export const cases = [['passes', () => {}]];\n" })], { encoding: 'utf8' });
    assert.equal(pass.status, 0, pass.stdout + pass.stderr);
    assert.match(pass.stdout, /floor self-test: 1 of 1 ok\n/);
    const fail = spawnSync('sh', [path.join(HERE, 'selftest.sh'), fixture({ 'b.test.mjs': "export const cases = [['fails', () => { throw new Error('no'); }]];\n" })], { encoding: 'utf8' });
    assert.equal(fail.status, 1);
    assert.match(fail.stdout, /floor self-test: 0 of 1 ok\n/);
  }],
  ['f3_selftest_sh_fails_when_the_runner_exits_0_on_a_failing_count', () => {
    const dir = fixture({ 'c.test.mjs': "process.on('exit', () => { process.exitCode = 0; });\nexport const cases = [['fails', () => { throw new Error('no'); }]];\n" });
    const node = spawnSync(process.execPath, [path.join(HERE, 'selftest.mjs'), dir], { encoding: 'utf8' });
    assert.equal(node.status, 0);
    assert.match(node.stdout, /floor self-test: 0 of 1 ok\n/);
    const sh = spawnSync('sh', [path.join(HERE, 'selftest.sh'), dir], { encoding: 'utf8' });
    assert.equal(sh.status, 1);
  }],
];
