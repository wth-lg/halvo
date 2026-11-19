import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';

function usage() {
  process.stdout.write(`Usage: node crates/halvo-web/fence/shipped.mjs [--control | --self-test]

The check of the fence of halvo-web (CP-186 N8, in the form of the capture run's W3-8): builds
what ships of the crate by one cargo call that makes no test target,
"cargo build --locked --release -p halvo-web", from the root of the checkout, and reads every
file that the call reports as an artifact of crates/halvo-web for the fence's marker, the byte
string that stands in a build with the feature test-fence and in no build without it. It prints
one line per file, "shipped file=<name> bytes=<n> marker=<yes|no>", and the verdict,
"shipped files=<n> with_marker=<n> result=<PASS|FAIL>": PASS when it read at least one file and
none holds the marker. With --control the call names the feature as well
("--features test-fence"), and the verdict is PASS when a file holds the marker: it shows that the
check sees the marker where the fence's means are built. --self-test reads files that it writes
itself and runs no cargo. Exit 0 for PASS, 1 for FAIL, 2 for a refusal: one line
REFUSED <usage|cargo>: <detail> on standard error. It prints file names, sizes and words alone.
`);
}

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

const MARKER = Buffer.from(['halvo-web', 'test-fence', 'this-build-has-it!'].join(':'));

function holds(bytes) {
  return bytes.indexOf(MARKER) >= 0;
}

function verdict(read, control) {
  const withMarker = read.filter((r) => r.marker).length;
  const pass = read.length > 0 && (control ? withMarker > 0 : withMarker === 0);
  return { withMarker, pass };
}

function report(read, control) {
  for (const r of read) {
    process.stdout.write(`shipped file=${r.name} bytes=${r.bytes} marker=${r.marker ? 'yes' : 'no'}\n`);
  }
  const { withMarker, pass } = verdict(read, control);
  const kind = control ? 'shipped control' : 'shipped';
  process.stdout.write(`${kind} files=${read.length} with_marker=${withMarker} result=${pass ? 'PASS' : 'FAIL'}\n`);
  return pass;
}

function readFile(path) {
  const bytes = readFileSync(path);
  return { name: basename(path), bytes: bytes.length, marker: holds(bytes) };
}

function artifacts(stdout) {
  const files = new Set();
  for (const line of stdout.split('\n')) {
    if (!line.startsWith('{')) continue;
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      continue;
    }
    if (m.reason !== 'compiler-artifact') continue;
    const manifest = String(m.manifest_path || '').replaceAll('\\', '/');
    if (!manifest.endsWith('/crates/halvo-web/Cargo.toml')) continue;
    for (const f of m.filenames || []) files.add(f);
    if (m.executable) files.add(m.executable);
  }
  return [...files].sort();
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), 'halvo-web-shipped-'));
  const cases = [
    ['marker', Buffer.concat([Buffer.from([0, 255, 7]), MARKER, Buffer.from([0])]), true],
    ['clean', Buffer.from([0, 1, 2, 3, 255]), false],
    ['part', MARKER.subarray(0, MARKER.length - 1), false],
    ['empty', Buffer.alloc(0), false],
  ];
  let bad = 0;
  for (const [name, bytes, want] of cases) {
    const path = join(dir, name);
    writeFileSync(path, bytes);
    const got = readFile(path).marker;
    if (got !== want) {
      bad += 1;
      process.stdout.write(`self-test case=${name} marker=${got ? 'yes' : 'no'} want=${want ? 'yes' : 'no'}\n`);
    }
  }
  const row = (marker) => ({ name: 'f', bytes: 1, marker });
  const verdicts = [
    [[row(false)], false, true],
    [[row(false), row(true)], false, false],
    [[], false, false],
    [[row(true)], true, true],
    [[row(false)], true, false],
    [[], true, false],
  ];
  for (const [read, control, want] of verdicts) {
    if (verdict(read, control).pass !== want) {
      bad += 1;
      process.stdout.write(`self-test verdict files=${read.length} control=${control} want=${want}\n`);
    }
  }
  rmSync(dir, { recursive: true, force: true });
  process.stdout.write(`self-test cases=${cases.length + verdicts.length} bad=${bad} result=${bad === 0 ? 'PASS' : 'FAIL'}\n`);
  return bad === 0;
}

const argv = process.argv.slice(2);
if (argv.includes('--help')) {
  usage();
  process.exit(0);
}
if (argv.length > 1 || (argv.length === 1 && !['--control', '--self-test'].includes(argv[0]))) {
  refuse('usage', 'see --help');
}
if (argv[0] === '--self-test') process.exit(selfTest() ? 0 : 1);
const control = argv[0] === '--control';
const args = ['build', '--locked', '--release', '-p', 'halvo-web', '--message-format=json-render-diagnostics'];
if (control) args.push('--features', 'test-fence');
const built = spawnSync('cargo', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
if (built.error) refuse('cargo', `the call did not run (${built.error.code || built.error.name})`);
if (built.status !== 0) refuse('cargo', `the build exited ${built.status}`);
const read = artifacts(built.stdout).map(readFile);
process.exit(report(read, control) ? 0 : 1);
