import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cell } from '../cell.mjs';
import { ROOT } from '../list.mjs';
import { holdsCargo, hostedEnv, standInPath } from './stand-in-cargo.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

/** The cases of ruling RD-41 D12: no case of the unit can start the trial app on this Mac. */
export const cases = [
  ['d12_the_path_of_a_ci_child_has_no_other_cargo', () => {
    const stand = standInPath();
    const dirs = stand.PATH.split(path.delimiter);
    assert.equal(dirs[0], stand.dir);
    for (const dir of dirs.slice(1)) assert.ok(!holdsCargo(dir), dir);
    const probe = spawnSync('sh', ['-c', 'cargo probe'], { encoding: 'utf8', env: hostedEnv('true', stand) });
    assert.equal(probe.status, 1, String(probe.error));
    assert.equal(fs.readFileSync(stand.record, 'utf8'), 'cargo probe\n');
  }],
  ['d12_the_stand_in_is_not_called_by_the_refusals_at_the_head', () => {
    const stand = standInPath();
    const run = spawnSync('sh', [path.join(HERE, 'run.sh'), '--arm'], { encoding: 'utf8', env: hostedEnv('true', stand), cwd: ROOT });
    assert.equal(run.status, 2);
    const usage = spawnSync(process.execPath, [path.join(HERE, 'cell.mjs')], { encoding: 'utf8', env: hostedEnv('true', stand) });
    assert.equal(usage.status, 2);
    assert.ok(!fs.existsSync(stand.record), fs.existsSync(stand.record) ? fs.readFileSync(stand.record, 'utf8') : '');
  }],
  ['d12_the_stand_in_records_a_call', () => {
    const stand = standInPath();
    const r = spawnSync('sh', ['-c', 'cargo build --release'], { encoding: 'utf8', env: hostedEnv('true', stand) });
    assert.equal(r.status, 1);
    assert.equal(fs.readFileSync(stand.record, 'utf8'), 'cargo build --release\n');
  }],
  ['d12_cell_refuses_not_hosted_by_itself', async () => {
    await assert.rejects(cell(path.join(ROOT, 'no-such-app'), {}, process.platform), (error) => error.cause === 'not_hosted');
    await assert.rejects(cell(path.join(ROOT, 'no-such-app'), { CI: '' }, process.platform), (error) => error.cause === 'not_hosted');
  }],
];
