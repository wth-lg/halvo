import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const READ = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'read.sh');
const FORM = /^reading trial=floor machine=mac fact=[a-z0-9_]{1,32} value=\S+ date=[0-9]{4}-[0-9]{2}-[0-9]{2}$/;

/** The cases of read.sh. */
export const cases = [
  ['read_prints_the_reading_lines', () => {
    const r = spawnSync('sh', [READ], { encoding: 'utf8' });
    assert.equal(r.status, 0);
    assert.equal(r.stderr, '');
    const lines = r.stdout.split('\n');
    assert.equal(lines.pop(), '');
    assert.ok(lines.length > 0);
    for (const line of lines) assert.match(line, FORM);
    assert.equal(lines.filter((l) => / fact=safari_of_macos14 value=17\.0 /.test(l)).length, 1);
    assert.equal(new Set(lines.map((l) => l.split(' ')[3])).size, lines.length);
  }],
  ['read_refuses_an_argument', () => {
    const r = spawnSync('sh', [READ, 'x'], { encoding: 'utf8' });
    assert.equal(r.status, 2);
    assert.equal(r.stdout, '');
    assert.match(r.stderr, /^REFUSED usage: [^\n]*\n$/);
  }],
];
