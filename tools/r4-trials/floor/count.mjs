// usage: node tools/r4-trials/floor/count.mjs TEXT
// Reads the output of the floor self-test and holds its last count line, so that its verdict
// does not rest on the runner's exit status alone. Exit: 0 when the last line of the form
// `floor self-test: K of N ok` has K equal to N and above 0, 1 otherwise.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Whether the last count line of `text` says K of K with K above 0. */
export function countHolds(text) {
  const lines = text.split('\n').map((line) => /^floor self-test: (\d+) of (\d+) ok$/.exec(line)).filter(Boolean);
  const last = lines[lines.length - 1];
  return Boolean(last) && last[1] === last[2] && Number(last[2]) > 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (countHolds(process.argv[2] ?? '')) {
    process.exitCode = 0;
  } else {
    process.stdout.write("miss the self-test's count line is not K of K ok with K above 0\n");
    process.exitCode = 1;
  }
}
