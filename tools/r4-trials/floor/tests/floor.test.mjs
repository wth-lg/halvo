import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LIST, parseList } from '../list.mjs';
import { checkResult, RESULT } from '../result.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

/** The case of docs/r4/trials/floor.md itself. */
export const cases = [
  ['the_result_file_holds_its_form_and_the_rule_s_verdict', () => {
    const readings = execFileSync('sh', [path.join(HERE, 'read.sh')], { encoding: 'utf8' }).split('\n').filter((l) => l !== '');
    const features = parseList(fs.readFileSync(LIST, 'utf8')).features;
    assert.deepEqual(checkResult(fs.readFileSync(RESULT, 'utf8'), readings, features), []);
  }],
];
