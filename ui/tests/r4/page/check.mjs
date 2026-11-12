import { fileRules } from './files.mjs';
import { pageCheck } from './run.mjs';

const USAGE = 'usage: node ui/tests/r4/page/check.mjs files --repo DIR --acorn DIR | page --repo DIR [--write] | selftest --repo DIR --acorn DIR';

function arg(list, name) {
  const i = list.indexOf(name);
  return i >= 0 && i + 1 < list.length ? list[i + 1] : null;
}

function refuse() {
  console.log(USAGE);
  console.log('check result=usage');
  process.exit(2);
}

const [command, ...rest] = process.argv.slice(2);
const repo = arg(rest, '--repo');
if (!repo) refuse();

if (command === 'files') {
  const acorn = arg(rest, '--acorn');
  if (!acorn || rest.length !== 4) refuse();
  const { lines, cause } = fileRules(repo, acorn);
  for (const l of lines) console.log(l);
  console.log('check result=' + (cause || 'ok'));
  process.exit(cause ? 1 : 0);
} else if (command === 'page') {
  const write = rest.includes('--write');
  if (rest.length !== (write ? 3 : 2)) refuse();
  const cause = await pageCheck({ repo, write, needs: !write, log: (l) => console.log(l) });
  console.log('check result=' + (cause || 'ok'));
  process.exit(cause ? 1 : 0);
} else if (command === 'selftest') {
  const acorn = arg(rest, '--acorn');
  if (!acorn || rest.length !== 4) refuse();
  const { selfTest } = await import('./selftest.mjs');
  const ok = await selfTest({ repo, acorn, log: (l) => console.log(l) });
  process.exit(ok ? 0 : 1);
} else {
  refuse();
}
