import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

function json(name) {
  return JSON.parse(readFileSync(new URL(name, import.meta.url), 'utf8'));
}

test('package: private, playwright 1.65.0 in devDependencies, a Node major in engines, and the lockfile agrees', () => {
  const manifest = json('./package.json');
  assert.equal(manifest.private, true);
  assert.deepEqual(manifest.devDependencies, { playwright: '1.65.0' });
  assert.equal(manifest.dependencies, undefined);
  assert.match(manifest.engines.node, /^>=[1-9][0-9]*$/);
  const lock = json('./package-lock.json');
  assert.equal(lock.lockfileVersion, 3);
  assert.equal(lock.name, manifest.name);
  assert.deepEqual(lock.packages[''].devDependencies, manifest.devDependencies);
  assert.deepEqual(lock.packages[''].engines, manifest.engines);
  assert.equal(lock.packages['node_modules/playwright'].version, '1.65.0');
  assert.equal(lock.packages['node_modules/playwright-core'].version, '1.65.0');
  assert.deepEqual(Object.keys(lock.packages).sort(), ['', 'node_modules/playwright', 'node_modules/playwright-core']);
  console.log(`package playwright=1.65.0 engines_node=${manifest.engines.node}`);
});
